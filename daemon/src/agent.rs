//! Code: a chat model that works on a folder with tools, one step at a time.
//!
//! The loop runs here in the service, not in the app, so a task goes on when the person looks
//! at another screen, and can be listed, followed and stopped. Every task is kept in memory and
//! as one JSON file under `<data dir>/agent/`, so finished tasks are there after a restart.
//!
//! A task has no limit on its steps: the model is on this computer and costs nothing to run, so
//! the work goes on until it is done. Three things make that hold: `pace` notices when the model
//! goes round in circles (and in the end asks the person, it never ends the task for it),
//! `recall` keeps a running note of what left the context window, and an engine error is tried
//! again after a short wait. When the service itself stops, the task is marked as cut off and
//! the person's next message goes on from where it was.
//!
//!   POST   /v1/agent/tasks                {folder, prompt, model?, mode, web?, connectors?} starts a task;
//!                                         {film, prompt, model?, mode, selection?} one of Studio's assistant (see `film`)
//!   GET    /v1/agent/tasks?folder=        tasks of Code, newest first; ?film= the assistant's on one film
//!   PUT    /v1/agent/tasks/{id}           {title}: the person names the task; the name is then theirs
//!   GET    /v1/agent/tasks/{id}           state, steps so far, what waits for an answer, and the
//!                                         answer being written (`writing`); ?brief=true leaves the steps out
//!   POST   /v1/agent/tasks/{id}/approve   {call, allow, always?}
//!   POST   /v1/agent/tasks/{id}/message   {text, images?, continue?, mode?, web?, connectors?} continues the
//!                                         task, or waits for its next turn while it works
//!   DELETE /v1/agent/tasks/{id}/queue/{q}  takes back a message that waits
//!   GET    /v1/agent/files?folder=&q=     files to name with @
//!   POST   /v1/agent/tasks/{id}/undo      takes back what the newest request changed
//!   DELETE /v1/agent/tasks/{id}           stops it; ?remove=true also forgets it
//!   GET    /v1/agent/skills?folder=       the skills a task there can use
//!   DELETE /v1/agent/tasks/{id}/app       stops the app the task started
//!   POST   /v1/agent/tasks/{id}/device    a new picture of the phone's screen
//!   GET    /v1/agent/tasks/{id}/shots/{name}.png  a picture the task took
//!   POST   /v1/agent/look                 {task?, url?, size?, do?} looks at a page on this computer
//!   GET    /v1/agent/looks/next, POST /v1/agent/looks/{id}  the app takes a look and answers it
//!
//! What a task may touch: files inside its folder (see `files`), commands the person allowed,
//! and, only when the person switched it on, a web search and public pages (see `web`) and the
//! tools of the connectors they added (see `mcp`). It can also run, look at and check its own
//! work (see `see`): start the project's app, open its pages (this computer only), take a
//! picture of the phone it runs on, and run the project's checks.

mod apps;
mod checks;
mod compose;
mod devices;
mod extra;
mod files;
mod film;
mod kit;
pub mod look;
pub mod mcp;
mod outline;
mod pace;
mod plan;
mod project;
mod recall;
pub mod see;
pub mod skills;
mod stream;
mod undo;
mod verify;
mod web;

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::Modality;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::{fail, Shared};
use files::Folder;
pub use undo::task_undo;

/// How long the loop waits before it asks the engine again after an error, one wait per try.
/// The model is on this computer: it may be loading again, or busy for a moment. Only when the
/// last wait did not help does the task fail, and the person's next message goes on from there.
const RETRY_WAITS: [Duration; 3] = [Duration::from_secs(2), Duration::from_secs(6), Duration::from_secs(20)];
/// How often in a row an answer with no tool call is sent back (it was empty, cut off, or the
/// plan still has steps to do) before it is taken as the answer.
const MAX_SEND_BACKS: usize = 2;
/// Characters of conversation sent to the model; older tool results are left out beyond it.
/// This is for a context of 8192 tokens, of which the tools, the rules and the answer need their
/// share; a model with a longer one gets as many times more (see `history_chars`).
const HISTORY_CHARS: usize = 12_000;
const MAX_ANSWER_TOKENS: u32 = 3072;

/// The characters of conversation a model with `context` tokens is sent.
fn history_chars(context: u32) -> usize {
    HISTORY_CHARS * (context.max(8192) / 8192) as usize
}

/// Tokens one answer may take, thinking included.
fn answer_tokens(context: u32) -> u32 {
    if context > 8192 {
        MAX_ANSWER_TOKENS * 3
    } else {
        MAX_ANSWER_TOKENS
    }
}
/// How often one request may be told that its tool call was cut off before the task gives up.
const MAX_CUT_OFFS: usize = 3;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PROMPT_CHARS: usize = 4000;
const MAX_RUNNING: usize = 4;
/// Largest request body that starts or continues a task: the words and a few pictures.
pub const MAX_BODY_BYTES: usize = 48 * 1024 * 1024;
const MAX_KEPT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// File changes and commands wait for the person.
    #[default]
    Ask,
    /// Nothing that changes a file or runs a command is offered.
    ReadOnly,
    /// File changes inside the folder apply at once; commands still wait.
    AutoEdits,
    /// Nothing waits: file changes, commands and connector tools go ahead. The person chose this
    /// knowingly; the folder limits and, on macOS, the command sandbox still hold.
    Bypass,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Step {
    User {
        text: String,
    },
    Model {
        text: String,
    },
    Tool {
        call: String,
        name: String,
        arguments: Value,
        /// "running", "waiting" (for the person), "done", "failed", "denied" or "stopped".
        status: String,
        result: Option<String>,
        /// The change as a unified diff, for `write_file` and `edit_file`.
        diff: Option<String>,
        /// The picture the step took (a page, a phone's screen): its name under the task's shots.
        #[serde(default)]
        image: Option<String>,
    },
    Note {
        text: String,
    },
    /// What the task asks the person when it cannot get further by itself.
    Question {
        text: String,
    },
    /// Pictures the person sent with the message before.
    Pictures {
        images: Vec<String>,
    },
    /// What the model thought before it answered, and for how long.
    Thought {
        text: String,
        ms: u64,
    },
}

/// A tool call that waits for the person's answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Pending {
    call: String,
    tool: String,
    path: Option<String>,
    command: Option<String>,
    diff: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Task {
    id: String,
    folder: String,
    title: String,
    /// Who wrote the title: "words" (the start of the request), "model" (see `crate::title`) or
    /// "person". Empty in a task from before this was kept.
    #[serde(default)]
    title_by: String,
    model: String,
    mode: Mode,
    web: bool,
    /// "running", "waiting" (for a yes or no to one step), "asking" (it put a question to the
    /// person and goes on with their next message), "done", "failed", "stopped" or "interrupted".
    state: String,
    created_ms: i64,
    updated_ms: i64,
    /// When the person sent the request that is being worked on, or was worked on last.
    #[serde(default)]
    request_ms: i64,
    steps: Vec<Step>,
    pending: Option<Pending>,
    error: Option<String>,
    /// Commands the person allowed for the rest of this task, exactly as written.
    #[serde(default)]
    always: Vec<String>,
    /// What the person said no to since their last message (tool name and arguments). The same
    /// call is refused without asking them again.
    #[serde(default)]
    denied: Vec<String>,
    /// Model turns taken for the current request.
    #[serde(default)]
    turns: usize,
    /// The conversation as the model sees it. Kept with the task, not shown by the API.
    #[serde(default)]
    messages: Vec<Value>,
    /// What the harness keeps with the task: see `extra`.
    #[serde(flatten)]
    extra: extra::Extra,
    /// "Use connectors", and the connector tools allowed for the rest of this task.
    #[serde(default)]
    mcp: mcp::TaskState,
    /// What was searched, and how the current request is getting on: see `pace`. Not shown by the API.
    #[serde(default)]
    pace: pace::Pace,
    /// What is kept of the part of the conversation that is no longer sent: see `recall`. Not
    /// shown by the API.
    #[serde(default)]
    memory: recall::Memory,
}

impl Task {
    fn active(&self) -> bool {
        self.state == "running" || self.state == "waiting"
    }

    /// The task as the API shows it.
    fn view(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("messages");
            o.remove("pace");
            o.remove("memory");
            o.remove("eye");
        }
        v
    }

    fn summary(&self) -> Value {
        json!({
            "id": self.id, "folder": self.folder, "title": self.title, "title_by": self.title_by, "model": self.model, "mode": self.mode,
            "state": self.state, "created_ms": self.created_ms, "updated_ms": self.updated_ms, "request_ms": self.request_ms,
            "steps": self.steps.len(), "film": self.extra.film,
        })
    }

    fn tool_step(&mut self, call: &str) -> Option<&mut Step> {
        self.steps.iter_mut().rev().find(|s| matches!(s, Step::Tool { call: c, .. } if c == call))
    }

    fn set_tool(&mut self, call: &str, new_status: &str, new_result: Option<String>, new_diff: Option<String>) {
        if let Some(Step::Tool { status, result, diff, .. }) = self.tool_step(call) {
            *status = new_status.to_string();
            if new_result.is_some() {
                *result = new_result;
            }
            if new_diff.is_some() {
                *diff = new_diff;
            }
        }
    }

    /// Whatever was in the middle of something when the task ended early.
    fn settle(&mut self, state: &str) {
        self.pending = None;
        for step in &mut self.steps {
            if let Step::Tool { status, .. } = step {
                if status == "running" || status == "waiting" {
                    *status = "stopped".into();
                }
            }
        }
        self.state = state.into();
    }
}

struct Decision {
    allow: bool,
    always: bool,
}

struct Running {
    abort: tokio::task::AbortHandle,
    started: Instant,
    /// Set while a tool call waits for the person.
    approve: Option<oneshot::Sender<Decision>>,
    /// The text of the answer the model is writing right now.
    writing: stream::Writing,
}

