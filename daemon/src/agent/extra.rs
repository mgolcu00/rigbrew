//! What the harness adds around the model so a small one gets further: the state it keeps with
//! a task beyond the conversation, the rules and tools that go with it, and what the model is
//! reminded of on every turn. Kept apart from the loop so the loop stays short.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::files::Folder;
use super::{compose, devices, files, look, outline, plan, see, undo, Mode, Step, Task};
use crate::Shared;

/// Saved with the task and shown by the API next to its other fields.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Extra {
    /// The plan the model wrote, as the person sees it.
    #[serde(default)]
    pub plan: Vec<plan::PlanStep>,
    /// The folder before each request that changed it, oldest first.
    #[serde(default)]
    pub undo: Vec<undo::Snapshot>,
    /// The page the task looked at last: what the preview shows under the running app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<look::Page>,
    /// The last picture of the phone's screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<devices::Screen>,
    /// The picture the model is shown on its next turns. Not shown by the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye: Option<look::Eye>,
    /// Messages the person sent while the task worked: they go in at the next turn.
    #[serde(default)]
    pub queued: Vec<compose::Queued>,
    /// How full the model's context was at its last turn, in tokens, by the engine's count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    /// The film a task of Studio's assistant works on (see `film`); empty for a task of Code.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub film: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub used: u32,
    pub size: u32,
}

/// Longest thought kept with a step.
const MAX_THOUGHT_CHARS: usize = 20_000;

/// Whether the model is asked to think before it answers. With a short context (a small model)
/// the thinking comes out of the 3,072 tokens one answer may take: measured on the Code tasks,
/// the small model thinking wrote its Python package in calls that were cut off until the task
/// failed, and took as long for the Rust fix in fewer turns. One with a long context thinks. A
/// model that always thinks does so either way.
pub fn thinks(context: u32) -> bool {
    context > 8192
}

/// How hard a model whose template takes `reasoning_effort` thinks: some think on every turn
/// whatever they are told, and at their own default a turn of a Code task takes thousands of
/// tokens of thinking. Medium kept the answers and took a fraction of the time; low for the rest.
pub fn effort(context: u32) -> &'static str {
    if thinks(context) {
        "medium"
    } else {
        "low"
    }
}

/// Characters of a thought that go back to the model with the call it led to: its end, where
/// the model decided what to do.
const KEPT_THOUGHT_CHARS: usize = 6000;

/// What a call the model made keeps of the thinking before it, for a model that thinks: the end
/// of the thought, so on its next turn the model goes on from what it decided instead of working
/// it all out again. Only the newest is sent (see `last_thought_only`).
pub fn kept_thought(answer: &Value, context: u32) -> Option<String> {
    let thought = answer["choices"][0]["message"]["reasoning_content"].as_str()?.trim();
    if !thinks(context) || thought.is_empty() {
        return None;
    }
    let all: Vec<char> = thought.chars().collect();
    Some(all[all.len().saturating_sub(KEPT_THOUGHT_CHARS)..].iter().collect())
}

/// The conversation as it is sent: the thought kept with the newest call stays, the older ones
/// are left out, so thinking does not take the room the files need.
pub fn last_thought_only(mut messages: Vec<Value>) -> Vec<Value> {
    let newest = messages.iter().rposition(|m| m["role"] == "assistant");
    for (i, message) in messages.iter_mut().enumerate() {
        if Some(i) != newest {
            if let Some(m) = message.as_object_mut() {
                m.remove("reasoning_content");
            }
        }
    }
    messages
}

/// What the engine said about the turn besides the answer: how much of the context it took, and
/// what the model thought, as a step of its own.
pub fn heard(task: &mut Task, answer: &Value, size: u32) {
    let usage = &answer["usage"];
    let used = usage["prompt_tokens"].as_u64().unwrap_or(0) + usage["completion_tokens"].as_u64().unwrap_or(0);
    if used > 0 {
        task.extra.context = Some(Context { used: used.min(u32::MAX as u64) as u32, size });
    }
    let thought = answer["choices"][0]["message"]["reasoning_content"].as_str().unwrap_or("").trim();
    if !thought.is_empty() {
        task.steps.push(Step::Thought { text: files::cap(thought, MAX_THOUGHT_CHARS), ms: answer["thought_ms"].as_u64().unwrap_or(0) });
    }
}

