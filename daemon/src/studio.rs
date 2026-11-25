//! Studio: films made of shots, kept in a folder the person chooses.
//!
//!   GET    /v1/studio/projects                    the films this install knows
//!   POST   /v1/studio/projects                    {"folder"?, "name"}: start a film in a folder (created if missing;
//!                                                 without one, a new folder named after the film under Movies/Rigbrew)
//!   GET    /v1/studio/projects/{id}               the film: its shots and what is being made
//!   PUT    /v1/studio/projects/{id}               {"name", "size", "auto_export", "sound", "sound_volume", "sound_once",
//!                                                 "fade_in", "fade_out", "shots": [{"id"?, "prompt", "frames",
//!                                                 "continues", "trim_start", "trim_end", "join", "volume", "mute"}]}
//!   DELETE /v1/studio/projects/{id}               forget the film (its folder stays)
//!   POST   /v1/studio/projects/{id}/render        {"shots": [id]?, "model"?}: make the listed shots, or every shot not made yet
//!   DELETE /v1/studio/projects/{id}/render        stop: the shot being made is stopped, the rest leave the queue
//!   POST   /v1/studio/projects/{id}/storyboard    {"idea", "shots"}: the chat model writes the shots in scenes (not saved)
//!   POST   /v1/studio/projects/{id}/export      join the made shots into `<folder>/<name>.mp4`, with the film's
//!                                               sound file under them ({"music"?, "music_volume"?} for another)
//!   GET    /v1/studio/projects/{id}/export      where the export is: {"state", "file", "error"}
//!   GET    /v1/studio/projects/{id}/sound       the film's sound file, to play under it in the app
//!   POST   /v1/studio/projects/{id}/narration   {"text"}: the voice package reads it; the WAV is saved in the
//!                                               film's folder and becomes the film's sound file
//!   POST   /v1/studio/projects/{id}/shots/{shot}/take          {"take": n}: use that take of the shot
//!   DELETE /v1/studio/projects/{id}/shots/{shot}/takes/{n}     delete a take and its clip
//!   GET    /v1/studio/projects/{id}/clips/{shot}.webm          the take in use; `{shot}.{n}.webm` for one take
//!   GET    /v1/studio/projects/{id}/clips/{shot}.jpg           one small picture of it, for the timeline
//!
//! A film is `<folder>/rigbrew-film.json` plus `<folder>/clips/<shot>.<take>.webm` (and `.jpg`; a film
//! from before takes has `<shot>.webm`, which becomes take 1 when the shot is made again); the list of folders is
//! `<data dir>/studio.json`. Shots are made one after another by the installed video package through
//! the same path as `/v1/videos/generations`, so a long film renders unattended, shot by shot. A film
//! with `auto_export` is exported by the same worker when its last shot is made. How long each shot
//! took is kept in `<data dir>/studio-pace.json`, and a film says from it how long it will still take.

use std::collections::VecDeque;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rigbrew_core::manifest::Modality;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cloud::now_ms;
use crate::{fail, video, App, Shared};

mod board;
mod export;
mod pace;

const FILE: &str = "rigbrew-film.json";
const MAX_SHOTS: usize = 200;
const MAX_PROMPT: usize = 1200;
/// The longest text the voice reads for one narration.
const MAX_NARRATION: usize = 6000;
/// The most shots one storyboard has: five minutes of five-second shots.
const MAX_STORYBOARD: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub id: String,
    pub prompt: String,
    pub frames: u32,
    /// "empty" (never made), "queued", "making", "done", "failed", or "changed" (made, then the description changed).
    #[serde(default = "empty")]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long the last render took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    /// The frames the clip really has: the video model rounds a length to what it can make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_frames: Option<u32>,
    /// Seconds cut from the start and from the end of the clip when the film is exported or played.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_start: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_end: f32,
    /// Start this shot from the last picture of the shot before it, so the two join without a cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continues: bool,
    /// How the film goes from the shot before into this one: "" (a cut), "dissolve" or "black".
    /// Not looked at when the shot continues the one before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub join: String,
    /// How loud the shot's own sound is in the film: 1.0 as made, up to 1.5.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mute: bool,
    /// The clips made for this shot, and the number of the one in use (0 before the first take).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<Take>,
    #[serde(default, skip_serializing_if = "is_none_yet")]
    pub take: u32,
    /// A seed the person wants the next clip made with; without one every clip gets a new seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
}

fn is_none_yet(n: &u32) -> bool {
    *n == 0
}

impl Default for Shot {
    /// A shot that is not made, at its own volume.
    fn default() -> Shot {
        Shot {
            id: String::new(),
            prompt: String::new(),
            frames: 121,
            state: empty(),
            error: None,
            seconds: None,
            made_frames: None,
            trim_start: 0.0,
            trim_end: 0.0,
            continues: false,
            join: String::new(),
            volume: 1.0,
            mute: false,
            takes: Vec::new(),
            take: 0,
            seed: None,
        }
    }
}

impl Shot {
    /// The length of the shot in seconds: of its clip when there is one, else what was asked for.
    fn length(&self) -> f32 {
        self.made_frames.filter(|_| self.state == "done" || self.state == "changed").unwrap_or(self.frames) as f32 / 24.0
    }
}

/// One clip of a shot. Making a shot again keeps the clip before as a take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub n: u32,
    /// The seed it was made with: the same seed and description give the same clip again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    /// The frames that were asked for, and the frames the clip has.
    pub frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_frames: Option<u32>,
    /// Whether it started from the last picture of the shot before it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continued: bool,
}

/// A shot keeps this many takes; the oldest one that is not in use goes when another is made.
const MAX_TAKES: usize = 4;

impl Shot {
    /// Notes that making the shot did not work. A shot that has a clip keeps it, and its place in the film.
    fn fail(&mut self, why: String) {
        if self.take == 0 && self.made_frames.is_none() {
            self.state = "failed".into();
        }
        self.error = Some(why);
    }

    /// Keeps a new take and puts it in use. Answers with the takes that have to go.
    fn add_take(&mut self, take: Take) -> Vec<u32> {
        self.take = take.n;
        self.state = "done".into();
        self.error = None;
        self.seconds = take.seconds;
        self.made_frames = take.made_frames;
        self.takes.retain(|t| t.n != take.n);
        self.takes.push(take);
        let mut gone = Vec::new();
        while self.takes.len() > MAX_TAKES {
            let Some(at) = self.takes.iter().position(|t| t.n != self.take) else { break };
            gone.push(self.takes.remove(at).n);
        }
        gone
    }

    /// Puts a take in use: the shot is then as long as that clip and counts as made.
    fn use_take(&mut self, n: u32) -> bool {
        let Some(take) = self.takes.iter().find(|t| t.n == n).cloned() else { return false };
        self.take = n;
        self.frames = take.frames;
        self.seconds = take.seconds;
        self.made_frames = take.made_frames;
        self.state = "done".into();
        self.error = None;
        // Trims that fitted a longer clip leave at least half a second of this one.
        let length = self.length();
        self.trim_start = self.trim_start.clamp(0.0, (length - 0.5).max(0.0));
        self.trim_end = self.trim_end.clamp(0.0, (length - 0.5 - self.trim_start).max(0.0));
        true
    }

