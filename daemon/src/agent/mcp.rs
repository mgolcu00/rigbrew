//! Connectors in a Code task (see `crate::connectors`).
//!
//! The model is never shown a connector's list of tools: the chat engine has room for 8192
//! tokens and one server can describe more tools than that. It gets two tools instead:
//! `find_tool` gives the three that fit a few words, and `call_tool` runs one. The system prompt
//! holds one line for each running connector.
//!
//! Every `call_tool` waits for the person, who sees the connector, the tool and the arguments,
//! unless they marked that tool as always allowed, in the connector's settings or for this task.
//! In a read-only task every call waits, whatever was marked.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{ask, refused_twice, Mode, Pending, DENIED, DENIED_BEFORE, DENIED_TWICE};
use crate::connectors;
use crate::Shared;

/// The longest result of a tool a model is handed, from a connector as from anything else.
pub const RESULT_CHARS: usize = super::files::RESULT_CHARS;

/// What a task keeps about connectors.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskState {
    /// "Use connectors": off until the person switches it on for the task.
    #[serde(default)]
    pub on: bool,
    /// Tools the person allowed for the rest of this task, as `connector/tool`.
    #[serde(default)]
    pub always: Vec<String>,
}

impl TaskState {
    pub fn new(on: bool) -> TaskState {
        TaskState { on, always: Vec::new() }
    }
}

/// The lines added to the system prompt, or nothing when the task does not use connectors or
/// none is running.
pub fn prompt(app: &Shared, state: &TaskState) -> String {
    if !state.on {
        return String::new();
    }
    let lines = app.connectors.lines();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\nConnectors give you more tools from outside this folder. To use one, first call find_tool with a few words for what you need, \
         then call_tool with the connector, the tool and its arguments. The person is asked before a tool runs. The connectors:\n{}",
        lines.join("\n")
    )
}

/// `find_tool` and `call_tool`, offered when the task uses connectors and one is running.
pub fn tool_defs(app: &Shared, state: &TaskState) -> Vec<Value> {
    if !state.on || app.connectors.lines().is_empty() {
        return Vec::new();
    }
    let text = |d: &str| json!({"type": "string", "description": d});
    vec![
        super::tool(
            "find_tool",
            "Find tools of the connectors by a few words. Gives the three that fit best, with their arguments.",
            json!({"query": text("What you need, in a few words, like \"add a note\"")}),
            &["query"],
        ),
        super::tool(
            "call_tool",
            "Run one tool of a connector. Use the names find_tool gave.",
            json!({
                "connector": text("Name of the connector"),
                "tool": text("Name of the tool"),
                "arguments": {"type": "object", "description": "The tool's arguments, by name"},
            }),
            &["connector", "tool"],
        ),
    ]
}

/// Runs `find_tool` or `call_tool`; nothing for any other tool.
pub async fn dispatch(app: &Shared, id: &str, call: &str, name: &str, args: &Value) -> Option<(String, &'static str)> {
    if name != "find_tool" && name != "call_tool" {
        return None;
    }
    let task = app.agent.get(id)?;
    if !task.mcp.on {
        return Some((format!("There is no tool named {name} in this task."), "failed"));
    }
    if name == "find_tool" {
        let Some(query) = args["query"].as_str().map(str::trim).filter(|q| !q.is_empty()) else {
            return Some(("\"query\" is needed.".into(), "failed"));
        };
        let running = app.connectors.running();
        let matches = connectors::rank(query, &running, 3);
        return Some((connectors::matches_text(query, &matches, &running), "done"));
    }
    let (wanted, tool) = (args["connector"].as_str().unwrap_or("").trim(), args["tool"].as_str().unwrap_or("").trim());
    if wanted.is_empty() || tool.is_empty() {
        return Some(("\"connector\" and \"tool\" are needed. Call find_tool first to get them.".into(), "failed"));
    }
    let Some(connector) = app.connectors.resolve(wanted) else {
        let known: Vec<String> = app.connectors.running().into_iter().map(|(c, _)| c.id).collect();
        return Some((format!("There is no connector named {wanted}. The connectors are: {}.", known.join(", ")), "failed"));
    };
    // A tool that is not there is said so before the person is asked about it.
    match app.connectors.tools(&connector.id) {
        Ok(tools) if tools.iter().any(|t| t.name == tool) => {}
        Ok(tools) => {
            let names: Vec<&str> = tools.iter().take(30).map(|t| t.name.as_str()).collect();
            return Some((
                format!("The connector {} has no tool named {tool}. Its tools are: {}.", connector.id, names.join(", ")),
                "failed",
            ));
        }
        Err(why) => return Some((why, "failed")),
    }
    let signature = format!("{name} {args}");
    if task.denied.contains(&signature) {
        return Some((DENIED_BEFORE.into(), "denied"));
    }
    if refused_twice(&task.denied, name) {
        return Some((DENIED_TWICE.into(), "denied"));
    }
    let key = format!("{}/{tool}", connector.id);
    let allowed = task.mode != Mode::ReadOnly && (connector.always_allow.iter().any(|t| t == tool) || task.mcp.always.contains(&key));
    if !allowed {
        // The card shows the connector by its name; the tool and the arguments are in the step.
        let pending = Pending { call: call.into(), tool: name.into(), path: Some(connector.name.clone()), command: None, diff: None };
        let decision = ask(app, id, pending, &signature).await;
        if !decision.allow {
            return Some((DENIED.into(), "denied"));
        }
        if decision.always {
            app.agent.change(id, |t| t.mcp.always.push(key));
        }
    }
    let result = app.connectors.call(&connector.id, tool, args["arguments"].clone()).await;
    // No tool name and no arguments: both come from outside or from the person's work.
    app.record("connector_tool_called", json!({"from": "code", "transport": connector.kind, "ok": result.is_ok()}));
    let status = if result.is_ok() { "done" } else { "failed" };
    Some((connectors::result_for_model(&connector.id, tool, &result, RESULT_CHARS), status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_without_the_switch_keeps_nothing_and_old_tasks_read() {
        assert_eq!(TaskState::default(), TaskState { on: false, always: vec![] });
        assert!(TaskState::new(true).on);
        let old: TaskState = serde_json::from_value(json!({})).unwrap();
        assert!(!old.on && old.always.is_empty());
        let kept: TaskState = serde_json::from_value(json!({"on": true, "always": ["notes/add_note"]})).unwrap();
        assert_eq!(kept.always, ["notes/add_note"]);
    }
}
