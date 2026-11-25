//! `rigbrewd`: the local service behind the Rigbrew app.
//!
//! Listens on 127.0.0.1 only. Every request must carry the bearer token written to
//! `<data dir>/daemon.json` when the service starts, so other programs and web pages on the
//! machine cannot use it.
//!
//!   GET    /v1/system                     hardware profile, rescanned on every call
//!   GET    /v1/packages                   catalogue with a recommendation and what is installed
//!   POST   /v1/packages/{id}/pull         start a download: {"variant": "q5"} (optional); 403 when the package needs an account and nobody is signed in
//!   DELETE /v1/packages/{id}              remove installed files: ?variant=q5 for one variant, all otherwise
//!   GET    /v1/jobs                       all download jobs of this run
//!   GET    /v1/jobs/{id}                  download progress
//!   DELETE /v1/jobs/{id}                  cancel a running download (the partial file is kept for resume)
//!   GET    /v1/engine                     which model is loaded, or null
//!   DELETE /v1/engine                     unload it and free its memory
//!   GET    /v1/consent                    accepted consent (or null) and the current consent version
//!   POST   /v1/consent                    {"version": "..."}
//!   DELETE /v1/consent                    withdraw
//!   POST   /v1/events                     {"kind": "...", "payload": {...}}
//!   GET    /v1/events/pending             events waiting to be sent: ?limit=200&order=newest|oldest
//!   PUT    /v1/session                    who is signed in (from the app); DELETE on sign-out
//!   GET    /v1/account                    the current session, or null
//!   GET    /v1/usage                      local totals, disk use and sync status
//!   GET    /v1/activity?days=365          per-day activity, most used packages, totals (for the profile page)
//!   GET    /v1/activity/recent?limit=50   recent activity in words, newest first
//!   GET    /v1/chats                      saved chats, newest first; DELETE removes them all
//!   GET    /v1/chats/{id}                 one chat; PUT {title, data} saves it (8 MB at most); DELETE removes it
//!   GET    /v1/images/files/{id}.png      a generated picture (saved by /v1/images/generations); DELETE removes it
//!   POST   /v1/sync                       send queued events now
//!   GET    /v1/app/config                 server's app config (updates, sponsors), cached for offline
//!   POST   /v1/chat/completions           OpenAI-compatible; "model" is a package id
//!   POST   /v1/images/generations         OpenAI-compatible: {"model", "prompt", "size": "1024x1024", "steps", "seed"};
//!                                         the answer also carries "image": {id, path, bytes} for the saved file
//!   GET    /v1/images/progress            stage and steps of the picture being made, or null
//!   POST   /v1/audio/speech               OpenAI-compatible: {"model", "input", "voice", "speed"}; answers with a WAV
//!   GET    /v1/audio/files/{id}.wav       a saved reading; DELETE removes it
//!   POST   /v1/audio/transcriptions       OpenAI-compatible, multipart: "file" (16-bit PCM WAV), "model", "language"
//!   POST   /v1/videos/generations         {"model", "prompt", "size", "frames", "steps", "seed"}: a clip, made in the background
//!   GET    /v1/videos/jobs[/{id}]         progress of clips; DELETE /v1/videos/jobs/{id} stops one
//!   GET    /v1/videos/files/{id}.webm     a saved clip; DELETE removes it

mod cloud;
mod library;
mod speech;
mod video;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rigbrew_core::chats::ChatStore;
use rigbrew_core::engine::{ImageRequest, LlamaConfig, LlamaServer, SdConfig, SdServer};
use rigbrew_core::events::EventStore;
use rigbrew_core::hardware::{self, HardwareProfile};
use rigbrew_core::manifest::{Engine as EngineKind, EngineBuild};
use rigbrew_core::manifest::{Modality, Package, Variant};
use rigbrew_core::select::{recommend, Verdict};
use rigbrew_core::{download, Manifest};
use serde::Deserialize;
use serde_json::{json, Value};

enum Engine {
    Llama(Box<LlamaServer>),
    Sd(Arc<SdServer>),
}

impl Engine {
    async fn stop(self) {
        match self {
            Engine::Llama(s) => (*s).stop().await,
            // Dropping the last handle kills the process (kill_on_drop).
            Engine::Sd(s) => {
                if let Ok(s) = Arc::try_unwrap(s) {
                    s.stop().await;
                }
            }
        }
    }
}

struct Loaded {
    package: String,
    variant: String,
    /// Memory the variant needs on the GPU, from the manifest.
    mem_mb: u64,
    /// When a request last used it; idle models are taken out of memory (`idle_loop`).
    last_used: std::time::Instant,
    server: Engine,
}

impl Loaded {
    fn is_chat(&self) -> bool {
        matches!(self.server, Engine::Llama(_))
    }
}

/// Stops the loaded engines `which` picks. A picture or video engine busy with a clip is refused.
async fn evict(app: &App, slots: &mut Vec<Loaded>, wanted: &str, which: impl Fn(&Loaded) -> bool) -> Result<(), Box<Response>> {
    if slots.iter().any(|l| which(l) && !l.is_chat()) {
        if let Some(resp) = video::refuse_busy(app, wanted) {
            return Err(Box::new(resp));
        }
    }
    let (gone, kept): (Vec<Loaded>, Vec<Loaded>) = std::mem::take(slots).into_iter().partition(|l| which(l));
    *slots = kept;
    for l in gone {
        l.server.stop().await;
    }
    Ok(())
}

/// Makes room for `need_mb` more: a chat model and a picture model stay in memory together when
/// both fit the budget, which saves two swaps for every picture asked for in a chat.
async fn make_room(app: &App, slots: &mut Vec<Loaded>, wanted: &str, need_mb: u64) -> Result<(), Box<Response>> {
    let used: u64 = slots.iter().map(|l| l.mem_mb).sum();
    if used + need_mb > app.profile.accel_budget_mb {
        evict(app, slots, wanted, |_| true).await?;
    }
    Ok(())
}

#[derive(Clone, Default, serde::Serialize)]
struct Job {
    package: String,
    variant: String,
    /// "running", "done" or "failed".
    state: String,
    file: String,
    done: u64,
    total: u64,
    error: Option<String>,
}