    /// Takes a take away. When it was the one in use, the newest one left is; with none left the shot is not made.
    fn remove_take(&mut self, n: u32) -> bool {
        let before = self.takes.len();
        self.takes.retain(|t| t.n != n);
        if self.takes.len() == before {
            return false;
        }
        if self.take == n {
            match self.takes.iter().map(|t| t.n).max() {
                Some(newest) => {
                    self.use_take(newest);
                }
                None => {
                    self.take = 0;
                    self.state = empty();
                    self.seconds = None;
                    self.made_frames = None;
                }
            }
        }
        true
    }
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn half() -> f32 {
    0.5
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn empty() -> String {
    "empty".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder: String,
    /// "640x384": every shot of a film has the same shape.
    pub size: String,
    pub shots: Vec<Shot>,
    /// Export the film by itself when its last shot is made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_export: bool,
    /// A sound file (its full path) that plays under the whole film: music or narration.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sound: String,
    /// 0 to 1: how loud the sound file is next to the shots' own sound.
    #[serde(default = "half")]
    pub sound_volume: f32,
    /// The sound file plays once (a narration); otherwise it repeats until the film ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sound_once: bool,
    /// The film comes up from black at its start, and goes down to black at its end.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fade_in: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fade_out: bool,
    pub updated_ms: i64,
}

/// What is waiting to be made and what is being made now. One film renders at a time.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<(String, String)>,
    /// (project, shot, video job)
    now: Option<(String, String, String)>,
    working: bool,
    model: Option<String>,
    /// Stop was asked for before the shot being made had a job in the engine.
    stop_now: bool,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);

fn with_queue<T>(f: impl FnOnce(&mut Queue) -> T) -> T {
    let mut guard = QUEUE.lock().unwrap();
    f(guard.get_or_insert_with(Queue::default))
}

fn index_path(app: &App) -> PathBuf {
    app.root.join("studio.json")
}

/// How long shots took on this machine, newest last.
fn paces(app: &App) -> Vec<pace::Sample> {
    std::fs::read_to_string(app.root.join("studio-pace.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The measurements a film's times are read from: this machine's, or the film's own takes when
/// this machine has made nothing since measurements are kept.
fn paces_for(app: &App, p: &Project) -> Vec<pace::Sample> {
    let seen = paces(app);
    if !seen.is_empty() {
        return seen;
    }
    let mut own = Vec::new();
    for s in &p.shots {
        for t in &s.takes {
            pace::remember(&mut own, pace::Sample { frames: t.frames, continued: t.continued, seconds: t.seconds.unwrap_or(0.0) });
        }
        if s.takes.is_empty() {
            pace::remember(&mut own, pace::Sample { frames: s.frames, continued: s.continues, seconds: s.seconds.unwrap_or(0.0) });
        }
    }
    own
}

/// About how long the film still needs: `all` for every described shot that is not made, `left`
/// for the shots that wait or are being made now (less the time the one being made has had).
fn time_left(p: &Project, seen: &[pace::Sample], waiting: &[String], now: Option<(&str, f32)>) -> (f32, f32) {
    let (mut all, mut left) = (0.0, 0.0);
    for (i, s) in p.shots.iter().enumerate() {
        let needs = pace::estimate(seen, s.frames, s.continues && i > 0);
        let making = now.filter(|(id, _)| *id == s.id);
        if s.state != "done" && !s.prompt.is_empty() {
            all += needs;
        }
        if let Some((_, elapsed)) = making {
            // A shot that runs longer than shots like it did is still not done: a little is always left.
            left += (needs - elapsed).max(needs * 0.05);
        } else if waiting.contains(&s.id) {
            left += needs;
        }
    }
    (all, left)
}

/// id -> folder
fn index(app: &App) -> Vec<(String, String)> {
    std::fs::read_to_string(index_path(app)).ok().and_then(|t| serde_json::from_str::<Vec<(String, String)>>(&t).ok()).unwrap_or_default()
}

fn write_index(app: &App, list: &[(String, String)]) -> std::io::Result<()> {
    std::fs::write(index_path(app), serde_json::to_string_pretty(list).unwrap_or_default())
}

fn load(app: &App, id: &str) -> Option<Project> {
    let folder = index(app).into_iter().find(|(i, _)| i == id)?.1;
    let mut p: Project = serde_json::from_str(&std::fs::read_to_string(FsPath::new(&folder).join(FILE)).ok()?).ok()?;
    // The folder may have been moved and added again: where it is now wins.
    p.folder = folder;
    p.id = id.to_string();
    Some(p)
}

fn store(p: &mut Project) -> std::io::Result<()> {
    p.updated_ms = now_ms();
    let path = FsPath::new(&p.folder).join(FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(p).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

/// The clip of a shot: of the take in use, or the one clip of a shot from before takes.
fn clip_path(p: &Project, shot: &str) -> PathBuf {
    FsPath::new(&p.folder).join("clips").join(format!("{}.webm", clip_stem(p, shot)))
}

/// "<shot>.<take>" for the take in use; "<shot>" for a shot from before takes.
fn clip_stem(p: &Project, shot: &str) -> String {
    match p.shots.iter().find(|s| s.id == shot).map(|s| s.take).filter(|n| *n > 0) {
        Some(n) => format!("{shot}.{n}"),
        None => shot.to_string(),
    }
}

/// The clip of one take, and its small picture.
fn take_files(p: &Project, shot: &str, n: u32) -> (PathBuf, PathBuf) {
    let dir = FsPath::new(&p.folder).join("clips");
    (dir.join(format!("{shot}.{n}.webm")), dir.join(format!("{shot}.{n}.jpg")))
}

fn poster_path(p: &Project, shot: &str) -> PathBuf {
    FsPath::new(&p.folder).join("clips").join(format!("{}.jpg", clip_stem(p, shot)))
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn valid_size(size: &str) -> bool {
    size.split_once('x')
        .is_some_and(|(w, h)| [w, h].iter().all(|v| v.parse::<u32>().is_ok_and(|n| (256..=1280).contains(&n) && n % 32 == 0)))
}

/// Where films go when the person does not choose a folder: `Movies/Rigbrew` in their home.
fn films_home() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(if cfg!(target_os = "macos") { "Movies" } else { "Videos" }).join("Rigbrew"))
}

/// A folder for a new film named `name` inside `home`: the first of "Name", "Name 2", ... that holds no film yet.
fn new_folder(home: &FsPath, name: &str) -> PathBuf {
    let stem = file_stem(name);
    (1..)
        .map(|n| home.join(if n == 1 { stem.clone() } else { format!("{stem} {n}") }))
        .find(|folder| !folder.join(FILE).exists())
        .unwrap_or_else(|| home.join(&stem))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Whether the installed video model can start a shot from a picture.
fn can_continue(app: &App) -> bool {
    app.manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video)
        .any(|pk| app.installed(pk).iter().any(|v| v.files.iter().any(|f| f.role == "mmproj")))
}

/// The film as the app sees it: the stored film plus what the queue says right now.
fn view(app: &App, mut p: Project) -> Value {
    let (now, waiting) =
        with_queue(|q| (q.now.clone(), q.waiting.iter().filter(|(pid, _)| *pid == p.id).map(|(_, s)| s.clone()).collect::<Vec<_>>()));
    let mut progress = Value::Null;
    let mut elapsed = None;
    for shot in &mut p.shots {
        if now.as_ref().is_some_and(|(pid, sid, _)| *pid == p.id && *sid == shot.id) {
            shot.state = "making".into();
            if let Some((_, _, job)) = &now {
                if let Some(j) = app.videos.lock().unwrap().get(job) {
                    progress = json!({"shot": shot.id, "stage": j.stage, "step": j.step, "steps": j.steps, "elapsed_ms": j.elapsed_ms});
                    elapsed = Some(j.elapsed_ms as f32 / 1000.0);
                }
            }
        } else if waiting.contains(&shot.id) {
            shot.state = "queued".into();
        }
    }
    let made = p.shots.iter().filter(|s| s.state == "done").count();
    let seconds: f32 = p.shots.iter().map(Shot::length).sum();
    let continues = can_continue(app);
    let export = export_of(&p.id);
    let mine = now.as_ref().filter(|(pid, _, _)| *pid == p.id).map(|(_, sid, _)| (sid.as_str(), elapsed.unwrap_or(0.0)));
    let (all, left) = time_left(&p, &paces_for(app, &p), &waiting, mine);
    json!({"project": p, "progress": progress, "rendering": mine.is_some() || !waiting.is_empty(), "made": made, "seconds": seconds,
           "can_continue": continues, "export": export, "estimate": {"all": all, "left": left}})
}

async fn list(State(app): State<Shared>) -> Json<Value> {
    let films: Vec<Value> = index(&app)
        .into_iter()
        .map(|(id, folder)| match load(&app, &id) {
            Some(p) => json!({"id": id, "name": p.name, "folder": folder, "shots": p.shots.len(), "made": p.shots.iter().filter(|s| s.state == "done").count(), "updated_ms": p.updated_ms}),
            None => json!({"id": id, "name": "", "folder": folder, "missing": true}),
        })
        .collect();
    // What a five-second shot takes here, for the start screen to say how long a film will take.
    let seen = paces(&app);
    let pace = json!({"frames": 121, "plain": pace::estimate(&seen, 121, false), "continuing": pace::estimate(&seen, 121, true), "measured": !seen.is_empty()});
    Json(
        json!({"projects": films, "home": films_home().map(|h| h.to_string_lossy().to_string()), "pace": pace, "can_continue": can_continue(&app)}),
    )
}

#[derive(Deserialize)]
struct NewProject {
    #[serde(default)]
    folder: String,
    #[serde(default)]
    name: String,
}

async fn create(State(app): State<Shared>, Json(body): Json<NewProject>) -> Response {
    let folder = if body.folder.trim().is_empty() {
        match films_home() {
            Some(home) => new_folder(&home, body.name.trim()),
            None => return fail(StatusCode::BAD_REQUEST, "choose a folder for the film"),
        }
    } else {
        PathBuf::from(body.folder.trim())
    };
    if !folder.is_absolute() || body.folder.len() > 1024 {
        return fail(StatusCode::BAD_REQUEST, "choose a folder by its full path");
    }
    if let Err(e) = std::fs::create_dir_all(folder.join("clips")) {
        return fail(StatusCode::BAD_REQUEST, format!("that folder cannot be used: {e}"));
    }
    let folder_text = folder.to_string_lossy().to_string();
    let mut list = index(&app);
    // A folder that already holds a film is opened, not overwritten.
    let existing = std::fs::read_to_string(folder.join(FILE)).ok().and_then(|t| serde_json::from_str::<Project>(&t).ok());
    let id = list
        .iter()
        .find(|(_, f)| *f == folder_text)
        .map(|(i, _)| i.clone())
        .or_else(|| existing.as_ref().map(|p| p.id.clone()).filter(|i| valid_id(i)))
        .unwrap_or_else(new_id);
    let mut project = existing.unwrap_or_else(|| Project {
        id: id.clone(),
        name: String::new(),
        folder: folder_text.clone(),
        size: "640x384".into(),
        shots: vec![],
        auto_export: false,
        sound: String::new(),
        sound_volume: half(),
        sound_once: false,
        fade_in: false,
        fade_out: false,
        updated_ms: 0,
    });
    project.id = id.clone();
    project.folder = folder_text.clone();
    if project.name.is_empty() {
        let fallback = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Film".into());
        project.name = if body.name.trim().is_empty() { fallback } else { body.name.trim().chars().take(80).collect() };
    }
    if let Err(e) = store(&mut project) {
        return fail(StatusCode::BAD_REQUEST, format!("the film could not be saved there: {e}"));
    }
    list.retain(|(i, f)| *i != id && *f != folder_text);
    list.insert(0, (id, folder_text));
    write_index(&app, &list).ok();
    app.record("studio_project_opened", json!({"shots": project.shots.len()}));
    Json(view(&app, project)).into_response()
}

async fn read(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone"),
    }
}

#[derive(Deserialize, Default)]
struct ShotIn {
    #[serde(default)]
    id: Option<String>,
    prompt: String,
    #[serde(default)]
    frames: Option<u32>,
    #[serde(default)]
    trim_start: f32,
    #[serde(default)]
    trim_end: f32,
    #[serde(default)]
    continues: bool,
    #[serde(default)]
    join: String,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    seed: Option<i64>,
}

#[derive(Deserialize)]
struct Save {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    shots: Option<Vec<ShotIn>>,
    #[serde(default)]
    auto_export: Option<bool>,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    sound_volume: Option<f32>,
    #[serde(default)]
    sound_once: Option<bool>,
    #[serde(default)]
    fade_in: Option<bool>,
    #[serde(default)]
    fade_out: Option<bool>,
}

/// The new list of shots: a shot that keeps its id and its description keeps its clip; one whose
/// description or length changed is marked "changed" until it is made again.
fn merge_shots(old: &[Shot], new: Vec<ShotIn>, size_changed: bool) -> Result<Vec<Shot>, String> {
    if new.len() > MAX_SHOTS {
        return Err(format!("a film has at most {MAX_SHOTS} shots"));
    }
    let mut out: Vec<Shot> = Vec::with_capacity(new.len());
    for s in new {
        let prompt: String = s.prompt.trim().chars().take(MAX_PROMPT).collect();
        let frames = s.frames.unwrap_or(121);
        if !(5..=241).contains(&frames) {
            return Err("a shot is between 5 and 241 frames".into());
        }
        let id = s.id.filter(|i| valid_id(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(new_id);
        // The first shot has nothing to continue.
        let continues = s.continues && !out.is_empty();
        let mut shot = match old.iter().position(|o| o.id == id) {
            Some(at) => {
                let o = &old[at];
                // A shot that continues another was made from that shot's last picture: after a
                // move it stands behind a different one and has to be made again.
                let before = |list: &[Shot], at: usize| at.checked_sub(1).map(|i| list[i].id.clone());
                let moved = continues && before(old, at) != out.last().map(|l| l.id.clone());
                let same = o.prompt == prompt && o.frames == frames && o.continues == continues && !size_changed && !moved;
                let state = if same || o.state == "empty" { o.state.clone() } else { "changed".into() };
                Shot { prompt, frames, state, error: if same { o.error.clone() } else { None }, continues, ..o.clone() }
            }
            None => Shot { id, prompt, frames, state: empty(), continues, ..Shot::default() },
        };
        // How a shot joins the film and how loud it is change nothing about its clip.
        shot.join = match s.join.as_str() {
            "dissolve" | "black" => s.join,
            _ => String::new(),
        };
        shot.volume = s.volume.filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.5);
        shot.mute = s.mute;
        shot.seed = s.seed.filter(|seed| (0..=u32::MAX as i64).contains(seed));
        // Trims leave at least half a second of the shot.
        let length = shot.length();
        shot.trim_start = s.trim_start.clamp(0.0, (length - 0.5).max(0.0));
        shot.trim_end = s.trim_end.clamp(0.0, (length - 0.5 - shot.trim_start).max(0.0));
        out.push(shot);
    }
    Ok(out)
}

async fn save(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Save>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    if let Some(name) = body.name.map(|n| n.trim().chars().take(80).collect::<String>()).filter(|n| !n.is_empty()) {
        p.name = name;
    }
    let mut size_changed = false;
    if let Some(size) = body.size {
        if !valid_size(&size) {
            return fail(StatusCode::BAD_REQUEST, "size must be WIDTHxHEIGHT, each 256 to 1280 and a multiple of 32");
        }
        size_changed = size != p.size;
        p.size = size;
    }
    if let Some(shots) = body.shots {
        match merge_shots(&p.shots, shots, size_changed) {
            Ok(list) => p.shots = list,
            Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        }
    } else if size_changed {
        for s in p.shots.iter_mut().filter(|s| s.state == "done") {
            s.state = "changed".into();
        }
    }
    if let Some(auto) = body.auto_export {
        p.auto_export = auto;
    }
    if let Some(sound) = body.sound {
        let sound = sound.trim();
        if !sound.is_empty() && (!FsPath::new(sound).is_absolute() || sound.len() > 1024) {
            return fail(StatusCode::BAD_REQUEST, "give the sound file by its full path");
        }
        p.sound = sound.to_string();
    }
    if let Some(volume) = body.sound_volume.filter(|v| v.is_finite()) {
        p.sound_volume = volume.clamp(0.0, 1.0);
    }
    p.sound_once = body.sound_once.unwrap_or(p.sound_once);
    p.fade_in = body.fade_in.unwrap_or(p.fade_in);
    p.fade_out = body.fade_out.unwrap_or(p.fade_out);
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    Json(view(&app, p)).into_response()
}

async fn forget(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let mut list = index(&app);
    let before = list.len();
    list.retain(|(i, _)| *i != id);
    if list.len() == before {
        return fail(StatusCode::NOT_FOUND, "no such film");
    }
    with_queue(|q| q.waiting.retain(|(pid, _)| *pid != id));
    write_index(&app, &list).ok();
    Json(json!({"forgotten": id})).into_response()
}

#[derive(Deserialize, Default)]
struct Render {
    #[serde(default)]
    shots: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
}

async fn render(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Render>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let body = body.map(|b| b.0).unwrap_or_default();
    let Some(package) = app
        .manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video && !app.installed(pk).is_empty())
        .find(|pk| body.model.as_deref().is_none_or(|m| m == pk.id))
    else {
        return fail(StatusCode::CONFLICT, "No video model is installed. Add Video under Models first.");
    };
    let wanted: Vec<String> = match body.shots {
        Some(ids) => p.shots.iter().filter(|s| ids.contains(&s.id) && !s.prompt.is_empty()).map(|s| s.id.clone()).collect(),
        None => p.shots.iter().filter(|s| s.state != "done" && !s.prompt.is_empty()).map(|s| s.id.clone()).collect(),
    };
    if wanted.is_empty() {
        return fail(StatusCode::CONFLICT, "Every shot is made already. Change a shot or pick the ones to make again.");
    }
    let start = with_queue(|q| {
        for shot in &wanted {
            let key = (id.clone(), shot.clone());
            let making = q.now.as_ref().is_some_and(|(pid, sid, _)| *pid == key.0 && *sid == key.1);
            if !making && !q.waiting.contains(&key) {
                q.waiting.push_back(key);
            }
        }
        q.model = Some(package.id.clone());
        !std::mem::replace(&mut q.working, true)
    });
    app.record("studio_render_started", json!({"package": package.id, "shots": wanted.len()}));
    if start {
        tokio::spawn(work(app.clone()));
    }
    Json(view(&app, p)).into_response()
}

async fn stop(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let job = with_queue(|q| {
        q.waiting.retain(|(pid, _)| *pid != id);
        let job = q.now.as_ref().filter(|(pid, _, _)| *pid == id).map(|(_, _, job)| job.clone());
        if job.as_deref() == Some("") {
            q.stop_now = true;
        }
        job.filter(|j| !j.is_empty())
    });
    if let Some(job) = job {
        if let Some(j) = app.videos.lock().unwrap().get_mut(&job) {
            j.cancel = true;
        }
    }
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

fn set_shot(app: &App, project: &str, shot: &str, f: impl FnOnce(&mut Shot)) {
    if let Some(mut p) = load(app, project) {
        if let Some(s) = p.shots.iter_mut().find(|s| s.id == shot) {
            f(s);
            store(&mut p).ok();
        }
    }
}

/// Makes the waiting shots one after another until none is left.
async fn work(app: Shared) {
    loop {
        let Some((project, shot)) = with_queue(|q| {
            let next = q.waiting.pop_front();
            match &next {
                // The shot counts as being made from this moment, before the engine has a job for it.
                Some((project, shot)) => q.now = Some((project.clone(), shot.clone(), String::new())),
                None => {
                    q.working = false;
                    q.now = None;
                }
            }
            next
        }) else {
            return;
        };
        let Some(p) = load(&app, &project) else { continue };
        let Some(s) = p.shots.iter().find(|s| s.id == shot).cloned() else { continue };
        let model = with_queue(|q| q.model.clone()).unwrap_or_default();
        // A shot that continues the one before it starts from that shot's last picture.
        let first = if s.continues {
            let before = p
                .shots
                .iter()
                .position(|x| x.id == shot)
                .and_then(|i| i.checked_sub(1))
                .map(|i| clip_path(&p, &p.shots[i].id))
                .filter(|c| c.is_file());
            match before {
                Some(clip) => last_picture(&app, &clip).await,
                None => None,
            }
        } else {
            None
        };
        // The seed the person asked for, else a new one: it is kept with the take.
        let seed = s.seed.unwrap_or_else(|| uuid::Uuid::new_v4().as_u128() as u32 as i64);
        // Another clip (from the chat) may be in the making: wait for the engine to be free.
        let job = loop {
            let resp = video::generate(
                State(app.clone()),
                Json(video::VideoBody::new(&model, &s.prompt, &p.size, s.frames).with_seed(seed).starting_from(first.clone())),
            )
            .await;
            let status = resp.status();
            let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap_or_default();
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if let Some(job) = v["job"].as_str() {
                break Ok(job.to_string());
            }
            if status != StatusCode::CONFLICT {
                break Err(v["error"]["message"].as_str().unwrap_or("the shot could not be started").to_string());
            }
            // Stopped while waiting for a free engine.
            if !with_queue(|q| q.working) {
                break Err("stopped".into());
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        };
        let job = match job {
            Ok(job) => job,
            Err(e) => {
                set_shot(&app, &project, &shot, |s| s.fail(e));
                continue;
            }
        };
        let stop = with_queue(|q| {
            q.now = Some((project.clone(), shot.clone(), job.clone()));
            std::mem::take(&mut q.stop_now)
        });
        if stop {
            if let Some(j) = app.videos.lock().unwrap().get_mut(&job) {
                j.cancel = true;
            }
        }
        let (state, error, seconds, file) = loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let seen = app.videos.lock().unwrap().get(&job).map(|j| (j.state.clone(), j.error.clone(), j.elapsed_ms, j.video.clone()));
            match seen {
                Some((state, error, ms, video)) if state != "loading" && state != "running" => {
                    break (state, error, ms as f32 / 1000.0, video)
                }
                None => break ("failed".into(), Some("the shot was lost".into()), 0.0, None),
                _ => {}
            }
        };
        with_queue(|q| q.now = None);
        let from = file
            .as_ref()
            .and_then(|v| v["path"].as_str())
            .and_then(|path| path.rsplit('/').next())
            .map(|name| app.root.join("videos").join(name));
        let made = match from.filter(|_| state == "done") {
            Some(from) => {
                keep_take(
                    &app,
                    &project,
                    &shot,
                    &from,
                    Take {
                        n: 0,
                        seed: Some(seed),
                        seconds: Some(seconds),
                        frames: s.frames,
                        made_frames: None,
                        continued: first.is_some(),
                    },
                )
                .await
            }
            None => false,
        };
        if !made {
            set_shot(&app, &project, &shot, |s| {
                if state == "cancelled" {
                    // Stopped by the person: the shot is as it was before.
                    s.error = None;
                } else {
                    s.fail(error.unwrap_or_else(|| "the clip could not be saved in the film's folder".into()));
                }
            });
        }
        if made {
            app.record("studio_shot_made", json!({"frames": s.frames, "seconds": seconds, "continues": s.continues}));
            let mut seen = paces(&app);
            pace::remember(&mut seen, pace::Sample { frames: s.frames, continued: first.is_some(), seconds });
            std::fs::write(app.root.join("studio-pace.json"), serde_json::to_string(&seen).unwrap_or_default()).ok();
            stale_after(&app, &project, &shot);
        }
        // The last waiting shot of a film that exports by itself: join it now, with the screen open or not.
        if made && !with_queue(|q| q.waiting.iter().any(|(pid, _)| *pid == project)) {
            export_when_complete(&app, &project).await;
        }
    }
}

/// A made shot that continues `shot` started from a last picture that is not the last picture any more.
fn stale_after(app: &App, project: &str, shot: &str) {
    if let Some(mut p) = load(app, project) {
        let after = p.shots.iter().position(|x| x.id == shot).map(|i| i + 1);
        if let Some(next) = after.and_then(|i| p.shots.get_mut(i)).filter(|n| n.continues && n.state == "done") {
            next.state = "changed".into();
            store(&mut p).ok();
        }
    }
}

/// Puts a finished clip into the film's folder as a new take of its shot and puts it in use.
/// The clip the shot had stays as a take; a film from before takes has its one clip named as take 1.
async fn keep_take(app: &Shared, project: &str, shot: &str, from: &FsPath, mut take: Take) -> bool {
    let Some(p) = load(app, project) else { return false };
    let Some(now) = p.shots.iter().find(|s| s.id == shot) else { return false };
    let dir = FsPath::new(&p.folder).join("clips");
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let old = (dir.join(format!("{shot}.webm")), dir.join(format!("{shot}.jpg")));
    let adopted = (now.takes.is_empty() && old.0.is_file()).then(|| {
        let (clip, picture) = take_files(&p, shot, 1);
        std::fs::rename(&old.0, clip).ok();
        std::fs::rename(&old.1, picture).ok();
        Take { n: 1, seed: None, seconds: now.seconds, frames: now.frames, made_frames: now.made_frames, continued: now.continues }
    });
    take.n = now.takes.iter().map(|t| t.n).max().unwrap_or(0).max(adopted.as_ref().map_or(0, |t| t.n)) + 1;
    let (clip, picture) = take_files(&p, shot, take.n);
    if std::fs::copy(from, &clip).is_err() {
        return false;
    }
    // What the clip really holds, and a small picture of it for the timeline.
    take.made_frames = look_at(app, &clip, &picture).await;
    let mut gone = Vec::new();
    set_shot(app, project, shot, |s| {
        s.takes.extend(adopted);
        gone = s.add_take(take);
    });
    for n in gone {
        let (clip, picture) = take_files(&p, shot, n);
        std::fs::remove_file(clip).ok();
        std::fs::remove_file(picture).ok();
    }
    true
}

#[derive(Deserialize)]
struct UseTake {
    take: u32,
}

async fn use_take(State(app): State<Shared>, Path((id, shot)): Path<(String, String)>, Json(body): Json<UseTake>) -> Response {
    let mut found = false;
    set_shot(&app, &id, &shot, |s| found = s.use_take(body.take));
    if !found {
        return fail(StatusCode::NOT_FOUND, "no such take");
    }
    stale_after(&app, &id, &shot);
    app.record("studio_take_used", json!({"take": body.take}));
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

async fn delete_take(State(app): State<Shared>, Path((id, shot, n)): Path<(String, String, u32)>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    if with_queue(|q| q.now.as_ref().is_some_and(|(pid, sid, _)| *pid == id && *sid == shot)) {
        return fail(StatusCode::CONFLICT, "This shot is being made. Stop it or wait for it first.");
    }
    let (mut found, mut used) = (false, false);
    set_shot(&app, &id, &shot, |s| {
        used = s.take == n;
        found = s.remove_take(n);
    });
    if !found {
        return fail(StatusCode::NOT_FOUND, "no such take");
    }
    let (clip, picture) = take_files(&p, &shot, n);
    std::fs::remove_file(clip).ok();
    std::fs::remove_file(picture).ok();
    if used {
        stale_after(&app, &id, &shot);
    }
    app.record("studio_take_deleted", json!({"take": n}));
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

/// Exports a film that asked for it once every described shot is made.
async fn export_when_complete(app: &Shared, project: &str) {
    let Some(p) = load(app, project).filter(|p| p.auto_export) else { return };
    if p.shots.iter().any(|s| s.state != "done" && !s.prompt.is_empty()) {
        return;
    }
    if let Err((_, why)) = begin_export(app, p, Export::default()).await {
        set_export(project, json!({"state": "failed", "error": why}));
    }
}

/// The last picture of a clip as base64 JPEG, or `None` when the tool for it is missing or fails
/// (the shot is then made without it, with a cut).
async fn last_picture(app: &Shared, clip: &FsPath) -> Option<String> {
    use base64::Engine as _;
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await.ok()?;
    let out = std::env::temp_dir().join(format!("rigbrew-last-{}.jpg", uuid::Uuid::new_v4().simple()));
    let status = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", "-0.2", "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-update", "1", "-q:v", "2"])
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .ok()?;
    let bytes = std::fs::read(&out).ok();
    std::fs::remove_file(&out).ok();
    status.success().then_some(())?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes?))
}

/// The number after the last "frame=" in what ffmpeg printed while it read a clip.
fn frames_read(text: &str) -> Option<u32> {
    let rest = text.rsplit("frame=").next().filter(|r| r.len() < text.len())?;
    rest.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok().filter(|n| *n > 0)
}

/// Reads a clip once: writes a small picture from its middle to `poster` and answers how many
/// frames it has. `None` when the tool for it is missing or cannot read the clip.
async fn look_at(app: &Shared, clip: &FsPath, poster: &FsPath) -> Option<u32> {
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await.ok()?;
    let counted = tokio::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-stats", "-i"])
        .arg(clip)
        .args(["-map", "0:v:0", "-f", "null", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let frames = frames_read(&String::from_utf8_lossy(&counted.stderr));
    let middle = format!("{:.3}", frames.unwrap_or(0) as f32 / 48.0);
    tokio::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &middle, "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-update", "1", "-vf", "scale=320:-2", "-q:v", "4"])
        .arg(poster)
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .ok();
    frames
}

/// "<shot>.webm", "<shot>.jpg", or with a take: "<shot>.<n>.webm": the shot, the take, and whether it is the picture.
fn clip_name(name: &str) -> Option<(&str, Option<u32>, bool)> {
    let (rest, picture) = match name.rsplit_once('.')? {
        (rest, "webm") => (rest, false),
        (rest, "jpg") => (rest, true),
        _ => return None,
    };
    let (shot, take) = match rest.split_once('.') {
        Some((shot, n)) => (shot, Some(n.parse::<u32>().ok().filter(|n| (1..100_000).contains(n))?)),
        None => (rest, None),
    };
    valid_id(shot).then_some((shot, take, picture))
}

async fn clip(State(app): State<Shared>, Path((id, name)): Path<(String, String)>) -> Response {
    let Some((shot, take, picture)) = clip_name(&name) else {
        return fail(StatusCode::BAD_REQUEST, "bad clip name");
    };
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    let (clip, poster) = match take {
        Some(n) => take_files(&p, shot, n),
        None => (clip_path(&p, shot), poster_path(&p, shot)),
    };
    let file = if picture { poster } else { clip.clone() };
    // A film made before pictures were kept gets them when they are first asked for.
    if picture && !file.is_file() && clip.is_file() {
        let frames = look_at(&app, &clip, &file).await;
        if take.is_none() {
            set_shot(&app, &id, shot, |s| s.made_frames = s.made_frames.or(frames));
        }
    }
    match tokio::fs::read(file).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, if picture { "image/jpeg" } else { "video/webm" })
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "that shot has no clip yet"),
    }
}

/// The kinds of sound file the app plays under a film, by the end of their name.
fn sound_kind(path: &FsPath) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" | "mp4" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "aif" | "aiff" => "audio/aiff",
        _ => return None,
    })
}

