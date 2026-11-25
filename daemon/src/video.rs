//! Short clips with an installed video package (the Pour family), made in the background: a clip
//! takes minutes, so the request returns a job and the app follows its progress.
//!
//!   POST   /v1/videos/generations       {"model", "prompt", "size": "480x832", "frames": 33, "steps": 20, "seed"} -> {"job": id}
//!   GET    /v1/videos/jobs              video jobs of this run, newest first
//!   GET    /v1/videos/jobs/{id}         state, stage, steps done, elapsed time; the saved clip once done
//!   DELETE /v1/videos/jobs/{id}         stop it (the engine stops at its next step)
//!   GET    /v1/videos/files/{id}.webm   a saved clip; DELETE removes it
//!
//! While a clip is being made the picture engine is busy with it: requests that would swap the
//! engine for another model are refused until it finishes or is stopped.

use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use rigbrew_core::engine::{Job as EngineJob, VideoRequest};
use rigbrew_core::manifest::Modality;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cloud::now_ms;
use crate::{ensure_sd, fail, App, Shared};

#[derive(Clone, Serialize)]
pub struct VideoJob {
    pub package: String,
    pub variant: String,
    /// "loading", "running", "done", "failed" or "cancelled".
    pub state: String,
    /// The engine's stage: "conditioning", "sampling", "vae_decode", "encoding", ...
    pub stage: String,
    pub step: Option<u64>,
    pub steps: Option<u64>,
    pub elapsed_ms: u64,
    pub created_ms: i64,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub video: Option<Value>,
    pub error: Option<String>,
    #[serde(skip)]
    pub engine_job: Option<String>,
    #[serde(skip)]
    pub cancel: bool,
}

impl VideoJob {
    fn active(&self) -> bool {
        self.state == "loading" || self.state == "running"
    }
}

/// Whether a clip is being made with a package other than `package`.
pub fn busy_with_other(app: &App, package: &str) -> Option<String> {
    app.videos.lock().unwrap().values().find(|j| j.active() && j.package != package).map(|j| j.package.clone())
}

pub fn refuse_busy(app: &App, package: &str) -> Option<Response> {
    busy_with_other(app, package).map(|_| {
        fail(
            StatusCode::CONFLICT,
            "A video is being made. Wait for it to finish or stop it, then try again: only one model fits in memory at a time.",
        )
    })
}

#[derive(Deserialize)]
pub struct VideoBody {
    model: String,
    prompt: String,
    #[serde(default)]
    size: Option<String>,
    /// Default and limits come from the package (`defaults` in the manifest).
    #[serde(default)]
    frames: Option<u32>,
    #[serde(default)]
    steps: Option<u32>,
    #[serde(default)]
    seed: Option<i64>,
    /// A picture the clip starts from, base64 (JPEG or PNG). Needs a package that can see pictures.
    #[serde(default)]
    init_image: Option<String>,
}

impl VideoBody {
    /// A request made inside the service (Studio makes its shots this way).
    pub fn new(model: &str, prompt: &str, size: &str, frames: u32) -> VideoBody {
        VideoBody {
            model: model.into(),
            prompt: prompt.into(),
            size: Some(size.into()),
            frames: Some(frames),
            steps: None,
            seed: None,
            init_image: None,
        }
    }

    /// The same request, starting from a picture.
    pub fn starting_from(mut self, image: Option<String>) -> VideoBody {
        self.init_image = image;
        self
    }
}

/// How long the engine may fail to answer about a clip before the clip counts as lost.
const SILENCE: Duration = Duration::from_secs(45);

const NEGATIVE: &str = "blurry, static, low quality, jpeg artifacts, deformed, extra limbs, text, subtitles, watermark";

