//! The tools with which a task runs, looks at and checks its own work, and where they meet
//! the loop: `start_app` (see `apps`), `look_at_page` (see `look`), `check_project` (see
//! `checks`) and `simulator` (see `devices`).
//!
//! Who may use what:
//!
//! - `start_app` and `check_project` run commands in the folder, so they follow the rules of
//!   `run_command`: not offered in Read only, asked for in Ask first and Auto edits (with
//!   "always" for the same command), not asked in Bypass.
//! - `look_at_page` only opens an address on this computer and `simulator` only reads from a
//!   device that is already running: every mode has them and neither asks first.
//!
//! The model is never trusted to check by itself: `before_finish` holds a request open once
//! when files were changed and the project's checks did not run, or did not pass, since.

use std::path::Path;

use serde_json::{json, Value};

use super::files::Folder;
use super::{apps, ask, checks, devices, look, undo, verify, web, Mode, Pending, Step, Task, DENIED};
use crate::Shared;

pub const TOOLS: &[&str] = &["start_app", "look_at_page", "check_project", "simulator"];

/// Whether this task has the tool at all.
pub(super) fn offered(name: &str, mode: Mode, root: &Path) -> bool {
    match name {
        "start_app" | "check_project" => mode != Mode::ReadOnly,
        "look_at_page" => true,
        "simulator" => devices::available() && devices::mobile_project(root),
        _ => false,
    }
}

pub(super) fn tool_defs(mode: Mode, root: &Path) -> Vec<Value> {
    let text = |d: &str| json!({"type": "string", "description": d});
    let all = [
        json!({"type": "function", "function": {
            "name": "start_app",
            "description": "Start this project's app in the background (dev server, static page) and wait until it answers. Gives its address. It keeps running.",
            "parameters": {"type": "object", "required": [], "properties": {
                "command": text("Leave out to let Rigbrew find it. Otherwise the command; write {port} where the port goes"),
            }},
        }}),
        json!({"type": "function", "function": {
            "name": "look_at_page",
            "description": "Open a page of the running app in a browser and see it: title, text, console errors, failed requests and a screenshot. Only addresses on this computer.",
            "parameters": {"type": "object", "required": ["url"], "properties": {
                "url": text("Like http://localhost:5173/ or http://localhost:5173/cart"),
                "size": text("\"desktop\" (if left out), \"tablet\" or \"phone\""),
                "do": {"type": "array", "description": "Optional steps before looking: {\"click\": \"text or CSS selector\"} or {\"type\": \"CSS selector\", \"text\": \"what to type\"}", "items": {"type": "object"}},
            }},
        }}),
        json!({"type": "function", "function": {
            "name": "check_project",
            "description": "Run the project's own checks (type check or build, linter, unit tests) and get a short result: what passed, what failed, with file and line.",
            "parameters": {"type": "object", "required": [], "properties": {
                "what": text("\"fast\" (if left out): types, lint and unit tests. \"ui\": tests that drive the app. \"all\": both"),
            }},
        }}),
        json!({"type": "function", "function": {
            "name": "simulator",
            "description": "See the phone app on the booted iOS simulator or the connected Android device: its screen, or its last log lines.",
            "parameters": {"type": "object", "required": ["action"], "properties": {
                "action": {"type": "string", "enum": ["list", "screenshot", "logs"], "description": "\"list\" names the devices, \"screenshot\" shows the screen, \"logs\" reads the log"},
                "device": text("Name of the device, when there is more than one"),
                "app": text("For logs: the app's name or id, to see only its lines"),
            }},
        }}),
    ];
    all.into_iter().filter(|t| t["function"]["name"].as_str().is_some_and(|name| offered(name, mode, root))).collect()
}

/// Lines for the system message.
pub(super) fn rules(mode: Mode, root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    if mode != Mode::ReadOnly {
        lines.push("When you made or changed something a person looks at (a web page, an app), see it yourself before you finish: start_app, then look_at_page. Fix what looks wrong or shows errors, then look again.".to_string());
        lines.push(
            "After changes, call check_project and fix what it reports. Do not finish with failing checks unless you say so plainly."
                .to_string(),
        );
    }
    if offered("simulator", mode, root) {
        lines.push("For a phone app: build and start it with run_command, then use simulator to see its screen and its log.".to_string());
    }
    lines
}