struct App {
    profile: HardwareProfile,
    manifest: Manifest,
    root: PathBuf,
    token: String,
    events: Mutex<EventStore>,
    chats: Mutex<ChatStore>,
    jobs: Mutex<HashMap<String, Job>>,
    tasks: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    wanted_lock: Mutex<()>,
    /// Models in memory: at most one chat engine and one picture or video engine, both only when
    /// they fit the memory budget together. The one used last is at the end.
    engine: tokio::sync::Mutex<Vec<Loaded>>,
    http: reqwest::Client,
    /// Client for the server on the internet (the other one never leaves loopback).
    net: reqwest::Client,
    server: String,
    session: Mutex<Option<cloud::Session>>,
    sync: Mutex<cloud::SyncState>,
    sync_now: tokio::sync::Notify,
    /// `DO_NOT_TRACK=1` or `RIGBREW_NO_TELEMETRY=1` at start: nothing is uploaded.
    telemetry_off: bool,
    /// Held while the voice engine reads something aloud.
    voice: tokio::sync::Mutex<()>,
    /// Held while the listening engine writes down a recording.
    listen: tokio::sync::Mutex<()>,
    /// Video clips of this run, by job id.
    videos: Mutex<HashMap<String, video::VideoJob>>,
    /// Where the picture being made is: stage and steps, for the app's card.
    image_progress: Mutex<Option<rigbrew_core::engine::VideoProgress>>,
}

type Shared = Arc<App>;

/// Where the signed list of packages and engines lives.
const DEFAULT_MANIFEST: &str = "https://models.rigbrew.dev/manifest.json";

/// Version of the consent text this build ships. A stored consent for another version does not count.
const CONSENT_VERSION: &str = "draft-2026-11-20.2";

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": {"message": message.into()}}))).into_response()
}

impl App {
    fn packages_dir(&self) -> PathBuf {
        self.root.join("packages")
    }

    fn installed<'a>(&self, package: &'a Package) -> Vec<&'a Variant> {
        let dir = self.packages_dir();
        package.variants.iter().filter(|v| v.files.iter().all(|f| rigbrew_core::bundle::present(&dir, f))).collect()
    }

    fn record(&self, kind: &str, payload: Value) {
        if let Ok(store) = self.events.lock() {
            store.record(kind, payload).ok();
        }
    }
}

async fn auth(State(app): State<Shared>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| t == app.token);
    if ok {
        next.run(req).await
    } else {
        fail(StatusCode::UNAUTHORIZED, "missing or wrong token")
    }
}

async fn system(State(app): State<Shared>) -> Json<Value> {
    // Free memory and disk change while the app runs; the tier and budgets do not.
    let root = app.root.clone();
    let fresh = tokio::task::spawn_blocking(move || hardware::scan_with_data_dir(&root)).await.unwrap_or_else(|_| app.profile.clone());
    Json(json!(fresh))
}

async fn packages(State(app): State<Shared>) -> Json<Value> {
    let list: Vec<Value> = app
        .manifest
        .packages
        .iter()
        .map(|p| {
            json!({
                "id": p.id, "name": p.name, "modality": p.modality, "source": p.source,
                "variants": p.variants.iter().map(|v| json!({"id": v.id, "quality": v.quality, "disk_mb": v.disk_mb(), "accel_mem_mb": v.accel_mem_mb, "metrics": v.metrics})).collect::<Vec<_>>(),
                "recommendation": recommend(&app.profile, p).verdict,
                "installed": app.installed(p).iter().map(|v| v.id.clone()).collect::<Vec<_>>(),
                "account_required": cloud::account_required(&app, p),
                "defaults": p.defaults,
                "terms": p.terms,
                "credit": p.credit,
            })
        })
        .collect();
    Json(json!({"packages": list}))
}

/// The program an engine kind runs, if it runs as a separate program.
fn engine_binary_name(kind: EngineKind) -> Option<&'static str> {
    match kind {
        EngineKind::Llama => Some("llama-server"),
        EngineKind::Sdcpp => Some("sd-server"),
        EngineKind::Sherpa => Some(rigbrew_core::voice::PROGRAM),
    }
}

/// The engine build this machine needs for `kind`, unless a development engine folder is set.
fn engine_build_for(app: &App, kind: EngineKind) -> Option<EngineBuild> {
    if std::env::var_os("RIGBREW_ENGINE_DIR").is_some() {
        return None;
    }
    let backends: Vec<_> = app.profile.gpus.iter().map(|g| g.backend).collect();
    rigbrew_core::engines::pick(&app.manifest, kind, &backends).cloned()
}

/// Path of the program for `kind`: from `RIGBREW_ENGINE_DIR` in development, otherwise from the
/// engines listed in the signed manifest, downloading and unpacking the build on first use.
async fn engine_program(app: &App, kind: EngineKind) -> Result<PathBuf, String> {
    let name = engine_binary_name(kind).ok_or("this engine does not run as a separate program")?;
    engine_tool(app, kind, name).await
}

/// Path of the program `name` from the engine build for `kind` (a build can carry several, like
/// sherpa-onnx's voice and listening programs), installing the build on first use.
async fn engine_tool(app: &App, kind: EngineKind, name: &str) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("RIGBREW_ENGINE_DIR") {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        return Ok(PathBuf::from(dir).join(file));
    }
    let build = engine_build_for(app, kind)
        .ok_or_else(|| format!("there is no {name} build for {} in the manifest", rigbrew_core::engines::platform()))?;
    if let Some(path) = rigbrew_core::engines::installed(&app.root, &build, name) {
        return Ok(path);
    }
    install_engine(app, &build, &mut |_| {}).await?;
    rigbrew_core::engines::installed(&app.root, &build, name).ok_or_else(|| format!("{name} is missing after installing the engine"))
}

