//! Reading text aloud with an installed voice package (the Whistle family).
//!
//!   POST   /v1/audio/speech               OpenAI-compatible: {"model", "input", "voice"?, "speed"?}; answers
//!                                         with the WAV, and saves it as /v1/audio/files/<id>.wav
//!   GET    /v1/audio/files/{id}.wav       a saved reading; DELETE removes it
//!
//! The voice engine runs on the processor next to whatever model is loaded, one reading at a time.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::{Engine as EngineKind, Modality};
use rigbrew_core::voice::{self, Speech, VoiceModel};
use serde::Deserialize;
use serde_json::json;

use crate::{engine_program, fail, App, Shared};

#[derive(Deserialize)]
pub struct SpeechBody {
    model: String,
    input: String,
    /// Speaker number, or its name as a number in a string.
    #[serde(default)]
    voice: Option<serde_json::Value>,
    #[serde(default)]
    speed: Option<f32>,
}

fn audio_file(app: &App, name: &str) -> Option<std::path::PathBuf> {
    let id = name.strip_suffix(".wav")?;
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).then(|| app.root.join("audio").join(name))
}

pub async fn speech(State(app): State<Shared>, Json(body): Json<SpeechBody>) -> Response {
    let Some(package) = app.manifest.package(&body.model).filter(|p| p.modality == Modality::Voice) else {
        return fail(StatusCode::NOT_FOUND, format!("{} is not a voice package", body.model));
    };
    let Some(variant) = app.installed(package).into_iter().next() else {
        return fail(StatusCode::CONFLICT, format!("{} is not installed", package.name));
    };
    let Some(bundle) = variant.files.iter().find(|f| rigbrew_core::bundle::is_bundle(f)) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{} has no voice bundle", package.name));
    };
    let text = voice::plain(&body.input);
    if text.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "there is nothing to read in the input");
    }
    let speaker = match &body.voice {
        None | Some(serde_json::Value::Null) => 0,
        Some(v) => match v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())) {
            Some(n) if n < 1000 => n as u32,
            _ => return fail(StatusCode::BAD_REQUEST, "voice must be a speaker number"),
        },
    };
    let binary = match engine_program(&app, EngineKind::Sherpa).await {
        Ok(p) => p,
        Err(e) => return fail(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
    let model = VoiceModel { binary, dir: rigbrew_core::bundle::dir(&app.packages_dir(), bundle), threads };
    let speech = Speech { text: text.clone(), speaker, speed: body.speed.unwrap_or(1.0) };

    let dir = app.root.join("audio");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("{id}.wav");
    let tmp = dir.join(format!(".{name}.tmp.wav"));
    app.record("prompt", json!({"package": package.id, "variant": variant.id, "modality": "voice", "input": text}));

    // One reading at a time: each run uses several processor cores.
    let _turn = app.voice.lock().await;
    match voice::speak(&model, &speech, &tmp).await {
        Ok(stats) => {
            if let Err(e) = std::fs::rename(&tmp, dir.join(&name)) {
                return fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
            }
            app.record(
                "speech_generated",
                json!({"package": package.id, "variant": variant.id, "chars": text.chars().count(), "audio_seconds": stats.audio_seconds,
                       "seconds": stats.seconds, "threads": threads, "speaker": speaker}),
            );
            match tokio::fs::read(dir.join(&name)).await {
                Ok(bytes) => Response::builder()
                    .header(header::CONTENT_TYPE, "audio/wav")
                    .header("x-rigbrew-audio", format!("/v1/audio/files/{name}"))
                    .header("x-rigbrew-audio-seconds", format!("{:.2}", stats.audio_seconds))
                    .header("x-rigbrew-seconds", format!("{:.2}", stats.seconds))
                    .body(Body::from(bytes))
                    .unwrap(),
                Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        Err(e) => {
            std::fs::remove_file(&tmp).ok();
            app.record("speech_failed", json!({"package": package.id, "variant": variant.id, "error": e.to_string()}));
            fail(StatusCode::BAD_GATEWAY, e.to_string())
        }
    }
}

pub async fn audio_get(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some(path) = audio_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad audio name");
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "audio/wav")
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such audio"),
    }
}

pub async fn audio_delete(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let Some(path) = audio_file(&app, &name) else {
        return fail(StatusCode::BAD_REQUEST, "bad audio name");
    };
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Json(json!({"deleted": name})).into_response(),
        Err(_) => fail(StatusCode::NOT_FOUND, "no such audio"),
    }
}
