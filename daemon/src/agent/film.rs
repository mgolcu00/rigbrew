//! Studio's assistant: the loop of Code, working on a film instead of a folder. The person says
//! what they want in plain words and the model does it with the film tools of
//! `studio::assist`, one step at a time, while the open workstation follows each step.
//!
//! Who may do what: every edit of the film (shots, lines, cast, look, sound) goes ahead, since
//! the person takes a whole request back with one undo. What takes long or costs work (making
//! shots, a dub, an export) waits for the person first, unless they chose Bypass; "always"
//! stops the asking for that tool for the rest of the conversation. In Read only nothing is
//! changed: the model looks and describes.

use serde_json::{json, Value};

use super::files::Folder;
use super::{ask, extra, plan, see, tool, Mode, Pending, Step, Task, DENIED, DENIED_BEFORE, DENIED_TWICE};
use crate::studio::assist::{self, Done};
use crate::Shared;

/// What only reads the film.
const READS: &[&str] = &["read_film", "read_shot", "look_at_shot"];
/// What changes the film and needs no asking: the person can undo it.
const EDITS: &[&str] = &[
    "edit_shot",
    "add_shot",
    "remove_shot",
    "move_shot",
    "set_look",
    "set_language",
    "cast_add",
    "cast_edit",
    "rewrite_shot",
    "subtitles",
    "add_sound_region",
];
/// What takes long or costs work: asked for first.
const LONG: &[&str] = &["make_shots", "dub", "export"];

/// Every tool of the film, besides the plan and finishing.
pub const TOOLS: &[&str] = &[
    "read_film",
    "read_shot",
    "look_at_shot",
    "edit_shot",
    "add_shot",
    "remove_shot",
    "move_shot",
    "set_look",
    "set_language",
    "cast_add",
    "cast_edit",
    "rewrite_shot",
    "subtitles",
    "add_sound_region",
    "make_shots",
    "dub",
    "export",
];

/// Turns a request has taken before its first `finish` is answered with the request itself.
const LOOK_BACK_AFTER: usize = 3;

fn offered(name: &str, mode: Mode, sees: bool) -> bool {
    match name {
        "look_at_shot" => sees,
        _ if READS.contains(&name) => true,
        _ if EDITS.contains(&name) || LONG.contains(&name) => mode != Mode::ReadOnly,
        _ => false,
    }
}