async fn sound(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    let path = PathBuf::from(&p.sound);
    let Some(kind) = sound_kind(&path).filter(|_| path.is_absolute()) else {
        return fail(StatusCode::NOT_FOUND, "this film has no sound file the app can play");
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, kind)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "the sound file was not found"),
    }
}

/// The port this service listens on, for the requests it makes to its own chat and voice routes.
fn own_port(app: &App) -> Option<u64> {
    std::fs::read_to_string(app.root.join("daemon.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["port"].as_u64())
}

#[derive(Deserialize)]
struct Idea {
    idea: String,
    #[serde(default)]
    shots: Option<u32>,
}

async fn storyboard(State(app): State<Shared>, Path(_id): Path<String>, Json(body): Json<Idea>) -> Response {
    let idea: String = body.idea.trim().chars().take(2000).collect();
    if idea.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "say what the film is about in \"idea\"");
    }
    let count = body.shots.unwrap_or(6).clamp(1, MAX_STORYBOARD as u32) as usize;
    let Some(chat) = app.manifest.packages.iter().find(|p| p.modality == Modality::Chat && !app.installed(p).is_empty()) else {
        return fail(StatusCode::CONFLICT, "No chat model is installed. Add Chat under Models first.");
    };
    let Some(port) = own_port(&app) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "the service does not know its own address");
    };
    // A long film is written in parts, each part told what came before it.
    let mut shots: Vec<board::Line> = Vec::new();
    for _ in 0..count / board::PART + 3 {
        let ask = (count - shots.len()).min(board::PART);
        if ask == 0 {
            break;
        }
        let request = json!({
            "model": chat.id,
            "messages": [
                {"role": "system", "content": board::system_prompt(ask)},
                {"role": "user", "content": board::part_request(&idea, &shots, ask, count)},
            ],
            "max_tokens": 140 * ask + 200,
            "temperature": 0.7,
            "chat_template_kwargs": {"enable_thinking": false},
        });
        let answer = app
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .bearer_auth(&app.token)
            .json(&request)
            .timeout(Duration::from_secs(300))
            .send()
            .await;
        let text = match answer {
            Ok(r) if r.status().is_success() => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v["choices"][0]["message"]["content"].as_str().map(str::to_string))
                .unwrap_or_default(),
            // What is written so far is still a film.
            _ if !shots.is_empty() => break,
            Ok(r) => {
                let v: Value = r.json().await.unwrap_or(Value::Null);
                return fail(
                    StatusCode::BAD_GATEWAY,
                    v["error"]["message"].as_str().unwrap_or("the chat model could not answer").to_string(),
                );
            }
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
        };
        let part = board::shot_lines(&text, ask);
        if part.is_empty() {
            break;
        }
        shots.extend(part);
    }
    if shots.is_empty() {
        return fail(StatusCode::BAD_GATEWAY, "The chat model did not write a list of shots. Try again, or say more about the film.");
    }
    // A film starts with a cut.
    shots[0].continues = false;
    let scenes = shots.iter().filter(|s| !s.continues).count();
    app.record("studio_storyboard_written", json!({"shots": shots.len(), "scenes": scenes, "asked": count}));
    Json(json!({"shots": shots.into_iter().map(|s| json!({"prompt": s.prompt, "continues": s.continues})).collect::<Vec<_>>()}))
        .into_response()
}