/// Every task of this installation, and the ones working right now.
pub struct Agents {
    dir: PathBuf,
    tasks: Mutex<HashMap<String, Task>>,
    running: Mutex<HashMap<String, Running>>,
    /// The apps tasks started with `start_app`.
    pub apps: apps::Apps,
    /// Pages that wait for the app's window to open them.
    looks: look::Looks,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Agents {
    /// Reads the saved tasks. One that was working when the service stopped is marked as cut off.
    pub fn load(root: &FsPath) -> Agents {
        let dir = root.join("agent");
        let mut tasks = HashMap::new();
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Some(mut task) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Task>(&t).ok()) else { continue };
            if !valid_id(&task.id) {
                continue;
            }
            if task.active() {
                task.settle("interrupted");
                task.error = Some("Rigbrew was closed while this was running. Send a message to go on.".into());
            }
            tasks.insert(task.id.clone(), task);
        }
        Agents { dir, tasks: Mutex::new(tasks), running: Mutex::new(HashMap::new()), apps: Default::default(), looks: Default::default() }
    }

    fn save(&self, task: &Task) {
        std::fs::create_dir_all(&self.dir).ok();
        let path = self.dir.join(format!("{}.json", task.id));
        // Written next to the file and renamed, so a crash never leaves half a task.
        let tmp = self.dir.join(format!(".{}.json.tmp", task.id));
        if std::fs::write(&tmp, serde_json::to_vec(task).unwrap_or_default()).is_ok() {
            std::fs::rename(&tmp, &path).ok();
        }
    }

    fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().unwrap().get(id).cloned()
    }

    /// Changes a task and saves it.
    fn change<R>(&self, id: &str, f: impl FnOnce(&mut Task) -> R) -> Option<R> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks.get_mut(id)?;
        let out = f(task);
        task.updated_ms = now_ms();
        self.save(task);
        Some(out)
    }

    fn insert(&self, task: Task) {
        let mut tasks = self.tasks.lock().unwrap();
        // Old finished tasks make room.
        if tasks.len() >= MAX_KEPT {
            let mut old: Vec<(i64, String)> = tasks.values().filter(|t| !t.active()).map(|t| (t.updated_ms, t.id.clone())).collect();
            old.sort();
            for (_, id) in old.into_iter().take(tasks.len() + 1 - MAX_KEPT) {
                if let Some(gone) = tasks.remove(&id) {
                    undo::forget(&self.dir, &gone);
                    self.let_go(&id);
                }
                std::fs::remove_file(self.dir.join(format!("{id}.json"))).ok();
            }
        }
        self.save(&task);
        tasks.insert(task.id.clone(), task);
    }

    /// What a task leaves outside its own file: its running app and its pictures.
    fn let_go(&self, id: &str) {
        self.apps.stop(id);
        look::forget(&self.dir, id);
    }

    fn remove(&self, id: &str) {
        if let Some(gone) = self.tasks.lock().unwrap().remove(id) {
            undo::forget(&self.dir, &gone);
        }
        self.let_go(id);
        std::fs::remove_file(self.dir.join(format!("{id}.json"))).ok();
    }
}

// --- what the model is told -----------------------------------------------------------------

fn system_prompt(folder: &Folder, mode: Mode, web: bool, skills: &[skills::Skill]) -> String {
    let mut lines = vec![
        format!("You are Steep, Rigbrew's model, helping with the code in one folder on this computer: {}", folder.root().display()),
        "Work in small steps with the tools. Look before you answer or change anything: list_files, search and read_file.".to_string(),
        "Paths are relative to the folder. Nothing outside the folder can be reached.".to_string(),
    ];
    match mode {
        Mode::ReadOnly => {
            lines.push("This task is read only: you cannot change files or run commands. Describe the change instead.".into())
        }
        Mode::Ask | Mode::AutoEdits | Mode::Bypass => {
            lines.push(
                "To change a file use edit_file with the exact text to replace. Use write_file only for a new file or a full rewrite."
                    .into(),
            );
            lines.push(
                "One write_file call holds at most about 80 lines. For a longer file: write_file with the first part, then append_file for each next part of about 80 lines, until it is complete."
                    .into(),
            );
            lines.push("The person sees each change and command and may say no. After a no, do not try the same thing again.".into());
        }
    }
    if web {
        lines.push(format!(
            "web_search and fetch_url reach the internet. Use them only when the folder cannot answer the question. Search once or twice, then open the best page with fetch_url. One request gets at most {} searches, and a search you already made gives the same result again.",
            pace::SEARCH_BUDGET
        ));
    }
    if !skills.is_empty() {
        lines.push("Skills are instructions for a kind of work. When one fits, call use_skill with its name first:".into());
        for s in skills {
            lines.push(format!("- {}: {}", s.name, s.description));
        }
    }
    lines.extend(extra::rules(mode));
    lines.push("Never look up the same thing twice. After a few look-ups, act: make the change, run the check, or answer.".into());
    lines.push("When the work is done, call finish with a short summary of what you did or found. Keep it short and plain.".into());
    lines.join("\n")
}

/// What the model is told when its tool call did not fit in one answer.
const CUT_OFF_HINT: &str = "Your last tool call was cut off because it was too long, so nothing was done. Do it again in smaller parts: \
write_file with at most about 80 lines of the file, then append_file for each next part. Do not explain, just call the tool.";

/// Whether an engine error says that a tool call's arguments were not complete JSON.
fn cut_off_call(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("parse tool call") || (e.contains("tool call") && e.contains("json") && e.contains("parse"))
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type": "function", "function": {"name": name, "description": description,
        "parameters": {"type": "object", "properties": properties, "required": required}}})
}

/// The tools offered on this turn. What the mode or the web switch rules out is not offered at all.
fn tool_defs(mode: Mode, web: bool, with_skills: bool) -> Vec<Value> {
    let text = |d: &str| json!({"type": "string", "description": d});
    let number = |d: &str| json!({"type": "integer", "description": d});
    let mut out = vec![
        tool(
            "list_files",
            "List the files and folders under a path.",
            json!({"path": text("Folder to list, \".\" for the top"), "depth": number("Levels to show, 1 to 4 (2 if left out)")}),
            &[],
        ),
        tool(
            "read_file",
            "Read a text file, or some lines of it.",
            json!({"path": text("File to read"), "from_line": number("First line to read (1 if left out)"), "lines": number("How many lines (200 if left out)")}),
            &["path"],
        ),
        tool(
            "search",
            "Find lines that contain a text or match a regular expression, in all text files under a path. Files git ignores are skipped.",
            json!({"pattern": text("Text or regular expression"), "path": text("Folder or file to look in (the whole folder if left out)")}),
            &["pattern"],
        ),
    ];
    if mode != Mode::ReadOnly {
        out.push(tool(
            "edit_file",
            "Replace one piece of text in a file. \"old\" must be copied exactly from the file and be there only once.",
            json!({"path": text("File to change"), "old": text("Exact text in the file now"), "new": text("Text to put in its place")}),
            &["path", "old", "new"],
        ));
        out.push(tool(
            "write_file",
            "Create a file, or replace its whole content. Keep one call under about 80 lines: for a longer file write the first part here and add the rest with append_file.",
            json!({"path": text("File to write"), "content": text("The new content, or its first part")}),
            &["path", "content"],
        ));
        out.push(tool(
            "append_file",
            "Add text to the end of a file, to write a long file in parts of at most about 80 lines.",
            json!({"path": text("File to add to"), "content": text("The next part")}),
            &["path", "content"],
        ));
        out.push(tool(
            "run_command",
            "Run one shell command in the folder (tests, builds, git status). It stops after two minutes.",
            json!({"command": text("The command line")}),
            &["command"],
        ));
    }
    if web {
        out.push(tool(
            "web_search",
            "Search the web. Gives titles, addresses and a line of text.",
            json!({"query": text("Words to search for")}),
            &["query"],
        ));
        out.push(tool(
            "fetch_url",
            "Read the text of one public web page.",
            json!({"url": text("Address starting with https://")}),
            &["url"],
        ));
    }
    if with_skills {
        out.push(tool("use_skill", "Get the instructions of a skill by name.", json!({"name": text("Name of the skill")}), &["name"]));
    }
    out.extend(extra::tool_defs());
    out.push(tool(
        "finish",
        "End the task with a summary for the person.",
        json!({"summary": text("What was done or found")}),
        &["summary"],
    ));
    out
}

/// An engine error that waiting does not cure.
fn lasting(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("not installed") || e.contains("no model file") || e.contains("no chat model")
}

/// What to do with an answer that has text and no tool call.
#[derive(Debug, PartialEq)]
enum Plain {
    /// It is the answer: the request is done.
    Answer,
    /// Send it back with this line; the work goes on.
    Back(String),
}

/// A small model often stops in the middle: it writes nothing, its answer is cut off, or it
/// says what it will do next and calls no tool. Each of these is sent back with a short line,
/// at most `MAX_SEND_BACKS` times in a row. A question to the person is always an answer.
fn plain_answer(text: &str, cut_off: bool, plan: &[plan::PlanStep], sent_back: usize) -> Plain {
    if sent_back >= MAX_SEND_BACKS {
        return Plain::Answer;
    }
    if text.is_empty() {
        return Plain::Back("You answered with nothing. Call a tool to go on, or call finish with a summary of what you did.".into());
    }
    if cut_off {
        return Plain::Back("Your answer was cut off. Go on exactly where it stopped. Do not repeat what you already wrote.".into());
    }
    if !plan.is_empty() && !plan::finished(plan) && !text.trim_end().ends_with('?') {
        return Plain::Back(format!(
            "Your plan still has steps to do:\n{}\nDo not stop here: go on with the next step using the tools. If the work is really done, call finish.",
            plan::brief(plan)
        ));
    }
    Plain::Answer
}

/// What `finish` answers while the plan still has steps to do: the work is not ended, the
/// model is told what is left. `None` when the plan is done or there is none.
fn not_finished(plan: &[plan::PlanStep]) -> Option<String> {
    (!plan.is_empty() && !plan::finished(plan)).then(|| {
        format!(
            "Not finished: your plan still has steps to do:\n{}\nDo them now. If they are really done, mark them with update_plan and call finish again.",
            plan::brief(plan)
        )
    })
}

/// A request is long work once it took this many turns.
const LONG_WORK: usize = 10;

