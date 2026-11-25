//! Studio: films made of shots, kept in a folder the person chooses.
//!
//!   GET    /v1/studio/projects                    the films this install knows
//!   POST   /v1/studio/projects                    {"folder", "name"}: start a film in a folder (created if missing)
//!   GET    /v1/studio/projects/{id}               the film: its shots and what is being made
//!   PUT    /v1/studio/projects/{id}               {"name", "size", "shots": [{"id"?, "prompt", "frames"}]}
//!   DELETE /v1/studio/projects/{id}               forget the film (its folder stays)
//!   POST   /v1/studio/projects/{id}/render        {"shots": [id]?, "model"?}: make the listed shots, or every shot not made yet
//!   DELETE /v1/studio/projects/{id}/render        stop: the shot being made is stopped, the rest leave the queue
//!   POST   /v1/studio/projects/{id}/storyboard    {"idea", "shots"}: the chat model writes shot descriptions (not saved)
//!   GET    /v1/studio/projects/{id}/clips/{shot}.webm
//!
//! A film is `<folder>/rigbrew-film.json` plus `<folder>/clips/<shot>.webm`; the list of folders is
//! `<data dir>/studio.json`. Shots are made one after another by the installed video package through
//! the same path as `/v1/videos/generations`, so a long film renders unattended, shot by shot.

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

const FILE: &str = "rigbrew-film.json";
const MAX_SHOTS: usize = 200;
const MAX_PROMPT: usize = 1200;

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
    json!({"project": p, "progress": progress, "rendering": now.is_some() || !waiting.is_empty(), "made": made, "seconds": seconds})
}

async fn list(State(app): State<Shared>) -> Json<Value> {
    let films: Vec<Value> = index(&app)
        .into_iter()
        .map(|(id, folder)| match load(&app, &id) {
            Some(p) => json!({"id": id, "name": p.name, "folder": folder, "shots": p.shots.len(), "made": p.shots.iter().filter(|s| s.state == "done").count(), "updated_ms": p.updated_ms}),
            None => json!({"id": id, "name": "", "folder": folder, "missing": true}),
        })
        .collect();
    Json(json!({"projects": films}))
}

#[derive(Deserialize)]
struct NewProject {
    folder: String,
    #[serde(default)]
    name: String,
}

async fn create(State(app): State<Shared>, Json(body): Json<NewProject>) -> Response {
    let folder = PathBuf::from(body.folder.trim());
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
}

#[derive(Deserialize)]
struct Save {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    shots: Option<Vec<ShotIn>>,
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
        let shot = match old.iter().find(|o| o.id == id) {
            Some(o) => {
                let same = o.prompt == prompt && o.frames == frames && !size_changed;
                let state = if same || o.state == "empty" { o.state.clone() } else { "changed".into() };
                Shot { id, prompt, frames, state, error: if same { o.error.clone() } else { None }, seconds: o.seconds }
            }
            None => Shot { id, prompt, frames, state: empty(), error: None, seconds: None },
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
        q.now.as_ref().filter(|(pid, _, _)| *pid == id).map(|(_, _, job)| job.clone())
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
            if next.is_none() {
                q.working = false;
                q.now = None;
            }
            next
        }) else {
            return;
        };
        let Some(p) = load(&app, &project) else { continue };
        let Some(s) = p.shots.iter().find(|s| s.id == shot).cloned() else { continue };
        let model = with_queue(|q| q.model.clone()).unwrap_or_default();
        // Another clip (from the chat) may be in the making: wait for the engine to be free.
        let job = loop {
            let resp = video::generate(State(app.clone()), Json(video::VideoBody::new(&model, &s.prompt, &p.size, s.frames))).await;
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
        with_queue(|q| q.now = Some((project.clone(), shot.clone(), job.clone())));
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
    }
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

/// "1. A fox ..." lines out of what the chat model wrote.
fn shot_lines(text: &str, limit: usize) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', '•']).trim();
            let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
            // Numbered ("3. ...", "3) ...") or bulleted lines only; headings and chatter are dropped.
            let numbered = rest.len() < line.len() && (rest.starts_with('.') || rest.starts_with(')') || rest.starts_with(':'));
            let body = if numbered { rest[1..].trim() } else { "" };
            let body = body.trim_matches(['"', '*', '`']).trim();
            (body.chars().count() >= 12).then(|| body.chars().take(MAX_PROMPT).collect())
        })
        .take(limit)
        .collect()
}