/// The export of each film: "working", "done" or "failed", with the file or what went wrong.
static EXPORTS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());

fn export_of(id: &str) -> Value {
    EXPORTS.lock().unwrap().iter().find(|(i, _)| i == id).map(|(_, v)| v.clone()).unwrap_or_else(|| json!({"state": "none"}))
}

fn set_export(id: &str, value: Value) {
    let mut all = EXPORTS.lock().unwrap();
    all.retain(|(i, _)| i != id);
    all.push((id.to_string(), value));
}

#[derive(Deserialize, Default)]
struct Export {
    /// A sound file to lay under the whole film (a full path), mixed with the shots' own sound.
    #[serde(default)]
    music: Option<String>,
    /// 0 to 1; how loud the music is next to the shots' sound. Default 0.5.
    #[serde(default)]
    music_volume: Option<f32>,
}

/// A file name made of the film's name: letters, digits, spaces, dashes.
fn file_stem(name: &str) -> String {
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { ' ' }).collect();
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    if stem.is_empty() {
        "film".into()
    } else {
        stem.chars().take(80).collect()
    }
}

/// The made shots of a film whose clip is in its folder, in order: what an export holds.
fn exported(p: &Project) -> Vec<&Shot> {
    p.shots.iter().filter(|s| (s.state == "done" || s.state == "changed") && clip_path(p, &s.id).is_file()).collect()
}