async fn install_engine(app: &App, build: &EngineBuild, progress: &mut (dyn FnMut(download::Progress<'_>) + Send)) -> Result<(), String> {
    let started = std::time::Instant::now();
    match rigbrew_core::engines::install(&download::client(), &app.manifest, build, &app.root, progress).await {
        Ok(_) => {
            app.record("engine_installed", json!({"engine": build.engine, "version": build.version, "platform": build.platform, "seconds": started.elapsed().as_secs_f64()}));
            Ok(())
        }
        Err(e) => {
            app.record("engine_install_failed", json!({"engine": build.engine, "version": build.version, "error": e.to_string()}));
            Err(e.to_string())
        }
    }
}

/// Downloads that were asked for and have not finished: survives a restart of the service.
fn wanted_path(app: &App) -> PathBuf {
    app.root.join("state").join("pulls.json")
}

fn wanted_read(app: &App) -> Vec<(String, String)> {
    std::fs::read_to_string(wanted_path(app)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn wanted_update(app: &App, change: impl FnOnce(&mut Vec<(String, String)>)) {
    let _guard = app.wanted_lock.lock().unwrap();
    let mut list = wanted_read(app);
    change(&mut list);
    let path = wanted_path(app);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    // Write to a temporary file and rename, so a crash never leaves a half-written list.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(&list).unwrap_or_default()).is_ok() {
        std::fs::rename(&tmp, &path).ok();
    }
}

/// Starts (or resumes) a download in the background and returns its job id.
fn start_pull(app: &Shared, package: Package, variant: Variant) -> String {
    let app = app.clone();
    let key = (package.id.clone(), variant.id.clone());
    wanted_update(&app, |list| {
        if !list.contains(&key) {
            list.push(key.clone());
        }
    });
    let job_id = uuid::Uuid::new_v4().simple().to_string();
    // The engine that runs this model comes with the first model that needs it.
    let engine = engine_build_for(&app, package.engine)
        .filter(|b| engine_binary_name(package.engine).is_some_and(|name| rigbrew_core::engines::installed(&app.root, b, name).is_none()));
    let total = variant.files.iter().map(|f| f.size).sum::<u64>() + engine.as_ref().map_or(0, |b| b.archive.size);
    let job = Job { package: package.id.clone(), variant: variant.id.clone(), state: "running".into(), total, ..Job::default() };
    app.jobs.lock().unwrap().insert(job_id.clone(), job);
    app.record("package_pull_started", json!({"package": package.id, "variant": variant.id}));

    let (task_app, task_id) = (app.clone(), job_id.clone());
    let handle = tokio::spawn(async move {
        let app = task_app;
        let mut sizes: HashMap<String, u64> = variant.files.iter().map(|f| (f.path.clone(), f.size)).collect();
        if let Some(b) = &engine {
            sizes.insert(b.archive.path.clone(), b.archive.size);
        }
        let mut finished = 0u64;
        let mut current = String::new();
        let progress_app = app.clone();
        let progress_id = task_id.clone();
        let mut on_progress = move |p: download::Progress<'_>| {
            if current != p.path {
                finished += sizes.get(&current).copied().unwrap_or(0);
                current = p.path.to_string();
            }
            if let Some(job) = progress_app.jobs.lock().unwrap().get_mut(&progress_id) {
                job.file = current.clone();
                job.done = finished + p.done;
            }
        };
        let free_mb = hardware::scan_with_data_dir(&app.root).storage.free_mb;
        let mut result =
            download::pull(&download::client(), &app.manifest, &package, &variant, &app.packages_dir(), free_mb, &mut on_progress)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
        if let (Ok(()), Some(build)) = (&result, &engine) {
            result = install_engine(&app, build, &mut on_progress).await;
        }
        let mut jobs = app.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(&task_id) {
            match &result {
                Ok(_) => {
                    job.state = "done".into();
                    job.done = job.total;
                }
                Err(e) => {
                    job.state = "failed".into();
                    job.error = Some(e.clone());
                }
            }
        }
        drop(jobs);
        if result.is_ok() {
            let done = (package.id.clone(), variant.id.clone());
            wanted_update(&app, |list| list.retain(|k| *k != done));
        }
        let kind = if result.is_ok() { "package_pull_finished" } else { "package_pull_failed" };
        app.record(kind, json!({"package": package.id, "variant": variant.id, "error": result.err()}));
        app.tasks.lock().unwrap().remove(&task_id);
    });
    app.tasks.lock().unwrap().insert(job_id.clone(), handle.abort_handle());
    job_id
}

#[derive(Deserialize, Default)]
struct PullBody {
    variant: Option<String>,
    /// The person agreed to the package's licence terms (asked for when it has any).
    #[serde(default)]
    accept_terms: bool,
}

async fn pull(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<PullBody>>) -> Response {
    let Some(package) = app.manifest.package(&id).cloned() else {
        return fail(StatusCode::NOT_FOUND, format!("no package {id}"));
    };
    // Some packages are only for signed-in accounts (the manifest or the server's app config says which).
    if cloud::account_required(&app, &package) && !cloud::signed_in(&app) {
        app.record("package_pull_refused", json!({"package": package.id, "reason": "account"}));
        return fail(
            StatusCode::FORBIDDEN,
            format!("{} can only be downloaded with a Rigbrew account. Sign in, then install it.", package.name),
        );
    }
    let agreed = body.as_ref().is_some_and(|b| b.0.accept_terms);
    if package.terms.is_some() && !agreed {
        return fail(StatusCode::CONFLICT, format!("{} has licence terms to agree to before it is downloaded", package.name));
    }
    let wanted = body.and_then(|b| b.0.variant).or_else(|| match recommend(&app.profile, &package).verdict {
        Verdict::Run { variant, .. } => Some(variant),
        _ => None,
    });
    let Some(variant) = wanted.and_then(|w| package.variants.iter().find(|v| v.id == w).cloned()) else {
        return fail(StatusCode::CONFLICT, format!("{} has no variant that fits this machine", package.name));
    };
    if package.terms.is_some() {
        app.record("package_terms_accepted", json!({"package": package.id}));
    }
    let job_id = start_pull(&app, package, variant);
    Json(json!({"job": job_id})).into_response()
}

async fn job(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.jobs.lock().unwrap().get(&id) {
        Some(job) => Json(json!(job)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such job"),
    }
}

async fn jobs(State(app): State<Shared>) -> Json<Value> {
    let jobs = app.jobs.lock().unwrap();
    Json(json!({"jobs": jobs.iter().map(|(id, j)| json!({"id": id, "job": j})).collect::<Vec<_>>()}))
}

async fn job_cancel(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(handle) = app.tasks.lock().unwrap().remove(&id) else {
        return fail(StatusCode::NOT_FOUND, "no running job with that id");
    };
    handle.abort();
    if let Some(job) = app.jobs.lock().unwrap().get_mut(&id) {
        job.state = "cancelled".into();
        let key = (job.package.clone(), job.variant.clone());
        wanted_update(&app, |list| list.retain(|k| *k != key));
        app.record("package_pull_cancelled", json!({"package": job.package, "variant": job.variant}));
    }
    Json(json!({"cancelled": id})).into_response()
}

#[derive(Deserialize, Default)]
struct RemoveQuery {
    variant: Option<String>,
}

async fn package_remove(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<RemoveQuery>) -> Response {
    let Some(package) = app.manifest.package(&id) else {
        return fail(StatusCode::NOT_FOUND, format!("no package {id}"));
    };
    {
        let mut slots = app.engine.lock().await;
        if let Err(resp) = evict(&app, &mut slots, &id, |l| l.package == id).await {
            return *resp;
        }
    }
    let dir = app.packages_dir();
    let installed: Vec<&Variant> = app.installed(package);
    let remove: Vec<&Variant> = installed.iter().copied().filter(|v| q.variant.as_ref().is_none_or(|w| *w == v.id)).collect();
    if remove.is_empty() {
        return fail(StatusCode::NOT_FOUND, "that variant is not installed");
    }
    // Files shared with a variant that stays installed (text encoder, VAE) are kept.
    let keep: std::collections::HashSet<&str> =
        installed.iter().filter(|v| !remove.iter().any(|r| r.id == v.id)).flat_map(|v| v.files.iter().map(|f| f.path.as_str())).collect();
    let mut freed = 0u64;
    let mut removed = Vec::new();
    for v in &remove {
        for f in &v.files {
            if keep.contains(f.path.as_str()) {
                continue;
            }
            let gone = if rigbrew_core::bundle::is_bundle(f) {
                rigbrew_core::bundle::remove(&dir, f)
            } else {
                std::fs::remove_file(dir.join(&f.path)).is_ok()
            };
            if gone {
                freed += f.size;
            }
        }
        removed.push(v.id.clone());
    }
    app.record("package_removed", json!({"package": id, "variants": removed, "freed_bytes": freed}));
    Json(json!({"removed": removed, "freed_bytes": freed})).into_response()
}

async fn engine_get(State(app): State<Shared>) -> Json<Value> {
    let slots = app.engine.lock().await;
    let show = |l: &Loaded| {
        json!({"package": l.package, "variant": l.variant, "mem_mb": l.mem_mb,
               "engine": match l.server { Engine::Llama(_) => "llama", Engine::Sd(_) => "sdcpp" }})
    };
    // "loaded" is the one used last; "all" lists every model in memory.
    Json(json!({"loaded": slots.last().map(show), "all": slots.iter().map(show).collect::<Vec<_>>()}))
}

async fn engine_unload(State(app): State<Shared>) -> Response {
    let mut slots = app.engine.lock().await;
    let was: Vec<String> = slots.iter().map(|l| l.package.clone()).collect();
    if let Err(resp) = evict(&app, &mut slots, "", |_| true).await {
        return *resp;
    }
    for package in &was {
        app.record("engine_unloaded", json!({"package": package}));
    }
    Json(json!({"unloaded": was.first(), "all": was})).into_response()
}

async fn consent_get(State(app): State<Shared>) -> Json<Value> {
    Json(json!({"consent": app.events.lock().unwrap().consent().ok().flatten(), "current_version": CONSENT_VERSION}))
}

#[derive(Deserialize)]
struct ConsentBody {
    version: String,
}

async fn consent_post(State(app): State<Shared>, Json(body): Json<ConsentBody>) -> Response {
    if body.version != CONSENT_VERSION {
        return fail(StatusCode::CONFLICT, format!("this build asks for consent version {CONSENT_VERSION}"));
    }
    let result = app.events.lock().unwrap().accept_consent(&body.version);
    match result {
        Ok(c) => {
            app.record("consent_accepted", json!({"version": c.version}));
            Json(json!({"consent": c})).into_response()
        }
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn consent_delete(State(app): State<Shared>) -> Response {
    match app.events.lock().unwrap().withdraw_consent() {
        Ok(()) => Json(json!({"consent": null})).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct EventBody {
    kind: String,
    #[serde(default)]
    payload: Value,
}

async fn event_post(State(app): State<Shared>, Json(body): Json<EventBody>) -> Response {
    let valid = (1..=48).contains(&body.kind.len()) && body.kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !valid {
        return fail(StatusCode::BAD_REQUEST, "event kinds are 1 to 48 of a-z, 0-9 and _");
    }
    let recorded = app.events.lock().unwrap().record(&body.kind, body.payload).unwrap_or(false);
    Json(json!({"recorded": recorded})).into_response()
}

#[derive(Deserialize, Default)]
struct PendingQuery {
    limit: Option<usize>,
    order: Option<String>,
}

async fn events_pending(State(app): State<Shared>, Query(q): Query<PendingQuery>) -> Json<Value> {
    let store = app.events.lock().unwrap();
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let events = if q.order.as_deref() == Some("newest") { store.pending_newest(limit) } else { store.pending(limit) };
    Json(json!({"count": store.pending_count().unwrap_or(0), "events": events.unwrap_or_default()}))
}

/// The installed variant to run: the one the selector recommends for this machine if it is
/// installed, otherwise the best-quality installed one.
fn preferred_installed<'a>(app: &App, package: &'a Package) -> Option<&'a Variant> {
    let mut installed = app.installed(package);
    if let Verdict::Run { variant, .. } = recommend(&app.profile, package).verdict {
        if let Some(v) = installed.iter().find(|v| v.id == variant) {
            return Some(v);
        }
    }
    installed.sort_by(|a, b| b.quality.total_cmp(&a.quality));
    installed.first().copied()
}

/// Makes sure the engine for `package` is running and returns its (port, key).
async fn ensure_engine(app: &App, package: &Package) -> Result<(u16, String, String), Box<Response>> {
    let mut slots = app.engine.lock().await;
    if let Some(pos) = slots.iter().position(|l| l.package == package.id && l.is_chat()) {
        // The one used last goes to the end: "loaded" in GET /v1/engine.
        let mut l = slots.remove(pos);
        l.last_used = std::time::Instant::now();
        let out = match &l.server {
            Engine::Llama(server) => (server.port(), server.key().to_string(), l.variant.clone()),
            Engine::Sd(_) => unreachable!(),
        };
        slots.push(l);
        return Ok(out);
    }
    let Some(variant) = preferred_installed(app, package) else {
        return Err(Box::new(fail(StatusCode::CONFLICT, format!("{} is not installed", package.name))));
    };
    // Another chat model goes; a picture model stays if both fit.
    evict(app, &mut slots, &package.id, Loaded::is_chat).await?;
    make_room(app, &mut slots, &package.id, variant.accel_mem_mb).await?;
    let dir = app.packages_dir();
    let file = |role: &str| variant.files.iter().find(|f| f.role == role).map(|f| dir.join(&f.path));
    let Some(model) = file("model") else {
        return Err(Box::new(fail(StatusCode::INTERNAL_SERVER_ERROR, "package has no model file")));
    };
    let cfg = LlamaConfig {
        binary: match engine_program(app, EngineKind::Llama).await {
            Ok(path) => path,
            Err(e) => return Err(Box::new(fail(StatusCode::SERVICE_UNAVAILABLE, e))),
        },
        model,
        mmproj: file("mmproj"),
        context: 8192,
        gpu_layers: if variant.accel_mem_mb <= app.profile.accel_budget_mb { 999 } else { 0 },
        log: app.root.join("logs").join("llama-server.log"),
    };
    let server =
        LlamaServer::start(&cfg, Duration::from_secs(300)).await.map_err(|e| Box::new(fail(StatusCode::BAD_GATEWAY, e.to_string())))?;
    app.record("engine_started", json!({"package": package.id, "variant": variant.id, "load_seconds": server.load_seconds}));
    let out = (server.port(), server.key().to_string(), variant.id.clone());
    slots.push(Loaded {
        package: package.id.clone(),
        variant: variant.id.clone(),
        mem_mb: variant.accel_mem_mb,
        last_used: std::time::Instant::now(),
        server: Engine::Llama(Box::new(server)),
    });
    Ok(out)
}

async fn chat(State(app): State<Shared>, Json(mut body): Json<Value>) -> Response {
    let Some(model) = body["model"].as_str().map(str::to_string) else {
        return fail(StatusCode::BAD_REQUEST, "\"model\" is required");
    };
    let Some(package) = app.manifest.package(&model).filter(|p| p.modality == Modality::Chat) else {
        return fail(StatusCode::NOT_FOUND, format!("no chat package {model}"));
    };
    let (port, key, variant) = match ensure_engine(&app, package).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    app.record("prompt", json!({"package": package.id, "variant": variant, "modality": "chat", "messages": body["messages"]}));
    // The engine serves a single model; its name there is irrelevant.
    if let Some(obj) = body.as_object_mut() {
        obj.remove("model");
    }
    let started = std::time::Instant::now();
    let upstream = app.http.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).bearer_auth(key).json(&body).send().await;
    let resp = match upstream {
        Ok(resp) => resp,
        Err(e) => {
            app.record("error", json!({"where": "chat", "message": e.to_string()}));
            return fail(StatusCode::BAD_GATEWAY, e.to_string());
        }
    };
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type =
        resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/json").to_string();
    // Forward the answer as it arrives and keep a copy, so the full response can be logged when it ends.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(64);
    let (log_app, package_id) = (app.clone(), package.id.clone());
    let streaming = content_type.contains("event-stream");
    tokio::spawn(async move {
        use futures_util::StreamExt;
        let mut raw = Vec::new();
        let mut upstream = resp.bytes_stream();
        let mut client_gone = false;
        while let Some(chunk) = upstream.next().await {
            let Ok(chunk) = chunk else { break };
            raw.extend_from_slice(&chunk);
            if !client_gone && tx.send(Ok(chunk)).await.is_err() {
                // The app stopped listening (Stop button). Drop the upstream so the engine stops too.
                client_gone = true;
                break;
            }
        }
        drop(tx);
        let mut answer = summarize_answer(&String::from_utf8_lossy(&raw), streaming);
        answer["package"] = json!(package_id);
        answer["variant"] = json!(variant);
        answer["seconds"] = json!(started.elapsed().as_secs_f64());
        answer["stopped_by_user"] = json!(client_gone);
        log_app.record("response", answer);
        touch(&log_app, &package_id).await;
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)))
        .unwrap()
}

/// Pulls the text, tool calls and counters out of an engine answer (SSE stream or one JSON body).
fn summarize_answer(raw: &str, streaming: bool) -> Value {
    let mut text = String::new();
    let mut tools: Vec<Value> = Vec::new();
    let mut out = json!({});
    let mut take = |v: &Value| {
        let choice = &v["choices"][0];
        for part in [&choice["delta"], &choice["message"]] {
            if let Some(t) = part["content"].as_str() {
                text.push_str(t);
            }
            if let Some(calls) = part["tool_calls"].as_array() {
                for c in calls {
                    let i = c["index"].as_u64().unwrap_or(tools.len() as u64) as usize;
                    while tools.len() <= i {
                        tools.push(json!({"name": "", "arguments": ""}));
                    }
                    if let Some(n) = c["function"]["name"].as_str() {
                        tools[i]["name"] = json!(format!("{}{n}", tools[i]["name"].as_str().unwrap_or("")));
                    }
                    if let Some(a) = c["function"]["arguments"].as_str() {
                        tools[i]["arguments"] = json!(format!("{}{a}", tools[i]["arguments"].as_str().unwrap_or("")));
                    }
                }
            }
        }
        if let Some(r) = choice["finish_reason"].as_str() {
            out["finish_reason"] = json!(r);
        }
        if let Some(t) = v.get("timings") {
            out["tokens_in"] = t["prompt_n"].clone();
            out["tokens_out"] = t["predicted_n"].clone();
            out["tokens_per_second"] = t["predicted_per_second"].clone();
        } else if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            out["tokens_in"] = u["prompt_tokens"].clone();
            out["tokens_out"] = u["completion_tokens"].clone();
        }
    };
    if streaming {
        for line in raw.lines() {
            if let Some(data) = line.trim().strip_prefix("data: ") {
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    take(&v);
                }
            }
        }
    } else if let Ok(v) = serde_json::from_str::<Value>(raw) {
        take(&v);
    }
    out["text"] = json!(text);
    if !tools.is_empty() {
        out["tool_calls"] = json!(tools);
    }
    out
}

/// Makes sure `sd-server` is running for `package` and returns a handle to it.
async fn ensure_sd(app: &App, package: &Package) -> Result<(Arc<SdServer>, String), Box<Response>> {
    let mut slots = app.engine.lock().await;
    if let Some(pos) = slots.iter().position(|l| l.package == package.id && !l.is_chat()) {
        let mut l = slots.remove(pos);
        l.last_used = std::time::Instant::now();
        let out = match &l.server {
            Engine::Sd(server) => (server.clone(), l.variant.clone()),
            Engine::Llama(_) => unreachable!(),
        };
        slots.push(l);
        return Ok(out);
    }
    let Some(variant) = preferred_installed(app, package) else {
        return Err(Box::new(fail(StatusCode::CONFLICT, format!("{} is not installed", package.name))));
    };
    // Another picture or video model goes; the chat model stays if both fit.
    evict(app, &mut slots, &package.id, |l| !l.is_chat()).await?;
    make_room(app, &mut slots, &package.id, variant.accel_mem_mb).await?;
    let dir = app.packages_dir();
    let file = |role: &str| variant.files.iter().find(|f| f.role == role).map(|f| dir.join(&f.path));
    let (vae, tae) = (file("vae"), file("tae"));
    let (Some(dit), Some(text_encoder), true) = (file("dit"), file("te"), vae.is_some() || tae.is_some()) else {
        return Err(Box::new(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{} is missing a part (dit, te, and vae or tae)", package.name),
        )));
    };
    let binary = engine_program(app, EngineKind::Sdcpp).await.map_err(|e| Box::new(fail(StatusCode::SERVICE_UNAVAILABLE, e)))?;
    let connector = file("connector");
    // A video package with a T5 text encoder says so by having no connector; everything else is an LLM.
    let t5 = package.modality == Modality::Video && connector.is_none();
    let cfg = SdConfig {
        binary,
        dit,
        text_encoder,
        text_encoder_flag: if t5 { "--t5xxl" } else { "--llm" },
        vae,
        tae,
        connector,
        audio_vae: file("audio_vae"),
        extra_args: package.engine_args.clone(),
        log: app.root.join("logs").join("sd-server.log"),
    };
    let server =
        SdServer::start(&cfg, Duration::from_secs(600)).await.map_err(|e| Box::new(fail(StatusCode::BAD_GATEWAY, e.to_string())))?;
    app.record("engine_started", json!({"package": package.id, "variant": variant.id, "load_seconds": server.load_seconds}));
    let server = Arc::new(server);
    slots.push(Loaded {
        package: package.id.clone(),
        variant: variant.id.clone(),
        mem_mb: variant.accel_mem_mb,
        last_used: std::time::Instant::now(),
        server: Engine::Sd(server.clone()),
    });
    Ok((server, variant.id.clone()))
}

