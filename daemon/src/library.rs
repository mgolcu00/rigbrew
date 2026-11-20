//! What the person keeps on this machine: saved chats, generated pictures, and a readable
//! list of recent activity. All local; nothing here is sent anywhere.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{fail, App, Shared};

pub async fn chats_list(State(app): State<Shared>) -> Response {
    match app.chats.lock().unwrap().list() {
        Ok(chats) => Json(json!({"chats": chats})).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn chat_get(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.chats.lock().unwrap().get(&id) {
        Ok(Some(chat)) => Json(json!(chat)).into_response(),
        Ok(None) => fail(StatusCode::NOT_FOUND, "no chat with that id"),
        Err(rigbrew_core::chats::ChatError::BadId) => fail(StatusCode::BAD_REQUEST, "bad chat id"),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct ChatBody {
    #[serde(default)]
    title: String,
    #[serde(default)]
    data: Value,
}

pub async fn chat_put(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<ChatBody>) -> Response {
    use rigbrew_core::chats::ChatError;
    match app.chats.lock().unwrap().put(&id, &body.title, &body.data) {
        Ok(meta) => Json(json!(meta)).into_response(),
        Err(ChatError::BadId) => fail(StatusCode::BAD_REQUEST, ChatError::BadId.to_string()),
        Err(e @ ChatError::TooLarge(_)) => fail(StatusCode::PAYLOAD_TOO_LARGE, e.to_string()),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn chat_delete(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.chats.lock().unwrap().delete(&id) {
        Ok(true) => Json(json!({"deleted": id})).into_response(),
        Ok(false) => fail(StatusCode::NOT_FOUND, "no chat with that id"),
        Err(rigbrew_core::chats::ChatError::BadId) => fail(StatusCode::BAD_REQUEST, "bad chat id"),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn chats_delete_all(State(app): State<Shared>) -> Response {
    match app.chats.lock().unwrap().delete_all() {
        Ok(n) => Json(json!({"deleted": n})).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Pictures are stored as `<32 hex>.png`; nothing else can be read or deleted through this API.
fn image_file(app: &App, name: &str) -> Option<std::path::PathBuf> {
    let id = name.strip_suffix(".png")?;
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).then(|| app.root.join("images").join(name))
}

/// Saves a generated picture and returns its id, path and size.
pub fn save_image(app: &App, b64: &str) -> Option<Value> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = app.root.join("images");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{id}.png");
    // Write next to the target and rename, so a reader never sees half a file.
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, &bytes).ok()?;
    std::fs::rename(&tmp, dir.join(&name)).ok()?;
    Some(json!({"id": id, "path": format!("/v1/images/files/{name}"), "bytes": bytes.len()}))
}

pub async fn image_get(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some(path) = image_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad picture name");
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such picture"),
    }
}

pub async fn image_delete(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some(path) = image_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad picture name");
    };
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Json(json!({"deleted": name})).into_response(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such picture"),
    }
}

const RECENT_KINDS: &[&str] =
    &["prompt", "image_generated", "image_failed", "package_pull_finished", "package_pull_failed", "package_removed", "engine_started"];

fn clip(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        format!("{}…", one_line.chars().take(max).collect::<String>().trim_end())
    }
}

/// Text of the last user message in an OpenAI-style message list.
fn last_user_text(messages: &Value) -> String {
    let Some(list) = messages.as_array() else { return String::new() };
    for m in list.iter().rev() {
        if m["role"] == "user" {
            return match &m["content"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "),
                _ => String::new(),
            };
        }
    }
    String::new()
}

#[derive(Deserialize, Default)]
pub struct RecentQuery {
    limit: Option<usize>,
}

/// Recent activity in words, newest first: what the profile page lists.
pub async fn recent(State(app): State<Shared>, Query(q): Query<RecentQuery>) -> Json<Value> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let events = app.events.lock().unwrap().recent(RECENT_KINDS, limit).unwrap_or_default();
    let name = |id: &Value| -> String {
        let id = id.as_str().unwrap_or("");
        app.manifest.package(id).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string())
    };
    let items: Vec<Value> = events
        .iter()
        .map(|e| {
            let p = &e.payload;
            let (title, detail) = match e.kind.as_str() {
                "prompt" if p["modality"] == "image" => ("Asked for a picture".to_string(), clip(p["prompt"].as_str().unwrap_or(""), 140)),
                "prompt" => (format!("Asked {}", name(&p["package"])), clip(&last_user_text(&p["messages"]), 140)),
                "image_generated" => (
                    format!("Picture made by {}", name(&p["package"])),
                    format!(
                        "{} × {} · {:.0} s",
                        p["width"].as_u64().unwrap_or(0),
                        p["height"].as_u64().unwrap_or(0),
                        p["seconds"].as_f64().unwrap_or(0.0)
                    ),
                ),
                "image_failed" => ("A picture failed".to_string(), clip(p["error"].as_str().unwrap_or(""), 140)),
                "package_pull_finished" => (format!("Installed {}", name(&p["package"])), p["variant"].as_str().unwrap_or("").to_string()),
                "package_pull_failed" => {
                    (format!("Install of {} failed", name(&p["package"])), clip(p["error"].as_str().unwrap_or(""), 140))
                }
                "package_removed" => {
                    (format!("Removed {}", name(&p["package"])), format!("{:.1} GB freed", p["freed_bytes"].as_f64().unwrap_or(0.0) / 1e9))
                }
                "engine_started" => (
                    format!("Loaded {} into memory", name(&p["package"])),
                    format!("in {:.1} s", p["load_seconds"].as_f64().unwrap_or(0.0)),
                ),
                _ => (e.kind.clone(), String::new()),
            };
            json!({"ts_ms": e.ts_ms, "kind": e.kind, "title": title, "detail": detail, "package": p["package"]})
        })
        .collect();
    Json(json!({"items": items}))
}
