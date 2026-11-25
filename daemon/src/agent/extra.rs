//! What the harness adds around the model so a small one gets further: the state it keeps with
//! a task beyond the conversation, the rules and tools that go with it, and what the model is
//! reminded of on every turn. Kept apart from the loop so the loop stays short.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{plan, undo, Mode, Task};
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
}

/// Tools of the harness itself. They change nothing in the folder, so every mode has them.
pub const TOOLS: &[&str] = &["update_plan"];

pub fn tool_defs() -> Vec<Value> {
    vec![plan::tool_def()]
}

/// Lines for the system message that say how to work with these tools.
pub fn rules(_mode: Mode) -> Vec<String> {
    vec!["For anything longer than one step, call update_plan first with the steps. Keep exactly one step \"doing\", and update the plan when a step is done."
        .into()]
}

/// What goes at the end of the system message on every turn: the state the model must not lose
/// when older messages are left out.
pub fn context(task: &Task) -> String {
    let mut out = String::new();
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
pub async fn call(app: &Shared, id: &str, name: &str, args: &Value) -> (String, &'static str) {
    match name {
        "update_plan" => match plan::parse(args) {
            Ok(steps) => {
                let said = plan::saved(&steps);
                app.agent.change(id, |t| t.extra.plan = steps);
                (said, "done")
            }
            Err(why) => (why, "failed"),
        },
        _ => (format!("There is no tool named {name}."), "failed"),
    }
}