/// The files a request names that are not in the folder: `tests/test_cli.py`, `README.md`. Only
/// what plainly is a file name counts (a path with a slash, or a lower-case name with an
/// ending), so "Node.js" and "example.com" are not taken for one.
fn named_but_missing(request: &str, root: &FsPath) -> Vec<String> {
    static NAME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let name = NAME.get_or_init(|| regex::Regex::new(r"[A-Za-z0-9_][A-Za-z0-9_./-]*\.[A-Za-z][A-Za-z0-9]{0,5}").expect("static pattern"));
    const NOT_FILES: &[&str] = &["com", "org", "net", "io", "dev", "app", "ai", "co", "html"];
    const DOCS: &[&str] = &["README", "LICENSE", "CHANGELOG", "CONTRIBUTING", "Makefile", "Dockerfile"];
    let mut out: Vec<String> = Vec::new();
    for found in name.find_iter(request) {
        let before = &request[..found.start()];
        let token = found.as_str().trim_end_matches('.');
        let Some((stem, ending)) = token.rsplit_once('.') else { continue };
        let file_like = token.contains('/')
            || DOCS.contains(&stem)
            || (stem.len() >= 2 && stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'));
        if !file_like
            || token.contains("..")
            || token.contains("//")
            || before.ends_with('/')
            || before.ends_with('.')
            || before.ends_with('@')
            || (NOT_FILES.contains(&ending) && !token.contains('/'))
        {
            continue;
        }
        if !root.join(token).exists() && !out.iter().any(|o| o == token) {
            out.push(token.to_string());
        }
        if out.len() == 12 {
            break;
        }
    }
    out
}

/// After long work that changed files, the first `finish` is answered with the request itself,
/// word for word, and with the files it names that are not there: by then a small model has
/// often lost a part of what was asked. `None` for short work, or when nothing was changed.
fn last_look(steps: &[Step], turns: usize, root: &FsPath) -> Option<String> {
    let start = steps.iter().rposition(|s| matches!(s, Step::User { .. }))?;
    let Step::User { text } = &steps[start] else { return None };
    let changed = steps[start..]
        .iter()
        .any(|s| matches!(s, Step::Tool { name, status, .. } if status == "done" && pace::kind_of(name) == pace::Kind::Change));
    (turns >= LONG_WORK && changed).then(|| {
        let missing = named_but_missing(text, root);
        let missing = if missing.is_empty() {
            String::new()
        } else {
            format!("These files are named there and are not in the folder: {}. Write them now if they were asked for.\n\n", missing.join(", "))
        };
        format!(
            "Before you finish, here is what the person asked for, word for word:\n\n{text}\n\n{missing}Go through it point by point against what you did. If something is missing, do it now. If everything is there, call finish again."
        )
    })
}

fn weight(message: &Value) -> usize {
    let content = message["content"].as_str().map_or(0, |s| s.chars().count());
    let calls =
        message["tool_calls"].as_array().map_or(0, |c| c.iter().map(|x| x["function"]["arguments"].as_str().map_or(0, str::len)).sum());
    content + calls
}

const LEFT_OUT: &str = "[left out to save space; call the tool again if you need it]";

/// Makes one message small: a tool result becomes a note, long arguments of a call are dropped.
fn shrink(message: &mut Value) {
    if message["role"] == "tool" {
        if message["content"].as_str().is_some_and(|c| c.chars().count() > 160) {
            message["content"] = json!(LEFT_OUT);
        }
        return;
    }
    if let Some(calls) = message["tool_calls"].as_array_mut() {
        for call in calls {
            let Some(args) = call["function"]["arguments"].as_str().filter(|a| a.len() > 300) else { continue };
            let mut parsed: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
            if let Some(o) = parsed.as_object_mut() {
                for v in o.values_mut() {
                    if v.as_str().is_some_and(|s| s.len() > 200) {
                        *v = json!("[left out]");
                    }
                }
            }
            call["function"]["arguments"] = json!(parsed.to_string());
        }
    }
}

/// The last resort after `recall::compact`, for a newest part that is too long by itself: when
/// the conversation is longer than `budget` characters, the oldest tool results and long call
/// arguments are left out first. What the person wrote always stays, and so does the last message.
fn fit(messages: &[Value], budget: usize) -> Vec<Value> {
    let mut out = messages.to_vec();
    let mut total: usize = out.iter().map(weight).sum();
    let last = out.len().saturating_sub(1);
    for (i, message) in out.iter_mut().enumerate() {
        if total <= budget || i >= last {
            break;
        }
        if message["role"] == "user" {
            continue;
        }
        let before = weight(message);
        shrink(message);
        total -= before.saturating_sub(weight(message));
    }
    out
}

/// A call the model made must be followed by its result. A task that was stopped in the middle
/// can lack some; they are filled in so the next request is a valid conversation.
fn repair(messages: &mut Vec<Value>) {
    let mut out: Vec<Value> = Vec::with_capacity(messages.len());
    let mut open: Vec<String> = Vec::new();
    let close = |out: &mut Vec<Value>, open: &mut Vec<String>| {
        for id in open.drain(..) {
            out.push(json!({"role": "tool", "tool_call_id": id, "content": "This did not run: the task was stopped."}));
        }
    };
    for message in messages.drain(..) {
        if message["role"] == "tool" {
            let id = message["tool_call_id"].as_str().unwrap_or("");
            if let Some(pos) = open.iter().position(|o| o == id) {
                open.remove(pos);
                out.push(message);
            }
            continue;
        }
        close(&mut out, &mut open);
        if let Some(calls) = message["tool_calls"].as_array() {
            open = calls.iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect();
        }
        out.push(message);
    }
    close(&mut out, &mut open);
    *messages = out;
}

// --- the loop --------------------------------------------------------------------------------

/// How a request ended: the task's new state and, when it did not simply finish, why.
struct Ended {
    state: &'static str,
    error: Option<String>,
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args[key].as_str()
}

/// A whole number argument; small models often send it as text.
fn arg_usize(args: &Value, key: &str) -> Option<usize> {
    args[key].as_u64().or_else(|| args[key].as_str().and_then(|s| s.trim().parse().ok())).map(|n| n as usize)
}

const KNOWN_TOOLS: &[&str] = &[
    "list_files",
    "read_file",
    "search",
    "edit_file",
    "write_file",
    "append_file",
    "run_command",
    "web_search",
    "fetch_url",
    "use_skill",
    "finish",
];

/// Whether a name is one of the service's own tools (connector tools are not).
fn known(name: &str) -> bool {
    KNOWN_TOOLS.contains(&name) || extra::TOOLS.contains(&name) || see::TOOLS.contains(&name) || film::TOOLS.contains(&name)
}

const DENIED: &str = "The person did not allow this. Do not try it again. Go on without it, or say what you could not do.";
const DENIED_TWICE: &str =
    "The person said no to this kind of step twice. It was not asked again. Finish without it and say what you could not do.";
const DENIED_BEFORE: &str =
    "The person already said no to exactly this. It was not asked again. Go on without it and say what you could not do.";

/// Whether the person already said no twice to this tool since their last message.
fn refused_twice(denied: &[String], tool: &str) -> bool {
    denied.iter().filter(|d| d.split(' ').next() == Some(tool)).count() >= 2
}

/// Waits for the person's answer to one tool call.
async fn ask(app: &Shared, id: &str, pending: Pending, signature: &str) -> Decision {
    let agents = &app.agent;
    let (tx, rx) = oneshot::channel();
    let (call, tool) = (pending.call.clone(), pending.tool.clone());
    if let Some(r) = agents.running.lock().unwrap().get_mut(id) {
        r.approve = Some(tx);
    }
    agents.change(id, |t| {
        t.set_tool(&call, "waiting", None, pending.diff.clone());
        t.pending = Some(pending);
        t.state = "waiting".into();
    });
    let decision = rx.await.unwrap_or(Decision { allow: false, always: false });
    agents.change(id, |t| {
        t.pending = None;
        t.state = "running".into();
        t.set_tool(&call, if decision.allow { "running" } else { "denied" }, None, None);
        if !decision.allow {
            t.denied.push(signature.to_string());
        }
    });
    app.record(
        "agent_approval",
        json!({"tool": tool, "decision": if decision.allow { "allow" } else { "deny" }, "always": decision.always}),
    );
    decision
}

/// Runs one tool call and returns (result for the model, whether it worked, step status).
async fn call_tool(app: &Shared, id: &str, folder: &Folder, call: &str, name: &str, args: &Value) -> (String, &'static str) {
    let agents = &app.agent;
    let Some(task) = agents.get(id) else { return ("The task is gone.".into(), "failed") };
    if let Some(out) = skills::dispatch(app, folder.root(), name, args) {
        return out;
    }
    if let Some(out) = mcp::dispatch(app, id, call, name, args).await {
        return out;
    }
    let offered = match name {
        "edit_file" | "write_file" | "append_file" | "run_command" => task.mode != Mode::ReadOnly,
        "web_search" | "fetch_url" => task.web,
        _ if see::TOOLS.contains(&name) => see::offered(name, task.mode, folder.root()),
        _ => KNOWN_TOOLS.contains(&name) || extra::TOOLS.contains(&name),
    };
    if !offered {
        return (format!("There is no tool named {name} in this task."), "failed");
    }
    let done = |r: Result<String, String>| match r {
        Ok(text) => (text, "done"),
        Err(text) => (text, "failed"),
    };
    let blocking = |f: Box<dyn FnOnce() -> Result<String, String> + Send>| async move {
        tokio::task::spawn_blocking(f).await.unwrap_or_else(|_| Err("That did not work.".into()))
    };
    let missing = |key: &str| (format!("\"{key}\" is needed."), "failed");
    let signature = format!("{name} {args}");
    if task.denied.contains(&signature) {
        return (DENIED_BEFORE.into(), "denied");
    }
    // Two refusals of the same kind of step: a small model keeps trying variations, and the
    // person should not have to say no a third time.
    if refused_twice(&task.denied, name) {
        return (DENIED_TWICE.into(), "denied");
    }
    match name {
        "list_files" => {
            let (f, path, depth) =
                (folder.clone(), arg_str(args, "path").unwrap_or(".").to_string(), arg_usize(args, "depth").unwrap_or(2));
            done(blocking(Box::new(move || files::list(&f, &path, depth))).await)
        }
        "read_file" => {
            let Some(path) = arg_str(args, "path").map(str::to_string) else { return missing("path") };
            let (f, from, lines) = (folder.clone(), arg_usize(args, "from_line"), arg_usize(args, "lines"));
            done(blocking(Box::new(move || files::read(&f, &path, from, lines))).await)
        }
        "search" => {
            let Some(pattern) = arg_str(args, "pattern").map(str::to_string) else { return missing("pattern") };
            let (f, path) = (folder.clone(), arg_str(args, "path").map(str::to_string));
            done(blocking(Box::new(move || files::search(&f, &pattern, path.as_deref()))).await)
        }
        "edit_file" | "write_file" | "append_file" => {
            let Some(path) = arg_str(args, "path") else { return missing("path") };
            let plan = if name == "edit_file" {
                let (Some(old), Some(new)) = (arg_str(args, "old"), arg_str(args, "new")) else { return missing("old\" and \"new") };
                files::plan_edit(folder, path, old, new)
            } else if name == "append_file" {
                let Some(content) = arg_str(args, "content") else { return missing("content") };
                // What is there now plus the new part, as one change the person can read.
                let before = folder.existing(path).ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
                files::plan_write(folder, path, &format!("{before}{content}"))
            } else {
                let Some(content) = arg_str(args, "content") else { return missing("content") };
                files::plan_write(folder, path, content)
            };
            let change = match plan {
                Ok(c) => c,
                Err(e) => return (e, "failed"),
            };
            let diff = change.diff();
            if task.mode == Mode::Ask {
                let pending =
                    Pending { call: call.into(), tool: name.into(), path: Some(change.shown.clone()), command: None, diff: Some(diff) };
                let decision = ask(app, id, pending, &signature).await;
                if !decision.allow {
                    return (DENIED.into(), "denied");
                }
                if decision.always {
                    // "Always allow" on a file change: the rest of this task edits without asking.
                    agents.change(id, |t| t.mode = Mode::AutoEdits);
                }
            } else {
                agents.change(id, |t| t.set_tool(call, "running", None, Some(diff)));
            }
            undo::before_change(app, id, folder, Some(&change.shown)).await;
            verify::after_change(folder.root(), done(change.apply(folder)))
        }
        "run_command" => {
            let Some(command) = arg_str(args, "command").map(str::trim).filter(|c| !c.is_empty()) else { return missing("command") };
            if see::offered("start_app", task.mode, folder.root()) && apps::serves(command) {
                return (apps::SERVES.into(), "failed");
            }
            if task.mode != Mode::Bypass && !task.always.iter().any(|c| c == command) {
                let pending = Pending { call: call.into(), tool: name.into(), path: None, command: Some(command.to_string()), diff: None };
                let decision = ask(app, id, pending, &signature).await;
                if !decision.allow {
                    return (DENIED.into(), "denied");
                }
                if decision.always {
                    agents.change(id, |t| t.always.push(command.to_string()));
                }
            }
            undo::before_change(app, id, folder, None).await;
            let ran = files::run(folder, command, COMMAND_TIMEOUT, apps::net_of(app, &task)).await;
            (checks::digest(&ran, folder.root()), if ran.ok { "done" } else { "failed" })
        }
        "web_search" => {
            let Some(query) = arg_str(args, "query") else { return missing("query") };
            done(web::search(&app.server, &app.net, query).await)
        }
        "fetch_url" => {
            let Some(url) = arg_str(args, "url") else { return missing("url") };
            done(web::fetch_url(url).await)
        }
        "use_skill" => {
            let wanted = arg_str(args, "name").unwrap_or("").trim().to_string();
            let all = skills::load(&app.root, Some(folder.root()));
            match all.iter().find(|s| s.name == wanted) {
                Some(s) => (skills::used(app, s, "code"), "done"),
                None => (
                    format!(
                        "There is no skill named {wanted}. The skills are: {}.",
                        all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                    "failed",
                ),
            }
        }
        _ if extra::TOOLS.contains(&name) => extra::call(app, id, folder, name, args).await,
        _ if see::TOOLS.contains(&name) => see::call(app, id, folder, call, name, args).await,
        _ => (format!("There is no tool named {name}."), "failed"),
    }
}

/// Works on the task until the model answers or finishes, or until it has a question for the
/// person. There is no limit on the steps: the model is on this computer, and a task takes as
/// long as it takes. What stops a task that goes round in circles is `pace`.
async fn work(app: &Shared, id: &str) -> Ended {
    let agents = &app.agent;
    let failed = |why: String| Ended { state: "failed", error: Some(why) };
    let Some(task) = agents.get(id) else { return failed("The task is gone.".into()) };
    let kit = match kit::Kit::open(app, &task) {
        Ok(k) => k,
        Err(e) => return failed(e),
    };
    // A model from the manifest, or one the person added from the community.
    let Some(package) = app.package(&task.model).filter(|p| p.modality == Modality::Chat) else {
        return failed(format!("There is no chat model named {}.", task.model));
    };
    let package = &package;
    agents.change(id, |t| repair(&mut t.messages));
    let writing = agents.running.lock().unwrap().get(id).map(|r| r.writing.clone()).unwrap_or_default();
    let context = crate::context_of(app, package);
    let mut budget = history_chars(context);
    // A tool call too long for one answer comes back from the engine as an error, not as a call.
    let mut cut_offs = 0;
    // Engine errors in a row, and answers without a tool call that were sent back in a row.
    let (mut errors, mut sent_back) = (0, 0);
    // How often `finish` was answered with what is still to do. Counted for the whole request.
    let mut held_open = 0;
    // Whether the request was read back to the model once before it finished.
    let mut looked_back = false;
    loop {
        // What the person sent while the last step ran joins the conversation now.
        for skill in agents.change(id, compose::deliver).flatten().unwrap_or_default() {
            read_skill(app, id, kit.folder().root(), &skill);
        }
        // What no longer fits leaves the conversation and stays as notes.
        agents.change(id, |t| recall::compact(&mut t.messages, &mut t.memory, budget));
        let Some(task) = agents.get(id) else { return failed("The task is gone.".into()) };
        let (system, mut tools) = kit.turn(app, &task);
        let mut messages = vec![json!({"role": "system", "content": system})];
        let sees = see::sees(app, &task);
        let read = |name: &str| look::read_shot(&agents.dir, id, name);
        let conversation = extra::last_thought_only(fit(&recall::conversation(&task.memory, &task.messages), budget));
        let mut sent = compose::with_pictures(conversation, sees, read);
        // The newest picture goes with the result of the step that took it, for a few turns,
        // when the model takes pictures at all.
        if let Some(eye) = task.extra.eye.as_ref().filter(|e| !e.call.is_empty() && e.sends < look::EYE_SENDS && sees) {
            if let Some(png) = look::read_shot(&agents.dir, id, &eye.shot) {
                sent = look::with_picture(sent, eye, &png);
                agents.change(id, |t| {
                    if let Some(eye) = t.extra.eye.as_mut() {
                        eye.sends += 1;
                    }
                });
            }
        }
        messages.extend(sent);
        // A tool that is used up for this request is not offered again until the next one.
        tools.retain(|t| !t["function"]["name"].as_str().is_some_and(|name| task.pace.closed(name)));
        let body = json!({
            "model": package.id,
            "messages": messages,
            "tools": tools,
            "parallel_tool_calls": false,
            "stream": true,
            "temperature": 0.2,
            "max_tokens": answer_tokens(context),
            "chat_template_kwargs": {"enable_thinking": extra::thinks(context), "reasoning_effort": extra::effort(context)},
        });
        let answer = crate::complete(app, package, body, &mut |so_far| writing.hear(so_far)).await;
        writing.clear();
        let answer = match answer {
            Ok(v) => v,
            // Too much for the model's context: send less of the history, once.
            Err(e) if e.to_lowercase().contains("context") && budget > history_chars(context) / 4 => {
                budget /= 2;
                continue;
            }
            // The model wrote a tool call longer than one answer may be (a whole file at once): the
            // engine could not read it. Say so and let it try again in smaller parts.
            Err(e) if cut_off_call(&e) && cut_offs < MAX_CUT_OFFS => {
                cut_offs += 1;
                app.record("agent_call_cut_off", json!({"package": package.id}));
                agents.change(id, |t| {
                    t.messages.push(recall::hint(CUT_OFF_HINT));
                    t.steps.push(Step::Note { text: "Too long for one go. Writing it in parts.".into() });
                });
                continue;
            }
            // The engine would not take the picture: the work goes on without it.
            Err(e) if task.extra.eye.is_some() && look::picture_refused(&e) => {
                app.record("agent_picture_refused", json!({"package": package.id}));
                agents.change(id, |t| t.extra.eye = None);
                continue;
            }
            // Anything else may pass: the model is loading again, the engine was busy or went
            // away. Wait a little and ask again; the conversation is as it was.
            Err(e) if errors < RETRY_WAITS.len() && !lasting(&e) => {
                app.record("agent_engine_retry", json!({"package": package.id, "try": errors + 1}));
                tokio::time::sleep(RETRY_WAITS[errors]).await;
                errors += 1;
                continue;
            }
            Err(e) => return failed(e),
        };
        errors = 0;
        agents.change(id, |t| {
            t.turns += 1;
            extra::heard(t, &answer, context);
        });
        let message = &answer["choices"][0]["message"];
        let cut_off = answer["choices"][0]["finish_reason"] == "length";
        let text = message["content"].as_str().unwrap_or("");
        let text = text.rsplit_once("</think>").map_or(text, |(_, after)| after).trim().to_string();
        let calls: Vec<Value> = message["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            // Text and no tool call: that is the answer, unless the model plainly stopped halfway.
            let mut back = match plain_answer(&text, cut_off, &task.extra.plan, sent_back) {
                Plain::Answer => None,
                Plain::Back(line) => Some(line),
            };
            // The same last look as before `finish`, unless the text is a question to the person.
            if back.is_none() && !looked_back && !text.is_empty() && !text.trim_end().ends_with('?') {
                looked_back = true;
                back = kit.before_finish(&task);
            }
            agents.change(id, |t| {
                if !text.is_empty() {
                    t.messages.push(json!({"role": "assistant", "content": text}));
                    t.steps.push(Step::Model { text: text.clone() });
                } else if back.is_none() {
                    t.messages.push(json!({"role": "assistant", "content": ""}));
                    t.steps.push(Step::Note { text: "No answer.".into() });
                }
                match &back {
                    Some(line) => t.messages.push(recall::hint(line)),
                    None => kit.answered(t, &text),
                }
            });
            if back.is_some() {
                sent_back += 1;
                app.record("agent_sent_back", json!({"package": package.id}));
                continue;
            }
            return Ended { state: "done", error: None };
        }
        sent_back = 0;
        // Every call gets an id the results can point to.
        let calls: Vec<Value> = calls
            .into_iter()
            .map(|mut c| {
                if c["id"].as_str().is_none_or(str::is_empty) {
                    c["id"] = json!(uuid::Uuid::new_v4().simple().to_string());
                }
                json!({"id": c["id"], "type": "function", "function": {"name": c["function"]["name"], "arguments": c["function"]["arguments"].as_str().unwrap_or("{}")}})
            })
            .collect();
        agents.change(id, |t| {
            let mut said = json!({"role": "assistant", "content": text, "tool_calls": calls});
            if let Some(thought) = extra::kept_thought(&answer, context) {
                said["reasoning_content"] = json!(thought);
            }
            t.messages.push(said);
            if !text.is_empty() {
                t.steps.push(Step::Model { text: text.clone() });
            }
        });
        for c in &calls {
            let model_id = c["id"].as_str().unwrap_or("").to_string();
            let name = c["function"]["name"].as_str().unwrap_or("").to_string();
            let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
            let parsed: Option<Value> = serde_json::from_str::<Value>(raw).ok().filter(Value::is_object);
            let args = parsed.clone().unwrap_or_else(|| json!({}));
            app.record("agent_tool_called", json!({"tool": if known(&name) { name.as_str() } else { "unknown" }}));
            if name == "finish" {
                // Finishing with steps of its own plan undone: a small model does that. It is
                // told what is left, twice at most; after that its word is taken.
                let now = agents.get(id);
                let mut left = now.as_ref().and_then(|t| not_finished(&t.extra.plan)).filter(|_| held_open < MAX_SEND_BACKS);
                if left.is_none() && !looked_back {
                    looked_back = true;
                    left = now.as_ref().and_then(|t| kit.before_finish(t));
                } else if left.is_some() {
                    held_open += 1;
                }
                if let Some(left) = left {
                    app.record("agent_sent_back", json!({"package": package.id}));
                    agents.change(id, |t| t.messages.push(json!({"role": "tool", "tool_call_id": model_id, "content": left})));
                    continue;
                }
                let summary = arg_str(&args, "summary").unwrap_or("").trim().to_string();
                let said = format!("{text}\n{summary}");
                agents.change(id, |t| {
                    t.messages.push(json!({"role": "tool", "tool_call_id": model_id, "content": "Finished."}));
                    if !summary.is_empty() && summary != text {
                        t.steps.push(Step::Model { text: summary });
                    } else if text.is_empty() {
                        t.steps.push(Step::Note { text: "Finished.".into() });
                    }
                    kit.answered(t, &said);
                });
                return Ended { state: "done", error: None };
            }
            let call = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            agents.change(id, |t| {
                t.steps.push(Step::Tool {
                    call: call.clone(),
                    name: name.clone(),
                    arguments: args.clone(),
                    status: "running".into(),
                    result: None,
                    diff: None,
                    image: None,
                })
            });
            // A search already made, a budget used up, the same call a third time: answered here.
            let held = if parsed.is_some() { agents.change(id, |t| t.pace.before(&name, &args)).flatten() } else { None };
            let (result, status) = if parsed.is_none() {
                let why = if cut_off { CUT_OFF_HINT } else { "The arguments were not valid JSON. Call the tool again with a JSON object." };
                (why.to_string(), "failed")
            } else if let Some(held) = &held {
                (held.text.clone(), if held.refused { "failed" } else { "done" })
            } else {
                kit.call(app, id, &call, &name, &args).await
            };
            // Is the work going round in circles? A nudge goes to the model with the result; a
            // question goes to the person.
            let verdict =
                agents.change(id, |t| t.pace.after(&name, &args, &result, status, held.as_ref().map(|h| h.refused))).unwrap_or_default();
            let result = match &verdict.nudge {
                Some(nudge) => format!("{result}\n\n{nudge}"),
                None => result,
            };
            agents.change(id, |t| {
                t.set_tool(&call, status, Some(result.clone()), None);
                t.messages.push(json!({"role": "tool", "tool_call_id": model_id, "content": result}));
                // A picture this step took is sent after this result.
                if let Some(eye) = t.extra.eye.as_mut().filter(|e| e.step == call) {
                    eye.call = model_id.clone();
                }
            });
            if let Some(question) = verdict.ask {
                app.record("agent_asked_person", json!({"tool": if known(&name) { name.as_str() } else { "unknown" }}));
                agents.change(id, |t| {
                    t.messages.push(json!({"role": "assistant", "content": question}));
                    t.steps.push(Step::Question { text: question.clone() });
                });
                return Ended { state: "asking", error: None };
            }
        }
    }
}

/// Starts (or continues) the work on a task in the background.
fn start(app: &Shared, id: &str, follow_up: bool) {
    let agents = &app.agent;
    if let Some(task) = agents.get(id) {
        app.record(
            "agent_task_started",
            json!({"package": task.model, "mode": task.mode, "web": task.web, "follow_up": follow_up, "film": !task.extra.film.is_empty()}),
        );
    }
    // Held while the work is registered, so work that ends at once still finds its entry.
    let mut running = agents.running.lock().unwrap();
    let (task_app, task_id) = (app.clone(), id.to_string());
    let handle = tokio::spawn(async move {
        let mut ended = work(&task_app, &task_id).await;
        // Messages the person sent while it worked are the next request.
        while matches!(ended.state, "done" | "asking") && task_app.agent.change(&task_id, next_from_queue).unwrap_or(false) {
            ended = work(&task_app, &task_id).await;
        }
        // A request that failed leaves no app behind; one that is done keeps it for the person to see.
        if ended.state == "failed" {
            task_app.agent.apps.stop(&task_id);
        }
        task_app.agent.change(&task_id, |t| {
            t.settle(ended.state);
            t.error = ended.error;
        });
        finished(&task_app, &task_id);
    });
    running.insert(
        id.to_string(),
        Running { abort: handle.abort_handle(), started: Instant::now(), approve: None, writing: Default::default() },
    );
    drop(running);
    if !follow_up {
        tokio::spawn(name(app.clone(), id.to_string()));
    }
}

/// What a new task is named after: its request and, when the model has said something by then,
/// the start of that. `None` until the first turn is over or the task has ended.
fn named_after(task: &Task) -> Option<(String, String)> {
    let request = task.steps.iter().find_map(|s| match s {
        Step::User { text } => Some(text.clone()),
        _ => None,
    })?;
    if task.active() && task.turns == 0 {
        return None;
    }
    let said = task.steps.iter().find_map(|s| match s {
        Step::Model { text } | Step::Question { text } if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    });
    let plan = task.extra.plan.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n");
    Some((request, said.unwrap_or(plan)))
}

/// Sets a title the model wrote, unless the person has named the task since.
fn retitle(task: &mut Task, title: String) -> bool {
    if task.title_by == "person" || title.trim().is_empty() {
        return false;
    }
    task.title = title;
    task.title_by = "model".into();
    true
}

/// Names a new task once its first turn is over: the chat model is in memory by then. The work
/// does not wait for this, and a task that never got a turn keeps the start of its request.
async fn name(app: Shared, id: String) {
    let mut about = None;
    // A first turn can take minutes on a slow computer (the model is loaded for it).
    for _ in 0..1800 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let Some(task) = app.agent.get(&id) else { return };
        about = named_after(&task);
        if about.is_some() {
            break;
        }
    }
    let Some((request, said)) = about else { return };
    let Some(task) = app.agent.get(&id) else { return };
    let (title, by) = crate::title::write(&app, &request, &said, Some(&task.model), false).await;
    if by == "model" {
        app.agent.change(&id, |t| retitle(t, title));
    }
}

/// Starts the next request of the person: their message is in the conversation by now.
fn begin_request(t: &mut Task) {
    t.state = "running".into();
    t.request_ms = now_ms();
    t.error = None;
    t.turns = 0;
    t.denied.clear();
    t.pace.next_request();
    extra::next_request(t);
}

/// When the work ended with messages waiting: the next request starts with them.
fn next_from_queue(t: &mut Task) -> bool {
    if t.extra.queued.is_empty() {
        return false;
    }
    begin_request(t);
    true
}

/// A skill a command named, read for the model as if it had called `use_skill` itself: the step
/// shows it, and its instructions are in the conversation before the work starts.
fn read_skill(app: &Shared, id: &str, root: &FsPath, name: &str) {
    let Some(skill) = skills::load(&app.root, Some(root)).into_iter().find(|s| s.name == name) else { return };
    let text = skills::used(app, &skill, "code");
    let call = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let args = json!({"name": name});
    app.agent.change(id, |t| {
        t.steps.push(Step::Tool {
            call: call.clone(),
            name: "use_skill".into(),
            arguments: args.clone(),
            status: "done".into(),
            result: Some(text.clone()),
            diff: None,
            image: None,
        });
        t.messages.push(json!({"role": "assistant", "content": "", "tool_calls": [{"id": call, "type": "function", "function": {"name": "use_skill", "arguments": args.to_string()}}]}));
        t.messages.push(json!({"role": "tool", "tool_call_id": call, "content": text}));
    });
}

/// Characters one message of the person may bring along (files named with `@`, the change for
/// `/review`) for a model with `context` tokens: a third of the conversation it is sent.
fn room(context: u32) -> usize {
    history_chars(context) / 3
}

/// What the person sent, as the model will read it: a command expanded, the files named with
/// `@` brought along, as much as the model's context has room for. `Err` is said to the person,
/// and nothing is sent.
fn understood(app: &Shared, folder: &Folder, model: &str, text: &str) -> Result<(String, Option<String>), String> {
    let skills = skills::load(&app.root, Some(folder.root()));
    let room = room(app.package(model).map_or(8192, |p| crate::context_of(app, &p)));
    let (asked, skill) = match project::expand(text, folder.root(), &skills, room) {
        Some(Ok(e)) => (e.text, e.skill),
        Some(Err(why)) => return Err(why),
        None => (text.to_string(), None),
    };
    Ok((compose::with_mentions(&asked, folder, room), skill))
}

/// Takes the task out of the running ones and records how it went.
fn finished(app: &Shared, id: &str) -> bool {
    let Some(was) = app.agent.running.lock().unwrap().remove(id) else { return false };
    if let Some(task) = app.agent.get(id) {
        let tools = task.steps.iter().filter(|s| matches!(s, Step::Tool { .. })).count();
        app.record(
            "agent_task_finished",
            json!({"state": task.state, "steps": task.turns, "tools": tools, "searches": task.pace.searches(), "seconds": was.started.elapsed().as_secs_f64()}),
        );
    }
    true
}

// --- the API -----------------------------------------------------------------------------------

fn not_found() -> Response {
    fail(StatusCode::NOT_FOUND, "no such task")
}

#[derive(Deserialize)]
pub struct NewTask {
    /// The folder of a task of Code.
    #[serde(default)]
    folder: String,
    /// The film of a task of Studio's assistant, by its id: then no folder is given.
    #[serde(default)]
    film: Option<String>,
    /// For a film: the shots selected in the tracks, by id, which the request is about.
    #[serde(default)]
    selection: Vec<String>,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    mode: Mode,
    /// "Allow web search": offer `web_search` and `fetch_url`.
    #[serde(default)]
    web: bool,
    /// "Use connectors": offer `find_tool` and `call_tool`.
    #[serde(default)]
    connectors: bool,
    /// Pictures that go with the request: PNG, base64.
    #[serde(default)]
    images: Vec<String>,
}

pub async fn task_create(State(app): State<Shared>, Json(body): Json<NewTask>) -> Response {
    let prompt = body.prompt.trim();
    if prompt.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "Say what you would like done.");
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return fail(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("That is too long for the model. Keep it under {MAX_PROMPT_CHARS} characters."),
        );
    }
    let film = body.film.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(str::to_string);
    let folder = match &film {
        // A film works with the film tools alone: no file of its folder is read or written.
        Some(film) => match crate::studio::assist::folder_of(&app, film).map(|f| Folder::open(&f)) {
            Some(Ok(f)) => f,
            Some(Err(e)) => return fail(StatusCode::BAD_REQUEST, e),
            None => return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone"),
        },
        None => match Folder::open(&body.folder) {
            Ok(f) => f,
            Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        },
    };
    // Rigbrew's own files (the token of this service among them) are never part of a task.
    let own = std::fs::canonicalize(&app.root).unwrap_or_else(|_| app.root.clone());
    if film.is_none() && (own.starts_with(folder.root()) || folder.root().starts_with(&own)) {
        return fail(StatusCode::BAD_REQUEST, "That folder holds Rigbrew's own files. Choose the folder of one project.");
    }
    let all = app.all_packages();
    let chat_models = || all.iter().filter(|p| p.modality == Modality::Chat && !app.installed(p).is_empty());
    let package = match &body.model {
        Some(wanted) => chat_models().find(|p| p.id == *wanted),
        None => chat_models().next(),
    };
    let Some(package) = package else {
        let who = if film.is_some() { "The assistant" } else { "Code" };
        return fail(StatusCode::CONFLICT, format!("{who} needs a chat model. Install one under Models first."));
    };
    if app.agent.running.lock().unwrap().len() >= MAX_RUNNING {
        return fail(StatusCode::TOO_MANY_REQUESTS, format!("{MAX_RUNNING} tasks are already running. Stop one, or wait for it."));
    }
    // What the model is given: for Code, the request with what it names; for a film, with the
    // shots selected in the tracks.
    let (sent, skill, note) = match &film {
        Some(film) => {
            let (said, note) = film::request(&app, film, prompt, &body.selection);
            (said, None, note)
        }
        None => match understood(&app, &folder, &package.id, prompt) {
            Ok((sent, skill)) => (sent, skill, None),
            Err(why) => return fail(StatusCode::BAD_REQUEST, why),
        },
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let images = match compose::keep_pictures(&app.agent.dir, &id, &body.images) {
        Ok(names) => names,
        Err(why) => return fail(StatusCode::BAD_REQUEST, why),
    };
    let now = now_ms();
    let mut task = Task {
        id,
        folder: folder.root().to_string_lossy().into_owned(),
        title: prompt.lines().next().unwrap_or("").chars().take(80).collect(),
        title_by: "words".into(),
        model: package.id.clone(),
        mode: body.mode,
        web: body.web,
        state: "running".into(),
        created_ms: now,
        updated_ms: now,
        request_ms: now,
        steps: Vec::new(),
        pending: None,
        error: None,
        always: Vec::new(),
        denied: Vec::new(),
        turns: 0,
        messages: Vec::new(),
        extra: extra::Extra { film: film.clone().unwrap_or_default(), ..Default::default() },
        mcp: mcp::TaskState::new(body.connectors),
        pace: Default::default(),
        memory: Default::default(),
    };
    compose::add(&mut task, prompt, &sent, &images);
    task.steps.extend(note.map(|text| Step::Note { text }));
    let id = task.id.clone();
    app.agent.insert(task);
    if let Some(skill) = skill {
        read_skill(&app, &id, folder.root(), &skill);
    }
    let view = app.agent.get(&id).map(|t| t.view()).unwrap_or_default();
    start(&app, &id, false);
    Json(json!({"task": view})).into_response()
}

