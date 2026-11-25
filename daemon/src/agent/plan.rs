//! The plan of a task: a short list of steps the model writes before longer work and keeps up
//! to date. The person sees it as a checklist, and the model gets it back on every turn, so it
//! is still there when older parts of the conversation were left out to save space.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_STEPS: usize = 12;
const MAX_STEP_CHARS: usize = 140;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Pending,
    Doing,
    Done,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub text: String,
    pub status: Status,
}

/// The status as the model wrote it. Small models use many words for the same three things.
fn status(word: &str) -> Status {
    match word.trim().to_lowercase().replace([' ', '-'], "_").as_str() {
        "doing" | "in_progress" | "active" | "current" | "started" | "working" | "now" => Status::Doing,
        "done" | "completed" | "complete" | "finished" | "x" => Status::Done,
        _ => Status::Pending,
    }
}

/// Reads the arguments of `update_plan`. Forgiving on purpose: the list may arrive as text, a
/// step may be a bare sentence, and the status may be any of the usual words. What comes out
/// has at most one step that is being done: the first one marked so, or, when work is left and
/// none is marked, the first step that is not done.
pub fn parse(args: &Value) -> Result<Vec<PlanStep>, String> {
    let raw = match &args["steps"] {
        Value::String(text) => serde_json::from_str::<Value>(text).unwrap_or(Value::Null),
        other => other.clone(),
    };
    let Some(items) = raw.as_array() else {
        return Err("\"steps\" is needed: a list like [{\"text\": \"Read the test\", \"status\": \"doing\"}].".into());
    };
    let mut steps: Vec<PlanStep> = Vec::new();
    for item in items.iter().take(MAX_STEPS) {
        let (text, state) = match item {
            Value::String(text) => (text.as_str(), Status::Pending),
            other => (
                other["text"].as_str().or_else(|| other["step"].as_str()).or_else(|| other["title"].as_str()).unwrap_or(""),
                match &other["status"] {
                    Value::String(word) => status(word),
                    _ if other["done"] == true => Status::Done,
                    _ => Status::Pending,
                },
            ),
        };
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_STEP_CHARS).collect();
        if !text.is_empty() {
            steps.push(PlanStep { text, status: state });
        }
    }
    if steps.is_empty() {
        return Err("The plan has no steps. Give each step a \"text\".".into());
    }
    let mut doing = false;
    for step in &mut steps {
        if step.status == Status::Doing {
            if doing {
                step.status = Status::Pending;
            }
            doing = true;
        }
    }
    if !doing {
        if let Some(next) = steps.iter_mut().find(|s| s.status != Status::Done) {
            next.status = Status::Doing;
        }
    }
    Ok(steps)
}

/// The plan in a few short lines, as the model reads it back.
pub fn brief(steps: &[PlanStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let mark = match s.status {
                Status::Done => "[x]",
                Status::Doing => "[>]",
                Status::Pending => "[ ]",
            };
            format!("{mark} {}", s.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `update_plan` answers: where the plan stands and what to do next.
pub fn saved(steps: &[PlanStep]) -> String {
    let done = steps.iter().filter(|s| s.status == Status::Done).count();
    match steps.iter().find(|s| s.status == Status::Doing) {
        Some(now) => format!("Plan saved: {done} of {} steps done. Now: {}", steps.len(), now.text),
        None => format!("Plan saved: all {} steps are done. Check the result, then call finish.", steps.len()),
    }
}

/// Whether nothing is left to do in it.
pub fn finished(steps: &[PlanStep]) -> bool {
    steps.iter().all(|s| s.status == Status::Done)
}

pub fn tool_def() -> Value {
    json!({"type": "function", "function": {
        "name": "update_plan",
        "description": "Write or update the plan the person sees. Send the whole list every time, with exactly one step \"doing\".",
        "parameters": {"type": "object", "required": ["steps"], "properties": {"steps": {
            "type": "array",
            "description": "Every step of the plan, in order",
            "items": {"type": "object", "required": ["text", "status"], "properties": {
                "text": {"type": "string", "description": "The step in a few words"},
                "status": {"type": "string", "enum": ["pending", "doing", "done"]},
            }},
        }}},
    }})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(steps: &[PlanStep]) -> Vec<Status> {
        steps.iter().map(|s| s.status).collect()
    }

    #[test]
    fn a_plan_is_read_and_has_one_step_in_progress() {
        let steps = parse(&json!({"steps": [
            {"text": "Read the test", "status": "done"},
            {"text": "Fix   the\nfunction", "status": "doing"},
            {"text": "Run the tests", "status": "pending"},
        ]}))
        .unwrap();
        assert_eq!(states(&steps), [Status::Done, Status::Doing, Status::Pending]);
        assert_eq!(steps[1].text, "Fix the function");
        assert_eq!(brief(&steps), "[x] Read the test\n[>] Fix the function\n[ ] Run the tests");
        assert_eq!(saved(&steps), "Plan saved: 1 of 3 steps done. Now: Fix the function");
        assert!(!finished(&steps));
    }

    #[test]
    fn what_small_models_send_is_understood() {
        // Two steps marked as being done: the first one is.
        let steps = parse(&json!({"steps": [{"text": "a", "status": "in_progress"}, {"text": "b", "status": "doing"}]})).unwrap();
        assert_eq!(states(&steps), [Status::Doing, Status::Pending]);
        // None marked: the first step that is left.
        let steps =
            parse(&json!({"steps": [{"text": "a", "status": "completed"}, {"text": "b"}, {"text": "c", "status": "todo"}]})).unwrap();
        assert_eq!(states(&steps), [Status::Done, Status::Doing, Status::Pending]);
        // Bare sentences, and the whole list sent as text.
        let steps = parse(&json!({"steps": "[\"look\", {\"step\": \"change\", \"done\": true}]"})).unwrap();
        assert_eq!(states(&steps), [Status::Doing, Status::Done]);
        assert_eq!(steps[1].text, "change");
        let steps = parse(&json!({"steps": [{"text": "a", "status": "Done"}, {"text": "b", "status": "finished"}]})).unwrap();
        assert!(finished(&steps));
        assert!(saved(&steps).contains("all 2 steps are done"));
    }

    #[test]
    fn a_plan_without_steps_is_refused_and_a_long_one_is_cut() {
        assert!(parse(&json!({})).is_err());
        assert!(parse(&json!({"steps": "first this, then that"})).is_err());
        assert!(parse(&json!({"steps": [{"text": "  "}, {"status": "doing"}]})).is_err());
        let many: Vec<Value> = (0..40).map(|i| json!({"text": format!("step {i} {}", "x".repeat(400))})).collect();
        let steps = parse(&json!({"steps": many})).unwrap();
        assert_eq!(steps.len(), MAX_STEPS);
        assert!(steps.iter().all(|s| s.text.chars().count() <= MAX_STEP_CHARS));
    }

    #[test]
    fn the_tool_names_the_three_states() {
        let def = tool_def();
        assert_eq!(def["function"]["name"], "update_plan");
        assert_eq!(
            def["function"]["parameters"]["properties"]["steps"]["items"]["properties"]["status"]["enum"],
            json!(["pending", "doing", "done"])
        );
    }
}
