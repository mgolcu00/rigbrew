//! What a task works on, and with which tools. The loop in `agent.rs` is the same for every
//! task: it asks the kit for the system message and the tools of each turn, runs each call
//! through it, and asks it for a last look before a request ends.
//!
//! Code works on the files of a folder (its tools are in `agent.rs`, `see`, `skills`, `mcp`).

use serde_json::Value;

use super::files::Folder;
use super::{call_tool, extra, mcp, see, skills, system_prompt, tool_defs, verify, Task};
use crate::Shared;

pub(super) enum Kit {
    /// The code in one folder: files, commands, the web, skills and connectors.
    Code(Folder),
}

impl Kit {
    /// The kit of a task, or why it cannot work.
    pub(super) fn open(task: &Task) -> Result<Kit, String> {
        Folder::open(&task.folder).map(Kit::Code)
    }

    /// The system message and the tools of one turn.
    pub(super) fn turn(&self, app: &Shared, task: &Task) -> (String, Vec<Value>) {
        match self {
            Kit::Code(folder) => {
                let mut skills = skills::load(&app.root, Some(folder.root()));
                skills.retain(|s| s.source != "built-in" || see::skill_fits(&s.name, folder.root()));
                let system = format!(
                    "{}{}{}{}{}",
                    system_prompt(folder, task.mode, task.web, &skills),
                    see::rules(task.mode, folder.root()).iter().map(|l| format!("\n{l}")).collect::<String>(),
                    mcp::prompt(app, &task.mcp),
                    extra::context(task),
                    task.pace.note(task.web)
                );
                let mut tools = tool_defs(task.mode, task.web, !skills.is_empty());
                tools.extend(see::tool_defs(task.mode, folder.root()));
                tools.extend(skills::extra_tools(&skills));
                tools.extend(mcp::tool_defs(app, &task.mcp));
                (system, tools)
            }
        }
    }

    /// Runs one tool call: (result for the model, step status).
    pub(super) async fn call(&self, app: &Shared, id: &str, call: &str, name: &str, args: &Value) -> (String, &'static str) {
        match self {
            Kit::Code(folder) => call_tool(app, id, folder, call, name, args).await,
        }
    }

    /// What the model is told once before a request ends, when something is left to do.
    pub(super) fn before_finish(&self, task: &Task) -> Option<String> {
        match self {
            Kit::Code(folder) => see::before_finish(task, folder.root()),
        }
    }

    /// The model gave its answer: what it says about the work is noted with the task.
    pub(super) fn answered(&self, task: &mut Task, said: &str) {
        match self {
            Kit::Code(_) => verify::note_unchecked(task, said),
        }
    }
}