#[derive(Deserialize, Default)]
pub struct FolderQuery {
    folder: Option<String>,
    /// The conversations of Studio's assistant on one film. Without it, only the tasks of Code.
    #[serde(default)]
    film: Option<String>,
}

pub async fn tasks(State(app): State<Shared>, Query(q): Query<FolderQuery>) -> Json<Value> {
    // The folder as tasks store it: with links followed.
    let wanted = q
        .folder
        .filter(|f| !f.trim().is_empty())
        .map(|f| std::fs::canonicalize(f.trim()).map(|p| p.to_string_lossy().into_owned()).unwrap_or(f));
    let film = q.film.unwrap_or_default();
    let tasks = app.agent.tasks.lock().unwrap();
    let mut list: Vec<&Task> =
        tasks.values().filter(|t| t.extra.film == film.trim() && wanted.as_ref().is_none_or(|w| *w == t.folder)).collect();
    list.sort_by_key(|t| std::cmp::Reverse(t.created_ms));
    Json(json!({"tasks": list.iter().map(|t| t.summary()).collect::<Vec<_>>()}))
}

pub async fn task_get(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<stream::Shown>) -> Response {
    match app.agent.get(&id) {
        Some(task) => Json(json!({"task": stream::shown(&app, &task, &q)})).into_response(),
        None => not_found(),
    }
}

