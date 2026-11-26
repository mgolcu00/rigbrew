//! The film tools of Studio's assistant; `agent/film.rs` offers them to the model. Each one is
//! small and checked, and answers with one line the person reads and the model is told ("Shot
//! 3: description changed"), with what it was before and after, and a plain refusal otherwise.
//!
//! Every change goes through `apply_save`, the path the workstation's own edits take: it is
//! checked the same way, a made shot whose description changes is marked changed, and the open
//! workstation takes the film as it is after each step. Shots are named by their number in the
//! film (1 is the first), as the person and the model speak of them.

use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::Response;
use axum::Json;
use rigbrew_core::manifest::Modality;
use serde_json::Value;

use super::cast::{self, Member, MemberIn};
use super::sound::{self, RegionIn, Track, TrackIn};
use super::words::{self, Spoken, SpokenIn};
use super::{apply_save, dub, export_of, export_path, exported, installed_one, job_of, load, missing, shot_in, store, with_queue};
use super::{Project, Save, Shot, MAX_PROMPT, MAX_SHOTS};
use crate::{App, Shared};

/// The longest description of a shot shown on every turn; `read_film` gives them whole.
const BRIEF_CHARS: usize = 160;
/// Shots shown on every turn; `read_film` lists them all.
const BRIEF_SHOTS: usize = 40;
/// How long the assistant waits for a dub or an export it started before it says it goes on.
const LONG_WAIT: Duration = Duration::from_secs(30 * 60);

/// What a change did: one line for the person and the model, and before and after.
#[derive(Debug, Default, PartialEq)]
pub struct Done {
    /// "Shot 3: description changed".
    pub said: String,
    /// What it was and what it is, as lines starting with "- " and "+ ".
    pub diff: String,
    /// More for the model, when it needs to know (a made shot is now marked changed).
    pub more: String,
}

impl Done {
    fn new(said: impl Into<String>) -> Done {
        Done { said: said.into(), ..Done::default() }
    }

    /// What the model is told.
    pub fn text(&self) -> String {
        if self.more.is_empty() {
            format!("{}.", self.said)
        } else {
            format!("{}.\n{}", self.said, self.more)
        }
    }
}

// --- reading the arguments ----------------------------------------------------------------------

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args[key].as_str().map(str::trim)
}

/// A number; small models often send it as text ("7", "7 s", "7 seconds").
fn number(args: &Value, key: &str) -> Option<f64> {
    args[key].as_f64().or_else(|| {
        let t = args[key].as_str()?.trim().to_lowercase();
        let t = t.trim_end_matches("seconds").trim_end_matches("second").trim_end_matches('s').trim();
        t.parse().ok()
    })
}

fn flag(args: &Value, key: &str) -> Option<bool> {
    args[key].as_bool().or_else(|| match args[key].as_str()?.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    })
}

fn one_row(text: &str, most: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(most).collect()
}

fn cut(text: &str, most: usize) -> String {
    let one = one_row(text, usize::MAX);
    if one.chars().count() <= most {
        one
    } else {
        format!("{}…", one.chars().take(most).collect::<String>().trim_end())
    }
}

fn seconds(frames: u32) -> String {
    format!("{:.1} s", frames as f32 / 24.0)
}

/// "1", "2 and 3", "1, 2 and 4".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// --- the film, by number and by name ------------------------------------------------------------

fn film(app: &App, id: &str) -> Result<Project, String> {
    load(app, id).ok_or_else(|| "The film is gone: its folder was moved or removed.".to_string())
}

/// The folder of a film, for a task that works on it.
pub fn folder_of(app: &App, id: &str) -> Option<String> {
    load(app, id).map(|p| p.folder)
}

/// Where a shot is in the film, from its number (1 is the first: "3", "shot 3", 3) or its id.
fn shot_at(p: &Project, wanted: &Value) -> Result<usize, String> {
    if p.shots.is_empty() {
        return Err("The film has no shots yet. add_shot makes one.".into());
    }
    let asked = match wanted {
        Value::Number(n) => n.as_f64().filter(|f| f.fract() == 0.0).map_or_else(|| n.to_string(), |f| (f as i64).to_string()),
        Value::String(s) => s.trim().to_string(),
        Value::Null => return Err("\"shot\" is needed: the number of the shot, 1 for the first.".into()),
        other => other.to_string(),
    };
    if let Some(at) = p.shots.iter().position(|s| s.id == asked) {
        return Ok(at);
    }
    let lower = asked.to_lowercase();
    let bare = lower.trim_start_matches("shot").trim_start_matches(['#', ' ']).trim();
    match bare.parse::<usize>() {
        Ok(n) if (1..=p.shots.len()).contains(&n) => Ok(n - 1),
        _ => Err(format!("There is no shot {asked}. The shots are 1 to {}.", p.shots.len())),
    }
}

/// A member of the cast by name, whatever the case.
fn member<'a>(p: &'a Project, name: &str) -> Result<&'a Member, String> {
    let name = one_row(name, 40);
    p.cast.iter().find(|m| m.name.to_lowercase() == name.to_lowercase()).ok_or_else(|| {
        if p.cast.is_empty() {
            format!("No one named {name} is in the cast: the film has no cast yet. cast_add adds someone.")
        } else {
            format!(
                "No one named {name} is in the cast. The cast: {}. cast_add adds someone.",
                p.cast.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
            )
        }
    })
}

/// Who says a line: a member's id, or empty for the narrator.
fn speaker(p: &Project, who: &str) -> Result<String, String> {
    let w = who.trim().to_lowercase();
    if ["", "narrator", "the narrator", "voice", "voice-over", "voiceover", "voice over"].contains(&w.as_str()) {
        return Ok(String::new());
    }
    member(p, who).map(|m| m.id.clone())
}

fn speaker_name<'a>(p: &'a Project, who: &str) -> &'a str {
    p.cast.iter().find(|m| m.id == who).map_or("Narrator", |m| m.name.as_str())
}

fn said_by(p: &Project, line: &Spoken) -> String {
    format!("{}: \"{}\"", speaker_name(p, &line.who), line.text)
}

/// The lines in `args[key]`: `[{"who", "text"}]`, or one such object.
fn lines_in(p: &Project, value: &Value) -> Result<Vec<SpokenIn>, String> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        Value::String(text) => return Ok(vec![SpokenIn { text: text.clone(), ..SpokenIn::default() }]),
        _ => return Err("A line is {\"who\": a name or \"narrator\", \"text\": what is said}.".into()),
    };
    let mut out = Vec::new();
    for item in items {
        let line_text = item["text"].as_str().or_else(|| item.as_str()).unwrap_or("").trim();
        if line_text.is_empty() {
            return Err("A line needs \"text\": what is said.".into());
        }
        out.push(SpokenIn {
            id: None,
            who: speaker(p, item["who"].as_str().unwrap_or(""))?,
            text: line_text.into(),
            said: Default::default(),
        });
    }
    if out.len() > words::MAX_LINES {
        return Err(format!("A shot holds at most {} lines.", words::MAX_LINES));
    }
    Ok(out)
}

fn kept_line(l: &Spoken) -> SpokenIn {
    SpokenIn { id: Some(l.id.clone()), who: l.who.clone(), text: l.text.clone(), said: l.said.clone() }
}

/// Whether `name` stands in `text` as a word of its own: "Mira" is in "Mira's boat", not in "admiral".
fn names(text: &str, name: &str) -> bool {
    !name.is_empty() && cast::named(&[Member { name: name.to_string(), ..Member::default() }], text).len() == 1
}

/// `text` with every `old` that stands as a word of its own made `new`.
fn rename_in(text: &str, old: &str, new: &str) -> String {
    let lower = text.to_lowercase();
    let wanted = old.to_lowercase();
    if wanted.is_empty() || lower.len() != text.len() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(&wanted) {
        let (start, end) = (from + found, from + found + wanted.len());
        let before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        out.push_str(&text[from..start]);
        out.push_str(if before && after { new } else { &text[start..end] });
        from = end;
    }
    out.push_str(&text[from..]);
    out
}

