//! Directing a film from a few plain words, and the cast's looks and portraits.
//!
//!   POST /v1/studio/brief                                  {"text"}: the chat model reads what the person wants and
//!                                                          answers with a brief: {"title", "shape", "size", "seconds",
//!                                                          "frames", "shots", "look", "speech", "language", "cast"}
//!                                                          (nothing is saved)
//!   POST /v1/studio/projects/{id}/cast/look                {"name", "words"}: the chat model writes how a character
//!                                                          looks from a few words (not saved)
//!   POST /v1/studio/projects/{id}/cast/{member}/portrait   the picture model makes a portrait from the member's look
//!   GET  /v1/studio/projects/{id}/cast/{member}/portrait   that portrait

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::{Modality, Package};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{ask, brief, cast, load, need, portrait_path, store, valid_id, view, MAX_STORYBOARD};
use crate::{fail, Shared};

#[derive(Deserialize)]
pub(super) struct Wish {
    text: String,
}

/// The size a video package makes a shape in at a quality (its own sizes when it has no such
/// quality), and the length it has closest to five seconds.
pub fn shape_of(package: &Package, shape: &str, quality: &str) -> ((u32, u32), u32) {
    let usual = match shape {
        "portrait" => (384, 640),
        "square" => (512, 512),
        _ => (640, 384),
    };
    let at = package.qualities().into_iter().find(|q| q.id == quality).and_then(|q| q.sizes.get(shape).copied());
    let own = &package.defaults["sizes"][shape];
    let size = match (at, own[0].as_u64(), own[1].as_u64()) {
        (Some([w, h]), _, _) if super::valid_size(&format!("{w}x{h}")) => (w, h),
        (_, Some(w), Some(h)) if super::valid_size(&format!("{w}x{h}")) => (w as u32, h as u32),
        _ => usual,
    };
    let lengths: Vec<u32> = package.defaults["lengths"]
        .as_array()
        .map(|list| list.iter().filter_map(|l| l["frames"].as_u64().map(|f| f as u32)).filter(|f| (5..=241).contains(f)).collect())
        .unwrap_or_default();
    (size, lengths.into_iter().min_by_key(|f| f.abs_diff(121)).unwrap_or(121))
}

/// The brief of a film as the app is answered with it: what the model chose, in the sizes and
/// lengths the video model makes at `quality`.
pub fn brief_view(b: &brief::Brief, video: &Package, quality: &str) -> Value {
    let ((width, height), frames) = shape_of(video, &b.shape, quality);
    let seconds = brief::offered(b.seconds);
    let shots = ((seconds as f32 / (frames as f32 / 24.0)).round() as usize).clamp(1, MAX_STORYBOARD);
    let cast: Vec<Value> = b.cast.iter().map(|(name, look)| json!({"name": name, "look": look})).collect();
    json!({"title": b.title, "shape": b.shape, "size": format!("{width}x{height}"), "quality": quality, "seconds": seconds, "frames": frames,
           "shots": shots, "look": b.look, "speech": b.speech, "language": b.language, "cast": cast})
}

pub(super) async fn brief(State(app): State<Shared>, Json(body): Json<Wish>) -> Response {
    let text: String = body.text.trim().chars().take(4000).collect();
    if text.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "say what the film is about in \"text\"");
    }
    // A film is directed to be made: without the video model, and the chat model that writes, nothing starts.
    let video = match need(&app, Modality::Video) {
        Ok(package) => package,
        Err(refusal) => return *refusal,
    };
    if let Err(refusal) = need(&app, Modality::Chat) {
        return *refusal;
    }
    // A model that cannot be reached is an error; one that answers badly still gives the usual film.
    let answer = match ask(&app, &brief::system_prompt(), &text, 500, 0.4).await {
        Ok(answer) => answer,
        Err((status, why)) => return fail(status, why),
    };
    let read = brief::read(&answer, &text);
    app.record(
        "studio_brief_written",
        json!({"shape": read.shape, "seconds": read.seconds, "cast": read.cast.len(), "speech": read.speech, "look": !read.look.is_empty()}),
    );
    // A film directed from a few words is made at the quality this machine makes well.
    let quality = super::default_quality(&app);
    Json(brief_view(&read, video, &quality)).into_response()
}

#[derive(Deserialize)]
pub(super) struct LookWish {
    #[serde(default)]
    name: String,
    #[serde(default)]
    words: String,
}

pub(super) async fn look(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<LookWish>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let name: String = body.name.trim().chars().take(40).collect();
    let words: String = body.words.trim().chars().take(400).collect();
    if name.is_empty() && words.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "Give the character a name or a few words first.");
    }
    if let Err(refusal) = need(&app, Modality::Chat) {
        return *refusal;
    }
    let answer = match ask(&app, &cast::look_prompt(), &cast::look_request(&name, &words, &p.look), 160, 0.6).await {
        Ok(answer) => answer,
        Err((status, why)) => return fail(status, why),
    };
    match cast::one_line(&answer) {
        Some(look) => {
            app.record("studio_look_written", json!({"chars": look.chars().count()}));
            Json(json!({"look": look})).into_response()
        }
        None => fail(StatusCode::BAD_GATEWAY, "The chat model did not write a look. Try again."),
    }
}