/// Whether a skill is worth a line of the system message in this folder: the two about phones
/// are named only where a phone app is. `use_skill` still finds them by name.
pub(super) fn skill_fits(name: &str, root: &Path) -> bool {
    !matches!(name, "ios-simulator" | "android-emulator") || devices::mobile_project(root)
}

/// The project's checks as one line of the system message, or nothing when it has none.
pub(super) fn context(mode: Mode, root: &Path) -> Option<String> {
    if mode == Mode::ReadOnly {
        return None;
    }
    let fast: Vec<String> =
        checks::detect(root).into_iter().filter(|c| c.kind != checks::Kind::Ui && c.missing.is_none()).map(|c| c.command).collect();
    (!fast.is_empty()).then(|| format!("check_project checks this project. It runs: {}", fast.join("; ")))
}

const CHECK_FIRST: &str = "Not finished: you changed files and nothing checked them since. Call check_project now. If it passes, call finish again. If it fails, fix what it reports.";
const CHECK_FAILED: &str = "Not finished: the last check after your change did not pass. Fix what it reported and call check_project again. If you cannot fix it, call finish and say plainly what still fails.";

/// What `finish` is answered with, once, when the work was not checked: files were changed in
/// this request, the project has checks that can run here, and none ran (or none passed) after
/// the last change. `None` when there is nothing to hold the request open for, and when the
/// person said no to a check.
pub(super) fn unchecked(task: &Task, root: &Path) -> Option<&'static str> {
    if task.mode == Mode::ReadOnly {
        return None;
    }
    let line = verify::unchecked(&task.steps)?;
    let start = task.steps.iter().rposition(|s| matches!(s, Step::User { .. })).map_or(0, |i| i + 1);
    let said_no = task.steps[start..]
        .iter()
        .any(|s| matches!(s, Step::Tool { name, status, .. } if status == "denied" && (name == "check_project" || name == "run_command")));
    if said_no || !checks::has_fast(root) {
        return None;
    }
    Some(if line == verify::NOT_PASSED { CHECK_FAILED } else { CHECK_FIRST })
}

/// The last look before a request ends: what was asked (after long work, see `last_look`) and
/// what was not checked, as one message.
pub(super) fn before_finish(task: &Task, root: &Path) -> Option<String> {
    match (super::last_look(&task.steps, task.turns, root), unchecked(task, root)) {
        (None, None) => None,
        (Some(look), None) => Some(look),
        (None, Some(check)) => Some(check.to_string()),
        (Some(look), Some(check)) => Some(format!("{look}\n\n{check}")),
    }
}

/// Whether the task's model takes pictures: its installed files have a vision part.
pub(super) fn sees(app: &Shared, task: &Task) -> bool {
    app.package(&task.model).is_some_and(|p| app.installed(&p).iter().any(|v| v.files.iter().any(|f| f.role == "mmproj")))
}

/// Keeps a picture with the step that took it, and as the one the model looks at next.
fn keep_picture(app: &Shared, id: &str, call: &str, png: &[u8], what: String) -> Option<String> {
    let shot = look::keep_shot(&app.agent.dir, id, png)?;
    app.agent.change(id, |t| {
        if let Some(Step::Tool { image, .. }) = t.tool_step(call) {
            *image = Some(shot.clone());
        }
        t.extra.eye = Some(look::Eye { step: call.to_string(), call: String::new(), shot: shot.clone(), what, sends: 0 });
    });
    Some(shot)
}

/// Asks the person before a command runs, the way `run_command` does. `Err` is the answer to
/// give when they said no.
async fn allowed(
    app: &Shared,
    id: &str,
    task: &Task,
    call: &str,
    name: &str,
    command: &str,
    signature: &str,
) -> Result<(), (String, &'static str)> {
    if task.mode == Mode::Bypass || task.always.iter().any(|c| c == command) {
        return Ok(());
    }
    let pending = Pending { call: call.into(), tool: name.into(), path: None, command: Some(command.to_string()), diff: None };
    let decision = ask(app, id, pending, signature).await;
    if !decision.allow {
        return Err((DENIED.into(), "denied"));
    }
    if decision.always {
        app.agent.change(id, |t| t.always.push(command.to_string()));
    }
    Ok(())
}