/// The installed video model's own lengths and usual length, as its package says.
fn video_defaults(app: &App) -> Value {
    installed_one(app, Modality::Video).map(|p| p.defaults.clone()).unwrap_or(Value::Null)
}

/// The length a new shot gets: the video model's usual one.
fn usual_frames(app: &App) -> u32 {
    video_defaults(app)["frames"].as_u64().map(|f| f as u32).filter(|f| (5..=241).contains(f)).unwrap_or(121)
}

/// Frames for a length in seconds: the nearest the video model makes (4k+1 frames at 24 a second).
fn frames_for(app: &App, wanted: f64) -> Result<u32, String> {
    if !wanted.is_finite() || !(0.2..=10.05).contains(&wanted) {
        return Err("A shot is 0.2 to 10 seconds long.".into());
    }
    let frames = wanted * 24.0;
    let presets: Vec<u32> = video_defaults(app)["lengths"]
        .as_array()
        .map(|l| l.iter().filter_map(|x| x["frames"].as_u64().map(|f| f as u32)).collect())
        .unwrap_or_default();
    let made = match presets.iter().min_by(|a, b| (**a as f64 - frames).abs().total_cmp(&(**b as f64 - frames).abs())) {
        Some(nearest) => *nearest,
        None => ((frames - 1.0) / 4.0).round().max(1.0) as u32 * 4 + 1,
    };
    Ok(made.clamp(5, 241))
}

/// One change to the film: loaded, changed through `apply_save`, stored. A made shot that the
/// change marks as changed is said, so the model knows it has to be made again.
fn change(app: &App, id: &str, f: impl FnOnce(&Project) -> Result<(Save, Done), String>) -> Result<Done, String> {
    let mut p = film(app, id)?;
    let made: Vec<String> = p.shots.iter().filter(|s| s.state == "done").map(|s| s.id.clone()).collect();
    let (body, mut done) = f(&p)?;
    apply_save(app, &mut p, body)?;
    store(&mut p).map_err(|e| format!("The film could not be saved: {e}"))?;
    let changed: Vec<String> = p
        .shots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.state == "changed" && made.contains(&s.id))
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    if !changed.is_empty() {
        let line = if changed.len() == 1 {
            format!("Shot {} was made before: it is marked changed until it is made again.", changed[0])
        } else {
            format!("Shots {} were made before: they are marked changed until they are made again.", and_list(&changed))
        };
        done.more = if done.more.is_empty() { line } else { format!("{}\n{line}", done.more) };
    }
    Ok(done)
}

/// The shots as they stand, with the one at `at` changed.
fn shots_with(p: &Project, at: usize, shot: super::ShotIn) -> Vec<super::ShotIn> {
    let mut shots: Vec<super::ShotIn> = p.shots.iter().map(shot_in).collect();
    shots[at] = shot;
    shots
}

// --- reading the film ---------------------------------------------------------------------------

/// How long a shot is to be: its clip's length while that is in use, else the length asked for
/// (a made shot whose length was changed is that long once it is made again).
fn asked(s: &Shot) -> f32 {
    if s.state == "done" {
        s.length()
    } else {
        s.frames as f32 / 24.0
    }
}

/// The length of a shot in words, with its clip's when that is still another.
fn length_of(s: &Shot) -> String {
    let (to_be, now) = (asked(s), s.length());
    if (to_be - now).abs() >= 0.05 {
        format!("{to_be:.1} s (its clip from before is {now:.1} s)")
    } else {
        format!("{to_be:.1} s")
    }
}

fn state_of(s: &Shot, waiting: &[String], making: Option<&str>) -> &'static str {
    if making == Some(s.id.as_str()) {
        return "being made";
    }
    if waiting.contains(&s.id) {
        return "waiting to be made";
    }
    match s.state.as_str() {
        "done" => "made",
        "changed" => "changed since it was made",
        "failed" => "failed to make",
        _ => "not made",
    }
}

/// The film in words: on every turn (`full` false, descriptions cut) and for `read_film`.
fn describe(p: &Project, full: bool) -> String {
    let (waiting, making) = with_queue(|q| {
        (
            q.waiting.iter().filter(|(f, _)| *f == p.id).map(|(_, s)| s.clone()).collect::<Vec<_>>(),
            q.now.as_ref().filter(|(f, _, _)| *f == p.id).map(|(_, s, _)| s.clone()),
        )
    });
    let total: f32 = dub::film_times(&p.shots).last().map_or(0.0, |(start, length)| start + length);
    let mut out =
        vec![format!("\"{}\": {} shots, {:.1} s, {} pixels. Lines are in {}.", p.name, p.shots.len(), total, p.size, dub::own_language(p))];
    out.push(if p.look.is_empty() { "Look: none set.".into() } else { format!("Look: {}", p.look) });
    if p.cast.is_empty() {
        out.push("Cast: no one yet.".into());
    } else {
        let who: Vec<String> = p
            .cast
            .iter()
            .map(|m| if m.look.is_empty() { format!("{} (no look yet)", m.name) } else { format!("{} ({})", m.name, cut(&m.look, 90)) })
            .collect();
        out.push(format!("Cast: {}", who.join("; ")));
    }
    if p.shots.is_empty() {
        out.push("Shots: none yet.".into());
    } else {
        out.push("Shots:".into());
    }
    let shown = if full { p.shots.len() } else { BRIEF_SHOTS };
    for (i, s) in p.shots.iter().enumerate().take(shown) {
        let mut row = format!("{}. {}, {}", i + 1, length_of(s), state_of(s, &waiting, making.as_deref()));
        if s.continues && i > 0 {
            row.push_str(&format!(", continues shot {i}"));
        }
        let prompt = if s.prompt.is_empty() {
            "(no description)".to_string()
        } else if full {
            s.prompt.clone()
        } else {
            cut(&s.prompt, BRIEF_CHARS)
        };
        row.push_str(&format!(". {prompt}"));
        if !s.lines.is_empty() {
            let lines: Vec<String> = s.lines.iter().map(|l| if full { said_by(p, l) } else { cut(&said_by(p, l), 80) }).collect();
            row.push_str(&format!(" Lines: {}", lines.join(" ")));
        }
        out.push(row);
    }
    if p.shots.len() > shown {
        out.push(format!("…and {} more shots: read_film lists them all.", p.shots.len() - shown));
    }
    let tracks: Vec<String> = p
        .tracks
        .iter()
        .map(|t| format!("{} ({} {})", t.name, t.regions.len(), if t.regions.len() == 1 { "region" } else { "regions" }))
        .collect();
    if !tracks.is_empty() {
        out.push(format!("Sound tracks: {}", tracks.join(", ")));
    }
    out.join("\n")
}

/// The film as the model is shown it on every turn.
pub fn context(app: &App, id: &str) -> String {
    match load(app, id) {
        Some(p) => describe(&p, false),
        None => "The film is gone: its folder was moved or removed.".into(),
    }
}

/// `read_film`: the whole film.
pub fn read_film(app: &App, id: &str) -> Result<String, String> {
    Ok(describe(&film(app, id)?, true))
}