#[derive(Deserialize)]
pub struct Approval {
    call: String,
    allow: bool,
    #[serde(default)]
    always: bool,
}

pub async fn task_approve(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Approval>) -> Response {
    let Some(task) = app.agent.get(&id) else { return not_found() };
    if task.pending.as_ref().is_none_or(|p| p.call != body.call) {
        return fail(StatusCode::CONFLICT, "That step is not waiting for an answer.");
    }
    let sender = app.agent.running.lock().unwrap().get_mut(&id).and_then(|r| r.approve.take());
    let Some(sender) = sender else {
        return fail(StatusCode::CONFLICT, "That step is not waiting for an answer.");
    };
    sender.send(Decision { allow: body.allow, always: body.allow && body.always }).ok();
    // The work goes on in the background; give it a moment so the answer shows the new state.
    tokio::time::sleep(Duration::from_millis(30)).await;
    match app.agent.get(&id) {
        Some(task) => Json(json!({"task": task.view()})).into_response(),
        None => not_found(),
    }
}

#[derive(Deserialize)]
pub struct FollowUp {
    #[serde(default)]
    text: String,
    /// Pictures that go with it: PNG, base64.
    #[serde(default)]
    images: Vec<String>,
    /// Go on where the task was stopped, with no new words.
    #[serde(default, rename = "continue")]
    go_on: bool,
    #[serde(default)]
    mode: Option<Mode>,
    #[serde(default)]
    web: Option<bool>,
    #[serde(default)]
    connectors: Option<bool>,
    /// For a film: the shots selected in the tracks, by id.
    #[serde(default)]
    selection: Vec<String>,
}