/// Tools of the harness itself. They change nothing in the folder, so every mode has them.
pub const TOOLS: &[&str] = &["update_plan", "outline"];

pub fn tool_defs() -> Vec<Value> {
    vec![
        plan::tool_def(),
        json!({"type": "function", "function": {
            "name": "outline",
            "description": "List what a source file defines (functions, classes, types) with line numbers, or the same for each source file in a folder.",
            "parameters": {"type": "object", "required": ["path"], "properties": {
                "path": {"type": "string", "description": "Source file or folder"},
            }},
        }}),
    ]
}

/// Lines for the system message that say how to work with these tools.
pub fn rules(mode: Mode) -> Vec<String> {
    let mut lines = vec![
        "For anything longer than one step, call update_plan first with the steps. Keep exactly one step \"doing\", and update the plan when a step is done.".to_string(),
        "Before you read a large file, call outline to see what is on which line. Then read only the part you need with read_file, from_line and lines.".to_string(),
    ];
    if mode != Mode::ReadOnly {
        lines.push("Commands already run inside the folder. Do not use cd or absolute paths in a command.".to_string());
        lines.push("After a change, check it (check_project, or the project's own test command), unless the person said not to. Never say that something works, passes or is fixed unless a command you ran after your last change showed it. If you could not check, say so.".to_string());
    }
    lines
}

/// What goes at the end of the system message on every turn: the state the model must not lose
/// when older messages are left out.
pub fn context(task: &Task) -> String {
    let mut out = String::new();
    if let Some(checks) = see::context(task.mode, std::path::Path::new(&task.folder)) {
        out.push_str(&format!("\n\n{checks}"));
    }
    if let Some(note) = undo::context(task) {
        out.push_str("\n\n");
        out.push_str(note);
    }
    if !task.extra.plan.is_empty() {
        out.push_str(&format!("\n\nYour plan so far ([x] done, [>] doing now, [ ] to do):\n{}", plan::brief(&task.extra.plan)));
    }
    out
}

/// A new message from the person: a plan that was finished belongs to the request before.
pub fn next_request(task: &mut Task) {
    if plan::finished(&task.extra.plan) {
        task.extra.plan.clear();
    }
}