pub async fn generate(State(app): State<Shared>, Json(body): Json<VideoBody>) -> Response {
    let Some(package) = app.manifest.package(&body.model).filter(|p| p.modality == Modality::Video).cloned() else {
        return fail(StatusCode::NOT_FOUND, format!("{} is not a video package", body.model));
    };
    if app.installed(&package).is_empty() {
        return fail(StatusCode::CONFLICT, format!("{} is not installed", package.name));
    }
    if body.prompt.trim().is_empty() {
        return fail(StatusCode::BAD_REQUEST, "describe the clip in \"prompt\"");
    }
    let (width, height) = body
        .size
        .as_deref()
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .unwrap_or((832, 480));
    if !(256..=1280).contains(&width) || !(256..=1280).contains(&height) || width % 32 != 0 || height % 32 != 0 {
        return fail(StatusCode::BAD_REQUEST, "size must be WIDTHxHEIGHT, each 256 to 1280 and a multiple of 32");
    }
    // What the package says about itself (the manifest's `defaults`), else what the first video package wanted.
    let d = &package.defaults;
    let num = |key: &str, fallback: u64| d[key].as_u64().unwrap_or(fallback) as u32;
    let (frame_step, max_frames) = (num("frame_step", 4).max(1), num("max_frames", 121));
    let frames = body.frames.unwrap_or_else(|| num("frames", 33));
    if !(5..=max_frames).contains(&frames) || (frames - 1) % frame_step != 0 {
        return fail(StatusCode::BAD_REQUEST, format!("frames must be {frame_step}k+1 between 5 and {max_frames}"));
    }
    // A package with a fixed schedule (a distilled model) takes its own step count, whatever was asked.
    let steps = if d["fixed_steps"].as_bool().unwrap_or(false) {
        num("steps", 20)
    } else {
        body.steps.unwrap_or_else(|| num("steps", 20)).clamp(1, 50)
    };
    // A clip can start from a picture only when the installed version carries the part that sees pictures.
    let sees = app.installed(&package).iter().any(|v| v.files.iter().any(|f| f.role == "mmproj"));
    let init_image = body.init_image.filter(|i| !i.is_empty());
    if init_image.is_some() && !sees {
        return fail(StatusCode::CONFLICT, format!("{} cannot start a clip from a picture", package.name));
    }
    if init_image.as_ref().is_some_and(|i| i.len() > 12 * 1024 * 1024) {
        return fail(StatusCode::BAD_REQUEST, "the picture is too large");
    }
    if app.videos.lock().unwrap().values().any(VideoJob::active) {
        return fail(StatusCode::CONFLICT, "A video is already being made. Wait for it or stop it first.");
    }
    if let Some(resp) = refuse_busy(&app, &package.id) {
        return resp;
    }

    let id = uuid::Uuid::new_v4().simple().to_string();
    let job = VideoJob {
        package: package.id.clone(),
        variant: String::new(),
        state: "loading".into(),
        stage: "loading".into(),
        step: None,
        steps: None,
        elapsed_ms: 0,
        created_ms: now_ms(),
        width,
        height,
        frames,
        video: None,
        error: None,
        engine_job: None,
        cancel: false,
    };
    app.videos.lock().unwrap().insert(id.clone(), job);
    app.record("prompt", json!({"package": package.id, "modality": "video", "prompt": body.prompt, "width": width, "height": height, "frames": frames, "steps": steps}));

    let req = VideoRequest {
        prompt: body.prompt,
        negative: NEGATIVE.into(),
        width,
        height,
        frames,
        fps: num("fps", 24),
        steps,
        seed: body.seed.unwrap_or(-1),
        cfg: d["cfg"].as_f64().unwrap_or(6.0) as f32,
        flow_shift: if d["cfg"].is_null() { Some(3.0) } else { d["flow_shift"].as_f64().map(|v| v as f32) },
        tiling: false,
        init_image,
    };
    tokio::spawn(run(app.clone(), id.clone(), package, req));
    (StatusCode::ACCEPTED, Json(json!({"job": id}))).into_response()
}

fn update(app: &App, id: &str, f: impl FnOnce(&mut VideoJob)) {
    if let Some(job) = app.videos.lock().unwrap().get_mut(id) {
        f(job);
    }
}