/// `read_shot`: one shot with everything about it.
pub fn read_shot(app: &App, id: &str, args: &Value) -> Result<String, String> {
    let p = film(app, id)?;
    let at = shot_at(&p, &args["shot"])?;
    let s = &p.shots[at];
    let (waiting, making) = with_queue(|q| {
        (
            q.waiting.iter().filter(|(f, _)| *f == p.id).map(|(_, s)| s.clone()).collect::<Vec<_>>(),
            q.now.as_ref().filter(|(f, _, _)| *f == p.id).map(|(_, s, _)| s.clone()),
        )
    });
    let mut out = vec![format!("Shot {} of {}: {}, {}.", at + 1, p.shots.len(), length_of(s), state_of(s, &waiting, making.as_deref()))];
    out.push(format!("Description: {}", if s.prompt.is_empty() { "(none)" } else { &s.prompt }));
    let in_it: Vec<&str> = cast::named(&p.cast, &s.prompt).iter().map(|m| m.name.as_str()).collect();
    if !in_it.is_empty() {
        out.push(format!("In it: {}", in_it.join(", ")));
    }
    out.push(match (at, s.continues, s.join.as_str()) {
        (0, _, _) => "It opens the film.".to_string(),
        (_, true, _) => format!("It continues shot {at}: it starts from its last picture."),
        (_, _, "dissolve") => format!("It dissolves in from shot {at}."),
        (_, _, "black") => format!("It comes in through black after shot {at}."),
        _ => format!("It follows shot {at} with a cut."),
    });
    if s.lines.is_empty() {
        out.push("Lines: none.".into());
    } else {
        out.push("Lines:".into());
        for l in &s.lines {
            let other: Vec<String> = l.said.iter().map(|(lang, t)| format!("{lang}: \"{t}\"")).collect();
            let other = if other.is_empty() { String::new() } else { format!(" ({})", other.join("; ")) };
            out.push(format!("- {}{other}", said_by(&p, l)));
        }
    }
    if !s.takes.is_empty() {
        out.push(format!("Takes: {} (take {} in use).", s.takes.len(), s.take));
    }
    if let Some(e) = &s.error {
        out.push(format!("The last try to make it said: {e}"));
    }
    Ok(out.join("\n"))
}

/// What cannot be done on this computer now, as lines for the model: no video model, no voice.
pub fn cannot(app: &App) -> Vec<String> {
    let mut out = Vec::new();
    if installed_one(app, Modality::Video).is_err() {
        out.push("No video model is installed: make_shots refuses until the person adds one under Models.".to_string());
    }
    if installed_one(app, Modality::Voice).is_err() {
        out.push("No voice is installed: dub cannot read lines aloud.".to_string());
    }
    out
}

/// How many shots the film has as a request is made: what the person says refers to that.
pub fn count(app: &App, id: &str) -> usize {
    load(app, id).map_or(0, |p| p.shots.len())
}

/// The shots that are selected in the tracks, by number, for the request that goes with them.
pub fn selection(app: &App, id: &str, shots: &[String]) -> Option<String> {
    let p = load(app, id)?;
    let numbers: Vec<String> =
        p.shots.iter().enumerate().filter(|(_, s)| shots.contains(&s.id)).map(|(i, _)| (i + 1).to_string()).collect();
    match numbers.len() {
        0 => None,
        1 => Some(format!("Shot {} is selected", numbers[0])),
        _ => Some(format!("Shots {} are selected", and_list(&numbers))),
    }
}

// --- changing shots -----------------------------------------------------------------------------

/// `edit_shot`: the description, the length, the lines, who is in it, whether it continues the one before.
pub fn edit_shot(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let at = shot_at(p, &args["shot"])?;
        let old = &p.shots[at];
        let mut shot = shot_in(old);
        let (mut what, mut diff) = (Vec::<String>::new(), Vec::<String>::new());
        if let Some(described) = text(args, "description") {
            let described: String = one_row(described, MAX_PROMPT);
            if described.is_empty() {
                return Err("A description cannot be empty: say what is seen.".into());
            }
            if described != old.prompt {
                what.push("description changed".into());
                shot.prompt = described;
            }
        }
        if let Some(names_in) = args["cast"].as_array().or_else(|| args["people"].as_array()) {
            let mut added = Vec::new();
            for name in names_in.iter().filter_map(Value::as_str) {
                let m = member(p, name)?;
                if !names(&shot.prompt, &m.name) {
                    shot.prompt = format!("{} {} is in the shot.", shot.prompt.trim_end(), m.name).trim().to_string();
                    added.push(m.name.clone());
                }
            }
            if !added.is_empty() {
                what.push(format!("{} in it", and_list(&added)));
            }
        }
        if shot.prompt != old.prompt {
            diff.push(format!("- {}\n+ {}", old.prompt, shot.prompt));
        }
        if let Some(wanted) = number(args, "seconds") {
            let frames = frames_for(app, wanted)?;
            // The length it has, given again (a clip of 120 frames is "5 seconds" too), is no change.
            let now = asked(old);
            if (wanted as f32 - now).abs() >= 0.1 && frames != old.frames {
                what.push(format!("{now:.1} s → {}", seconds(frames)));
                diff.push(format!("- {now:.1} s\n+ {}", seconds(frames)));
                shot.frames = Some(frames);
            }
        }
        let mut lines: Option<Vec<SpokenIn>> = None;
        if !args["lines"].is_null() {
            lines = Some(lines_in(p, &args["lines"])?);
        }
        if !args["add_line"].is_null() {
            let mut all: Vec<SpokenIn> = lines.take().unwrap_or_else(|| old.lines.iter().map(kept_line).collect());
            all.extend(lines_in(p, &args["add_line"])?);
            if all.len() > words::MAX_LINES {
                return Err(format!("Shot {} already has {} lines: a shot holds at most {}.", at + 1, old.lines.len(), words::MAX_LINES));
            }
            lines = Some(all);
        }
        if let Some(new_lines) = &lines {
            let before: Vec<String> = old.lines.iter().map(|l| said_by(p, l)).collect();
            let after: Vec<String> =
                new_lines.iter().map(|l| format!("{}: \"{}\"", speaker_name(p, &l.who), one_row(&l.text, words::MAX_LINE))).collect();
            if before != after {
                let added = after.len().saturating_sub(before.len());
                what.push(if before.is_empty() || (added > 0 && after.starts_with(&before)) {
                    if added == 1 {
                        "a line added".into()
                    } else {
                        format!("{added} lines added")
                    }
                } else {
                    "lines changed".into()
                });
                diff.extend(before.iter().map(|l| format!("- {l}")));
                diff.extend(after.iter().filter(|l| !before.contains(l)).map(|l| format!("+ {l}")));
                shot.lines = lines;
            }
        }
        if let Some(continues) = flag(args, "continues") {
            if continues && at == 0 {
                return Err("Shot 1 opens the film: it has nothing to continue.".into());
            }
            if continues != old.continues {
                what.push(if continues { format!("continues shot {at}") } else { "starts with a cut".into() });
                shot.continues = continues;
            }
        }
        if what.is_empty() {
            return Err(format!(
                "Nothing changed: shot {} is like that already. Give what to change: description, seconds, lines, add_line, cast or continues.",
                at + 1
            ));
        }
        let shots = shots_with(p, at, shot);
        let done = Done { said: format!("Shot {}: {}", at + 1, what.join(", ")), diff: diff.join("\n"), more: String::new() };
        Ok((Save { shots: Some(shots), ..Save::default() }, done))
    })
}

/// `add_shot`: a new shot at the end, or after the one named.
pub fn add_shot(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        if p.shots.len() >= MAX_SHOTS {
            return Err(format!("A film has at most {MAX_SHOTS} shots."));
        }
        let described: String = one_row(text(args, "description").unwrap_or(""), MAX_PROMPT);
        if described.is_empty() {
            return Err("\"description\" is needed: what is seen in the new shot.".into());
        }
        let at = match &args["after"] {
            Value::Null => p.shots.len(),
            Value::Number(n) if n.as_u64() == Some(0) => 0,
            Value::String(s) if s.trim() == "0" => 0,
            after => shot_at(p, after)? + 1,
        };
        let frames = match number(args, "seconds") {
            Some(s) => frames_for(app, s)?,
            None => usual_frames(app),
        };
        let lines = if args["lines"].is_null() { None } else { Some(lines_in(p, &args["lines"])?) };
        let continues = flag(args, "continues").unwrap_or(false) && at > 0;
        let new = super::ShotIn { prompt: described.clone(), frames: Some(frames), continues, lines, ..super::ShotIn::default() };
        let mut shots: Vec<super::ShotIn> = p.shots.iter().map(shot_in).collect();
        shots.insert(at, new);
        let more = if at + 1 < shots.len() {
            format!("The shots after it moved down by one: the film has {} shots now.", shots.len())
        } else {
            format!("The film has {} shots now.", shots.len())
        };
        let done = Done { said: format!("Added shot {}", at + 1), diff: format!("+ {described}"), more };
        Ok((Save { shots: Some(shots), ..Save::default() }, done))
    })
}

