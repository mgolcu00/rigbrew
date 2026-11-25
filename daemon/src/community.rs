//! Community models: a person pastes a link to an open chat model on Hugging Face or GitHub and
//! it becomes a package of its own, next to the ones from the signed manifest.
//!
//!   POST   /v1/community/lookup   {"url"}: what the link holds
//!   POST   /v1/community          {"repo", "file"}: add it and start the download
//!   DELETE /v1/community/{id}     remove it and its files
//!   GET    /v1/community/catalog  the models Rigbrew suggests, from a signed file (`?refresh=1` asks again now)
//!
//! The checks on links and file names, and the list kept in `community.json`, are in
//! `rigbrew_core::community`. Downloads go through the same jobs as every other package.

use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use rigbrew_core::catalog::{self, Settled};
use rigbrew_core::community::{self as models, Found, Host, LookupError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{evict, fail, start_pull, wanted_update, App, Shared};

/// The memory a community model may take: the accelerator's budget, or the processor's on a
/// machine without one.
fn budget_mb(app: &App) -> u64 {
    if app.profile.accel_budget_mb > 0 {
        app.profile.accel_budget_mb
    } else {
        app.profile.cpu_budget_mb
    }
}

fn host_label(host: Host) -> &'static str {
    match host {
        Host::HuggingFace => "huggingface",
        Host::GitHub => "github",
    }
}

fn refuse(app: &App, what: &str, e: LookupError) -> Response {
    let (status, reason) = match &e {
        LookupError::Refused(_) => (StatusCode::BAD_REQUEST, "refused"),
        LookupError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        LookupError::Site(_) => (StatusCode::BAD_GATEWAY, "site"),
    };
    app.record(what, json!({"ok": false, "reason": reason}));
    fail(status, e.to_string())
}

async fn find(app: &App, input: &str) -> Result<Found, LookupError> {
    let target = models::parse(input)?;
    models::lookup(&app.net, &target).await
}

#[derive(Deserialize)]
pub struct LookupBody {
    url: String,
}

pub async fn lookup(State(app): State<Shared>, Json(body): Json<LookupBody>) -> Response {
    let found = match find(&app, &body.url).await {
        Ok(found) => found,
        Err(e) => return refuse(&app, "community_lookup", e),
    };
    let budget = budget_mb(&app);
    let fits = |size: u64| models::needs_mb(size) <= budget;
    // The file the link named, when it fits; otherwise the largest one that does.
    let pick = found
        .wanted
        .as_ref()
        .filter(|w| found.files.iter().any(|f| f.file == **w && fits(f.size)))
        .cloned()
        .or_else(|| models::pick(&found.files, budget).map(|f| f.file.clone()));
    let files: Vec<Value> = found
        .files
        .iter()
        .map(|f| {
            let id = models::package_id(&found.owner, &found.repo, &f.file);
            let installed = app.package(&id).is_some_and(|p| !app.installed(&p).is_empty());
            json!({
                "file": f.file, "size": f.size, "needs_mb": models::needs_mb(f.size), "fits": fits(f.size),
                "checked": f.sha256.is_some(), "id": id, "installed": installed,
            })
        })
        .collect();
    app.record(
        "community_lookup",
        json!({
            "ok": true, "host": host_label(found.host), "files": files.len(), "skipped": found.skipped, "links": found.links.len(),
            "fits": found.files.iter().filter(|f| fits(f.size)).count(), "gated": found.gated,
        }),
    );
    Json(json!({
        "host": host_label(found.host), "site": found.host.label(), "owner": found.owner, "repo": found.repo, "url": found.url,
        "license": found.license, "gated": found.gated, "budget_mb": budget, "files": files, "pick": pick,
        "skipped": found.skipped, "links": found.links,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct AddBody {
    /// The link or id that was looked up.
    #[serde(alias = "url")]
    repo: String,
    /// One of the files the lookup listed.
    file: String,
}

pub async fn add(State(app): State<Shared>, Json(body): Json<AddBody>) -> Response {
    if let Err(e) = models::check_file(&body.file) {
        return refuse(&app, "community_add_refused", e.into());
    }
    // Sizes and checksums come from the site again, never from the request.
    let found = match find(&app, &body.repo).await {
        Ok(found) => found,
        Err(e) => return refuse(&app, "community_add_refused", e),
    };
    let Some(file) = found.files.iter().find(|f| f.file == body.file) else {
        app.record("community_add_refused", json!({"ok": false, "reason": "no_such_file"}));
        return fail(StatusCode::NOT_FOUND, format!("{}/{} has no chat model file named {}.", found.owner, found.repo, body.file));
    };
    if found.gated {
        app.record("community_add_refused", json!({"ok": false, "reason": "gated"}));
        return fail(
            StatusCode::CONFLICT,
            format!(
                "{} asks people to sign in on {} and agree to its terms before downloading. Rigbrew cannot do that.",
                found.owner,
                found.host.label()
            ),
        );
    }
    let (needs, budget) = (models::needs_mb(file.size), budget_mb(&app));
    if needs > budget {
        app.record("community_add_refused", json!({"ok": false, "reason": "too_big", "needs_mb": needs, "budget_mb": budget}));
        return fail(StatusCode::CONFLICT, format!("That file needs about {needs} MB of memory and this computer can give {budget} MB."));
    }
    let entry = models::package(&found, file);
    let (package, id) = (entry.package.clone(), entry.package.id.clone());
    if app.manifest.package(&id).is_some() {
        return fail(StatusCode::CONFLICT, "A Rigbrew model already has that name.");
    }
    // A download of the same file that is already running is the one to follow.
    let running = app.jobs.lock().unwrap().iter().find(|(_, j)| j.package == id && j.state == "running").map(|(job, _)| job.clone());
    if let Some(job) = running {
        return Json(json!({"job": job, "id": id, "name": package.name})).into_response();
    }
    if let Err(e) = app.community.lock().unwrap().put(entry) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("could not save the list of community models: {e}"));
    }
    app.record(
        "community_added",
        json!({"host": host_label(found.host), "size_mb": file.size / (1024 * 1024), "needs_mb": needs, "checked": file.sha256.is_some()}),
    );
    let variant = package.variants[0].clone();
    let job = start_pull(&app, package.clone(), variant);
    Json(json!({"job": job, "id": id, "name": package.name})).into_response()
}

pub async fn remove(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(entry) = app.community.lock().unwrap().get(&id).cloned() else {
        return fail(StatusCode::NOT_FOUND, format!("no community model {id}"));
    };
    // A download of it that is still running stops first.
    let running: Vec<String> =
        app.jobs.lock().unwrap().iter().filter(|(_, j)| j.package == id && j.state == "running").map(|(job, _)| job.clone()).collect();
    for job in &running {
        if let Some(handle) = app.tasks.lock().unwrap().remove(job) {
            handle.abort();
        }
        if let Some(j) = app.jobs.lock().unwrap().get_mut(job) {
            j.state = "cancelled".into();
        }
    }
    wanted_update(&app, |list| list.retain(|(package, _)| *package != id));
    if !running.is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    {
        let mut slots = app.engine.lock().await;
        if let Err(resp) = evict(&app, &mut slots, &id, |l| l.package == id).await {
            return *resp;
        }
    }
    let dir = models::files_dir(&app.root, &entry);
    let mut freed = 0u64;
    for f in entry.package.variants.iter().flat_map(|v| &v.files) {
        let path = dir.join(&f.path);
        for file in [path.clone(), std::path::PathBuf::from(format!("{}.part", path.display()))] {
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&file).is_ok() {
                freed += size;
            }
        }
        // Folders left empty go too, up to the repo's own (another file of the repo may still be there).
        let mut folder = path.parent();
        while let Some(d) = folder.filter(|d| d.starts_with(&dir)) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            folder = d.parent();
        }
    }
    if let Err(e) = app.community.lock().unwrap().remove(&id) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("could not save the list of community models: {e}"));
    }
    app.record("community_removed", json!({"host": host_label(entry.host), "freed_bytes": freed}));
    Json(json!({"removed": id, "freed_bytes": freed})).into_response()
}