/// The concat list ffmpeg reads for a film of plain cuts: every shot with its trims.
fn concat_list(p: &Project, shots: &[&Shot]) -> String {
    let mut list = String::from("ffconcat version 1.0\n");
    for s in shots {
        let length = s.length();
        list.push_str(&format!("file '{}'\n", clip_path(p, &s.id).to_string_lossy().replace('\'', "'\\''")));
        if s.trim_start > 0.0 {
            list.push_str(&format!("inpoint {:.3}\n", s.trim_start));
        }
        if s.trim_end > 0.0 {
            list.push_str(&format!("outpoint {:.3}\n", (length - s.trim_end).max(s.trim_start + 0.1)));
        }
    }
    list
}

/// The shots as the pieces of the film: what is kept of each clip, how it joins, how loud it is.
fn pieces(shots: &[&Shot], sounds: &[bool]) -> Vec<export::Piece> {
    shots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let whole = s.length();
            let start = s.trim_start.clamp(0.0, (whole - 0.1).max(0.0));
            let kept = (whole - start - s.trim_end.max(0.0)).max(0.1);
            export::Piece {
                start,
                frames: ((kept * 24.0).round() as u32).max(1),
                // A shot that continues the one before it joins it with nothing in between.
                join: if i == 0 || s.continues { export::Join::Cut } else { export::Join::named(&s.join) },
                volume: if s.mute { 0.0 } else { s.volume.clamp(0.0, 1.5) },
                has_sound: sounds.get(i).copied().unwrap_or(true),
            }
        })
        .collect()
}