pub(super) async fn make_portrait(State(app): State<Shared>, Path((id, member)): Path<(String, String)>) -> Response {
    use base64::Engine as _;
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let Some(who) = p.cast.iter().find(|m| m.id == member).cloned() else {
        return fail(StatusCode::NOT_FOUND, "no such character");
    };
    let package = match need(&app, Modality::Image) {
        Ok(package) => package,
        Err(refusal) => return *refusal,
    };
    // One model fits in memory at a time: a portrait waits for the shot that is being made.
    if let Some(busy) = crate::video::refuse_busy(&app, &package.id) {
        return busy;
    }
    let (server, variant) = match crate::ensure_sd(&app, package).await {
        Ok(found) => found,
        Err(refusal) => return *refusal,
    };
    let (width, height) = cast::portrait_size(&p.size);
    let request = rigbrew_core::engine::ImageRequest { prompt: cast::portrait_prompt(&who, &p.look), width, height, steps: 8, seed: -1 };
    let made = crate::make_image(&app, &server, &request).await;
    crate::touch(&app, &package.id).await;
    let (b64, seconds) = match made {
        Ok(made) => made,
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) else {
        return fail(StatusCode::BAD_GATEWAY, "the picture model answered with something that is no picture");
    };
    // The film was open while the picture was made: take it as it is now.
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let Some(at) = p.cast.iter().position(|m| m.id == member) else {
        return fail(StatusCode::NOT_FOUND, "no such character");
    };
    let before = portrait_path(&p, &p.cast[at]);
    p.cast[at].portrait += 1;
    let file = portrait_path(&p, &p.cast[at]);
    let written = std::fs::create_dir_all(file.parent().unwrap_or(&file)).and_then(|()| std::fs::write(&file, &bytes));
    if let Err(e) = written {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the portrait could not be saved in the film's folder: {e}"));
    }
    std::fs::remove_file(before).ok();
    // A shot that starts from this portrait started from the one before: it is made again.
    for s in p.shots.iter_mut().filter(|s| s.from == member && !s.continues && s.state == "done") {
        s.state = "changed".into();
    }
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    app.record(
        "studio_portrait_made",
        json!({"package": package.id, "variant": variant, "seconds": seconds, "width": width, "height": height}),
    );
    Json(view(&app, p)).into_response()
}

pub(super) async fn portrait(State(app): State<Shared>, Path((id, member)): Path<(String, String)>) -> Response {
    let found = load(&app, &id).and_then(|p| {
        let who = p.cast.iter().find(|m| valid_id(&member) && m.id == member && m.portrait > 0)?;
        Some(portrait_path(&p, who))
    });
    let Some(file) = found else {
        return fail(StatusCode::NOT_FOUND, "that character has no portrait");
    };
    match tokio::fs::read(file).await {
        Ok(bytes) => {
            // The picture model answers with a PNG or a JPEG: the first bytes say which.
            let kind = if bytes.starts_with(&[0xff, 0xd8]) { "image/jpeg" } else { "image/png" };
            Response::builder()
                .header(header::CONTENT_TYPE, kind)
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(_) => fail(StatusCode::NOT_FOUND, "that character has no portrait"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(defaults: Value) -> Package {
        serde_json::from_value(json!({
            "id": "pour-test", "name": "Pour", "modality": "video", "version": 1, "engine": "sdcpp", "access": "public",
            "source": {"name": "s", "url": "u", "license": "l", "changes": "c"}, "quality_floor": 0.5, "variants": [], "defaults": defaults,
        }))
        .unwrap()
    }

    #[test]
    fn a_brief_is_answered_in_what_the_video_model_makes() {
        let package = video(json!({"sizes": {"landscape": [640, 384], "portrait": [384, 640], "square": [512, 512]},
                                   "lengths": [{"frames": 49, "label": "2 s"}, {"frames": 121, "label": "5 s"}, {"frames": 241, "label": "10 s"}]}));
        let read = brief::read(
            "TITLE: Night Market\nSHAPE: tall\nLENGTH: 50\nNARRATION: yes\nCAST:\n- Lena: a woman in her thirties with a silver bob",
            "x",
        );
        let v = brief_view(&read, &package, "");
        assert_eq!(
            (v["title"].as_str(), v["shape"].as_str(), v["size"].as_str()),
            (Some("Night Market"), Some("portrait"), Some("384x640"))
        );
        // 50 seconds is offered as a minute: twelve shots of five seconds.
        assert_eq!((v["seconds"].as_u64(), v["frames"].as_u64(), v["shots"].as_u64()), (Some(60), Some(121), Some(12)));
        assert_eq!(v["cast"], json!([{"name": "Lena", "look": "a woman in her thirties with a silver bob"}]));
        assert_eq!((v["speech"].as_bool(), v["language"].as_str()), (Some(true), Some("English")));
        // A package that says nothing about itself gets the usual shape and five seconds.
        assert_eq!(shape_of(&video(Value::Null), "square", ""), ((512, 512), 121));
        assert_eq!(
            shape_of(&video(json!({"sizes": {"landscape": [100, 7]}, "lengths": [{"frames": 33}]})), "landscape", ""),
            ((640, 384), 33)
        );
        // At a quality the package has, the shape is made at that quality's size.
        let qualities = video(json!({"sizes": {"portrait": [384, 640]}, "qualities": [
            {"id": "draft", "label": "Draft", "sizes": {"portrait": [384, 640]}},
            {"id": "standard", "label": "Standard", "sizes": {"portrait": [512, 896], "landscape": [896, 512]}}]}));
        assert_eq!(shape_of(&qualities, "portrait", "standard").0, (512, 896));
        assert_eq!(shape_of(&qualities, "portrait", "draft").0, (384, 640));
        assert_eq!(shape_of(&qualities, "portrait", "high").0, (384, 640));
        let v = brief_view(&read, &qualities, "standard");
        assert_eq!((v["size"].as_str(), v["quality"].as_str()), (Some("512x896"), Some("standard")));
    }
}
