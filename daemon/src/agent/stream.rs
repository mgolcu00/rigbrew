//! The answer while it is being written. The engine streams the model's text to the service;
//! the part so far is kept here, next to the running task and never on disk, and the API shows
//! it as `writing`, so the screen has words to show before the turn is over.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use super::Task;
use crate::Shared;

/// The text of the turn in progress. Empty between turns and while a tool call is written.
#[derive(Clone, Default)]
pub struct Writing(Arc<Mutex<String>>);

impl Writing {
    /// Takes the whole text of the turn so far.
    pub fn hear(&self, so_far: &str) {
        *self.0.lock().unwrap() = visible(so_far).to_string();
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    pub fn text(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}

/// The part of a half-written answer that is meant for the person: nothing of the model's
/// thinking, whether it is closed yet or not.
pub fn visible(so_far: &str) -> &str {
    match so_far.rsplit_once("</think>") {
        Some((_, after)) => after.trim_start(),
        None if so_far.trim_start().starts_with("<think>") || "<think>".starts_with(so_far.trim_start()) => "",
        None => so_far.trim_start(),
    }
}

#[derive(Deserialize, Default)]
pub struct Shown {
    /// Only what changes while the model writes: for asking several times a second.
    #[serde(default)]
    brief: bool,
}

/// One task as `GET /v1/agent/tasks/{id}` shows it.
pub fn shown(app: &Shared, task: &Task, how: &Shown) -> Value {
    let writing = app.agent.running.lock().unwrap().get(&task.id).map(|r| r.writing.text()).unwrap_or_default();
    let mut view = if how.brief { task.summary() } else { task.view() };
    view["writing"] = json!(writing);
    // The app the task started, while the service knows it: never saved with the task.
    view["app"] = json!(app.agent.apps.view(&task.id));
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_is_never_shown() {
        assert_eq!(visible("The test fails because"), "The test fails because");
        assert_eq!(visible("\n\nHello"), "Hello");
        assert_eq!(visible("<think>let me see"), "");
        assert_eq!(visible("<thi"), "");
        assert_eq!(visible("<think>let me see</think>\n\nThe answer"), "The answer");
        assert_eq!(visible(""), "");
        let writing = Writing::default();
        writing.hear("<think>hm</think>Now ");
        assert_eq!(writing.text(), "Now ");
        writing.clear();
        assert_eq!(writing.text(), "");
    }
}