/// Whether a clip has sound in it, as ffmpeg lists its streams.
async fn has_sound(ffmpeg: &FsPath, clip: &FsPath) -> bool {
    let listed =
        tokio::process::Command::new(ffmpeg).args(["-hide_banner", "-i"]).arg(clip).stdin(std::process::Stdio::null()).output().await;
    listed.map(|o| String::from_utf8_lossy(&o.stderr).contains("Audio:")).unwrap_or(true)
}

async fn export(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Export>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    match begin_export(&app, p, body.map(|b| b.0).unwrap_or_default()).await {
        Ok(count) => (StatusCode::ACCEPTED, Json(json!({"state": "working", "shots": count}))).into_response(),
        Err((status, why)) => fail(status, why),
    }
}

/// Starts joining the made shots of a film into its MP4; answers with how many shots go in.
async fn begin_export(app: &Shared, mut p: Project, body: Export) -> Result<usize, (StatusCode, String)> {
    let id = p.id.clone();
    if exported(&p).is_empty() {
        return Err((StatusCode::CONFLICT, "No shot is made yet. Make the shots first.".into()));
    }
    // The film's own sound file, unless the request names another.
    let music = match body.music.as_deref().or(Some(p.sound.as_str())).map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if FsPath::new(m).is_absolute() && FsPath::new(m).is_file() => Some(m.to_string()),
        Some(_) => {
            return Err((StatusCode::BAD_REQUEST, "The sound file of this film was not found. Choose it again, or take it out.".into()))
        }
        None => None,
    };
    if EXPORTS.lock().unwrap().iter().any(|(i, v)| *i == id && v["state"] == "working") {
        return Err((StatusCode::CONFLICT, "This film is being exported already.".into()));
    }
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg")
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("The tool that joins the shots could not be set up: {e}")))?;
    set_export(&id, json!({"state": "working", "shots": exported(&p).len()}));
    // A clip from before lengths were kept is measured now: joins are counted in frames.
    let unmeasured: Vec<String> = exported(&p).iter().filter(|s| s.made_frames.is_none()).map(|s| s.id.clone()).collect();
    for shot in unmeasured {
        let frames = look_at(app, &clip_path(&p, &shot), &poster_path(&p, &shot)).await;
        set_shot(app, &id, &shot, |s| s.made_frames = frames);
        if let Some(s) = p.shots.iter_mut().find(|s| s.id == shot) {
            s.made_frames = frames;
        }
    }
    let shots = exported(&p);
    let count = shots.len();
    let clips: Vec<PathBuf> = shots.iter().map(|s| clip_path(&p, &s.id)).collect();
    let mut sounds = Vec::with_capacity(count);
    for clip in &clips {
        sounds.push(has_sound(&ffmpeg, clip).await);
    }
    let pieces = pieces(&shots, &sounds);
    // A film of plain cuts is read as one list; anything more is a filter graph over every clip.
    let simple = export::plain(&pieces);
    let film = if simple { export::as_it_is(export::film_frames(&pieces)) } else { export::joined(&pieces) };
    let volume = body.music_volume.unwrap_or(p.sound_volume).clamp(0.0, 1.0);
    let finish = export::Finish {
        fade_in: p.fade_in,
        fade_out: p.fade_out,
        scale: 1,
        music: music.as_ref().map(|_| (if simple { 1 } else { count }, volume)),
    };
    let graph = export::finished(film, &finish);
    let folder = PathBuf::from(&p.folder);
    let out = folder.join(format!("{}.mp4", file_stem(&p.name)));
    let list_path = folder.join(".rigbrew-export.txt");
    let graph_path = folder.join(".rigbrew-export-graph.txt");
    let written = if simple { std::fs::write(&list_path, concat_list(&p, &shots)) } else { Ok(()) }.and_then(|()| {
        if graph.filter.is_empty() {
            Ok(())
        } else {
            std::fs::write(&graph_path, &graph.filter)
        }
    });
    if let Err(e) = written {
        set_export(&id, json!({"state": "failed", "error": format!("the film's folder cannot be written to: {e}")}));
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("the film's folder cannot be written to: {e}")));
    }
    let joins = (
        pieces.iter().skip(1).filter(|x| x.join == export::Join::Dissolve).count(),
        pieces.iter().skip(1).filter(|x| x.join == export::Join::Black).count(),
    );
    let once = p.sound_once;
    let app2 = app.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let tmp = out.with_extension("part.mp4");
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        if simple {
            cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&list_path);
        } else {
            for clip in &clips {
                cmd.arg("-i").arg(clip);
            }
        }
        if let Some(m) = &music {
            // Music repeats under the whole film; a narration is said once. Either ends with the film.
            if !once {
                cmd.args(["-stream_loop", "-1"]);
            }
            cmd.arg("-i").arg(m);
        }
        if !graph.filter.is_empty() {
            // The graph of a long film is longer than a command line may be: it is read from a file.
            cmd.arg("-/filter_complex").arg(&graph_path);
            cmd.args(["-map", &graph.video, "-map", &graph.audio]);
        }
        // Apple's own encoders: H.264 in hardware and AAC. Plays everywhere.
        cmd.args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "8M",
            "-pix_fmt",
            "yuv420p",
            "-r",
            "24",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(&tmp);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        let result = match cmd.output().await {
            Ok(o) if o.status.success() => std::fs::rename(&tmp, &out).map_err(|e| e.to_string()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("the shots could not be joined")
                .chars()
                .take(300)
                .collect()),
            Err(e) => Err(e.to_string()),
        };
        std::fs::remove_file(&list_path).ok();
        std::fs::remove_file(&graph_path).ok();
        match result {
            Ok(()) => {
                let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                app2.record(
                    "studio_exported",
                    json!({"shots": count, "seconds": started.elapsed().as_secs_f32(), "bytes": bytes, "music": music.is_some(),
                           "film_seconds": graph.frames as f32 / 24.0, "dissolves": joins.0, "through_black": joins.1, "plain": simple}),
                );
                set_export(
                    &id,
                    json!({"state": "done", "file": out.to_string_lossy(), "bytes": bytes, "shots": count, "seconds": graph.frames as f32 / 24.0}),
                );
            }
            Err(e) => {
                std::fs::remove_file(&tmp).ok();
                app2.record("studio_export_failed", json!({"shots": count, "plain": simple}));
                set_export(&id, json!({"state": "failed", "error": e}));
            }
        }
    });
    Ok(count)
}

#[derive(Deserialize)]
struct Narration {
    text: String,
}

