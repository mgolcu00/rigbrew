//! Everything that touches the server: the signed-in session, sending queued events, the app
//! config (updates, sponsors), and the local usage summary.
//!
//! The server is optional. Every function here degrades to "later" when it cannot be reached.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{fail, App, Shared};

/// Who is using the app, as told by the app after Firebase sign-in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub uid: String,
    /// Firebase ID token; sent to the server as a bearer token. Never logged, never stored on disk.
    #[serde(skip_serializing)]
    pub id_token: String,
    #[serde(default)]
    pub expires_at_ms: i64,
    #[serde(default)]
    pub anonymous: bool,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncState {
    pub last_sent_ms: Option<i64>,
    pub last_error: Option<String>,
    #[serde(skip)]
    failures: u32,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub async fn session_put(State(app): State<Shared>, Json(session): Json<Session>) -> Response {
    if session.uid.is_empty() || session.id_token.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "uid and id_token are required");
    }
    let changed = {
        let mut slot = app.session.lock().unwrap();
        let changed = slot.as_ref().map(|s| (s.uid.clone(), s.anonymous)) != Some((session.uid.clone(), session.anonymous));
        *slot = Some(session.clone());
        changed
    };
    app.events.lock().unwrap().set_user(Some(session.uid.clone()));
    // The app pushes its session after every daemon restart; only a different account is a sign-in.
    let marker = app.root.join("state").join("last-account");
    let label = format!("{}:{}", session.uid, session.anonymous);
    let known = std::fs::read_to_string(&marker).ok().as_deref() == Some(label.as_str());
    if changed && !known {
        std::fs::create_dir_all(app.root.join("state")).ok();
        std::fs::write(&marker, &label).ok();
        app.record("signed_in", json!({"anonymous": session.anonymous}));
        app.sync_now.notify_one();
    }
    Json(json!({"account": session})).into_response()
}

pub async fn session_delete(State(app): State<Shared>) -> Json<Value> {
    let had = app.session.lock().unwrap().take().is_some();
    std::fs::remove_file(app.root.join("state").join("last-account")).ok();
    if had {
        app.record("signed_out", json!({}));
    }
    app.events.lock().unwrap().set_user(None);
    Json(json!({"account": null}))
}

pub async fn account(State(app): State<Shared>) -> Json<Value> {
    Json(json!({"account": app.session.lock().unwrap().clone()}))
}

fn bearer(app: &App) -> Option<String> {
    let s = app.session.lock().unwrap();
    s.as_ref().filter(|s| s.expires_at_ms == 0 || s.expires_at_ms > now_ms() + 30_000).map(|s| s.id_token.clone())
}