pub async fn task_message(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<FollowUp>) -> Response {
    let text = body.text.trim().to_string();
    if text.is_empty() && body.images.is_empty() && !body.go_on {
        return fail(StatusCode::BAD_REQUEST, "Say what you would like done.");
    }
    if text.chars().count() > MAX_PROMPT_CHARS {
        return fail(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("That is too long for the model. Keep it under {MAX_PROMPT_CHARS} characters."),
        );
    }
    let Some(task) = app.agent.get(&id) else { return not_found() };
    let folder = match Folder::open(&task.folder) {
        Ok(f) => f,
        Err(e) => return fail(StatusCode::CONFLICT, e),
    };
    // For a film: what the model is given, with the shots selected in the tracks.
    let (sent, skill, note) = if task.extra.film.is_empty() {
        match understood(&app, &folder, &task.model, &text) {
            Ok((sent, skill)) => (sent, skill, None),
            Err(why) => return fail(StatusCode::BAD_REQUEST, why),
        }
    } else {
        let (said, note) = film::request(&app, &task.extra.film, &text, &body.selection);
        (said, None, note)
    };
    let images = match compose::keep_pictures(&app.agent.dir, &id, &body.images) {
        Ok(names) => names,
        Err(why) => return fail(StatusCode::BAD_REQUEST, why),
    };
    let words = text.clone();
    let sent = if sent.is_empty() && !images.is_empty() { "Look at the picture.".to_string() } else { sent };
    // While it works, the message waits for the next turn.
    let queued = app.agent.change(&id, |t| {
        if !t.active() {
            return Ok(false);
        }
        if text.is_empty() && images.is_empty() {
            return Ok(true);
        }
        if t.extra.queued.len() >= compose::MAX_QUEUED {
            return Err(());
        }
        let q = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        t.extra.queued.push(compose::Queued {
            id: q,
            text: words.clone(),
            sent: sent.clone(),
            images: images.clone(),
            skill: skill.clone(),
        });
        Ok(true)
    });
    match queued {
        None => return not_found(),
        Some(Err(())) => return fail(StatusCode::TOO_MANY_REQUESTS, "Enough messages are waiting. Wait for the next turn."),
        Some(Ok(true)) => return app.agent.get(&id).map_or_else(not_found, |t| Json(json!({"task": t.view()})).into_response()),
        Some(Ok(false)) => {}
    }
    if app.agent.running.lock().unwrap().len() >= MAX_RUNNING {
        return fail(StatusCode::TOO_MANY_REQUESTS, format!("{MAX_RUNNING} tasks are already running. Stop one, or wait for it."));
    }
    // Checked and changed in one step, so two messages cannot both start the work.
    let accepted = app.agent.change(&id, |t| {
        if t.active() {
            return None;
        }
        // What waited when the work was stopped goes first.
        let waited = compose::deliver(t).unwrap_or_default();
        if !text.is_empty() || !images.is_empty() {
            compose::add(t, &words, &sent, &images);
            t.steps.extend(note.map(|text| Step::Note { text }));
        } else if body.go_on {
            t.messages.push(recall::hint("The person stopped you, and now asks you to go on from where you were."));
        }
        begin_request(t);
        if let Some(mode) = body.mode {
            t.mode = mode;
        }
        if let Some(web) = body.web {
            t.web = web;
        }
        if let Some(on) = body.connectors {
            t.mcp.on = on;
        }
        Some(waited)
    });
    match accepted.map(|a| a.map(|waited| waited.into_iter().chain(skill.clone()))) {
        None => not_found(),
        Some(None) => fail(StatusCode::CONFLICT, "This task is still working. Wait for it, or stop it first."),
        Some(Some(skills)) => {
            for skill in skills {
                read_skill(&app, &id, folder.root(), &skill);
            }
            start(&app, &id, true);
            match app.agent.get(&id) {
                Some(task) => Json(json!({"task": task.view()})).into_response(),
                None => not_found(),
            }
        }
    }
}