/// The voice package reads a text; the reading is saved in the film's folder and becomes its sound file.
async fn narration(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Narration>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let text: String = body.text.trim().chars().take(MAX_NARRATION).collect();
    if text.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "write what is to be said in \"text\"");
    }
    let Some(voice) = app.manifest.packages.iter().find(|pk| pk.modality == Modality::Voice && !app.installed(pk).is_empty()) else {
        return fail(StatusCode::CONFLICT, "No voice is installed. Add a voice under Models first.");
    };
    let Some(port) = own_port(&app) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "the service does not know its own address");
    };
    let answer = app
        .http
        .post(format!("http://127.0.0.1:{port}/v1/audio/speech"))
        .bearer_auth(&app.token)
        .json(&json!({"model": voice.id, "input": text}))
        .timeout(Duration::from_secs(900))
        .send()
        .await;
    let wav = match answer {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
        },
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            return fail(
                StatusCode::BAD_GATEWAY,
                v["error"]["message"].as_str().unwrap_or("the voice could not read the text").to_string(),
            );
        }
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let file = FsPath::new(&p.folder).join("narration.wav");
    if let Err(e) = std::fs::write(&file, &wav) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the narration could not be saved in the film's folder: {e}"));
    }
    // The film was open while the voice read: take it as it is now.
    if let Some(now) = load(&app, &id) {
        p = now;
    }
    p.sound = file.to_string_lossy().to_string();
    p.sound_once = true;
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    app.record("studio_narration_made", json!({"package": voice.id, "chars": text.chars().count(), "bytes": wav.len()}));
    Json(view(&app, p)).into_response()
}