/// Runs one of `TOOLS`.
pub(super) async fn call(app: &Shared, id: &str, folder: &Folder, call: &str, name: &str, args: &Value) -> (String, &'static str) {
    let Some(task) = app.agent.get(id) else { return ("The task is gone.".into(), "failed") };
    let arg = |key: &str| args[key].as_str().map(str::trim).filter(|s| !s.is_empty());
    let signature = format!("{name} {args}");
    match name {
        "start_app" => {
            let Some(command) = arg("command").map(str::to_string).or_else(|| apps::detect(folder.root())) else {
                return (
                    "Rigbrew could not tell how this project starts. Call start_app with \"command\": the command that starts it, with {port} where the port goes (for example \"python3 -m http.server {port}\").".into(),
                    "failed",
                );
            };
            if let Err(no) = allowed(app, id, &task, call, name, &command, &signature).await {
                return no;
            }
            if let Err(full) = apps::make_room(app) {
                return (full, "failed");
            }
            undo::before_change(app, id, folder, None).await;
            // When the app ends by itself, the task says so to whoever is watching it.
            let (watching, watched) = (app.clone(), id.to_string());
            let changed = move || {
                watching.agent.change(&watched, |_| ());
            };
            match apps::start(&app.agent.apps, id, folder, &command, task.web, apps::READY, changed).await {
                Ok(started) => (started.text(), "done"),
                Err(why) => (why, "failed"),
            }
        }
        "look_at_page" => {
            let address = arg("url").map(str::to_string).or_else(|| app.agent.apps.port(id).map(|p| format!("http://localhost:{p}/")));
            let Some(address) = address else {
                return ("\"url\" is needed: the address of the running app. start_app starts it and gives the address.".into(), "failed");
            };
            let url = match web::local_url(&address) {
                Ok(u) => u,
                Err(e) => return (e, "failed"),
            };
            let (size, actions) = match (look::size(arg("size")), look::actions(args)) {
                (Ok(s), Ok(a)) => (s, a),
                (Err(e), _) | (_, Err(e)) => return (e, "failed"),
            };
            let seen = look::look(&app.agent.looks, &url, size, actions).await;
            let sees = sees(app, &task);
            let shot = seen.picture.as_ref().and_then(|png| keep_picture(app, id, call, png, format!("the page {}", seen.url)));
            let page = seen.page(shot);
            app.agent.change(id, |t| t.extra.page = Some(page));
            // A page that did not answer at all is a failed step; a page with errors on it was seen.
            let reached = seen.rendered || !seen.text.is_empty() || !seen.title.is_empty();
            (seen.text(sees), if reached { "done" } else { "failed" })
        }
        "check_project" => {
            let kinds = match checks::wanted(arg("what").unwrap_or("fast")) {
                Ok(k) => k,
                Err(e) => return (e, "failed"),
            };
            let chosen = checks::chosen(folder.root(), kinds);
            let runs: Vec<&str> = chosen.iter().filter(|c| c.missing.is_none()).map(|c| c.command.as_str()).collect();
            if !runs.is_empty() {
                if let Err(no) = allowed(app, id, &task, call, name, &runs.join(" && "), &signature).await {
                    return no;
                }
                undo::before_change(app, id, folder, None).await;
            }
            let (text, ok) = checks::report(&checks::run(folder, &chosen, apps::net_of(app, &task)).await);
            (text, if ok { "done" } else { "failed" })
        }
        "simulator" => {
            let found = devices::list().await;
            let action = arg("action").unwrap_or("screenshot");
            if action == "list" {
                return (devices::listed(&found), "done");
            }
            let device = match devices::pick(&found, arg("device")) {
                Ok(d) => d,
                Err(e) => return (e, "failed"),
            };
            match action {
                "screenshot" => match devices::screenshot(device, &app.agent.dir.join("shots")).await {
                    Ok(png) => {
                        let sees = sees(app, &task);
                        let shot = keep_picture(app, id, call, &png, format!("the screen of {}", device.name));
                        app.agent.change(id, |t| {
                            t.extra.device = shot.map(|shot| devices::Screen {
                                shot,
                                name: device.shown(),
                                id: device.id.clone(),
                                at_ms: super::now_ms(),
                            })
                        });
                        let said = if sees {
                            "The picture comes with this result. Look at it: is anything missing, broken or out of place?"
                        } else {
                            "It is shown to the person. You cannot see pictures: read the log, or ask the person what they see."
                        };
                        (format!("Took a screenshot of {}. {said}", device.shown()), "done")
                    }
                    Err(why) => (why, "failed"),
                },
                "logs" => match devices::logs(device, arg("app")).await {
                    Ok(text) => (text, "done"),
                    Err(why) => (why, "failed"),
                },
                other => (format!("\"action\" is \"list\", \"screenshot\" or \"logs\", not \"{other}\"."), "failed"),
            }
        }
        _ => (format!("There is no tool named {name}."), "failed"),
    }
}