#[derive(Deserialize)]
pub struct NewTitle {
    title: String,
}

/// The person names a task. From then on the name is theirs and nothing else changes it.
pub async fn task_rename(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<NewTitle>) -> Response {
    let title: String = body.title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    if title.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "A task needs a name.");
    }
    let renamed = app.agent.change(&id, |t| {
        t.title = title;
        t.title_by = "person".into();
        t.view()
    });
    match renamed {
        Some(view) => Json(json!({"task": view})).into_response(),
        None => not_found(),
    }
}

#[derive(Deserialize, Default)]
pub struct StopQuery {
    #[serde(default)]
    remove: bool,
}

pub async fn task_delete(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<StopQuery>) -> Response {
    if app.agent.get(&id).is_none() {
        return not_found();
    }
    // Stopping drops the work where it is: a request to the model is cut, a command is killed.
    let abort = app.agent.running.lock().unwrap().get(&id).map(|r| r.abort.clone());
    // Stopping a task stops the app it started.
    app.agent.apps.stop(&id);
    if let Some(abort) = abort {
        abort.abort();
        app.agent.change(&id, |t| {
            if t.active() {
                t.settle("stopped");
                t.error = None;
            }
        });
        finished(&app, &id);
    }
    if q.remove {
        app.agent.remove(&id);
        return Json(json!({"removed": id})).into_response();
    }
    match app.agent.get(&id) {
        Some(task) => Json(json!({"task": stream::shown(&app, &task, &Default::default())})).into_response(),
        None => not_found(),
    }
}

pub async fn skills_list(State(app): State<Shared>, Query(q): Query<FolderQuery>) -> Json<Value> {
    let folder = q.folder.as_deref().and_then(|f| Folder::open(f).ok());
    let all = skills::load(&app.root, folder.as_ref().map(|f| f.root()));
    let instructions: Vec<String> =
        folder.as_ref().map(|f| project::found(f.root()).into_iter().map(|(path, _)| path).collect()).unwrap_or_default();
    Json(json!({
        "skills": all.iter().map(|s| json!({"name": s.name, "description": s.description, "source": s.source})).collect::<Vec<_>>(),
        "dir": app.root.join("skills"),
        "folder_dir": ".rigbrew/skills",
        "instructions": instructions,
    }))
}

/// `DELETE /v1/agent/tasks/{id}/queue/{q}`: the person takes back a message before it went in.
pub async fn queue_remove(State(app): State<Shared>, Path((id, q)): Path<(String, String)>) -> Response {
    let removed = app.agent.change(&id, |t| {
        let before = t.extra.queued.len();
        t.extra.queued.retain(|m| m.id != q);
        before != t.extra.queued.len()
    });
    match removed {
        None => not_found(),
        Some(false) => fail(StatusCode::CONFLICT, "That message is no longer waiting."),
        Some(true) => app.agent.get(&id).map_or_else(not_found, |t| Json(json!({"task": t.view()})).into_response()),
    }
}

#[derive(Deserialize, Default)]
pub struct FilesQuery {
    folder: String,
    #[serde(default)]
    q: String,
}