fn tool_defs(mode: Mode, sees: bool) -> Vec<Value> {
    let text = |d: &str| json!({"type": "string", "description": d});
    let whole = |d: &str| json!({"type": "integer", "description": d});
    let number = |d: &str| json!({"type": "number", "description": d});
    let yes = |d: &str| json!({"type": "boolean", "description": d});
    let shot = || whole("Shot number: 1 is the first");
    let line =
        json!({"type": "object", "properties": {"who": text("A name from the cast, or \"narrator\""), "text": text("What is said")}});
    let all = [
        tool(
            "read_film",
            "Read the whole film: its look, cast, every shot with its full description and lines, and its sound tracks.",
            json!({}),
            &[],
        ),
        tool("read_shot", "Read one shot with everything about it.", json!({"shot": shot()}), &["shot"]),
        tool(
            "look_at_shot",
            "See a picture of a made shot.",
            json!({"shot": shot(), "at": number("Seconds into the shot; its middle if left out")}),
            &["shot"],
        ),
        tool(
            "edit_shot",
            "Change one shot. Give only what changes.",
            json!({
                "shot": shot(),
                "description": text("What is seen, in one or two plain sentences. Name characters, never describe them"),
                "seconds": number("Length in seconds, 0.2 to 10"),
                "lines": {"type": "array", "items": line, "description": "Every line said in the shot, in place of the ones there"},
                "add_line": {"type": "object", "properties": {"who": text("A name from the cast, or \"narrator\""), "text": text("What is said")}, "description": "One more line said in the shot"},
                "cast": {"type": "array", "items": {"type": "string"}, "description": "Names from the cast who are in the shot"},
                "continues": yes("Starts from the last picture of the shot before, with no cut"),
            }),
            &["shot"],
        ),
        tool(
            "add_shot",
            "Add a new shot at the end, or after a shot.",
            json!({
                "description": text("What is seen, in one or two plain sentences"),
                "after": whole("Put it after this shot; 0 for the very start"),
                "seconds": number("Length in seconds"),
                "lines": {"type": "array", "items": line, "description": "Lines said in it"},
            }),
            &["description"],
        ),
        tool("remove_shot", "Take a shot out of the film.", json!({"shot": shot()}), &["shot"]),
        tool("move_shot", "Move a shot to another place.", json!({"shot": shot(), "to": whole("Its number afterwards")}), &["shot", "to"]),
        tool(
            "set_look",
            "Set the look every shot shares: style, colours, light, lens, in one line. Empty clears it.",
            json!({"look": text("The look")}),
            &["look"],
        ),
        tool(
            "set_language",
            "Set the language the lines are written in.",
            json!({"language": text("By name, like Turkish")}),
            &["language"],
        ),
        tool(
            "cast_add",
            "Add a character to the cast, with the fixed words for how they look.",
            json!({"name": text("Their name"), "look": text("Age, build, face, hair, clothes and colours")}),
            &["name", "look"],
        ),
        tool(
            "cast_edit",
            "Change how a character looks, or their name.",
            json!({"name": text("Their name now"), "look": text("How they look"), "new_name": text("A new name")}),
            &["name"],
        ),
        tool(
            "rewrite_shot",
            "Have the description of a shot written again, with what should be different.",
            json!({"shot": shot(), "hint": text("What should be different, like \"at night\" or \"closer\"")}),
            &["shot"],
        ),
        tool(
            "subtitles",
            "Show the lines as subtitles, or burn them into the picture of the exported film.",
            json!({"show": yes("Show subtitles"), "burn": yes("Burn them into the picture")}),
            &[],
        ),
        tool(
            "add_sound_region",
            "Lay a sound file of the film on a sound track.",
            json!({"file": text("The file's name"), "at": number("Seconds into the film"), "shot": whole("Or: start with this shot"), "track": text("The track's name")}),
            &["file"],
        ),
        tool(
            "make_shots",
            "Make shots with the video model. It takes minutes a shot; the person is asked first.",
            json!({"shots": {"type": "array", "items": {"type": "integer"}, "description": "Shot numbers; every shot not made yet if left out"}}),
            &[],
        ),
        tool(
            "dub",
            "Have the voice read every line aloud. Another language is translated first.",
            json!({"language": text("The language; the lines' own if left out")}),
            &[],
        ),
        tool("export", "Join the made shots and the sound into the film's MP4 file.", json!({}), &[]),
    ];
    let mut out: Vec<Value> =
        all.into_iter().filter(|t| t["function"]["name"].as_str().is_some_and(|name| offered(name, mode, sees))).collect();
    out.push(plan::tool_def());
    out.push(tool(
        "finish",
        "End the request with one short sentence for the person.",
        json!({"summary": text("What was done or found")}),
        &["summary"],
    ));
    out
}

fn system_prompt(mode: Mode, sees: bool) -> String {
    let mut lines = vec![
        "You are Steep, Rigbrew's model, working with the person on a film in Studio. A film is a row of shots. Each shot has a description the video model makes it from, a length, and lines said in it by the narrator or by someone in the cast.".to_string(),
        "Do what the person asks with the tools, one step at a time. Shots are named by number: shot 1 is the first. The film as it is now is below, and it is up to date after every step.".to_string(),
        "A description says what is seen, in one or two plain sentences, present tense, in English. Name a character by name and never describe them in a shot: their look from the cast is added when it is made, and so is the look of the film.".to_string(),
        "Lines are what is said, as it is said, in the language of the film's lines. To write lines in another language, call set_language first.".to_string(),
        "To change what the film shows (night, rain, a place, a mood), change the description of every shot it concerns with edit_shot; for style, colour and light of the whole film, set_look. make_shots is only for when the person asks to make or render shots.".to_string(),
        "To put someone in shots: cast_add them first when they are not in the cast, then edit_shot with \"cast\" for each of those shots.".to_string(),
        "Shot numbers change when a shot is added, removed or moved. What the person asked refers to the film as it was when they asked: do each thing once, and never undo a step of your own.".to_string(),
    ];
    match mode {
        Mode::ReadOnly => lines.push("You can only read the film: say what you would change and why. Do not say that you changed anything.".into()),
        Mode::Bypass => lines.push("Edits apply at once, and make_shots, dub and export run without asking. The person can undo the edits of a request in one go.".into()),
        Mode::Ask | Mode::AutoEdits => lines.push("Edits apply at once, and the person can undo them in one go. make_shots, dub and export take long: the person is asked before they run. After a no, do not ask again.".into()),
    }
    if sees {
        lines.push("look_at_shot shows you a picture of a made shot.".into());
    }
    lines.push("When the person asks a question, answer it from the film; change nothing they did not ask for.".into());
    lines.push(
        "A tool that refuses because a model is not installed will refuse again: say so to the person and do not try it again.".into(),
    );
    lines.push("For more than two steps, call update_plan first with the steps, and keep it up to date.".into());
    lines.push("Between steps write nothing, or one short line. When the work is done, call finish with one short sentence of what you did or found.".into());
    lines.join("\n")
}