/// `remove_shot`: takes one shot out. Its clip stays in the folder; an undo puts it back.
pub fn remove_shot(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let at = shot_at(p, &args["shot"])?;
        let shots: Vec<super::ShotIn> = p.shots.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, s)| shot_in(s)).collect();
        let left = shots.len();
        let more = match (left, at + 1 == p.shots.len()) {
            (0, _) => "The film has no shots now.".to_string(),
            (_, true) => format!("It was the last one. The film has {left} shots now, 1 to {left}: this part of the request is done."),
            _ => format!("The shots after it moved up by one: the film has {left} shots now, 1 to {left}."),
        };
        let done = Done { said: format!("Removed shot {}", at + 1), diff: format!("- {}", p.shots[at].prompt), more };
        Ok((Save { shots: Some(shots), ..Save::default() }, done))
    })
}

/// `move_shot`: puts a shot at another place; `to` is its number afterwards.
pub fn move_shot(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let at = shot_at(p, &args["shot"])?;
        let to = shot_at(p, &args["to"]).map_err(|_| format!("\"to\" is the place it goes to: 1 to {}.", p.shots.len()))?;
        if to == at {
            return Err(format!("Shot {} is there already.", at + 1));
        }
        let mut shots: Vec<super::ShotIn> = p.shots.iter().map(shot_in).collect();
        let moving = shots.remove(at);
        shots.insert(to, moving);
        // The first shot has nothing to continue: `merge` sees to that.
        let done = Done { said: format!("Moved shot {} to {}", at + 1, to + 1), ..Done::default() };
        Ok((Save { shots: Some(shots), ..Save::default() }, done))
    })
}

/// `rewrite_shot`: the chat model writes the description again, in the words of the shots
/// around it, with what the person wants different; the new one goes in like any edit.
pub async fn rewrite_shot(app: &Shared, id: &str, args: &Value) -> Result<Done, String> {
    let p = film(app, id)?;
    let at = shot_at(&p, &args["shot"])?;
    let shot = p.shots[at].id.clone();
    let hint: String = one_row(text(args, "hint").unwrap_or(""), 400);
    let body = super::Rewrite { hint, prompt: None };
    let written = answer(super::rewrite(State(app.clone()), Path((id.to_string(), shot.clone())), Some(Json(body))).await).await?;
    let prompt = written["prompt"].as_str().unwrap_or("").to_string();
    let mut done = edit_shot(app, id, &serde_json::json!({"shot": shot, "description": prompt}))?;
    done.said = format!("Shot {}: description written again", at + 1);
    Ok(done)
}

// --- the film as a whole ------------------------------------------------------------------------

/// `set_look`: the line every shot shares (style, colours, light, lens). Empty clears it.
pub fn set_look(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let Some(look) = text(args, "look") else { return Err("\"look\" is needed: style, colours, light and lens in one line.".into()) };
        let look: String = one_row(look, 300);
        if look == p.look {
            return Err("The film has that look already.".into());
        }
        let said = if look.is_empty() {
            "Look cleared"
        } else if p.look.is_empty() {
            "Look set"
        } else {
            "Look changed"
        };
        let done = Done { said: said.into(), diff: format!("- {}\n+ {look}", p.look), more: String::new() };
        Ok((Save { look: Some(look), ..Save::default() }, done))
    })
}

/// `set_language`: the language the film's lines are written in.
pub fn set_language(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let language = words::language(text(args, "language").unwrap_or(""));
        if language.is_empty() {
            return Err("\"language\" is needed, by its name: \"Turkish\".".into());
        }
        let was = dub::own_language(p);
        if language == was {
            return Err(format!("The lines are in {language} already."));
        }
        let done = Done { said: format!("Lines are in {language}"), diff: format!("- {was}\n+ {language}"), more: String::new() };
        Ok((Save { language: Some(language), ..Save::default() }, done))
    })
}

fn cast_in(m: &Member) -> MemberIn {
    MemberIn { id: Some(m.id.clone()), name: m.name.clone(), look: m.look.clone(), voice: m.voice }
}

/// `cast_add`: someone new in the cast, with the fixed words for how they look.
pub fn cast_add(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let name = one_row(text(args, "name").unwrap_or(""), 40);
        if name.is_empty() {
            return Err("\"name\" is needed.".into());
        }
        if p.cast.iter().any(|m| m.name.to_lowercase() == name.to_lowercase()) {
            return Err(format!("{name} is in the cast already. cast_edit changes how they look."));
        }
        if p.cast.len() >= cast::MAX_CAST {
            return Err(format!("A film has at most {} characters.", cast::MAX_CAST));
        }
        let look = one_row(text(args, "look").unwrap_or(""), cast::MAX_LOOK);
        let mut all: Vec<MemberIn> = p.cast.iter().map(cast_in).collect();
        all.push(MemberIn { id: None, name: name.clone(), look: look.clone(), voice: None });
        let done = Done {
            said: format!("Added {name} to the cast"),
            diff: if look.is_empty() { String::new() } else { format!("+ {look}") },
            more: String::new(),
        };
        Ok((Save { cast: Some(all), ..Save::default() }, done))
    })
}

/// `cast_edit`: how someone looks, or their name; a new name is changed in the shots that name them.
pub fn cast_edit(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let m = member(p, text(args, "name").unwrap_or(""))?;
        let mut all: Vec<MemberIn> = p.cast.iter().map(cast_in).collect();
        let at = p.cast.iter().position(|x| x.id == m.id).unwrap_or(0);
        let (mut what, mut diff) = (Vec::<String>::new(), Vec::<String>::new());
        let mut body = Save::default();
        if let Some(look) = text(args, "look").map(|l| one_row(l, cast::MAX_LOOK)) {
            if look != m.look {
                what.push("look changed".to_string());
                diff.push(format!("- {}\n+ {look}", m.look));
                all[at].look = look;
            }
        }
        if let Some(new_name) = text(args, "new_name").map(|n| one_row(n, 40)).filter(|n| !n.is_empty() && *n != m.name) {
            if p.cast.iter().any(|x| x.id != m.id && x.name.to_lowercase() == new_name.to_lowercase()) {
                return Err(format!("{new_name} is in the cast already."));
            }
            let shots: Vec<super::ShotIn> =
                p.shots.iter().map(|s| super::ShotIn { prompt: rename_in(&s.prompt, &m.name, &new_name), ..shot_in(s) }).collect();
            let renamed = p.shots.iter().filter(|s| names(&s.prompt, &m.name)).count();
            what.push(match renamed {
                0 => format!("now {new_name}"),
                1 => format!("now {new_name}, in 1 shot"),
                n => format!("now {new_name}, in {n} shots"),
            });
            diff.push(format!("- {}\n+ {new_name}", m.name));
            all[at].name = new_name;
            body.shots = Some(shots);
        }
        if what.is_empty() {
            return Err(format!("Nothing changed: give {} a new \"look\" or a \"new_name\".", m.name));
        }
        body.cast = Some(all);
        Ok((body, Done { said: format!("{}: {}", m.name, what.join(", ")), diff: diff.join("\n"), more: String::new() }))
    })
}