/// `GET /v1/agent/files?folder=&q=`: the files of a folder whose path matches `q`, best first,
/// for naming one with `@`.
pub async fn files_list(Query(q): Query<FilesQuery>) -> Response {
    let folder = match Folder::open(&q.folder) {
        Ok(f) => f,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    let found = tokio::task::spawn_blocking(move || files::find(&folder, &q.q, 20)).await.unwrap_or_default();
    Json(json!({"files": found})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str, args: Value) -> Value {
        json!({"role": "assistant", "content": "", "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}]})
    }

    fn result(id: &str, content: &str) -> Value {
        json!({"role": "tool", "tool_call_id": id, "content": content})
    }

    #[test]
    fn a_tool_call_the_engine_could_not_read_is_recognised() {
        assert!(cut_off_call("Failed to parse tool call arguments as JSON: [json.exception.parse_error.101] parse error at line 1, column 7662: syntax error while parsing value - invalid string: missing closing quote"));
        assert!(!cut_off_call("the request exceeds the available context size"));
        assert!(!cut_off_call("connection refused"));
    }

    #[test]
    fn old_results_are_left_out_first_and_the_request_stays() {
        let long = "x".repeat(3000);
        let messages = vec![
            json!({"role": "user", "content": "fix the test please"}),
            call("a", "read_file", json!({"path": "a.txt"})),
            result("a", &long),
            call("b", "write_file", json!({"path": "b.txt", "content": long})),
            result("b", "Created b.txt (1 lines)."),
            call("c", "read_file", json!({"path": "c.txt"})),
            result("c", &long),
        ];
        assert_eq!(fit(&messages, 100_000), messages, "nothing changes under the budget");
        let out = fit(&messages, 4000);
        assert_eq!(out.len(), messages.len());
        assert_eq!(out[0], messages[0]);
        assert_eq!(out[2]["content"], LEFT_OUT);
        let args: Value = serde_json::from_str(out[3]["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args, json!({"path": "b.txt", "content": "[left out]"}));
        assert_eq!(out[6], messages[6], "the last message is never touched");
        assert!(out.iter().map(weight).sum::<usize>() <= 4000);
    }

    #[test]
    fn an_answer_that_stops_halfway_is_sent_back_and_a_real_one_is_not() {
        let plan = |steps: Value| plan::parse(&json!({ "steps": steps })).unwrap();
        let open = plan(json!([{"text": "Write the code", "status": "done"}, {"text": "Write the tests"}]));
        let done = plan(json!([{"text": "Write the code", "status": "done"}]));
        // No plan, or a finished one: text is the answer.
        assert_eq!(plain_answer("The test fails because of a typo.", false, &[], 0), Plain::Answer);
        assert_eq!(plain_answer("All done.", false, &done, 0), Plain::Answer);
        // Steps left: the model is told which, and to go on.
        let Plain::Back(line) = plain_answer("Now I will write the tests.", false, &open, 0) else { panic!("sent back") };
        assert!(line.contains("[x] Write the code\n[>] Write the tests") && line.contains("go on with the next step"), "{line}");
        // A question to the person is theirs to answer, plan or not.
        assert_eq!(plain_answer("Which test runner do you use?", false, &open, 0), Plain::Answer);
        // Nothing at all, or an answer the engine cut off.
        assert!(matches!(plain_answer("", false, &[], 0), Plain::Back(l) if l.contains("answered with nothing")));
        assert!(matches!(plain_answer("The first half of", true, &[], 1), Plain::Back(l) if l.contains("cut off")));
        // The same for finish: with steps left it is told which, and a plan that is done (or none) lets it end.
        assert!(not_finished(&open).unwrap().contains("[>] Write the tests"));
        assert_eq!(not_finished(&done), None);
        assert_eq!(not_finished(&[]), None);
        // Never more than twice in a row: then it is the answer, whatever it is.
        assert_eq!(plain_answer("Now I will write the tests.", false, &open, MAX_SEND_BACKS), Plain::Answer);
        assert_eq!(plain_answer("", false, &[], MAX_SEND_BACKS), Plain::Answer);
    }

    #[test]
    fn long_work_that_changed_files_is_checked_against_the_request_before_it_ends() {
        let tool = |name: &str, status: &str| Step::Tool {
            call: "c".into(),
            name: name.into(),
            arguments: json!({}),
            status: status.into(),
            result: None,
            diff: None,
            image: None,
        };
        let mut steps = vec![
            Step::User { text: "first thing".into() },
            tool("write_file", "done"),
            Step::User { text: "Write four test files.".into() },
            tool("read_file", "done"),
        ];
        let root = files::scratch("lastlook");
        assert_eq!(last_look(&steps, 30, &root), None, "nothing was changed for this request");
        steps.push(tool("write_file", "failed"));
        assert_eq!(last_look(&steps, 30, &root), None, "a change that did not go in is none");
        steps.push(tool("edit_file", "done"));
        assert_eq!(last_look(&steps, LONG_WORK - 1, &root), None, "short work needs no second look");
        let look = last_look(&steps, LONG_WORK, &root).unwrap();
        assert!(look.contains("word for word:\n\nWrite four test files.\n\nGo through it") && !look.contains("first thing"), "{look}");
        assert_eq!(last_look(&[], 30, &root), None);
        // Files the request names and the folder does not have are said by name.
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("tests/test_item.py"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        let request = "Write inventory/cli.py, then tests in tests/test_item.py, tests/test_cli.py and tests/test_cli.py. \
            Then a README.md and notes.txt. Use Node.js 22.1 like https://example.com/docs/setup.html says, e.g. from example.org, \
            and mail me@host.dev. Not ../outside.txt.";
        assert_eq!(named_but_missing(request, &root), ["inventory/cli.py", "tests/test_cli.py", "notes.txt"]);
        steps.push(Step::User { text: request.into() });
        steps.push(tool("write_file", "done"));
        let look = last_look(&steps, LONG_WORK, &root).unwrap();
        assert!(look.contains("not in the folder: inventory/cli.py, tests/test_cli.py, notes.txt. Write them now"), "{look}");
        assert!(named_but_missing("Summarise this folder.", &root).is_empty());
    }

    #[test]
    fn an_engine_error_is_tried_again_unless_waiting_cannot_help() {
        assert!(!lasting("error sending request for url (http://127.0.0.1:51234/v1/chat/completions)"));
        assert!(!lasting("The model stopped with an error."));
        assert!(!lasting("Loading model"));
        assert!(lasting("Steep is not installed"));
        assert!(lasting("package has no model file"));
        assert!(RETRY_WAITS.len() >= 3 && RETRY_WAITS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_long_task_is_sent_as_the_first_request_the_notes_and_the_newest_part() {
        let mut messages = vec![json!({"role": "user", "content": "Build the thing."})];
        for n in 0..40 {
            messages.push(call(&format!("r{n}"), "read_file", json!({"path": format!("f{n}.rs")})));
            messages.push(result(&format!("r{n}"), &format!("f{n}.rs (lines 1 to 9 of 9)\nfn f{n}() {{}}\n{}", "// filler\n".repeat(120))));
        }
        let mut memory = recall::Memory::default();
        assert!(recall::compact(&mut messages, &mut memory, HISTORY_CHARS));
        let sent = fit(&recall::conversation(&memory, &messages), HISTORY_CHARS);
        assert!(sent.iter().map(weight).sum::<usize>() <= HISTORY_CHARS);
        let first = sent[0]["content"].as_str().unwrap();
        assert!(first.starts_with("Build the thing.") && first.contains("read f0.rs (lines 1 to 9 of 9): defines f0"), "{first}");
        assert_eq!(sent.last().unwrap()["tool_call_id"], "r39");
        assert!(sent.last().unwrap()["content"].as_str().unwrap().len() > 1000, "the newest result is whole");
        // A conversation cut like this is still a valid one: nothing is left for `repair` to mend.
        let mut again = messages.clone();
        repair(&mut again);
        assert_eq!(again, messages);
    }

    #[test]
    fn calls_without_a_result_get_one() {
        let mut messages = vec![
            json!({"role": "user", "content": "go"}),
            call("a", "run_command", json!({"command": "make"})),
            json!({"role": "user", "content": "and now?"}),
            call("b", "read_file", json!({"path": "x"})),
            result("b", "x"),
            result("stray", "nobody asked"),
            call("c", "list_files", json!({})),
        ];
        repair(&mut messages);
        let roles: Vec<&str> = messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "user", "assistant", "tool", "assistant", "tool"]);
        assert_eq!(messages[2]["tool_call_id"], "a");
        assert_eq!(messages[5]["content"], "x");
        assert_eq!(messages[7]["tool_call_id"], "c");
        let again = messages.clone();
        repair(&mut messages);
        assert_eq!(messages, again);
    }

    #[test]
    fn the_mode_and_the_web_switch_decide_which_tools_are_offered() {
        let names = |mode, web, skills| -> Vec<String> {
            tool_defs(mode, web, skills).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(names(Mode::ReadOnly, false, false), ["list_files", "read_file", "search", "update_plan", "outline", "finish"]);
        let ask = names(Mode::Ask, false, true);
        assert!(ask.contains(&"edit_file".into()) && ask.contains(&"write_file".into()) && ask.contains(&"run_command".into()));
        assert!(ask.contains(&"use_skill".into()));
        assert!(!ask.contains(&"web_search".into()) && !ask.contains(&"fetch_url".into()));
        let web = names(Mode::ReadOnly, true, false);
        assert!(web.contains(&"web_search".into()) && web.contains(&"fetch_url".into()) && !web.contains(&"run_command".into()));
        assert_eq!(names(Mode::AutoEdits, true, true).len(), KNOWN_TOOLS.len() + extra::TOOLS.len());
    }

    #[test]
    fn the_api_view_leaves_the_model_conversation_out() {
        let task = Task {
            id: "0".repeat(32),
            folder: "/p".into(),
            title: "t".into(),
            title_by: "words".into(),
            model: "steep-4b".into(),
            mode: Mode::Ask,
            web: false,
            state: "waiting".into(),
            created_ms: 1,
            updated_ms: 1,
            request_ms: 1,
            steps: vec![Step::Tool {
                call: "c1".into(),
                name: "run_command".into(),
                arguments: json!({"command": "ls"}),
                status: "waiting".into(),
                result: None,
                diff: None,
                image: None,
            }],
            pending: Some(Pending { call: "c1".into(), tool: "run_command".into(), path: None, command: Some("ls".into()), diff: None }),
            error: None,
            always: vec![],
            denied: vec![],
            turns: 1,
            messages: vec![json!({"role": "user", "content": "secret"})],
            extra: Default::default(),
            mcp: Default::default(),
            pace: Default::default(),
            memory: Default::default(),
        };
        let view = task.view();
        assert!(view.get("messages").is_none() && view.get("pace").is_none() && view.get("memory").is_none());
        assert_eq!(view["mode"], "ask");
        assert_eq!(view["steps"][0]["kind"], "tool");
        assert_eq!(view["pending"]["command"], "ls");
        let mut stopped = task.clone();
        stopped.settle("stopped");
        assert!(stopped.pending.is_none());
        assert!(matches!(&stopped.steps[0], Step::Tool { status, .. } if status == "stopped"));
        // What is saved reads back, conversation included.
        let back: Task = serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn saved_tasks_come_back_and_running_ones_are_marked() {
        let root = files::scratch("tasks");
        let agents = Agents::load(&root);
        let mut task = Task {
            id: "a".repeat(32),
            folder: "/p".into(),
            title: "t".into(),
            title_by: "words".into(),
            model: "m".into(),
            mode: Mode::AutoEdits,
            web: true,
            state: "running".into(),
            created_ms: 1,
            updated_ms: 1,
            request_ms: 1,
            steps: vec![Step::User { text: "hello".into() }],
            pending: None,
            error: None,
            always: vec!["make test".into()],
            denied: vec![],
            turns: 3,
            messages: vec![],
            extra: Default::default(),
            mcp: Default::default(),
            pace: Default::default(),
            memory: Default::default(),
        };
        agents.insert(task.clone());
        task.id = "b".repeat(32);
        task.state = "done".into();
        agents.insert(task);
        std::fs::write(root.join("agent/garbage.json"), "{not json").unwrap();
        let again = Agents::load(&root);
        let a = again.get(&"a".repeat(32)).unwrap();
        assert_eq!(a.state, "interrupted");
        assert!(a.error.is_some());
        assert_eq!(a.always, ["make test"]);
        assert_eq!(again.get(&"b".repeat(32)).unwrap().state, "done");
        assert_eq!(again.tasks.lock().unwrap().len(), 2);
        again.remove(&"b".repeat(32));
        assert_eq!(Agents::load(&root).tasks.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_task_is_named_after_its_first_turn_and_a_name_the_person_gave_stays() {
        let mut task: Task = serde_json::from_value(json!({
            "id": "0123456789abcdef0123456789abcdef", "folder": "/p", "title": "fix the login test", "model": "m", "mode": "ask",
            "web": false, "state": "running", "created_ms": 1, "updated_ms": 1, "pending": null, "error": null,
            "steps": [{"kind": "user", "text": "fix the login test, it fails on CI"}],
        }))
        .unwrap();
        assert_eq!(task.title_by, "", "a task saved before titles said who wrote them still reads");
        assert_eq!(named_after(&task), None, "not before the first turn is over");
        task.turns = 1;
        task.steps.push(Step::Model { text: "The test waits for a cookie that is never set.".into() });
        let (request, said) = named_after(&task).unwrap();
        assert_eq!(request, "fix the login test, it fails on CI");
        assert!(said.starts_with("The test waits"));
        // A task that ended before any turn is still named after its request.
        task.steps.truncate(1);
        task.turns = 0;
        task.state = "failed".into();
        assert_eq!(named_after(&task), Some(("fix the login test, it fails on CI".into(), String::new())));

        assert!(retitle(&mut task, "Login test cookie fix".into()));
        assert_eq!((task.title.as_str(), task.title_by.as_str()), ("Login test cookie fix", "model"));
        assert!(!retitle(&mut task, "  ".into()));
        task.title = "Mine".into();
        task.title_by = "person".into();
        assert!(!retitle(&mut task, "Something else".into()));
        assert_eq!((task.title.as_str(), task.title_by.as_str()), ("Mine", "person"));
        assert_eq!(task.summary()["title_by"], "person");
    }

    #[test]
    fn two_refusals_of_a_tool_end_the_asking() {
        let mut denied = vec![format!("run_command {}", json!({"command": "make"}))];
        assert!(!refused_twice(&denied, "run_command"));
        denied.push(format!("edit_file {}", json!({"path": "a"})));
        assert!(!refused_twice(&denied, "run_command"));
        denied.push(format!("run_command {}", json!({"command": "make test"})));
        assert!(refused_twice(&denied, "run_command"));
        assert!(!refused_twice(&denied, "edit_file"));
        assert!(!refused_twice(&denied, "run"));
    }

    #[test]
    fn numbers_sent_as_text_are_read() {
        assert_eq!(arg_usize(&json!({"depth": 3}), "depth"), Some(3));
        assert_eq!(arg_usize(&json!({"depth": "3"}), "depth"), Some(3));
        assert_eq!(arg_usize(&json!({"depth": "deep"}), "depth"), None);
        assert_eq!(arg_usize(&json!({}), "depth"), None);
    }
}