/// Resolves on SIGTERM (never on platforms without it).
async fn terminated() {
    #[cfg(unix)]
    {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
            return;
        }
    }
    std::future::pending::<()>().await
}

/// Resolves when the process named by `RIGBREW_PARENT_PID` no longer exists (never if unset).
async fn parent_gone() {
    #[cfg(unix)]
    {
        if let Some(pid) = std::env::var("RIGBREW_PARENT_PID").ok().and_then(|p| p.parse::<i32>().ok()).filter(|p| *p > 1) {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                // Signal 0 checks existence without sending anything.
                let alive = unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
                if !alive {
                    return;
                }
            }
        }
    }
    std::future::pending::<()>().await
}

#[derive(Deserialize)]
struct ImageBody {
    model: String,
    prompt: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    steps: Option<u32>,
    #[serde(default)]
    seed: Option<i64>,
}

async fn image(State(app): State<Shared>, Json(body): Json<ImageBody>) -> Response {
    let Some(package) = app.manifest.package(&body.model).filter(|p| p.modality == Modality::Image) else {
        return fail(StatusCode::NOT_FOUND, format!("no image package {}", body.model));
    };
    let (width, height) = body
        .size
        .as_deref()
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .unwrap_or((1024, 1024));
    if !(256..=2048).contains(&width) || !(256..=2048).contains(&height) || width % 16 != 0 || height % 16 != 0 {
        return fail(StatusCode::BAD_REQUEST, "size must be WIDTHxHEIGHT, each 256 to 2048 and a multiple of 16");
    }
    let steps = body.steps.unwrap_or(8).clamp(1, 50);
    let (server, variant) = match ensure_sd(&app, package).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    app.record("prompt", json!({"package": package.id, "variant": variant, "modality": "image", "prompt": body.prompt, "width": width, "height": height, "steps": steps}));
    let req = ImageRequest { prompt: body.prompt, width, height, steps, seed: body.seed.unwrap_or(-1) };
    let made = make_image(&app, &server, &req).await;
    touch(&app, &package.id).await;
    match made {
        Ok((b64, seconds)) => {
            let saved = library::save_image(&app, &b64);
            app.record("image_generated", json!({"package": package.id, "variant": variant, "seconds": seconds, "width": width, "height": height, "steps": steps, "image_id": saved.as_ref().map(|s| s["id"].clone())}));
            Json(json!({"data": [{"b64_json": b64}], "seconds": seconds, "variant": variant, "image": saved})).into_response()
        }
        Err(e) => {
            app.record("image_failed", json!({"package": package.id, "variant": variant, "error": e.to_string()}));
            fail(StatusCode::BAD_GATEWAY, e.to_string())
        }
    }
}