/// `subtitles`: whether the lines are shown as subtitles, and burned into the picture on export.
pub fn subtitles(app: &App, id: &str, args: &Value) -> Result<Done, String> {
    change(app, id, |p| {
        let mut wanted = p.subtitles.clone();
        if let Some(show) = flag(args, "show") {
            wanted.show = show;
        }
        if let Some(burn) = flag(args, "burn") {
            wanted.burn = burn;
        }
        if wanted == p.subtitles {
            return Err(format!(
                "Subtitles are {} already{}.",
                if wanted.show { "shown" } else { "off" },
                if wanted.burn { ", burned into the picture" } else { "" }
            ));
        }
        let lines: usize = p.shots.iter().map(|s| s.lines.len()).sum();
        let said = match (wanted.show, wanted.burn) {
            (_, true) => "Subtitles burned into the picture on export",
            (true, false) => "Subtitles shown",
            (false, false) => "Subtitles off",
        };
        let more = if lines == 0 { "The film has no lines yet, so there is nothing to show.".to_string() } else { String::new() };
        Ok((Save { subtitles: Some(wanted), ..Save::default() }, Done { said: said.into(), diff: String::new(), more }))
    })
}

fn track_in(t: &Track) -> TrackIn {
    TrackIn {
        id: Some(t.id.clone()),
        name: t.name.clone(),
        color: t.color.clone(),
        volume: Some(t.volume),
        pan: t.pan,
        mute: t.mute,
        solo: t.solo,
        regions: t
            .regions
            .iter()
            .map(|r| RegionIn {
                id: Some(r.id.clone()),
                file: r.file.clone(),
                start: r.start,
                offset: r.offset,
                length: r.length,
                gain: Some(r.gain),
                fade_in: r.fade_in,
                fade_out: r.fade_out,
                loops: Some(r.loops),
            })
            .collect(),
    }
}

/// The sound files a film can use: in its folder and the ones added to it.
fn sound_files(p: &Project) -> Vec<PathBuf> {
    super::library(p)["sounds"]
        .as_array()
        .map(|l| l.iter().filter(|s| s["missing"] != true).filter_map(|s| s["file"].as_str().map(PathBuf::from)).collect())
        .unwrap_or_default()
}

/// The seconds of a plain WAV file, read from its header.
fn wav_seconds(path: &FsPath) -> Option<f32> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    // Chunks one after another; "fmt " says how many bytes make a second, "data" holds the sound.
    let (mut at, mut per_second) = (12, 0u32);
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        match &bytes[at..at + 4] {
            b"fmt " if at + 20 <= bytes.len() => per_second = u32::from_le_bytes(bytes[at + 16..at + 20].try_into().ok()?),
            b"data" if per_second > 0 => return Some(size.min(bytes.len() - at - 8) as f32 / per_second as f32),
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    None
}

/// `add_sound_region`: a sound file of the film's library laid on a sound track, at a moment
/// or at the start of a shot, as long as the file is.
pub async fn add_sound_region(app: &Shared, id: &str, args: &Value) -> Result<Done, String> {
    let p = film(app, id)?;
    let wanted = text(args, "file").unwrap_or("").to_string();
    if wanted.is_empty() {
        return Err("\"file\" is needed: the name of a sound file of the film.".into());
    }
    let files = sound_files(&p);
    let name_of = |f: &PathBuf| f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let Some(file) = files.iter().find(|f| f.to_string_lossy() == wanted || name_of(f).to_lowercase() == wanted.to_lowercase()).cloned()
    else {
        return Err(if files.is_empty() {
            "The film has no sound files: put one in its folder, or add one in the Library.".into()
        } else {
            format!(
                "There is no sound file named {wanted}. The film's sound files: {}.",
                files.iter().take(20).map(name_of).collect::<Vec<_>>().join(", ")
            )
        });
    };
    let length = match wav_seconds(&file) {
        Some(s) => Some(s),
        None => super::peaks_of(app, &p, &file, None).await.map(|(_, s)| s),
    };
    let length = length.filter(|s| *s >= 0.05).ok_or_else(|| format!("{} could not be read.", name_of(&file)))?;
    let times = dub::film_times(&p.shots);
    let (start, place) = match (number(args, "at"), args["shot"].is_null()) {
        (Some(at), _) if at.is_finite() && at >= 0.0 => (at as f32, format!("at {at:.1} s")),
        (Some(_), _) => return Err("\"at\" is seconds into the film, 0 or more.".into()),
        (None, false) => {
            let at = shot_at(&p, &args["shot"])?;
            (times[at].0, format!("at shot {}", at + 1))
        }
        (None, true) => (0.0, "at the start".to_string()),
    };
    let gain = number(args, "gain").map(|g| g as f32);
    let track_name = one_row(text(args, "track").unwrap_or(""), 40);
    let region = RegionIn { file: file.to_string_lossy().to_string(), start, length, gain, ..RegionIn::default() };
    let done = change(app, id, move |p| {
        let mut tracks: Vec<TrackIn> = p.tracks.iter().map(track_in).collect();
        let found = p
            .tracks
            .iter()
            .position(|t| t.kind != "voice" && (track_name.is_empty() || t.name.to_lowercase() == track_name.to_lowercase()));
        let on = match found.filter(|_| !track_name.is_empty() || p.tracks.iter().filter(|t| t.kind != "voice").count() == 1) {
            Some(at) => {
                tracks[at].regions.push(region);
                p.tracks[at].name.clone()
            }
            None => {
                let name = if track_name.is_empty() { format!("Sound {}", p.tracks.len() + 1) } else { track_name };
                tracks.push(TrackIn {
                    name: name.clone(),
                    color: sound::next_color(p.tracks.len()),
                    regions: vec![region],
                    ..TrackIn::default()
                });
                name
            }
        };
        let done = Done::new(format!("Added {} {place} on {on}", name_of(&file)));
        Ok((Save { tracks: Some(tracks), ..Save::default() }, done))
    })?;
    Ok(Done { more: format!("It plays for {length:.1} s."), ..done })
}

// --- what takes long: making, dubbing, exporting ------------------------------------------------

/// What a handler of the film answered: its JSON, or the sentence it refused with.
async fn answer(response: Response) -> Result<Value, String> {
    let ok = response.status().is_success();
    let body = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024).await.map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if ok {
        Ok(v)
    } else {
        Err(v["error"]["message"].as_str().unwrap_or("That did not work.").to_string())
    }
}

fn minutes(seconds: f32) -> String {
    let m = (seconds / 60.0).round() as u32;
    match m {
        0 => "under a minute".into(),
        1 => "about a minute".into(),
        m if m < 90 => format!("about {m} minutes"),
        m => format!("about {:.1} hours", m as f32 / 60.0),
    }
}

/// What `make_shots` would queue, and the question the person is asked first.
pub fn make_plan(app: &App, id: &str, args: &Value) -> Result<(Vec<String>, String), String> {
    let p = film(app, id)?;
    installed_one(app, Modality::Video).map_err(|_| missing(Modality::Video).to_string())?;
    let ids: Vec<String> = match args["shots"].as_array() {
        Some(list) if !list.is_empty() => {
            let mut out = Vec::new();
            for wanted in list {
                let at = shot_at(&p, wanted)?;
                if p.shots[at].prompt.is_empty() {
                    return Err(format!("Shot {} has no description: give it one first.", at + 1));
                }
                if !out.contains(&p.shots[at].id) {
                    out.push(p.shots[at].id.clone());
                }
            }
            out
        }
        _ => p.shots.iter().filter(|s| s.state != "done" && !s.prompt.is_empty()).map(|s| s.id.clone()).collect(),
    };
    if ids.is_empty() {
        return Err("Every shot is made already. Change a shot, or name the shots to make again.".into());
    }
    let seen = super::paces_for(app, &p);
    let time: f32 = p
        .shots
        .iter()
        .enumerate()
        .filter(|(_, s)| ids.contains(&s.id))
        .map(|(i, s)| super::pace::estimate(&seen, s.frames, s.continues && i > 0))
        .sum();
    let numbers: Vec<String> = p.shots.iter().enumerate().filter(|(_, s)| ids.contains(&s.id)).map(|(i, _)| (i + 1).to_string()).collect();
    let what = if ids.len() == 1 { format!("shot {}", numbers[0]) } else { format!("{} shots ({})", ids.len(), and_list(&numbers)) };
    Ok((ids, format!("Make {what}: {}", minutes(time))))
}