async fn storyboard(State(app): State<Shared>, Path(_id): Path<String>, Json(body): Json<Idea>) -> Response {
    let idea: String = body.idea.trim().chars().take(2000).collect();
    if idea.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "say what the film is about in \"idea\"");
    }
    let count = body.shots.unwrap_or(6).clamp(1, 24) as usize;
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
    let system = format!(
        "You plan short films for a text-to-video model. Write exactly {count} shots as a numbered list, one line each, nothing else. \
         Each line describes one continuous shot of a few seconds in English: who or what is seen, what moves, the place, the light, the camera, and the sound. \
         Describe the same characters and places with the same words in every shot so they look alike. No dialogue, no titles, no text on screen."
    );
    let request = json!({
        "model": chat.id,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": idea}],
        "max_tokens": 120 * count + 200,
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
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            return fail(StatusCode::BAD_GATEWAY, v["error"]["message"].as_str().unwrap_or("the chat model could not answer").to_string());
        }
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let shots = shot_lines(&text, count);
    if shots.is_empty() {
        return fail(StatusCode::BAD_GATEWAY, "The chat model did not write a list of shots. Try again, or say more about the film.");
    }
    app.record("studio_storyboard_written", json!({"shots": shots.len()}));
    Json(json!({"shots": shots.into_iter().map(|prompt| json!({"prompt": prompt})).collect::<Vec<_>>()})).into_response()
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/v1/studio/projects", get(list).post(create))
        .route("/v1/studio/projects/{id}", get(read).put(save).delete(forget))
        .route("/v1/studio/projects/{id}/render", post(render).delete(stop))
        .route("/v1/studio/projects/{id}/storyboard", post(storyboard))
        .route("/v1/studio/projects/{id}/clips/{name}", get(clip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: &str, prompt: &str, state: &str) -> Shot {
        Shot { id: id.into(), prompt: prompt.into(), frames: 121, state: state.into(), error: None, seconds: Some(200.0) }
    }

    #[test]
    fn a_shot_keeps_its_clip_until_its_description_changes() {
        let old = vec![shot("a1", "a fox in snow", "done"), shot("b2", "waves at dusk", "done")];
        let new = vec![
            ShotIn { id: Some("b2".into()), prompt: "waves at dusk".into(), frames: Some(121) },
            ShotIn { id: Some("a1".into()), prompt: "a fox in rain".into(), frames: Some(121) },
            ShotIn { id: None, prompt: "a new one".into(), frames: None },
        ];
        let out = merge_shots(&old, new, false).unwrap();
        assert_eq!(out.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["done", "changed", "empty"]);
        assert_eq!(out[0].id, "b2");
        assert!(valid_id(&out[2].id));
        // A new shape means every made shot has to be made again.
        let again =
            merge_shots(&old, vec![ShotIn { id: Some("a1".into()), prompt: "a fox in snow".into(), frames: Some(121) }], true).unwrap();
        assert_eq!(again[0].state, "changed");
    }

    #[test]
    fn shots_are_bounded_and_ids_cannot_repeat_or_leave_the_folder() {
        assert!(merge_shots(&[], vec![ShotIn { id: None, prompt: "x".into(), frames: Some(999) }], false).is_err());
        let twice = merge_shots(
            &[],
            vec![
                ShotIn { id: Some("a1".into()), prompt: "one".into(), frames: None },
                ShotIn { id: Some("a1".into()), prompt: "two".into(), frames: None },
            ],
            false,
        )
        .unwrap();
        assert_ne!(twice[0].id, twice[1].id);
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("A1") && valid_id("a1b2"));
        assert!(valid_size("640x384") && !valid_size("640x385") && !valid_size("64x64") && !valid_size("wide"));
    }

    #[test]
    fn a_storyboard_is_read_from_numbered_lines_only() {
        let text = "Here are your shots:\n1. A red fox trots through fresh snow between birch trees, soft light.\n2) The fox stops and looks up as snow falls from a branch.\n\n**Notes**: keep it calm.\n3. \"Close on the fox's face, breath in the cold air, quiet wind.\"";
        let shots = shot_lines(text, 10);
        assert_eq!(shots.len(), 3);
        assert!(shots[0].starts_with("A red fox") && shots[2].starts_with("Close on"));
        assert_eq!(shot_lines(text, 2).len(), 2);
    }
}