/// Marks a loaded model as just used.
async fn touch(app: &App, package: &str) {
    if let Some(l) = app.engine.lock().await.iter_mut().find(|l| l.package == package) {
        l.last_used = std::time::Instant::now();
    }
}

/// Takes models out of memory after `RIGBREW_IDLE_MINUTES` (10 by default; 0 keeps them) without
/// use, so an open app does not hold gigabytes it is not using. A model making a clip stays.
async fn idle_loop(app: Shared) {
    let minutes: u64 = std::env::var("RIGBREW_IDLE_MINUTES").ok().and_then(|m| m.parse().ok()).unwrap_or(10);
    if minutes == 0 {
        return;
    }
    let idle = Duration::from_secs(minutes * 60);
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let busy: Vec<String> = app
            .videos
            .lock()
            .unwrap()
            .values()
            .filter(|j| j.state == "loading" || j.state == "running")
            .map(|j| j.package.clone())
            .collect();
        let mut slots = app.engine.lock().await;
        let stale: Vec<String> =
            slots.iter().filter(|l| l.last_used.elapsed() > idle && !busy.contains(&l.package)).map(|l| l.package.clone()).collect();
        if stale.is_empty() {
            continue;
        }
        let (gone, kept): (Vec<Loaded>, Vec<Loaded>) = std::mem::take(&mut *slots).into_iter().partition(|l| stale.contains(&l.package));
        *slots = kept;
        drop(slots);
        for l in gone {
            app.record("engine_unloaded", json!({"package": l.package, "variant": l.variant, "reason": "idle", "idle_minutes": minutes}));
            l.server.stop().await;
        }
    }
}