async fn run(app: Shared, id: String, package: rigbrew_core::manifest::Package, req: VideoRequest) {
    let started = std::time::Instant::now();
    let finish = |state: &str, error: Option<String>| {
        let state = state.to_string();
        update(&app, &id, |j| {
            j.state = state;
            j.error = error;
            j.elapsed_ms = started.elapsed().as_millis() as u64;
        });
    };
    let (server, variant) = match ensure_sd(&app, &package).await {
        Ok(v) => v,
        Err(resp) => {
            let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap_or_default();
            let message = serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string));
            finish("failed", Some(message.unwrap_or_else(|| "the video engine could not start".into())));
            return;
        }
    };
    update(&app, &id, |j| {
        j.variant = variant.clone();
        j.state = "running".into();
    });
    let engine_job = match server.submit_video(&req).await {
        Ok(j) => j,
        Err(e) => {
            app.record("video_failed", json!({"package": package.id, "variant": variant, "error": e.to_string()}));
            finish("failed", Some(e.to_string()));
            return;
        }
    };
    update(&app, &id, |j| j.engine_job = Some(engine_job.clone()));
    let mut asked_to_stop = false;
    let mut silent_since: Option<std::time::Instant> = None;
    loop {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let cancel = app.videos.lock().unwrap().get(&id).is_some_and(|j| j.cancel);
        if cancel && !asked_to_stop {
            server.cancel_job(&engine_job).await;
            asked_to_stop = true;
        }
        let seen = server.job(&engine_job).await;
        if seen.is_ok() {
            silent_since = None;
        }
        match seen {
            Ok(EngineJob::Running(p)) => update(&app, &id, |j| {
                j.stage = p.stage;
                j.step = p.completed;
                j.steps = p.total;
                j.elapsed_ms = started.elapsed().as_millis() as u64;
            }),
            Ok(EngineJob::Done { b64, mime, .. }) => {
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
                    finish("failed", Some("the engine returned an unreadable clip".into()));
                    return;
                };
                let seconds = started.elapsed().as_secs_f32();
                let saved = save(&app, &bytes, &mime);
                app.record(
                    "video_generated",
                    json!({"package": package.id, "variant": variant, "seconds": seconds, "width": req.width, "height": req.height,
                           "frames": req.frames, "steps": req.steps, "bytes": bytes.len()}),
                );
                update(&app, &id, |j| {
                    j.stage = "done".into();
                    j.video = saved;
                });
                crate::touch(&app, &package.id).await;
                finish("done", None);
                return;
            }
            Ok(EngineJob::Cancelled) => {
                app.record(
                    "video_cancelled",
                    json!({"package": package.id, "variant": variant, "seconds": started.elapsed().as_secs_f32()}),
                );
                finish("cancelled", None);
                return;
            }
            Ok(EngineJob::Failed(why)) => {
                app.record("video_failed", json!({"package": package.id, "variant": variant, "error": why}));
                finish("failed", Some(why));
                return;
            }
            // A busy machine can miss an answer; the engine is gone only when it stays silent.
            Err(_) if silent_since.get_or_insert_with(std::time::Instant::now).elapsed() < SILENCE => continue,
            Err(e) => {
                // The engine went away (it was stopped or crashed).
                app.record("video_failed", json!({"package": package.id, "variant": variant, "error": e.to_string()}));
                finish("failed", Some(format!("the video engine stopped: {e}")));
                return;
            }
        }
    }
}

fn save(app: &App, bytes: &[u8], mime: &str) -> Option<Value> {
    let ext = if mime.contains("webp") { "webp" } else { "webm" };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = app.root.join("videos");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{id}.{ext}");
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes).ok()?;
    std::fs::rename(&tmp, dir.join(&name)).ok()?;
    Some(json!({"id": id, "path": format!("/v1/videos/files/{name}"), "bytes": bytes.len(), "mime": mime}))
}

pub async fn jobs(State(app): State<Shared>) -> Json<Value> {
    let videos = app.videos.lock().unwrap();
    let mut list: Vec<(&String, &VideoJob)> = videos.iter().collect();
    list.sort_by_key(|(_, j)| std::cmp::Reverse(j.created_ms));
    Json(json!({"jobs": list.into_iter().map(|(id, j)| json!({"id": id, "job": j})).collect::<Vec<_>>()}))
}

pub async fn job(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.videos.lock().unwrap().get(&id) {
        Some(j) => Json(json!(j)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such video job"),
    }
}

pub async fn cancel(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let mut videos = app.videos.lock().unwrap();
    match videos.get_mut(&id) {
        Some(j) if j.active() => {
            j.cancel = true;
            Json(json!({"cancelling": id})).into_response()
        }
        Some(_) => fail(StatusCode::CONFLICT, "that video job has already ended"),
        None => fail(StatusCode::NOT_FOUND, "no such video job"),
    }
}

/// Clips are stored as `<32 hex>.webm` (or `.webp`); nothing else can be read or deleted here.
fn video_file(app: &App, name: &str) -> Option<(std::path::PathBuf, &'static str)> {
    let (id, mime) = match name.rsplit_once('.') {
        Some((id, "webm")) => (id, "video/webm"),
        Some((id, "webp")) => (id, "image/webp"),
        _ => return None,
    };
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        .then(|| (app.root.join("videos").join(name), mime))
}

pub async fn file_get(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some((path, mime)) = video_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad video name");
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such video"),
    }
}

pub async fn file_delete(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some((path, _)) = video_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad video name");
    };
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Json(json!({"deleted": name})).into_response(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such video"),
    }
}