/// `make_shots`: the shots go into the queue and are made one after another; it does not wait.
pub async fn make_shots(app: &Shared, id: &str, ids: Vec<String>) -> Result<Done, String> {
    let count = ids.len();
    let body = super::Render { shots: Some(ids), model: None };
    answer(super::render(State(app.clone()), Path(id.to_string()), Some(Json(body))).await).await?;
    Ok(Done {
        said: format!("Queued {count} {}", if count == 1 { "shot" } else { "shots" }),
        more: "They are made one after another while you go on; the person sees them come in.".into(),
        ..Done::default()
    })
}

/// What `dub` would do, and the question the person is asked first: (language, question).
pub fn dub_plan(app: &App, id: &str, args: &Value) -> Result<(String, String), String> {
    let p = film(app, id)?;
    let lines: usize = p.shots.iter().map(|s| s.lines.len()).sum();
    if lines == 0 {
        return Err("This film has no spoken lines yet. Write a line under a shot first.".into());
    }
    let own = dub::own_language(&p);
    let language = text(args, "language").map(words::language).filter(|l| !l.is_empty()).unwrap_or_else(|| own.clone());
    let translated = !language.eq_ignore_ascii_case(&own);
    if installed_one(app, Modality::Voice).is_err() && !(translated && !dub::voice_reads(&language)) {
        return Err(missing(Modality::Voice).into());
    }
    if translated && installed_one(app, Modality::Chat).is_err() {
        return Err(missing(Modality::Chat).into());
    }
    let count = if lines == 1 { "1 line".to_string() } else { format!("{lines} lines") };
    let question = match (translated, dub::voice_reads(&language)) {
        (false, _) => format!("Dub the film: the voice reads {count} in {language}"),
        (true, true) => format!("Translate {count} into {language} and read them aloud"),
        (true, false) => format!("Translate {count} into {language} for the subtitles"),
    };
    Ok((language, question))
}

/// Waits for long work on the film (a dub, an export) to end, and answers with its state.
async fn wait_for(mut state: impl FnMut() -> Value) -> Value {
    let started = std::time::Instant::now();
    loop {
        let now = state();
        if now["state"] != "working" || started.elapsed() > LONG_WAIT {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
}

/// `dub`: the voice reads every line onto a voice track (another language is translated first).
pub async fn dub(app: &Shared, id: &str, language: String) -> Result<Done, String> {
    let body: dub::DubBody = serde_json::from_value(serde_json::json!({"language": language})).unwrap_or_default();
    answer(dub::dub(State(app.clone()), Path(id.to_string()), Some(Json(body))).await).await?;
    let ended = wait_for(|| job_of(id, "dub")).await;
    match ended["state"].as_str() {
        Some("done") if ended["spoken"] == false => Ok(Done::new(format!("Translated into {language} for the subtitles"))),
        Some("done") => {
            let n = ended["lines"].as_u64().unwrap_or(0);
            Ok(Done::new(format!("Dubbed {n} {} in {language}", if n == 1 { "line" } else { "lines" })))
        }
        Some("failed") => Err(ended["error"].as_str().unwrap_or("The dub did not work.").to_string()),
        _ => Ok(Done::new(format!("Dubbing in {language} goes on"))),
    }
}

/// The question asked before `export`.
pub fn export_plan(app: &App, id: &str) -> Result<String, String> {
    let p = film(app, id)?;
    let made = exported(&p).len();
    if made == 0 {
        return Err("No shot is made yet. Make the shots first.".into());
    }
    let file = export_path(&p, &p.export);
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let left = p.shots.len() - made;
    let out = match left {
        0 => String::new(),
        1 => " (1 shot is not made and is left out)".to_string(),
        n => format!(" ({n} shots are not made and are left out)"),
    };
    Ok(format!("Export {made} {} to {name}{out}", if made == 1 { "shot" } else { "shots" }))
}

/// `export`: joins the made shots and the sound into the film's MP4, and waits for it.
pub async fn export(app: &Shared, id: &str) -> Result<Done, String> {
    answer(super::export(State(app.clone()), Path(id.to_string()), None).await).await?;
    let ended = wait_for(|| export_of(id)).await;
    match ended["state"].as_str() {
        Some("done") => {
            let file = ended["file"].as_str().unwrap_or("");
            let name = FsPath::new(file).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Ok(Done { said: format!("Exported {name}"), more: format!("The file is {file}."), ..Done::default() })
        }
        Some("failed") => Err(ended["error"].as_str().unwrap_or("The export did not work.").to_string()),
        _ => Ok(Done::new("The export goes on")),
    }
}

// --- looking at a shot --------------------------------------------------------------------------

/// `look_at_shot`: one picture of a made shot (its middle, or `at` seconds into it), as PNG, and
/// what it is a picture of.
pub async fn look_at_shot(app: &Shared, id: &str, args: &Value) -> Result<(Vec<u8>, String), String> {
    let p = film(app, id)?;
    let at = shot_at(&p, &args["shot"])?;
    let s = &p.shots[at];
    let clip = super::clip_path(&p, &s.id);
    if !(s.state == "done" || s.state == "changed") || !clip.is_file() {
        return Err(format!("Shot {} is not made yet: there is no picture of it. read_shot says what it is meant to show.", at + 1));
    }
    let length = s.length();
    let when = number(args, "at").map(|t| t as f32).unwrap_or(length / 2.0).clamp(0.0, (length - 0.05).max(0.0));
    // The picture at the film's shape, at most 512 wide; the packaged FFmpeg writes no PNG, so it
    // hands over the pixels and they are put in a PNG here.
    let (fw, fh) = p.size.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).unwrap_or((640, 384));
    let width = fw.min(512);
    let height = ((fh as f32 * width as f32 / fw as f32 / 2.0).round() as u32 * 2).max(2);
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await?;
    let out = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{when:.3}"), "-i"])
        .arg(&clip)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:{height}"), "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("The picture could not be taken: {e}"))?;
    let wanted = (width * height * 3) as usize;
    if !out.status.success() || out.stdout.len() < wanted {
        return Err(format!("The picture could not be taken from shot {}.", at + 1));
    }
    Ok((png(width, height, &out.stdout[..wanted]), format!("shot {} at {when:.1} s", at + 1)))
}