/// Cancels the engine's job if the request that started it goes away (the app pressed Stop).
struct CancelOnDrop {
    server: Arc<SdServer>,
    job: Option<String>,
    app: Shared,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        *self.app.image_progress.lock().unwrap() = None;
        if let Some(job) = self.job.take() {
            let server = self.server.clone();
            tokio::spawn(async move { server.cancel_job(&job).await });
        }
    }
}

/// One picture through the engine's job queue: progress is published for the app, and a request
/// that is dropped half-way (Stop) stops the engine too.
async fn make_image(app: &Shared, server: &Arc<SdServer>, req: &ImageRequest) -> Result<(String, f32), String> {
    use rigbrew_core::engine::Job;
    let started = std::time::Instant::now();
    let id = server.submit_image(req).await.map_err(|e| e.to_string())?;
    let mut guard = CancelOnDrop { server: server.clone(), job: Some(id.clone()), app: app.clone() };
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match server.job(&id).await.map_err(|e| e.to_string())? {
            Job::Running(p) => *app.image_progress.lock().unwrap() = Some(p),
            Job::Done { b64, .. } => {
                guard.job = None;
                return Ok((b64, started.elapsed().as_secs_f32()));
            }
            Job::Failed(why) => {
                guard.job = None;
                return Err(why);
            }
            Job::Cancelled => {
                guard.job = None;
                return Err("the picture was stopped".into());
            }
        }
    }
}

