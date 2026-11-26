//! The film as the app plays it: its exported file, and addresses the webview's own `<video>`
//! plays a file of the film from, with ranges (see `media.rs`).
//!
//!   POST   /v1/studio/projects/{id}/media     `{}` for the exported film, `{"shot", "take"?}` for a clip
//!                                             (its playable copy): `{"url", "type", "bytes"}`
//!
//! The film's answer carries `film`: the exported file, how large, when it was written, and
//! whether it holds the film as it is now (`current`: no edit came after what it was made from).

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{clip_path, export_of, export_path, load, take_files, valid_id, Project};
use crate::{fail, Shared};

/// The exported file of a film: the one this run wrote last, else the one the film's options name.
fn film_file(p: &Project) -> Option<PathBuf> {
    let last = export_of(&p.id)["file"].as_str().map(PathBuf::from).filter(|f| f.is_file());
    last.or_else(|| Some(export_path(p, &p.export)).filter(|f| f.is_file()))
}

/// `{file, bytes, modified_ms, current}` for the film's exported file; null before the first export.
pub(super) fn film(p: &Project) -> Value {
    let Some(file) = film_file(p) else { return Value::Null };
    let Ok(meta) = std::fs::metadata(&file) else { return Value::Null };
    let written = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
    // What an export of this run was made from; for a file from before, when it was written.
    let last = export_of(&p.id);
    let from = last["edited_ms"].as_i64().filter(|_| last["file"].as_str() == file.to_str()).unwrap_or(written);
    json!({"file": file.to_string_lossy(), "bytes": meta.len(), "modified_ms": written, "current": from >= p.updated_ms})
}

#[derive(Deserialize, Default)]
pub(super) struct Media {
    #[serde(default)]
    shot: Option<String>,
    #[serde(default)]
    take: Option<u32>,
}

pub(super) async fn media(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Media>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let body = body.map(|b| b.0).unwrap_or_default();
    let (path, kind) = match body.shot.as_deref() {
        None => match film_file(&p) {
            Some(file) => (file, "video/mp4"),
            None => return fail(StatusCode::NOT_FOUND, "This film has no exported file yet. Export it first."),
        },
        Some(shot) => {
            if !valid_id(shot) || !p.shots.iter().any(|s| s.id == shot) {
                return fail(StatusCode::NOT_FOUND, "no such shot");
            }
            let clip = match body.take {
                Some(n) => take_files(&p, shot, n).0,
                None => clip_path(&p, shot),
            };
            if !clip.is_file() {
                return fail(StatusCode::NOT_FOUND, "that shot has no clip yet");
            }
            crate::playable::served(&app, &clip).await
        }
    };
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Json(json!({"url": crate::media::grant(path, kind), "type": kind, "bytes": bytes})).into_response()
}