/// Runs one of `TOOLS`.
pub async fn call(app: &Shared, id: &str, folder: &Folder, name: &str, args: &Value) -> (String, &'static str) {
    match name {
        "update_plan" => match plan::parse(args) {
            Ok(steps) => {
                let said = plan::saved(&steps);
                app.agent.change(id, |t| t.extra.plan = steps);
                (said, "done")
            }
            Err(why) => (why, "failed"),
        },
        "outline" => {
            let Some(path) = args["path"].as_str().map(str::to_string) else { return ("\"path\" is needed.".into(), "failed") };
            let folder = folder.clone();
            match tokio::task::spawn_blocking(move || outline::outline(&folder, &path)).await {
                Ok(Ok(text)) => (text, "done"),
                Ok(Err(why)) => (why, "failed"),
                Err(_) => ("That did not work.".into(), "failed"),
            }
        }
        _ => (format!("There is no tool named {name}."), "failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::files;
    use super::*;

    fn task(folder: &std::path::Path, mode: &str) -> Task {
        serde_json::from_value(json!({
            "id": "0".repeat(32), "folder": folder, "title": "t", "model": "m", "mode": mode, "web": false, "state": "running",
            "created_ms": 1, "updated_ms": 1, "steps": [{"kind": "user", "text": "go"}], "pending": null, "error": null,
        }))
        .unwrap()
    }

    #[test]
    fn the_model_is_reminded_of_what_it_must_not_lose() {
        let root = files::scratch("extra");
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        let mut t = task(&root, "ask");
        let checked = super::super::checks::has_fast(&root);
        if checked {
            assert!(context(&t).starts_with("\n\ncheck_project checks this project. It runs: cargo check"), "{}", context(&t));
        }
        t.extra.plan = plan::parse(&json!({"steps": [{"text": "Read", "status": "done"}, {"text": "Fix"}]})).unwrap();
        assert!(context(&t).ends_with("\n\nYour plan so far ([x] done, [>] doing now, [ ] to do):\n[x] Read\n[>] Fix"), "{}", context(&t));
        // After an undo the next request is told, and only that one.
        t.extra.undo.push(serde_json::from_value(json!({"request": 1, "kind": "copies", "undone": true, "undone_at": 1})).unwrap());
        assert!(!context(&t).contains("took back"));
        t.steps.push(super::super::Step::User { text: "again".into() });
        assert!(context(&t).contains("The person took back the changes of the last request."));
        t.steps.push(super::super::Step::User { text: "and again".into() });
        assert!(!context(&t).contains("took back"));
        // A plan that is finished goes with the next message; one that is not stays.
        next_request(&mut t);
        assert_eq!(t.extra.plan.len(), 2);
        t.extra.plan = plan::parse(&json!({"steps": [{"text": "Read", "status": "done"}]})).unwrap();
        next_request(&mut t);
        assert!(t.extra.plan.is_empty());
        // Nothing to run in a task that only reads.
        let reading = task(&root, "read_only");
        assert_eq!(context(&reading), "");
        assert!(!rules(Mode::ReadOnly).iter().any(|l| l.contains("Commands") || l.contains("passes")));
        assert!(rules(Mode::Ask).iter().any(|l| l.contains("Do not use cd")));
    }

    #[test]
    fn the_engine_s_count_and_the_thinking_are_kept() {
        let root = files::scratch("heard");
        let mut t = task(&root, "ask");
        heard(&mut t, &json!({"choices": [{"message": {"content": "ok"}}]}), 8192);
        assert_eq!((t.extra.context, t.steps.len()), (None, 1), "an engine that does not count leaves it as it was");
        let answer = json!({
            "choices": [{"message": {"content": "ok", "reasoning_content": "  The test wants 3.  "}}],
            "usage": {"prompt_tokens": 3000, "completion_tokens": 200}, "thought_ms": 1200,
        });
        heard(&mut t, &answer, 32_768);
        assert_eq!(t.extra.context, Some(Context { used: 3200, size: 32_768 }));
        assert!(matches!(t.steps.last(), Some(Step::Thought { text, ms: 1200 }) if text == "The test wants 3."));
        let view = t.view();
        assert_eq!(view["context"], json!({"used": 3200, "size": 32_768}));
        assert_eq!(view["queued"], json!([]));
        assert!(!thinks(8192) && thinks(32_768));
        // The end of a thought goes back with its call, for a model that thinks; only the newest.
        let long = json!({"choices": [{"message": {"reasoning_content": format!("{}decide: edit merge.rs", "x".repeat(9000))}}]});
        let kept = kept_thought(&long, 32_768).unwrap();
        assert!(kept.ends_with("decide: edit merge.rs") && kept.chars().count() == KEPT_THOUGHT_CHARS);
        assert_eq!(kept_thought(&long, 8192), None);
        assert_eq!(kept_thought(&json!({"choices": [{"message": {"content": "hi"}}]}), 32_768), None);
        let sent = last_thought_only(vec![
            json!({"role": "assistant", "content": "", "reasoning_content": "old"}),
            json!({"role": "tool", "content": "r"}),
            json!({"role": "assistant", "content": "", "reasoning_content": "new"}),
            json!({"role": "tool", "content": "r"}),
        ]);
        assert!(sent[0].get("reasoning_content").is_none() && sent[2]["reasoning_content"] == "new");
        assert_eq!((effort(8192), effort(32_768)), ("low", "medium"));
    }

    #[test]
    fn every_tool_of_the_harness_has_a_definition() {
        let names: Vec<String> = tool_defs().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, TOOLS);
    }
}