/// How long a fetched catalog is shown before the service asks for it again.
const CATALOG_FRESH: Duration = Duration::from_secs(6 * 60 * 60);
/// How soon the service asks again after a fetch that failed.
const CATALOG_RETRY: Duration = Duration::from_secs(10 * 60);

/// The catalog as it was last settled in this run.
#[derive(Default)]
pub struct CatalogState {
    settled: Settled,
    tried: Option<Instant>,
}

/// Reads at most `cap` bytes of text from `url`.
async fn fetch_text(client: &reqwest::Client, url: &str, cap: usize) -> Result<String, String> {
    let resp = client.get(url).timeout(Duration::from_secs(15)).send().await.map_err(|e| format!("{url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url} answered HTTP {}", resp.status().as_u16()));
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("{url}: {e}"))?;
        if body.len() + chunk.len() > cap {
            return Err(format!("{url} is larger than {cap} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| format!("{url} is not text"))
}

/// Gets the catalog from where it is published (`RIGBREW_COMMUNITY_URL` names another place: a
/// URL, or a file for development) and checks its signature. What cannot be fetched or does not
/// check out leaves the kept copy in place.
async fn settle_catalog(app: &App) -> Settled {
    let source =
        std::env::var("RIGBREW_COMMUNITY_URL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| catalog::DEFAULT_URL.into());
    let key = rigbrew_core::manifest::PUBLIC_KEY;
    let settled = if source.starts_with("https://") || source.starts_with("http://") {
        let sig_url = format!("{source}.minisig");
        let fetched =
            tokio::try_join!(fetch_text(&app.net, &source, catalog::MAX_BYTES), fetch_text(&app.net, &sig_url, catalog::MAX_SIG_BYTES));
        catalog::settle(&app.root, fetched, key)
    } else {
        match catalog::load_file(std::path::Path::new(&source), key) {
            Ok(catalog) => Settled { catalog, stale: false, problem: None },
            Err(e) => Settled { stale: true, problem: Some(e.to_string()), ..Settled::default() },
        }
    };
    if let Some(problem) = &settled.problem {
        eprintln!("rigbrewd: community catalog: {problem}");
    }
    settled
}

#[derive(Deserialize)]
pub struct CatalogQuery {
    /// `1` asks the site again now.
    refresh: Option<String>,
}

pub async fn catalog(State(app): State<Shared>, Query(q): Query<CatalogQuery>) -> Json<Value> {
    let force = q.refresh.is_some_and(|r| !matches!(r.as_str(), "" | "0" | "false"));
    let mut state = app.catalog.lock().await;
    let due = match state.tried {
        None => true,
        Some(at) => force || at.elapsed() >= if state.settled.stale { CATALOG_RETRY } else { CATALOG_FRESH },
    };
    if due {
        state.settled = settle_catalog(&app).await;
        state.tried = Some(Instant::now());
    }
    let have = app.community.lock().unwrap();
    let models: Vec<Value> = state
        .settled
        .catalog
        .models
        .iter()
        .map(|entry| {
            let mut row = serde_json::to_value(entry).unwrap_or_default();
            if entry.status == catalog::ADD {
                row["added"] = json!(catalog::added(entry, have.list()));
            }
            row
        })
        .collect();
    Json(json!({"updated": state.settled.catalog.updated, "stale": state.settled.stale, "models": models}))
}
