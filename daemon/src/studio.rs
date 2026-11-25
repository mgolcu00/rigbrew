//! Studio: films made of shots, kept in a folder the person chooses.
//!
//!   GET    /v1/studio/projects                    the films this install knows
//!   POST   /v1/studio/projects                    {"folder"?, "name"}: start a film in a folder (created if missing;
//!                                                 without one, a new folder named after the film under Movies/Rigbrew)
//!   GET    /v1/studio/projects/{id}               the film: its shots and what is being made
//!   PUT    /v1/studio/projects/{id}               {"name", "size", "auto_export", "shots": [{"id"?, "prompt", "frames"}]}
//!   DELETE /v1/studio/projects/{id}               forget the film (its folder stays)
//!   POST   /v1/studio/projects/{id}/render        {"shots": [id]?, "model"?}: make the listed shots, or every shot not made yet
//!   DELETE /v1/studio/projects/{id}/render        stop: the shot being made is stopped, the rest leave the queue
//!   POST   /v1/studio/projects/{id}/storyboard    {"idea", "shots"}: the chat model writes the shots in scenes (not saved)
//!   POST   /v1/studio/projects/{id}/export      {"music"?, "music_volume"?}: join the made shots into `<folder>/<name>.mp4`
//!   GET    /v1/studio/projects/{id}/export      where the export is: {"state", "file", "error"}
//!   GET    /v1/studio/projects/{id}/clips/{shot}.webm
//!
//! A film is `<folder>/rigbrew-film.json` plus `<folder>/clips/<shot>.webm`; the list of folders is
//! `<data dir>/studio.json`. Shots are made one after another by the installed video package through
//! the same path as `/v1/videos/generations`, so a long film renders unattended, shot by shot. A film
//! with `auto_export` is exported by the same worker when its last shot is made.

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

const FILE: &str = "rigbrew-film.json";
const MAX_SHOTS: usize = 200;
const MAX_PROMPT: usize = 1200;
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
    /// Seconds cut from the start and from the end of the clip when the film is exported or played.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_start: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_end: f32,
    /// Start this shot from the last picture of the shot before it, so the two join without a cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continues: bool,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
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

fn clip_path(p: &Project, shot: &str) -> PathBuf {
    FsPath::new(&p.folder).join("clips").join(format!("{shot}.webm"))
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

/// The film as the app sees it: the stored film plus what the queue says right now.
fn view(app: &App, mut p: Project) -> Value {
    let (now, waiting) =
        with_queue(|q| (q.now.clone(), q.waiting.iter().filter(|(pid, _)| *pid == p.id).map(|(_, s)| s.clone()).collect::<Vec<_>>()));
    let mut progress = Value::Null;
    for shot in &mut p.shots {
        if now.as_ref().is_some_and(|(pid, sid, _)| *pid == p.id && *sid == shot.id) {
            shot.state = "making".into();
            if let Some((_, _, job)) = &now {
                if let Some(j) = app.videos.lock().unwrap().get(job) {
                    progress = json!({"shot": shot.id, "stage": j.stage, "step": j.step, "steps": j.steps, "elapsed_ms": j.elapsed_ms});
                }
            }
        } else if waiting.contains(&shot.id) {
            shot.state = "queued".into();
        }
    }
    let made = p.shots.iter().filter(|s| s.state == "done").count();
    let seconds: f32 = p.shots.iter().map(|s| s.frames as f32 / 24.0).sum();
    let continues = app
        .manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video)
        .any(|pk| app.installed(pk).iter().any(|v| v.files.iter().any(|f| f.role == "mmproj")));
    let export = export_of(&p.id);
    json!({"project": p, "progress": progress, "rendering": now.is_some() || !waiting.is_empty(), "made": made, "seconds": seconds, "can_continue": continues, "export": export})
}