async fn image_progress(State(app): State<Shared>) -> Json<Value> {
    Json(json!({"progress": *app.image_progress.lock().unwrap()}))
}

/// Loads the manifest from a file (`<file>.minisig` next to it) or from a URL (`<url>.minisig`).
/// A URL is cached after its signature checks out, so the app also starts offline.
async fn load_manifest(source: &str) -> Result<Manifest, String> {
    if !(source.starts_with("https://") || source.starts_with("http://")) {
        let (manifest, signed) = rigbrew_core::manifest::load_file(std::path::Path::new(source)).map_err(|e| e.to_string())?;
        if !signed {
            eprintln!(
                "rigbrewd: warning: {source} is not signed; accepted because this is a development build or RIGBREW_ALLOW_UNSIGNED=1"
            );
        }
        return Ok(manifest);
    }
    let cache = hardware::default_data_dir().join("state").join("manifest.json");
    let client = reqwest::Client::builder()
        .user_agent(concat!("rigbrewd/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let fetch = |url: String| {
        let client = client.clone();
        async move {
            let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("{url} answered HTTP {}", resp.status().as_u16()));
            }
            resp.text().await.map_err(|e| e.to_string())
        }
    };
    match tokio::try_join!(fetch(source.to_string()), fetch(format!("{source}.minisig"))) {
        Ok((json, sig)) => {
            let manifest = Manifest::parse_signed(&json, &sig).map_err(|e| format!("{source}: {e}"))?;
            if let Some(dir) = cache.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            std::fs::write(&cache, &json).ok();
            std::fs::write(format!("{}.minisig", cache.display()), &sig).ok();
            Ok(manifest)
        }
        Err(e) => {
            eprintln!("rigbrewd: cannot fetch the manifest ({e}); using the last verified copy");
            let json = std::fs::read_to_string(&cache).map_err(|_| format!("cannot fetch {source} ({e}) and there is no saved copy"))?;
            let sig = std::fs::read_to_string(format!("{}.minisig", cache.display()))
                .map_err(|_| "the saved manifest has no signature".to_string())?;
            Manifest::parse_signed(&json, &sig).map_err(|e| e.to_string())
        }
    }
}