/// The system message and the tools of one turn.
pub(super) fn turn(app: &Shared, task: &Task, film: &str) -> (String, Vec<Value>) {
    let sees = see::sees(app, task);
    let mut system = format!("{}\n\nThe film now:\n{}", system_prompt(task.mode, sees), assist::context(app, film));
    for line in assist::cannot(app) {
        system.push_str(&format!("\n{line}"));
    }
    if !task.extra.plan.is_empty() {
        system.push_str(&format!("\n\nYour plan so far ([x] done, [>] doing now, [ ] to do):\n{}", plan::brief(&task.extra.plan)));
    }
    system.push_str(&task.pace.note(false));
    (system, tool_defs(task.mode, sees))
}

/// The message the model is given for a request, with the shots selected in the tracks; and the
/// note the person sees for them.
pub(super) fn request(app: &Shared, film: &str, text: &str, selected: &[String]) -> (String, Option<String>) {
    let shots = assist::count(app, film);
    let when = format!("[As this is asked, the film has {shots} {}.]", if shots == 1 { "shot" } else { "shots" });
    match assist::selection(app, film, selected) {
        Some(note) => (format!("{text}\n\n{when} [{note} in the tracks.]"), Some(note)),
        None => (format!("{text}\n\n{when}"), None),
    }
}

/// After a few turns that changed the film, the first `finish` is answered with the request
/// itself, word for word: by then a small model has often done only a part of it.
pub(super) fn before_finish(task: &Task) -> Option<String> {
    let start = task.steps.iter().rposition(|s| matches!(s, Step::User { .. }))?;
    let Step::User { text } = &task.steps[start] else { return None };
    let changed = task.steps[start..]
        .iter()
        .any(|s| matches!(s, Step::Tool { name, status, .. } if status == "done" && EDITS.contains(&name.as_str())));
    (task.turns >= LOOK_BACK_AFTER && changed).then(|| {
        format!(
            "Before you finish, here is what the person asked for, word for word:\n\n{text}\n\nCheck the film against it, point by point and shot by shot. If something is missing, do it now. If everything is there, call finish again."
        )
    })
}

/// A change made: its before and after go with the step, and the model is told what it did.
fn changed(app: &Shared, id: &str, call: &str, outcome: Result<Done, String>) -> (String, &'static str) {
    match outcome {
        Ok(done) => {
            if !done.diff.is_empty() {
                app.agent.change(id, |t| t.set_tool(call, "running", None, Some(done.diff.clone())));
            }
            (done.text(), "done")
        }
        Err(why) => (why, "failed"),
    }
}

/// Asks the person before something that takes long, the way Code asks before a command.
async fn allowed(
    app: &Shared,
    id: &str,
    task: &Task,
    call: &str,
    name: &str,
    question: String,
    signature: &str,
) -> Result<(), (String, &'static str)> {
    let key = format!("tool:{name}");
    if task.mode == Mode::Bypass || task.always.contains(&key) {
        return Ok(());
    }
    let pending = Pending { call: call.into(), tool: name.into(), path: None, command: Some(question), diff: None };
    let decision = ask(app, id, pending, signature).await;
    if !decision.allow {
        return Err((DENIED.into(), "denied"));
    }
    if decision.always {
        app.agent.change(id, |t| t.always.push(key));
    }
    Ok(())
}