async fn list(State(app): State<Shared>) -> Json<Value> {
    let films: Vec<Value> = index(&app)
        .into_iter()
        .map(|(id, folder)| match load(&app, &id) {
            Some(p) => json!({"id": id, "name": p.name, "folder": folder, "shots": p.shots.len(), "made": p.shots.iter().filter(|s| s.state == "done").count(), "updated_ms": p.updated_ms}),
            None => json!({"id": id, "name": "", "folder": folder, "missing": true}),
        })
        .collect();
    Json(json!({"projects": films, "home": films_home().map(|h| h.to_string_lossy().to_string())}))
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

#[derive(Deserialize)]
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
        // Trims leave at least half a second of the shot.
        let length = frames as f32 / 24.0;
        let trim_start = s.trim_start.clamp(0.0, (length - 0.5).max(0.0));
        let trim_end = s.trim_end.clamp(0.0, (length - 0.5 - trim_start).max(0.0));
        let id = s.id.filter(|i| valid_id(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(new_id);
        let shot = match old.iter().find(|o| o.id == id) {
            Some(o) => {
                let same = o.prompt == prompt && o.frames == frames && o.continues == s.continues && !size_changed;
                let state = if same || o.state == "empty" { o.state.clone() } else { "changed".into() };
                Shot {
                    id,
                    prompt,
                    frames,
                    state,
                    error: if same { o.error.clone() } else { None },
                    seconds: o.seconds,
                    trim_start,
                    trim_end,
                    continues: s.continues,
                }
            }
            None => Shot { id, prompt, frames, state: empty(), error: None, seconds: None, trim_start, trim_end, continues: s.continues },
        };
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
        // Another clip (from the chat) may be in the making: wait for the engine to be free.
        let job = loop {
            let resp = video::generate(
                State(app.clone()),
                Json(video::VideoBody::new(&model, &s.prompt, &p.size, s.frames).starting_from(first.clone())),
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
                set_shot(&app, &project, &shot, |s| {
                    s.state = "failed".into();
                    s.error = Some(e);
                });
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
        let made = state == "done"
            && file.as_ref().and_then(|v| v["path"].as_str()).and_then(|path| path.rsplit('/').next()).is_some_and(|name| {
                let from = app.root.join("videos").join(name);
                let to = clip_path(&p, &shot);
                to.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::copy(&from, &to).is_ok()
            });
        set_shot(&app, &project, &shot, |s| {
            if made {
                s.state = "done".into();
                s.error = None;
                s.seconds = Some(seconds);
            } else if state == "cancelled" {
                // Stopped by the person: the shot is as it was before.
                s.error = None;
            } else {
                s.state = "failed".into();
                s.error = Some(error.unwrap_or_else(|| "the clip could not be saved in the film's folder".into()));
            }
        });
        if made {
            app.record("studio_shot_made", json!({"frames": s.frames, "seconds": seconds}));
        }
        // The last waiting shot of a film that exports by itself: join it now, with the screen open or not.
        if made && !with_queue(|q| q.waiting.iter().any(|(pid, _)| *pid == project)) {
            export_when_complete(&app, &project).await;
        }
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

async fn clip(State(app): State<Shared>, Path((id, name)): Path<(String, String)>) -> Response {
    let Some(shot) = name.strip_suffix(".webm").filter(|s| valid_id(s)) else {
        return fail(StatusCode::BAD_REQUEST, "bad clip name");
    };
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    match tokio::fs::read(clip_path(&p, shot)).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "video/webm")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "that shot has no clip yet"),
    }
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
    let Some(port) = std::fs::read_to_string(app.root.join("daemon.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["port"].as_u64())
    else {
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

/// The concat list ffmpeg reads: every made shot in order, with its trims.
fn concat_list(p: &Project) -> (String, usize) {
    let mut list = String::from("ffconcat version 1.0\n");
    let mut count = 0;
    for s in p.shots.iter().filter(|s| s.state == "done" || s.state == "changed") {
        let path = clip_path(p, &s.id);
        if !path.is_file() {
            continue;
        }
        let length = s.frames as f32 / 24.0;
        list.push_str(&format!("file '{}'\n", path.to_string_lossy().replace('\'', "'\\''")));
        if s.trim_start > 0.0 {
            list.push_str(&format!("inpoint {:.3}\n", s.trim_start));
        }
        if s.trim_end > 0.0 {
            list.push_str(&format!("outpoint {:.3}\n", (length - s.trim_end).max(s.trim_start + 0.1)));
        }
        count += 1;
    }
    (list, count)
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
async fn begin_export(app: &Shared, p: Project, body: Export) -> Result<usize, (StatusCode, String)> {
    let id = p.id.clone();
    let (list, count) = concat_list(&p);
    if count == 0 {
        return Err((StatusCode::CONFLICT, "No shot is made yet. Make the shots first.".into()));
    }
    let music = match body.music.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if FsPath::new(m).is_absolute() && FsPath::new(m).is_file() => Some(m.to_string()),
        Some(_) => return Err((StatusCode::BAD_REQUEST, "the sound file was not found: give its full path".into())),
        None => None,
    };
    if EXPORTS.lock().unwrap().iter().any(|(i, v)| *i == id && v["state"] == "working") {
        return Err((StatusCode::CONFLICT, "This film is being exported already.".into()));
    }
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg")
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("The tool that joins the shots could not be set up: {e}")))?;
    let folder = PathBuf::from(&p.folder);
    let out = folder.join(format!("{}.mp4", file_stem(&p.name)));
    let list_path = folder.join(".rigbrew-export.txt");
    std::fs::write(&list_path, list)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("the film's folder cannot be written to: {e}")))?;
    set_export(&id, json!({"state": "working", "shots": count}));
    let volume = body.music_volume.unwrap_or(0.5).clamp(0.0, 1.0);
    let app2 = app.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let tmp = out.with_extension("part.mp4");
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"]).arg(&list_path);
        if let Some(m) = &music {
            // The music runs under the whole film and ends with it.
            cmd.args(["-stream_loop", "-1", "-i"]).arg(m);
            cmd.args([
                "-filter_complex",
                &format!("[1:a]volume={volume:.2}[m];[0:a][m]amix=inputs=2:duration=first:dropout_transition=0[a]"),
                "-map",
                "0:v",
                "-map",
                "[a]",
            ]);
        }
        // Apple's own encoders: H.264 in hardware and AAC. Plays everywhere.
        cmd.args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "8M",
            "-pix_fmt",
            "yuv420p",
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
        match result {
            Ok(()) => {
                let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                app2.record(
                    "studio_exported",
                    json!({"shots": count, "seconds": started.elapsed().as_secs_f32(), "bytes": bytes, "music": music.is_some()}),
                );
                set_export(&id, json!({"state": "done", "file": out.to_string_lossy(), "bytes": bytes, "shots": count}));
            }
            Err(e) => {
                std::fs::remove_file(&tmp).ok();
                app2.record("studio_export_failed", json!({"shots": count}));
                set_export(&id, json!({"state": "failed", "error": e}));
            }
        }
    });
    Ok(count)
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
            trim_start: 0.0,
            trim_end: 0.0,
            continues: false,
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
            },
            ShotIn {
                id: Some("a1".into()),
                prompt: "a fox in rain".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
            },
            ShotIn { id: None, prompt: "a new one".into(), frames: None, trim_start: 0.0, trim_end: 0.0, continues: false },
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
            vec![ShotIn { id: None, prompt: "x".into(), frames: Some(999), trim_start: 0.0, trim_end: 0.0, continues: false }],
            false
        )
        .is_err());
        let twice = merge_shots(
            &[],
            vec![
                ShotIn { id: Some("a1".into()), prompt: "one".into(), frames: None, trim_start: 0.0, trim_end: 0.0, continues: false },
                ShotIn { id: Some("a1".into()), prompt: "two".into(), frames: None, trim_start: 0.0, trim_end: 0.0, continues: false },
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
            updated_ms: 0,
        };
        let (list, count) = concat_list(&p);
        assert_eq!(count, 2);
        let lines: Vec<&str> = list.lines().collect();
        assert!(lines[0].starts_with("ffconcat") && lines[1].ends_with("a1.webm'") && lines[2].ends_with("c3.webm'"));
        assert_eq!(&lines[3..], ["inpoint 0.500", "outpoint 4.042"]);
        assert_eq!(file_stem(&p.name), "A film take 2");
        assert_eq!(file_stem("///"), "film");
        // Trims always leave part of the shot.
        let kept = merge_shots(
            &[],
            vec![ShotIn { id: None, prompt: "x".into(), frames: Some(49), trim_start: 9.0, trim_end: 9.0, continues: false }],
            false,
        )
        .unwrap();
        assert!(kept[0].trim_start <= 49.0 / 24.0 - 0.5 && kept[0].trim_end == 0.0);
        std::fs::remove_dir_all(dir).ok();
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