/// A PNG of RGB pixels, stored without compression: what the model is shown and the app keeps.
fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    fn crc32(parts: &[&[u8]]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in parts.iter().flat_map(|p| p.iter()) {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend(crc32(&[kind, data]).to_be_bytes());
    }
    // Every row starts with filter 0; then zlib with stored blocks and its Adler-32.
    let row = width as usize * 3;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgb.chunks(row).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let n = block.len() as u16;
        z.extend(n.to_le_bytes());
        z.extend((!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut head = Vec::with_capacity(13);
    head.extend(width.to_be_bytes());
    head.extend(height.to_be_bytes());
    head.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &head);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::bare_app;
    use super::super::{create, index, NewProject};
    use super::*;
    use serde_json::json;

    /// A film with three shots, one character and one line, in a folder of its own.
    fn a_film(video: bool) -> (Shared, PathBuf, String) {
        let (app, dir) = bare_app(video);
        let folder = dir.join("film");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(create(State(app.clone()), Json(NewProject { folder: folder.to_string_lossy().into(), name: "Rain".into() })));
        let id = index(&app)[0].0.clone();
        let mut p = load(&app, &id).unwrap();
        let body: Save = serde_json::from_value(json!({
            "cast": [{"id": "m1", "name": "Mira", "look": "a girl of eight in a yellow raincoat"}],
            "shots": [
                {"id": "a1", "prompt": "A quiet street in the rain.", "frames": 121},
                {"id": "a2", "prompt": "Mira splashes through a puddle.", "frames": 121, "lines": [{"who": "m1", "text": "Look!"}]},
                {"id": "a3", "prompt": "A paper boat floats away.", "frames": 73},
            ],
        }))
        .unwrap();
        apply_save(&app, &mut p, body).unwrap();
        store(&mut p).unwrap();
        (app, dir, id)
    }

    #[test]
    fn shots_are_found_by_number_or_id_and_a_wrong_one_is_said() {
        let (app, dir, id) = a_film(false);
        let p = load(&app, &id).unwrap();
        assert_eq!(shot_at(&p, &json!(2)), Ok(1));
        assert_eq!(shot_at(&p, &json!("3")), Ok(2));
        assert_eq!(shot_at(&p, &json!("shot 1")), Ok(0));
        assert_eq!(shot_at(&p, &json!("a2")), Ok(1));
        assert_eq!(shot_at(&p, &json!(9)), Err("There is no shot 9. The shots are 1 to 3.".into()));
        assert!(shot_at(&p, &Value::Null).unwrap_err().contains("\"shot\" is needed"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_film_is_described_with_its_cast_its_shots_and_their_lines() {
        let (app, dir, id) = a_film(false);
        let text = read_film(&app, &id).unwrap();
        assert!(text.starts_with("\"Rain\": 3 shots, 13.1 s, 640x384 pixels. Lines are in English."), "{text}");
        assert!(text.contains("Cast: Mira (a girl of eight in a yellow raincoat)"), "{text}");
        assert!(text.contains("2. 5.0 s, not made. Mira splashes through a puddle. Lines: Mira: \"Look!\""), "{text}");
        let shot = read_shot(&app, &id, &json!({"shot": 2})).unwrap();
        assert!(
            shot.contains("In it: Mira") && shot.contains("It follows shot 1 with a cut.") && shot.contains("- Mira: \"Look!\""),
            "{shot}"
        );
        assert_eq!(selection(&app, &id, &["a3".into(), "a1".into()]).as_deref(), Some("Shots 1 and 3 are selected"));
        assert_eq!(selection(&app, &id, &["zz".into()]), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_shot_is_edited_through_the_workstation_path_and_a_made_one_is_marked_changed() {
        let (app, dir, id) = a_film(false);
        // Made shot: changing its description marks it changed, as an edit in the workstation does.
        let mut p = load(&app, &id).unwrap();
        p.shots[0].state = "done".into();
        p.shots[0].made_from = None;
        store(&mut p).unwrap();
        let done = edit_shot(&app, &id, &json!({"shot": 1, "description": "A quiet street at night, in the rain."})).unwrap();
        assert_eq!(done.said, "Shot 1: description changed");
        assert_eq!(done.diff, "- A quiet street in the rain.\n+ A quiet street at night, in the rain.");
        assert!(done.more.contains("marked changed"), "{done:?}");
        let p = load(&app, &id).unwrap();
        assert_eq!((p.shots[0].prompt.as_str(), p.shots[0].state.as_str()), ("A quiet street at night, in the rain.", "changed"));
        // Length, a line, who is in it, continuing: one step, one line.
        let done = edit_shot(&app, &id, &json!({"shot": 3, "seconds": "7", "add_line": {"who": "narrator", "text": "And it was gone."}, "cast": ["mira"], "continues": true})).unwrap();
        assert_eq!(done.said, "Shot 3: Mira in it, 3.0 s → 7.0 s, a line added, continues shot 2");
        let s = &load(&app, &id).unwrap().shots[2];
        assert_eq!((s.frames, s.continues, s.prompt.as_str()), (169, true, "A paper boat floats away. Mira is in the shot."));
        assert_eq!((s.lines[0].who.as_str(), s.lines[0].text.as_str()), ("", "And it was gone."));
        // Lines given whole replace the ones there.
        edit_shot(&app, &id, &json!({"shot": 2, "lines": [{"who": "Narrator", "text": "Bir gün..."}]})).unwrap();
        assert_eq!(load(&app, &id).unwrap().shots[1].lines.len(), 1);
        // Refusals: no such shot, no such person, nothing to change, too long, the first shot continuing.
        assert!(edit_shot(&app, &id, &json!({"shot": 7, "description": "x"})).unwrap_err().contains("no shot 7"));
        assert!(edit_shot(&app, &id, &json!({"shot": 2, "add_line": {"who": "Ayşe", "text": "Merhaba"}}))
            .unwrap_err()
            .contains("No one named Ayşe"));
        assert!(edit_shot(&app, &id, &json!({"shot": 2})).unwrap_err().starts_with("Nothing changed"));
        assert_eq!(edit_shot(&app, &id, &json!({"shot": 2, "seconds": 40})).unwrap_err(), "A shot is 0.2 to 10 seconds long.");
        assert!(edit_shot(&app, &id, &json!({"shot": 2, "seconds": 5})).unwrap_err().starts_with("Nothing changed"), "the length it has");
        assert!(edit_shot(&app, &id, &json!({"shot": 1, "continues": true})).unwrap_err().contains("nothing to continue"));
        assert!(edit_shot(&app, &id, &json!({"shot": 1, "description": "  "})).unwrap_err().contains("cannot be empty"));
        // A made shot made longer: it is said at the length asked for, with its clip's, so the model sees the change took.
        let mut p = load(&app, &id).unwrap();
        (p.shots[1].state, p.shots[1].frames, p.shots[1].made_frames) = ("done".into(), 120, Some(120));
        store(&mut p).unwrap();
        assert_eq!(edit_shot(&app, &id, &json!({"shot": 2, "seconds": 10})).unwrap().said, "Shot 2: 5.0 s → 10.0 s");
        assert!(read_shot(&app, &id, &json!({"shot": 2}))
            .unwrap()
            .starts_with("Shot 2 of 3: 10.0 s (its clip from before is 5.0 s), changed"));
        assert!(read_film(&app, &id).unwrap().contains("2. 10.0 s (its clip from before is 5.0 s), changed since it was made."));
        assert!(edit_shot(&app, &id, &json!({"shot": 2, "seconds": 10})).unwrap_err().starts_with("Nothing changed"));
        assert_eq!(edit_shot(&app, &id, &json!({"shot": 2, "seconds": 8})).unwrap().said, "Shot 2: 10.0 s → 8.0 s");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn shots_are_added_removed_and_moved() {
        let (app, dir, id) = a_film(false);
        assert_eq!(add_shot(&app, &id, &json!({"description": "The sun comes out."})).unwrap().said, "Added shot 4");
        assert_eq!(add_shot(&app, &id, &json!({"description": "Thunder.", "after": 1, "seconds": 2})).unwrap().said, "Added shot 2");
        let p = load(&app, &id).unwrap();
        assert_eq!(p.shots.iter().map(|s| s.prompt.as_str()).collect::<Vec<_>>()[..2], ["A quiet street in the rain.", "Thunder."]);
        assert_eq!((p.shots[1].frames, p.shots[4].frames), (49, 121));
        assert!(add_shot(&app, &id, &json!({})).unwrap_err().contains("\"description\" is needed"));
        assert_eq!(move_shot(&app, &id, &json!({"shot": 5, "to": 1})).unwrap().said, "Moved shot 5 to 1");
        assert_eq!(load(&app, &id).unwrap().shots[0].prompt, "The sun comes out.");
        assert_eq!(move_shot(&app, &id, &json!({"shot": 2, "to": 2})).unwrap_err(), "Shot 2 is there already.");
        assert!(move_shot(&app, &id, &json!({"shot": 2, "to": 9})).unwrap_err().contains("\"to\""));
        let done = remove_shot(&app, &id, &json!({"shot": 5})).unwrap();
        assert_eq!((done.said.as_str(), done.diff.as_str()), ("Removed shot 5", "- A paper boat floats away."));
        assert!(done.more.starts_with("It was the last one. The film has 4 shots now"), "{done:?}");
        assert!(remove_shot(&app, &id, &json!({"shot": 1})).unwrap().more.contains("moved up by one"));
        assert_eq!(load(&app, &id).unwrap().shots.len(), 3);
        assert!(remove_shot(&app, &id, &json!({"shot": 0})).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_look_the_language_and_the_cast_change_and_a_new_name_follows_into_the_shots() {
        let (app, dir, id) = a_film(false);
        assert_eq!(set_look(&app, &id, &json!({"look": "night, blue light, wet streets"})).unwrap().said, "Look set");
        assert_eq!(set_look(&app, &id, &json!({"look": "night, blue light, wet streets"})).unwrap_err(), "The film has that look already.");
        assert_eq!(load(&app, &id).unwrap().look, "night, blue light, wet streets");
        assert_eq!(set_language(&app, &id, &json!({"language": "turkish"})).unwrap().said, "Lines are in Turkish");
        assert_eq!(set_language(&app, &id, &json!({"language": "Turkish"})).unwrap_err(), "The lines are in Turkish already.");
        let done = cast_add(&app, &id, &json!({"name": "Ayşe", "look": "a young woman in a red coat"})).unwrap();
        assert_eq!(done.said, "Added Ayşe to the cast");
        assert!(cast_add(&app, &id, &json!({"name": "ayşe"})).unwrap_err().contains("in the cast already"));
        assert!(cast_add(&app, &id, &json!({"look": "tall"})).unwrap_err().contains("\"name\" is needed"));
        let done = cast_edit(&app, &id, &json!({"name": "Mira", "new_name": "Mina", "look": "a girl of nine"})).unwrap();
        assert_eq!(done.said, "Mira: look changed, now Mina, in 1 shot");
        let p = load(&app, &id).unwrap();
        assert_eq!((p.cast[0].name.as_str(), p.cast[0].look.as_str()), ("Mina", "a girl of nine"));
        assert_eq!(p.shots[1].prompt, "Mina splashes through a puddle.");
        assert_eq!(p.shots[1].lines[0].who, "m1", "the line stays hers");
        assert!(cast_edit(&app, &id, &json!({"name": "Mina"})).unwrap_err().starts_with("Nothing changed"));
        assert!(cast_edit(&app, &id, &json!({"name": "Nobody", "look": "x"})).unwrap_err().contains("No one named Nobody"));
        assert!(cast_edit(&app, &id, &json!({"name": "Mina", "new_name": "Ayşe"})).unwrap_err().contains("in the cast already"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn subtitles_and_sound_go_on_and_a_missing_file_is_said() {
        let (app, dir, id) = a_film(false);
        assert_eq!(subtitles(&app, &id, &json!({"burn": true})).unwrap().said, "Subtitles burned into the picture on export");
        assert!(load(&app, &id).unwrap().subtitles.burn);
        assert!(subtitles(&app, &id, &json!({"burn": true})).unwrap_err().contains("already"));
        // One second of silence as a WAV file in the film's folder.
        let folder = load(&app, &id).unwrap().folder;
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data".to_vec();
        wav.extend((16000u32).to_le_bytes());
        wav.extend(vec![0u8; 16000]);
        std::fs::write(FsPath::new(&folder).join("rain.wav"), &wav).unwrap();
        assert_eq!(wav_seconds(&FsPath::new(&folder).join("rain.wav")), Some(1.0));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let done = rt.block_on(add_sound_region(&app, &id, &json!({"file": "Rain.wav", "shot": 2, "track": "Rain"}))).unwrap();
        assert_eq!(done.said, "Added rain.wav at shot 2 on Rain");
        let p = load(&app, &id).unwrap();
        assert_eq!((p.tracks.len(), p.tracks[0].regions[0].start, p.tracks[0].regions[0].length), (1, 121.0 / 24.0, 1.0));
        let again = rt.block_on(add_sound_region(&app, &id, &json!({"file": "rain.wav", "at": 1}))).unwrap();
        assert_eq!(again.said, "Added rain.wav at 1.0 s on Rain", "the one sound track is used");
        let missing = rt.block_on(add_sound_region(&app, &id, &json!({"file": "thunder.mp3"}))).unwrap_err();
        assert_eq!(missing, "There is no sound file named thunder.mp3. The film's sound files: rain.wav.");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn what_takes_long_needs_its_model_and_something_to_work_on() {
        let (app, dir, id) = a_film(false);
        assert_eq!(make_plan(&app, &id, &json!({})).unwrap_err(), missing(Modality::Video));
        assert_eq!(dub_plan(&app, &id, &json!({})).unwrap_err(), missing(Modality::Voice));
        assert_eq!(export_plan(&app, &id).unwrap_err(), "No shot is made yet. Make the shots first.");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(rt.block_on(rewrite_shot(&app, &id, &json!({"shot": 1, "hint": "at night"}))).unwrap_err().contains("chat model"));
        assert!(rt.block_on(look_at_shot(&app, &id, &json!({"shot": 1}))).unwrap_err().contains("not made yet"));
        // No lines: nothing to dub, whatever is installed.
        edit_shot(&app, &id, &json!({"shot": 2, "lines": []})).unwrap();
        assert!(dub_plan(&app, &id, &json!({})).unwrap_err().contains("no spoken lines"));
        std::fs::remove_dir_all(dir).ok();

        // With the video model the shots not made are what is made, and the person is asked with a time.
        let (app, dir, id) = a_film(true);
        let (ids, question) = make_plan(&app, &id, &json!({})).unwrap();
        assert_eq!(ids, ["a1", "a2", "a3"]);
        assert!(question.starts_with("Make 3 shots (1, 2 and 3): "), "{question}");
        let (ids, question) = make_plan(&app, &id, &json!({"shots": [2, "2"]})).unwrap();
        assert_eq!(ids, ["a2"]);
        assert!(question.starts_with("Make shot 2: "), "{question}");
        assert!(make_plan(&app, &id, &json!({"shots": [8]})).unwrap_err().contains("no shot 8"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_picture_is_a_png_any_reader_takes() {
        // Two by one: a red and a blue pixel.
        let made = png(2, 1, &[255, 0, 0, 0, 0, 255]);
        assert!(made.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x02\0\0\0\x01\x08\x02"));
        // The end every PNG has.
        assert!(made.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
        // The pixels are in one stored block, filter 0 first, then Adler-32.
        let idat = &made[33 + 8..made.len() - 12 - 4];
        assert_eq!(idat, [0x78, 0x01, 1, 7, 0, 0xf8, 0xff, 0, 255, 0, 0, 0, 0, 255, 0x07, 0x00, 0x01, 0xff]);
    }

    #[test]
    fn a_name_is_changed_only_where_it_stands_as_a_word() {
        assert_eq!(rename_in("Mira and mira's boat; admiral Mirage", "Mira", "Mina"), "Mina and Mina's boat; admiral Mirage");
        assert_eq!(rename_in("nothing here", "Mira", "Mina"), "nothing here");
        assert_eq!(and_list(&["1".into(), "2".into(), "4".into()]), "1, 2 and 4");
        assert_eq!(number(&json!({"s": "7 seconds"}), "s"), Some(7.0));
        assert_eq!(number(&json!({"s": 2.5}), "s"), Some(2.5));
    }
}