/// Runs one tool on the film: (result for the model, step status).
pub(super) async fn call(
    app: &Shared,
    id: &str,
    folder: &Folder,
    film: &str,
    call: &str,
    name: &str,
    args: &Value,
) -> (String, &'static str) {
    let Some(task) = app.agent.get(id) else { return ("The task is gone.".into(), "failed") };
    if name == "update_plan" {
        return extra::call(app, id, folder, name, args).await;
    }
    if !offered(name, task.mode, see::sees(app, &task)) {
        return (format!("There is no tool named {name} in this task."), "failed");
    }
    let signature = format!("{name} {args}");
    if task.denied.contains(&signature) {
        return (DENIED_BEFORE.into(), "denied");
    }
    if super::refused_twice(&task.denied, name) {
        return (DENIED_TWICE.into(), "denied");
    }
    let read = |r: Result<String, String>| match r {
        Ok(text) => (text, "done"),
        Err(why) => (why, "failed"),
    };
    match name {
        "read_film" => read(assist::read_film(app, film)),
        "read_shot" => read(assist::read_shot(app, film, args)),
        "look_at_shot" => match assist::look_at_shot(app, film, args).await {
            Ok((png, what)) => match see::keep_picture(app, id, call, &png, what.clone()) {
                Some(_) => (format!("The picture of {what} comes with this result. Look at it before you answer."), "done"),
                None => ("The picture could not be kept.".into(), "failed"),
            },
            Err(why) => (why, "failed"),
        },
        "edit_shot" => changed(app, id, call, assist::edit_shot(app, film, args)),
        "add_shot" => changed(app, id, call, assist::add_shot(app, film, args)),
        "remove_shot" => changed(app, id, call, assist::remove_shot(app, film, args)),
        "move_shot" => changed(app, id, call, assist::move_shot(app, film, args)),
        "set_look" => changed(app, id, call, assist::set_look(app, film, args)),
        "set_language" => changed(app, id, call, assist::set_language(app, film, args)),
        "cast_add" => changed(app, id, call, assist::cast_add(app, film, args)),
        "cast_edit" => changed(app, id, call, assist::cast_edit(app, film, args)),
        "subtitles" => changed(app, id, call, assist::subtitles(app, film, args)),
        "rewrite_shot" => changed(app, id, call, assist::rewrite_shot(app, film, args).await),
        "add_sound_region" => changed(app, id, call, assist::add_sound_region(app, film, args).await),
        "make_shots" => {
            let (shots, question) = match assist::make_plan(app, film, args) {
                Ok(plan) => plan,
                Err(why) => return (why, "failed"),
            };
            if let Err(no) = allowed(app, id, &task, call, name, question, &signature).await {
                return no;
            }
            changed(app, id, call, assist::make_shots(app, film, shots).await)
        }
        "dub" => {
            let (language, question) = match assist::dub_plan(app, film, args) {
                Ok(plan) => plan,
                Err(why) => return (why, "failed"),
            };
            if let Err(no) = allowed(app, id, &task, call, name, question, &signature).await {
                return no;
            }
            changed(app, id, call, assist::dub(app, film, language).await)
        }
        "export" => {
            let question = match assist::export_plan(app, film) {
                Ok(q) => q,
                Err(why) => return (why, "failed"),
            };
            if let Err(no) = allowed(app, id, &task, call, name, question, &signature).await {
                return no;
            }
            changed(app, id, call, assist::export(app, film).await)
        }
        _ => (format!("There is no tool named {name}."), "failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(mode: Mode, sees: bool) -> Vec<String> {
        tool_defs(mode, sees).iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_mode_decides_which_film_tools_are_offered() {
        assert_eq!(names(Mode::ReadOnly, false), ["read_film", "read_shot", "update_plan", "finish"]);
        assert_eq!(names(Mode::ReadOnly, true), ["read_film", "read_shot", "look_at_shot", "update_plan", "finish"]);
        let ask = names(Mode::Ask, false);
        assert!(EDITS.iter().chain(LONG).all(|t| ask.contains(&t.to_string())), "{ask:?}");
        assert_eq!(names(Mode::Bypass, true).len(), TOOLS.len() + 2);
        // Every tool has a place, and only one.
        for t in TOOLS {
            assert_eq!([READS, EDITS, LONG].iter().filter(|g| g.contains(t)).count(), 1, "{t}");
        }
        assert!(system_prompt(Mode::ReadOnly, false).contains("You can only read"));
        assert!(system_prompt(Mode::Ask, false).contains("the person is asked before they run"));
        assert!(
            system_prompt(Mode::Bypass, true).contains("run without asking") && system_prompt(Mode::Bypass, true).contains("look_at_shot")
        );
    }

    #[test]
    fn a_request_that_changed_the_film_is_read_back_once_before_it_ends() {
        let mut task: Task = serde_json::from_value(json!({
            "id": "0".repeat(32), "folder": "/f", "title": "t", "model": "m", "mode": "ask", "web": false, "state": "running",
            "created_ms": 1, "updated_ms": 1, "steps": [{"kind": "user", "text": "add a narrator line to every shot"}],
            "pending": null, "error": null, "film": "abc",
        }))
        .unwrap();
        assert_eq!(task.extra.film, "abc");
        task.turns = 5;
        assert_eq!(before_finish(&task), None, "nothing was changed yet");
        task.steps.push(Step::Tool {
            call: "c".into(),
            name: "edit_shot".into(),
            arguments: json!({}),
            status: "done".into(),
            result: None,
            diff: None,
            image: None,
        });
        let look = before_finish(&task).unwrap();
        assert!(look.contains("word for word:\n\nadd a narrator line to every shot\n\nCheck the film"), "{look}");
        task.turns = 2;
        assert_eq!(before_finish(&task), None, "short work needs no second look");
    }
}