async fn export_state(Path(id): Path<String>) -> Json<Value> {
    Json(export_of(&id))
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/v1/studio/projects", get(list).post(create))
        .route("/v1/studio/projects/{id}", get(read).put(save).delete(forget))
        .route("/v1/studio/projects/{id}/render", post(render).delete(stop))
        .route("/v1/studio/projects/{id}/storyboard", post(storyboard))
        .route("/v1/studio/projects/{id}/export", get(export_state).post(export))
        .route("/v1/studio/projects/{id}/sound", get(sound))
        .route("/v1/studio/projects/{id}/narration", post(narration))
        .route("/v1/studio/projects/{id}/shots/{shot}/take", post(use_take))
        .route("/v1/studio/projects/{id}/shots/{shot}/takes/{n}", axum::routing::delete(delete_take))
        .route("/v1/studio/projects/{id}/clips/{name}", get(clip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: &str, prompt: &str, state: &str) -> Shot {
        Shot {
            id: id.into(),
            prompt: prompt.into(),
            frames: 121,
            state: state.into(),
            error: None,
            seconds: Some(200.0),
            ..Shot::default()
        }
    }

    #[test]
    fn a_shot_keeps_its_clip_until_its_description_changes() {
        let old = vec![shot("a1", "a fox in snow", "done"), shot("b2", "waves at dusk", "done")];
        let new = vec![
            ShotIn {
                id: Some("b2".into()),
                prompt: "waves at dusk".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
            ShotIn {
                id: Some("a1".into()),
                prompt: "a fox in rain".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
            ShotIn {
                id: None,
                prompt: "a new one".into(),
                frames: None,
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
        ];
        let out = merge_shots(&old, new, false).unwrap();
        assert_eq!(out.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["done", "changed", "empty"]);
        assert_eq!(out[0].id, "b2");
        assert!(valid_id(&out[2].id));
        // A new shape means every made shot has to be made again.
        let again = merge_shots(
            &old,
            vec![ShotIn {
                id: Some("a1".into()),
                prompt: "a fox in snow".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            }],
            true,
        )
        .unwrap();
        assert_eq!(again[0].state, "changed");
    }

    #[test]
    fn shots_are_bounded_and_ids_cannot_repeat_or_leave_the_folder() {
        assert!(merge_shots(
            &[],
            vec![ShotIn {
                id: None,
                prompt: "x".into(),
                frames: Some(999),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            }],
            false
        )
        .is_err());
        let twice = merge_shots(
            &[],
            vec![
                ShotIn {
                    id: Some("a1".into()),
                    prompt: "one".into(),
                    frames: None,
                    trim_start: 0.0,
                    trim_end: 0.0,
                    continues: false,
                    ..ShotIn::default()
                },
                ShotIn {
                    id: Some("a1".into()),
                    prompt: "two".into(),
                    frames: None,
                    trim_start: 0.0,
                    trim_end: 0.0,
                    continues: false,
                    ..ShotIn::default()
                },
            ],
            false,
        )
        .unwrap();
        assert_ne!(twice[0].id, twice[1].id);
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("A1") && valid_id("a1b2"));
        assert!(valid_size("640x384") && !valid_size("640x385") && !valid_size("64x64") && !valid_size("wide"));
    }

    #[test]
    fn the_export_list_has_made_shots_in_order_with_their_trims() {
        let dir = std::env::temp_dir().join(format!("rigbrew-studio-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        for id in ["a1", "c3"] {
            std::fs::write(dir.join("clips").join(format!("{id}.webm")), b"x").unwrap();
        }
        let mut second = shot("c3", "waves", "done");
        second.trim_start = 0.5;
        second.trim_end = 1.0;
        let p = Project {
            id: "p".into(),
            name: "A film: take 2!".into(),
            folder: dir.to_string_lossy().into(),
            size: "640x384".into(),
            shots: vec![shot("a1", "fox", "done"), shot("b2", "not made", "empty"), second],
            auto_export: false,
            sound: String::new(),
            sound_volume: 0.5,
            sound_once: false,
            fade_in: false,
            fade_out: false,
            updated_ms: 0,
        };
        let made = exported(&p);
        assert_eq!(made.len(), 2);
        let list = concat_list(&p, &made);
        let lines: Vec<&str> = list.lines().collect();
        assert!(lines[0].starts_with("ffconcat") && lines[1].ends_with("a1.webm'") && lines[2].ends_with("c3.webm'"));
        assert_eq!(&lines[3..], ["inpoint 0.500", "outpoint 4.042"]);
        assert_eq!(file_stem(&p.name), "A film take 2");
        assert_eq!(file_stem("///"), "film");
        // Trims always leave part of the shot.
        let kept = merge_shots(
            &[],
            vec![ShotIn {
                id: None,
                prompt: "x".into(),
                frames: Some(49),
                trim_start: 9.0,
                trim_end: 9.0,
                continues: false,
                ..ShotIn::default()
            }],
            false,
        )
        .unwrap();
        assert!(kept[0].trim_start <= 49.0 / 24.0 - 0.5 && kept[0].trim_end == 0.0);
        // A clip is as long as the model made it, which is what the trims are held against.
        let mut long = shot("d4", "x", "done");
        long.frames = 49;
        long.made_frames = Some(56);
        assert_eq!(long.length(), 56.0 / 24.0);
        long.state = "empty".into();
        assert_eq!(long.length(), 49.0 / 24.0);
        std::fs::remove_dir_all(dir).ok();
    }

    fn asked(id: &str, prompt: &str, continues: bool) -> ShotIn {
        ShotIn {
            id: Some(id.into()),
            prompt: prompt.into(),
            frames: Some(121),
            trim_start: 0.0,
            trim_end: 0.0,
            continues,
            ..ShotIn::default()
        }
    }

    #[test]
    fn a_shot_that_continues_another_is_made_again_after_a_move() {
        let mut b = shot("b2", "the fox stops", "done");
        b.continues = true;
        let mut d = shot("d4", "the boat turns", "done");
        d.continues = true;
        let old = vec![shot("a1", "a fox", "done"), b, shot("c3", "a boat", "done"), d];
        let states = |order: &[(&str, &str, bool)]| {
            let new = order.iter().map(|(id, prompt, continues)| asked(id, prompt, *continues)).collect();
            merge_shots(&old, new, false).unwrap().into_iter().map(|s| (s.state, s.continues)).collect::<Vec<_>>()
        };
        let (done, changed) = (|c| ("done".to_string(), c), |c| ("changed".to_string(), c));
        // Nothing moved: nothing changes.
        assert_eq!(
            states(&[("a1", "a fox", false), ("b2", "the fox stops", true), ("c3", "a boat", false), ("d4", "the boat turns", true)]),
            [done(false), done(true), done(false), done(true)]
        );
        // The two scenes change places as a whole: each shot still follows the same one.
        assert_eq!(
            states(&[("c3", "a boat", false), ("d4", "the boat turns", true), ("a1", "a fox", false), ("b2", "the fox stops", true)]),
            [done(false), done(true), done(false), done(true)]
        );
        // A continuing shot behind a different shot is made again; so is one that lost its place behind the first.
        assert_eq!(
            states(&[("a1", "a fox", false), ("c3", "a boat", false), ("b2", "the fox stops", true), ("d4", "the boat turns", true)]),
            [done(false), done(false), changed(true), changed(true)]
        );
        // The first shot of a film continues nothing.
        assert_eq!(states(&[("b2", "the fox stops", true), ("a1", "a fox", false)]), [changed(false), done(false)]);
    }

    #[test]
    fn the_frames_of_a_clip_are_read_from_what_the_tool_prints() {
        assert_eq!(frames_read("frame=   12 fps=0.0 q=-0.0 size=N/A\rframe=   56 fps=0.0 q=-0.0 Lsize=N/A time=00:00:02.33"), Some(56));
        assert_eq!(frames_read("frame=121 fps=30"), Some(121));
        assert_eq!(frames_read("clip.webm: Invalid data found when processing input"), None);
        assert_eq!(frames_read(""), None);
    }

    #[test]
    fn the_pieces_of_a_film_follow_its_shots() {
        let mut a = shot("a1", "fox", "done");
        a.made_frames = Some(56);
        a.join = "dissolve".into();
        let mut b = a.clone();
        b.trim_start = 0.5;
        b.trim_end = 0.75;
        b.volume = 1.4;
        let mut c = a.clone();
        c.continues = true;
        c.mute = true;
        let mut d = a.clone();
        d.join = "black".into();
        let made = pieces(&[&a, &b, &c, &d], &[true, true, true, false]);
        // The first shot has no join; a shot that continues the one before it is always a cut.
        assert_eq!(
            made.iter().map(|x| x.join).collect::<Vec<_>>(),
            [export::Join::Cut, export::Join::Dissolve, export::Join::Cut, export::Join::Black]
        );
        assert_eq!(made.iter().map(|x| x.frames).collect::<Vec<_>>(), [56, 26, 56, 56]);
        assert_eq!(made[1].start, 0.5);
        assert_eq!(made.iter().map(|x| x.volume).collect::<Vec<_>>(), [1.0, 1.4, 0.0, 1.0]);
        assert!(!made[3].has_sound && !export::plain(&made));
        // Joins and volume are kept by a save and leave the clip as it is.
        let kept = merge_shots(
            &[a.clone(), b.clone()],
            vec![
                ShotIn { join: "black".into(), volume: Some(9.0), mute: true, ..asked("a1", "fox", false) },
                ShotIn { join: "wipe".into(), ..asked("b2", "fox", false) },
            ],
            false,
        )
        .unwrap();
        assert_eq!((kept[0].state.as_str(), kept[0].join.as_str(), kept[0].volume, kept[0].mute), ("done", "black", 1.5, true));
        assert_eq!((kept[1].join.as_str(), kept[1].volume), ("", 1.0));
    }

    fn take(n: u32, frames: u32) -> Take {
        Take { n, seed: Some(n as i64 * 11), seconds: Some(n as f32 * 10.0), frames, made_frames: Some(frames + 7), continued: false }
    }

    #[test]
    fn a_shot_keeps_its_last_takes() {
        let mut s = shot("a1", "fox", "changed");
        assert!(s.add_take(take(1, 49)).is_empty());
        assert_eq!((s.take, s.state.as_str(), s.made_frames, s.seconds), (1, "done", Some(56), Some(10.0)));
        for n in 2..=4 {
            assert!(s.add_take(take(n, 49)).is_empty());
        }
        // The fifth take pushes out the oldest; the one in use is never the one to go.
        assert_eq!(s.add_take(take(5, 121)), [1]);
        assert_eq!(s.takes.iter().map(|t| t.n).collect::<Vec<_>>(), [2, 3, 4, 5]);
        assert!(s.use_take(2) && !s.use_take(1));
        assert_eq!((s.take, s.frames, s.made_frames, s.seconds), (2, 49, Some(56), Some(20.0)));
        assert_eq!(s.add_take(take(6, 49)), [2]);
        assert_eq!(s.take, 6);
        // Trims that fitted a long take are held against the shorter one.
        s.use_take(5);
        s.trim_start = 4.0;
        s.use_take(3);
        assert!(s.trim_start <= 56.0 / 24.0 - 0.5);
        // Another try that fails leaves the shot made, with what went wrong next to it.
        s.fail("no memory".into());
        assert_eq!((s.state.as_str(), s.error.as_deref()), ("done", Some("no memory")));
        let mut never = shot("n1", "never made", "empty");
        never.fail("no memory".into());
        assert_eq!(never.state, "failed");
        // Deleting the take in use falls back to the newest one left; deleting the last leaves the shot not made.
        assert!(s.remove_take(3) && !s.remove_take(3));
        assert_eq!(s.take, 6);
        assert!(s.remove_take(4) && s.remove_take(5) && s.remove_take(6));
        assert_eq!((s.take, s.state.as_str(), s.made_frames, s.takes.len()), (0, "empty", None, 0));
    }

    #[test]
    fn clips_are_asked_for_by_shot_and_take() {
        assert_eq!(clip_name("a1b2.webm"), Some(("a1b2", None, false)));
        assert_eq!(clip_name("a1b2.jpg"), Some(("a1b2", None, true)));
        assert_eq!(clip_name("a1b2.3.webm"), Some(("a1b2", Some(3), false)));
        assert_eq!(clip_name("a1b2.12.jpg"), Some(("a1b2", Some(12), true)));
        for bad in ["a1b2", "a1b2.mp4", "a1b2.0.webm", "a1b2.x.webm", "../a1.webm", "a1b2.1.2.webm", ".webm", "A1.webm"] {
            assert_eq!(clip_name(bad), None, "{bad}");
        }
        // The clip of a shot is the take in use; a shot from before takes has one clip.
        let mut with = shot("a1", "fox", "done");
        with.take = 3;
        let p = Project {
            id: "p".into(),
            name: "Film".into(),
            folder: "/films/fox".into(),
            size: "640x384".into(),
            shots: vec![with, shot("b2", "boat", "done")],
            auto_export: false,
            sound: String::new(),
            sound_volume: 0.5,
            sound_once: false,
            fade_in: false,
            fade_out: false,
            updated_ms: 0,
        };
        assert_eq!(clip_path(&p, "a1"), FsPath::new("/films/fox/clips/a1.3.webm"));
        assert_eq!(poster_path(&p, "a1"), FsPath::new("/films/fox/clips/a1.3.jpg"));
        assert_eq!(clip_path(&p, "b2"), FsPath::new("/films/fox/clips/b2.webm"));
        assert_eq!(take_files(&p, "b2", 2).0, FsPath::new("/films/fox/clips/b2.2.webm"));
    }

    #[test]
    fn a_film_says_how_long_it_still_needs() {
        let mut second = shot("b2", "the fox stops", "empty");
        second.continues = true;
        let mut third = shot("c3", "a boat", "changed");
        third.frames = 49;
        let p = Project {
            id: "p".into(),
            name: "Film".into(),
            folder: "/films/fox".into(),
            size: "640x384".into(),
            shots: vec![shot("a1", "a fox", "done"), second, third, shot("d4", "", "empty")],
            auto_export: false,
            sound: String::new(),
            sound_volume: 0.5,
            sound_once: false,
            fade_in: false,
            fade_out: false,
            updated_ms: 0,
        };
        let seen =
            [pace::Sample { frames: 121, continued: true, seconds: 400.0 }, pace::Sample { frames: 49, continued: false, seconds: 90.0 }];
        // Nothing is waiting: the two shots that are not made, and not the one without a description.
        assert_eq!(time_left(&p, &seen, &[], None), (490.0, 0.0));
        // The second is being made for 100 seconds and the third waits.
        assert_eq!(time_left(&p, &seen, &["c3".into()], Some(("b2", 100.0))), (490.0, 390.0));
        // A shot that takes longer than expected still has a little left.
        assert_eq!(time_left(&p, &seen, &[], Some(("b2", 900.0))).1, 20.0);
        // A made shot that is made again counts as left, and not as missing.
        assert_eq!(time_left(&p, &[], &["a1".into()], None), (121.0 / 24.0 * 80.0 + 49.0 / 24.0 * 25.0, 121.0 / 24.0 * 25.0));
    }

    #[test]
    fn only_sound_files_are_played_under_a_film() {
        assert_eq!(sound_kind(FsPath::new("/music/Theme.MP3")), Some("audio/mpeg"));
        assert_eq!(sound_kind(FsPath::new("/film/narration.wav")), Some("audio/wav"));
        assert_eq!(sound_kind(FsPath::new("/home/me/.ssh/id_ed25519")), None);
        assert_eq!(sound_kind(FsPath::new("/home/me/notes.txt")), None);
    }

    #[test]
    fn a_new_film_gets_a_folder_of_its_own() {
        let home = std::env::temp_dir().join(format!("rigbrew-films-{}", uuid::Uuid::new_v4().simple()));
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film"));
        std::fs::create_dir_all(home.join("The fox a film")).unwrap();
        // An empty folder of that name is taken; one that holds a film is not.
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film"));
        std::fs::write(home.join("The fox a film").join(FILE), "{}").unwrap();
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film 2"));
        assert_eq!(new_folder(&home, ""), home.join("film"));
        std::fs::remove_dir_all(home).ok();
    }
}