pub mod api {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::Json;

    use super::*;
    use crate::fail;

    fn task_of(app: &Shared, id: &str) -> Response {
        match app.agent.get(id) {
            Some(task) => Json(json!({"task": super::super::stream::shown(app, &task, &Default::default())})).into_response(),
            None => fail(StatusCode::NOT_FOUND, "no such task"),
        }
    }

    /// `DELETE /v1/agent/tasks/{id}/app`: the person stops the task's app.
    pub async fn app_stop(State(app): State<Shared>, Path(id): Path<String>) -> Response {
        if app.agent.apps.stop(&id) {
            app.agent.change(&id, |_| ());
        }
        task_of(&app, &id)
    }

    /// `POST /v1/agent/tasks/{id}/device`: a new picture of the phone's screen, for the preview.
    pub async fn device_shot(State(app): State<Shared>, Path(id): Path<String>) -> Response {
        let Some(task) = app.agent.get(&id) else { return fail(StatusCode::NOT_FOUND, "no such task") };
        let found = devices::list().await;
        // The device the task looked at last, when it is still there.
        let wanted = task.extra.device.as_ref().map(|d| d.id.clone()).filter(|id| found.iter().any(|d| d.id == *id));
        let device = match devices::pick(&found, wanted.as_deref()) {
            Ok(d) => d,
            Err(e) => return fail(StatusCode::CONFLICT, e),
        };
        let png = match devices::screenshot(device, &app.agent.dir.join("shots")).await {
            Ok(png) => png,
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
        };
        let Some(shot) = look::keep_shot(&app.agent.dir, &id, &png) else {
            return fail(StatusCode::INTERNAL_SERVER_ERROR, "The picture could not be kept.");
        };
        app.agent.change(&id, |t| {
            t.extra.device = Some(devices::Screen { shot, name: device.shown(), id: device.id.clone(), at_ms: super::super::now_ms() })
        });
        task_of(&app, &id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::files;
    use super::*;

    fn task(folder: &Path, mode: &str, steps: Value) -> Task {
        serde_json::from_value(json!({
            "id": "0".repeat(32), "folder": folder, "title": "t", "model": "m", "mode": mode, "web": false, "state": "running",
            "created_ms": 1, "updated_ms": 1, "steps": steps, "pending": null, "error": null,
        }))
        .unwrap()
    }

    fn step(name: &str, status: &str) -> Value {
        json!({"kind": "tool", "call": "c", "name": name, "arguments": {}, "status": status, "result": null, "diff": null})
    }

    fn user() -> Value {
        json!({"kind": "user", "text": "Add a tax line to the cart."})
    }

    /// A project whose check can run wherever the tests do.
    fn checked_project() -> std::path::PathBuf {
        let root = files::scratch("see");
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        root
    }

    #[test]
    fn finishing_unchecked_work_is_answered_once_with_the_check() {
        let root = checked_project();
        if !checks::has_fast(&root) {
            return;
        }
        let held = |mode: &str, steps: Value| unchecked(&task(&root, mode, steps), &root);
        assert_eq!(held("ask", json!([user(), step("read_file", "done")])), None, "nothing was changed");
        assert_eq!(held("ask", json!([user(), step("edit_file", "done")])), Some(CHECK_FIRST));
        assert_eq!(held("bypass", json!([user(), step("check_project", "done"), step("write_file", "done")])), Some(CHECK_FIRST));
        assert_eq!(held("ask", json!([user(), step("edit_file", "done"), step("check_project", "done")])), None);
        assert_eq!(
            held("ask", json!([user(), step("edit_file", "done"), step("run_command", "done")])),
            None,
            "a command that ended well counts"
        );
        assert_eq!(held("ask", json!([user(), step("edit_file", "done"), step("check_project", "failed")])), Some(CHECK_FAILED));
        assert_eq!(held("ask", json!([user(), step("edit_file", "done"), step("check_project", "denied")])), None, "the person said no");
        assert_eq!(held("read_only", json!([user(), step("edit_file", "done")])), None);
        // Only the request being worked on counts.
        assert_eq!(held("ask", json!([user(), step("edit_file", "done"), user(), step("read_file", "done")])), None);
        // A project with nothing to run is not held open.
        let bare = files::scratch("see-bare");
        assert_eq!(unchecked(&task(&bare, "ask", json!([user(), step("edit_file", "done")])), &bare), None);
        // One message, not two: the long-work look and the check together.
        let short = task(&root, "ask", json!([user(), step("edit_file", "done")]));
        assert_eq!(before_finish(&short, &root).as_deref(), Some(CHECK_FIRST));
        let mut long = short.clone();
        long.turns = super::super::LONG_WORK;
        let both = before_finish(&long, &root).unwrap();
        assert!(
            both.starts_with("Before you finish, here is what the person asked for, word for word:\n\nAdd a tax line to the cart."),
            "{both}"
        );
        assert!(both.ends_with(CHECK_FIRST), "{both}");
        let done = task(&root, "ask", json!([user(), step("edit_file", "done"), step("check_project", "done")]));
        assert_eq!(before_finish(&done, &root), None);
    }

    #[test]
    fn each_mode_has_the_tools_it_may_use() {
        let root = files::scratch("see-tools");
        let names = |mode: Mode| -> Vec<String> {
            tool_defs(mode, &root).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(names(Mode::Ask), ["start_app", "look_at_page", "check_project"]);
        assert_eq!(names(Mode::Bypass), ["start_app", "look_at_page", "check_project"]);
        assert_eq!(names(Mode::ReadOnly), ["look_at_page"], "looking changes nothing");
        assert!(!offered("simulator", Mode::Bypass, &root), "not a phone app");
        assert!(!offered("run_command", Mode::Bypass, &root));
        assert!(rules(Mode::ReadOnly, &root).is_empty());
        assert!(rules(Mode::Ask, &root).iter().any(|l| l.contains("start_app, then look_at_page")));
        assert_eq!(context(Mode::Ask, &root), None);
        let checked = checked_project();
        if checks::has_fast(&checked) {
            assert_eq!(
                context(Mode::Ask, &checked).unwrap(),
                "check_project checks this project. It runs: cargo check --all-targets --message-format short; cargo test"
            );
            assert_eq!(context(Mode::ReadOnly, &checked), None);
        }
        assert!(!skill_fits("ios-simulator", &root) && skill_fits("preview-and-fix", &root));
        // What this adds to every request is small: the model's context is.
        assert!(serde_json::to_string(&tool_defs(Mode::Bypass, &root)).unwrap().len() < 2000);
        assert!(rules(Mode::Bypass, &root).join("\n").len() < 420);
        // Every tool named here has a definition, and the other way round.
        std::fs::create_dir_all(root.join("ios")).unwrap();
        assert!(skill_fits("ios-simulator", &root));
        let all: Vec<Value> = tool_defs(Mode::Bypass, &root);
        assert!(all.len() == TOOLS.len() || !devices::available());
        assert!(all.iter().all(|t| TOOLS.contains(&t["function"]["name"].as_str().unwrap())));
    }
}