/// Sends one batch. Ok(n) = events acknowledged by the server.
/// Developers can switch off every upload with `DO_NOT_TRACK=1` (or `RIGBREW_NO_TELEMETRY=1`).
pub fn telemetry_off() -> bool {
    ["DO_NOT_TRACK", "RIGBREW_NO_TELEMETRY"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// Sends one batch. Ok(n) = events acknowledged by the server.
///
/// With consent and an account the events go as recorded (content included) to `/v1/events`.
/// Otherwise they go to `/v1/telemetry` as basic usage data: content stripped, no account id.
async fn send_batch(app: &App) -> Result<usize, String> {
    if app.telemetry_off {
        return Ok(0);
    }
    let (consent, events, install_id) = {
        let store = app.events.lock().unwrap();
        (store.consent().map_err(|e| e.to_string())?, store.pending(200).map_err(|e| e.to_string())?, store.install_id().to_string())
    };
    if events.is_empty() {
        return Ok(0);
    }
    let token = consent.as_ref().and_then(|_| bearer(app));
    let full = token.is_some();
    let events: Vec<Value> = events
        .into_iter()
        .map(|e| {
            let mut v = json!(e);
            if !full {
                v["payload"] = rigbrew_core::events::strip_content(&e.payload);
                v["content"] = json!(false);
                v["user_id"] = Value::Null;
            }
            v
        })
        .collect();
    let body = json!({
        "install_id": install_id,
        "app_version": env!("CARGO_PKG_VERSION"),
        "platform": format!("{}-{}", app.profile.os.family, app.profile.os.arch),
        "tier": app.profile.tier,
        "consent": consent,
        "events": events,
    });
    let path = if full { "/v1/events" } else { "/v1/telemetry" };
    let mut req = app.net.post(format!("{}{path}", app.server)).json(&body).timeout(Duration::from_secs(30));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp =
        req.send().await.map_err(|e| if e.is_connect() || e.is_timeout() { "server not reachable".to_string() } else { e.to_string() })?;
    if !resp.status().is_success() {
        return Err(format!("server answered HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let accepted: Vec<String> =
        v["accepted"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    app.events.lock().unwrap().mark_sent(&accepted).map_err(|e| e.to_string())?;
    Ok(accepted.len())
}

/// Sends until the queue is empty or something fails. Returns how many events left the machine.
pub async fn drain(app: &App) -> Result<usize, String> {
    let mut total = 0;
    loop {
        match send_batch(app).await {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) => {
                let mut st = app.sync.lock().unwrap();
                st.failures += 1;
                st.last_error = Some(e.clone());
                return Err(e);
            }
        }
    }
    let mut st = app.sync.lock().unwrap();
    st.failures = 0;
    st.last_error = None;
    if total > 0 {
        st.last_sent_ms = Some(now_ms());
    }
    Ok(total)
}

/// Background loop: on start, every 60 s, and whenever asked; backs off from 5 s to 10 min on failure.
pub async fn sync_loop(app: Shared) {
    loop {
        let failures = app.sync.lock().unwrap().failures;
        let wait = if failures == 0 { Duration::from_secs(60) } else { Duration::from_secs((5u64 << failures.min(7)).min(600)) };
        let _ = drain(&app).await;
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = app.sync_now.notified() => {}
        }
    }
}

pub async fn sync_now(State(app): State<Shared>) -> Json<Value> {
    let result = drain(&app).await;
    let pending = app.events.lock().unwrap().pending_count().unwrap_or(0);
    match result {
        Ok(sent) => Json(json!({"sent": sent, "pending": pending})),
        Err(e) => Json(json!({"sent": 0, "pending": pending, "error": e})),
    }
}

pub async fn usage(State(app): State<Shared>) -> Json<Value> {
    let now = now_ms();
    let day = 86_400_000;
    let (today, week, all, pending) = {
        let store = app.events.lock().unwrap();
        (
            store.usage_since(now - now.rem_euclid(day)).unwrap_or_default(),
            store.usage_since(now - 7 * day).unwrap_or_default(),
            store.usage_since(0).unwrap_or_default(),
            store.pending_count().unwrap_or(0),
        )
    };
    let dir = app.packages_dir();
    let mut packages = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for p in &app.manifest.packages {
        for v in app.installed(p) {
            // A file shared by two variants is on disk once: count it once in the total.
            let bytes: u64 = v.files.iter().map(|f| std::fs::metadata(dir.join(&f.path)).map(|m| m.len()).unwrap_or(0)).sum();
            total += v
                .files
                .iter()
                .filter(|f| seen.insert(f.path.clone()))
                .map(|f| std::fs::metadata(dir.join(&f.path)).map(|m| m.len()).unwrap_or(0))
                .sum::<u64>();
            packages.push(json!({"id": p.id, "name": p.name, "variant": v.id, "bytes": bytes}));
        }
    }
    let root = app.root.clone();
    let free_mb = tokio::task::spawn_blocking(move || rigbrew_core::hardware::scan_with_data_dir(&root).storage.free_mb).await.unwrap_or(0);
    let sync = app.sync.lock().unwrap().clone();
    Json(json!({
        "today": today, "last_7_days": week, "all_time": all,
        "disk": {"packages": packages, "total_bytes": total, "free_bytes": free_mb * 1024 * 1024},
        "sync": {"pending": pending, "last_sent_ms": sync.last_sent_ms, "last_error": sync.last_error},
    }))
}

/// What the profile page draws: a day-by-day activity grid, the most used packages and totals.
pub async fn activity(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let days = q.get("days").and_then(|d| d.parse::<i64>().ok()).unwrap_or(365).clamp(1, 730);
    let since = now_ms() - days * 86_400_000;
    let store = app.events.lock().unwrap();
    let daily: Vec<Value> = store
        .daily_activity(since)
        .unwrap_or_default()
        .into_iter()
        .map(|(day, prompts, images, tokens_out, events)| json!({"day": day, "prompts": prompts, "images": images, "tokens_out": tokens_out, "events": events}))
        .collect();
    let names: std::collections::HashMap<&str, &str> = app.manifest.packages.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    let packages: Vec<Value> = store
        .package_use(since)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, prompts)| json!({"name": names.get(id.as_str()).copied().unwrap_or(id.as_str()), "id": id, "prompts": prompts}))
        .collect();
    Json(json!({
        "today": now_ms() / 86_400_000,
        "days": daily,
        "packages": packages,
        "first_event_ms": store.first_event_ms().unwrap_or(None),
        "install_id": store.install_id(),
        "all_time": store.usage_since(0).unwrap_or_default(),
    }))
}

fn config_cache(app: &App) -> std::path::PathBuf {
    app.root.join("state").join("app-config.json")
}

/// The server's app config; the last good answer is kept on disk and served (marked stale) offline.
pub async fn app_config(State(app): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    // The app's own version decides about updates; fall back to this service's version.
    let version = q
        .get("version")
        .filter(|v| !v.is_empty() && v.len() < 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)))
        .cloned()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let channel = q.get("channel").filter(|c| c.chars().all(|c| c.is_ascii_alphanumeric())).cloned().unwrap_or_else(|| "stable".into());
    let url = format!(
        "{}/v1/app/config?version={version}&platform={}-{}&channel={channel}",
        app.server, app.profile.os.family, app.profile.os.arch
    );
    let fresh = match app.net.get(&url).timeout(Duration::from_secs(8)).send().await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().filter(Value::is_object),
        _ => None,
    };
    let path = config_cache(&app);
    if let Some(mut cfg) = fresh {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&path, cfg.to_string()).ok();
        cfg["stale"] = json!(false);
        return Json(cfg);
    }
    let mut cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"update": "none"}));
    // Never block the app on an old cached "hard" verdict the server can no longer confirm.
    if cfg["update"] == "hard" {
        cfg["update"] = json!("soft");
    }
    cfg["stale"] = json!(true);
    Json(cfg)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use rigbrew_core::chats::ChatStore;
    use rigbrew_core::events::EventStore;
    use rigbrew_core::{hardware, Manifest};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::App;

    /// A stand-in for the server that accepts every event and counts the batches it receives.
    async fn fake_server() -> (String, Arc<Mutex<Vec<Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = vec![0u8; 65536];
                    // Read the head, then the body announced by Content-Length.
                    loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(head_end) = text.find("\r\n\r\n") {
                            let len = text[..head_end]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if buf.len() >= head_end + 4 + len {
                                let mut body: Value = serde_json::from_slice(&buf[head_end + 4..head_end + 4 + len]).unwrap_or(Value::Null);
                                let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
                                body["_path"] = json!(path);
                                let ids: Vec<Value> =
                                    body["events"].as_array().cloned().unwrap_or_default().iter().map(|e| e["id"].clone()).collect();
                                log.lock().unwrap().push(body);
                                let reply = json!({"accepted": ids}).to_string();
                                let head = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    reply.len()
                                );
                                sock.write_all(head.as_bytes()).await.ok();
                                sock.write_all(reply.as_bytes()).await.ok();
                                return;
                            }
                        }
                    }
                });
            }
        });
        (url, seen)
    }

    fn app(server: String) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rigbrewd-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = App {
            profile: hardware::scan_with_data_dir(&dir),
            manifest: Manifest::parse(r#"{"schema":1,"generated":"2026-11-20","base_url":"http://127.0.0.1:9/","packages":[]}"#).unwrap(),
            root: dir.clone(),
            token: "t".into(),
            events: Mutex::new(EventStore::open(&dir.join("events.db")).unwrap()),
            chats: Mutex::new(ChatStore::in_memory().unwrap()),
            jobs: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            wanted_lock: Mutex::new(()),
            engine: tokio::sync::Mutex::new(None),
            http: reqwest::Client::new(),
            net: reqwest::Client::new(),
            server,
            session: Mutex::new(None),
            sync: Mutex::new(SyncState::default()),
            sync_now: tokio::sync::Notify::new(),
            telemetry_off: false,
            voice: tokio::sync::Mutex::new(()),
            videos: std::sync::Mutex::new(std::collections::HashMap::new()),
            image_progress: std::sync::Mutex::new(None),
        };
        (app, dir)
    }

    fn sign_in(app: &App) {
        *app.session.lock().unwrap() =
            Some(Session { uid: "u1".into(), id_token: "tok".into(), expires_at_ms: 0, anonymous: true, email: None, name: None });
    }

    fn paths(seen: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        seen.lock().unwrap().iter().map(|b| b["_path"].as_str().unwrap_or("").to_string()).collect()
    }

    #[tokio::test]
    async fn content_leaves_only_with_consent_and_an_account_basic_usage_always() {
        let (url, seen) = fake_server().await;
        let (app, dir) = app(url);
        let all_text = |seen: &Arc<Mutex<Vec<Value>>>| serde_json::to_string(&*seen.lock().unwrap()).unwrap();

        // No consent: basic usage goes to /v1/telemetry, without the words.
        app.record("app_opened", json!({"screen": "welcome"}));
        app.record("prompt", json!({"modality": "chat", "messages": [{"role": "user", "content": "secret one"}]}));
        sign_in(&app);
        assert_eq!(drain(&app).await.unwrap(), 2);
        assert_eq!(paths(&seen), ["/v1/telemetry"]);
        assert!(!all_text(&seen).contains("secret one"));
        assert_eq!(seen.lock().unwrap()[0]["events"][1]["payload"]["user_chars"], 10);
        assert!(seen.lock().unwrap()[0]["events"][1]["user_id"].is_null());

        // Consent but no account (a build without the account module): still basic only.
        *app.session.lock().unwrap() = None;
        app.events.lock().unwrap().accept_consent("v1").unwrap();
        app.record("prompt", json!({"modality": "chat", "messages": [{"role": "user", "content": "secret two"}]}));
        assert_eq!(drain(&app).await.unwrap(), 1);
        assert_eq!(paths(&seen)[1], "/v1/telemetry");
        assert!(!all_text(&seen).contains("secret two"));

        // Consent and an account: content goes to /v1/events with the consent attached.
        sign_in(&app);
        app.record("prompt", json!({"modality": "chat", "messages": [{"role": "user", "content": "shared three"}]}));
        assert_eq!(drain(&app).await.unwrap(), 1);
        assert_eq!(paths(&seen)[2], "/v1/events");
        let third = seen.lock().unwrap()[2].clone();
        assert_eq!(third["consent"]["version"], "v1");
        assert_eq!(third["events"][0]["payload"]["messages"][0]["content"], "shared three");

        // Withdrawn: basic usage continues, the words stop.
        app.events.lock().unwrap().withdraw_consent().unwrap();
        app.record("prompt", json!({"modality": "chat", "messages": [{"role": "user", "content": "secret four"}]}));
        assert_eq!(drain(&app).await.unwrap(), 1);
        assert_eq!(paths(&seen)[3], "/v1/telemetry");
        assert!(!all_text(&seen).contains("secret four"));
        assert_eq!(app.events.lock().unwrap().pending_count().unwrap(), 0);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn do_not_track_stops_every_upload() {
        let (url, seen) = fake_server().await;
        let (app, dir) = app(url);
        let (mut app, dir) = (app, dir);
        app.telemetry_off = true;
        app.record("app_opened", json!({}));
        assert_eq!(drain(&app).await.unwrap(), 0);
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(app.events.lock().unwrap().pending_count().unwrap(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn an_unreachable_server_keeps_events_queued_and_reports_why() {
        let (app, dir) = app("http://127.0.0.1:9".into());
        app.events.lock().unwrap().accept_consent("v1").unwrap();
        sign_in(&app);
        app.record("prompt", json!({}));
        assert!(drain(&app).await.is_err());
        assert_eq!(app.events.lock().unwrap().pending_count().unwrap(), 1);
        assert!(app.sync.lock().unwrap().last_error.is_some());
        std::fs::remove_dir_all(dir).ok();
    }
}