#[tokio::main]
async fn main() {
    // The signed registry by default; a file or another URL for development.
    let manifest_path =
        std::env::args().nth(1).or_else(|| std::env::var("RIGBREW_MANIFEST").ok()).unwrap_or_else(|| DEFAULT_MANIFEST.to_string());
    let mut manifest = load_manifest(&manifest_path).await.unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if let Ok(base) = std::env::var("RIGBREW_BASE_URL") {
        manifest.base_url = base;
    }
    // Prompts, answers and saved chats live in the data folder: nothing created from here on is
    // readable by other users of this machine.
    #[cfg(unix)]
    unsafe {
        libc::umask(0o077);
    }
    let profile = hardware::scan();
    let root = profile.storage.data_dir.clone();
    std::fs::create_dir_all(&root).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).ok();
        for name in ["events.db", "chats.db"] {
            if root.join(name).exists() {
                std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o600)).ok();
            }
        }
    }
    // From here on a panic leaves a record for the next start (see `rigbrew_core::crash`).
    rigbrew_core::crash::install(root.clone(), "service", env!("CARGO_PKG_VERSION"));
    let events = EventStore::open(&root.join("events.db")).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let chats = ChatStore::open(&root.join("chats.db")).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if events.fresh_install() {
        events.record("app_installed", json!({"version": env!("CARGO_PKG_VERSION"), "tier": profile.tier, "os": profile.os.family, "os_version": profile.os.version, "arch": profile.os.arch})).ok();
    }
    // What crashed since the last start, the desktop shell included.
    for crash in rigbrew_core::crash::take(&root) {
        events.record("crash", crash).ok();
    }
    events.record("daemon_started", json!({"version": env!("CARGO_PKG_VERSION"), "tier": profile.tier, "os": profile.os.family})).ok();

    let token = uuid::Uuid::new_v4().simple().to_string();
    let app: Shared = Arc::new(App {
        profile,
        manifest,
        root: root.clone(),
        token: token.clone(),
        events: Mutex::new(events),
        chats: Mutex::new(chats),
        jobs: Mutex::new(HashMap::new()),
        tasks: Mutex::new(HashMap::new()),
        wanted_lock: Mutex::new(()),
        engine: tokio::sync::Mutex::new(Vec::new()),
        http: reqwest::Client::builder().no_proxy().build().expect("http client"),
        net: reqwest::Client::builder().user_agent(concat!("rigbrewd/", env!("CARGO_PKG_VERSION"))).build().expect("http client"),
        server: std::env::var("RIGBREW_SERVER").unwrap_or_else(|_| "https://api.rigbrew.dev".into()).trim_end_matches('/').to_string(),
        session: Mutex::new(None),
        sync: Mutex::new(cloud::SyncState::default()),
        sync_now: tokio::sync::Notify::new(),
        telemetry_off: cloud::telemetry_off(),
        voice: tokio::sync::Mutex::new(()),
        listen: tokio::sync::Mutex::new(()),
        videos: Mutex::new(HashMap::new()),
        image_progress: Mutex::new(None),
    });
    tokio::spawn(cloud::sync_loop(app.clone()));
    tokio::spawn(idle_loop(app.clone()));

    // Pick up downloads that were interrupted by a quit, a crash or a power cut.
    for (package_id, variant_id) in wanted_read(&app) {
        let found =
            app.manifest.package(&package_id).and_then(|p| p.variants.iter().find(|v| v.id == variant_id).map(|v| (p.clone(), v.clone())));
        match found {
            Some((package, variant)) => {
                start_pull(&app, package, variant);
            }
            None => wanted_update(&app, |list| list.retain(|k| *k != (package_id.clone(), variant_id.clone()))),
        }
    }

    let router = Router::new()
        .route("/v1/system", get(system))
        .route("/v1/packages", get(packages))
        .route("/v1/packages/{id}/pull", post(pull))
        .route("/v1/packages/{id}", axum::routing::delete(package_remove))
        .route("/v1/jobs", get(jobs))
        .route("/v1/jobs/{id}", get(job).delete(job_cancel))
        .route("/v1/engine", get(engine_get).delete(engine_unload))
        .route("/v1/consent", get(consent_get).post(consent_post).delete(consent_delete))
        .route("/v1/events", post(event_post))
        .route("/v1/events/pending", get(events_pending))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/images/generations", post(image))
        .route("/v1/images/progress", get(image_progress))
        .route("/v1/session", axum::routing::put(cloud::session_put).delete(cloud::session_delete))
        .route("/v1/account", get(cloud::account))
        .route("/v1/usage", get(cloud::usage))
        .route("/v1/activity", get(cloud::activity))
        .route("/v1/sync", post(cloud::sync_now))
        .route("/v1/app/config", get(cloud::app_config))
        .route("/v1/chats", get(library::chats_list).delete(library::chats_delete_all))
        .route(
            "/v1/chats/{id}",
            get(library::chat_get)
                .put(library::chat_put)
                .delete(library::chat_delete)
                .layer(axum::extract::DefaultBodyLimit::max(rigbrew_core::chats::MAX_DATA_BYTES + 64 * 1024)),
        )
        .route("/v1/images/files/{name}", get(library::image_get).delete(library::image_delete))
        .route("/v1/activity/recent", get(library::recent))
        .route("/v1/audio/speech", post(speech::speech))
        .route(
            "/v1/audio/transcriptions",
            post(speech::transcribe).layer(axum::extract::DefaultBodyLimit::max(rigbrew_core::listen::MAX_WAV_BYTES + 64 * 1024)),
        )
        .route("/v1/audio/files/{name}", get(speech::audio_get).delete(speech::audio_delete))
        .route("/v1/videos/generations", post(video::generate))
        .route("/v1/videos/jobs", get(video::jobs))
        .route("/v1/videos/jobs/{id}", get(video::job).delete(video::cancel))
        .route("/v1/videos/files/{name}", get(video::file_get).delete(video::file_delete))
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        // The app's webview is a different origin. Only the app's own origins may call the API;
        // the bearer token is still required. Preflight requests carry no token and pass here.
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(
                    ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost", "http://localhost:1420"]
                        .map(|o| o.parse::<axum::http::HeaderValue>().expect("static origin")),
                )
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::PUT, axum::http::Method::DELETE])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
                .expose_headers([
                    axum::http::HeaderName::from_static("x-rigbrew-audio"),
                    axum::http::HeaderName::from_static("x-rigbrew-audio-seconds"),
                    axum::http::HeaderName::from_static("x-rigbrew-seconds"),
                ]),
        )
        .with_state(app.clone());

    let port: u16 = std::env::var("RIGBREW_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    // The app reads this file to find the service. Readable by the owner only.
    let info = root.join("daemon.json");
    std::fs::write(&info, serde_json::to_string_pretty(&json!({"port": addr.port(), "token": token, "pid": std::process::id()})).unwrap())
        .expect("write daemon.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&info, std::fs::Permissions::from_mode(0o600)).ok();
    }
    println!("rigbrewd listening on http://{addr} (connection details in {})", info.display());

    let shutdown_app = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            // Stop on Ctrl-C, on SIGTERM, or when the app that started us is gone, so a crashed
            // app never leaves the service and a loaded model behind.
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminated() => {}
                _ = parent_gone() => {}
            }
            for loaded in std::mem::take(&mut *shutdown_app.engine.lock().await) {
                loaded.server.stop().await;
            }
        })
        .await
        .expect("server");
    std::fs::remove_file(info).ok();
}
