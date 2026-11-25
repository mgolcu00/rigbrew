//! Names for conversations: a chat, a Code task, a film.
//!
//! A conversation starts with its first words as its name. Once there is something to go by, the
//! chat model is asked for a better one: a few words, in the language of the conversation. That
//! is never worth waiting for and never worth moving a model for, so the model is only asked
//! when it is in memory already, or when nothing at all is in memory and nothing is being made.
//! In every other case, and whenever the answer is no use, the name stays the first words.
//!
//!   POST /v1/title   {"text", "answer"?, "model"?, "load"?} -> {"title", "by": "model" | "words"}
//!
//! `text` is what the person wrote and `answer` how the reply began. `load: false` asks only a
//! model that is in memory. Nothing of this is recorded as an event.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::Modality;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{fail, Shared};

/// The longest name, in characters.
pub const MAX_CHARS: usize = 48;
/// How much of what the person wrote, and of the answer, the model is shown.
const SHOWN_TEXT: usize = 1500;
const SHOWN_ANSWER: usize = 600;

const SYSTEM: &str = "You name conversations. Read the start of the conversation and answer with a title for it and nothing else: \
3 to 6 words, in the language the person writes in, saying what it is about. No quotes, no punctuation at the end, \
no names of AI models or products, no word like \"Title\" in front.";

/// A name out of the first words of what the person wrote.
pub fn first_words(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let words = line.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
    let title = capped(&words);
    if title.is_empty() {
        "Untitled".into()
    } else {
        title
    }
}

/// At most `MAX_CHARS` characters, cut between words when there is a place to, and nothing
/// hanging at the end.
fn capped(text: &str) -> String {
    let text = text.trim();
    let mut out: String = text.chars().take(MAX_CHARS).collect();
    if out.len() < text.len() && !text[out.len()..].starts_with(char::is_whitespace) {
        // The cut fell inside a word: go back to the end of the word before it.
        if let Some(at) = out.rfind(char::is_whitespace).filter(|at| out[..*at].chars().count() >= MAX_CHARS / 3) {
            out.truncate(at);
        }
    }
    out.trim_end_matches(|c: char| c.is_whitespace() || ".,;:!?…。、，！？-–—".contains(c)).to_string()
}

/// The title in what the model wrote, or nothing when there is none to be had from it.
pub fn clean(raw: &str) -> Option<String> {
    // Thinking is not part of the answer, closed or not.
    let said = match raw.rsplit_once("</think>") {
        Some((_, after)) => after,
        None if raw.trim_start().starts_with("<think>") => "",
        None => raw,
    };
    let wraps = |c: char| c.is_whitespace() || "\"'`*_#“”‘’«»「」『』".contains(c);
    let mut line = said.lines().map(|l| l.trim_matches(wraps)).find(|l| !l.is_empty())?;
    // "Title: ..." in a few languages is a label, not part of the name.
    if let Some((label, rest)) = line.split_once([':', '：']) {
        let label = label.trim_matches(wraps).to_lowercase();
        if ["title", "titel", "titre", "título", "titulo", "titolo", "başlık", "baslik", "name"].contains(&label.as_str()) {
            line = rest.trim_matches(wraps);
        }
    }
    let title = capped(&line.split_whitespace().collect::<Vec<_>>().join(" "));
    let title = title.trim_matches(wraps);
    // A name has letters in it; an empty answer, a dash or a shrug is not one.
    (title.chars().filter(|c| c.is_alphanumeric()).count() >= 2).then(|| title.to_string())
}

/// What the model is asked.
fn request(text: &str, answer: &str) -> Value {
    let text: String = text.trim().chars().take(SHOWN_TEXT).collect();
    let answer: String = answer.trim().chars().take(SHOWN_ANSWER).collect();
    let mut shown = format!("The person wrote:\n{text}");
    if !answer.is_empty() {
        shown.push_str(&format!("\n\nThe answer began:\n{answer}"));
    }
    shown.push_str("\n\nThe title:");
    json!({
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": shown}],
        "max_tokens": 32,
        "temperature": 0.2,
        "stream": false,
        "chat_template_kwargs": {"enable_thinking": false},
    })
}

/// Where a title can come from right now.
#[derive(Debug, PartialEq)]
enum From {
    /// A chat model is in memory: ask it.
    Loaded,
    /// Nothing is in memory and nothing is being made: a chat model may be brought in.
    Load,
    /// Something else is in memory or at work. It is not moved for a title.
    Nowhere,
}

fn source(chat_loaded: bool, anything_loaded: bool, busy: bool, may_load: bool) -> From {
    if chat_loaded {
        From::Loaded
    } else if anything_loaded || busy || !may_load {
        From::Nowhere
    } else {
        From::Load
    }
}

/// The chat engine to ask: its port and key. `None` when no title should be asked for now.
async fn engine(app: &Shared, model: Option<&str>, may_load: bool) -> Option<(u16, String)> {
    {
        let slots = app.engine.lock().await;
        let chat = slots.iter().rev().find_map(|l| match &l.server {
            crate::Engine::Llama(server) => Some((server.port(), server.key().to_string())),
            crate::Engine::Sd(_) => None,
        });
        // A clip, a reading or a recording in progress is work that a model being loaded would slow.
        let busy = app.videos.lock().unwrap().values().any(|j| j.state == "loading" || j.state == "running")
            || app.voice.try_lock().is_err()
            || app.listen.try_lock().is_err();
        match source(chat.is_some(), !slots.is_empty(), busy, may_load) {
            From::Loaded => return chat,
            From::Nowhere => return None,
            From::Load => {}
        }
    }
    let installed = |p: &rigbrew_core::manifest::Package| p.modality == Modality::Chat && !app.installed(p).is_empty();
    let package = match model.and_then(|id| app.package(id)).filter(|p| installed(p)) {
        Some(p) => p,
        None => app.all_packages().into_iter().find(|p| installed(p))?,
    };
    let (port, key, _) = crate::ensure_engine(app, &package).await.ok()?;
    Some((port, key))
}

/// A name for a conversation that began with `text` and was answered with `answer` (which may be
/// empty), and who wrote it: "model", or "words" when the model was not asked or was no use.
pub async fn write(app: &Shared, text: &str, answer: &str, model: Option<&str>, may_load: bool) -> (String, &'static str) {
    let fallback = || (first_words(text), "words");
    if text.trim().is_empty() {
        return fallback();
    }
    let Some((port, key)) = engine(app, model, may_load).await else { return fallback() };
    let sent = app
        .http
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(key)
        .json(&request(text, answer))
        .timeout(Duration::from_secs(60))
        .send()
        .await;
    let Ok(resp) = sent else { return fallback() };
    if !resp.status().is_success() {
        return fallback();
    }
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    match v["choices"][0]["message"]["content"].as_str().and_then(clean) {
        Some(title) => (title, "model"),
        None => fallback(),
    }
}

#[derive(Deserialize)]
pub struct TitleBody {
    text: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    load: Option<bool>,
}

pub async fn post(State(app): State<Shared>, Json(body): Json<TitleBody>) -> Response {
    if body.text.trim().is_empty() {
        return fail(StatusCode::BAD_REQUEST, "\"text\" is required");
    }
    let (title, by) = write(&app, &body.text, &body.answer, body.model.as_deref(), body.load.unwrap_or(true)).await;
    Json(json!({"title": title, "by": by})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_words_are_a_name() {
        assert_eq!(first_words("  fix the login test\nit fails on CI"), "fix the login test");
        assert_eq!(first_words("Why does the sky look blue at noon and red at dusk?"), "Why does the sky look blue");
        assert_eq!(first_words("Merhaba, bugün hava nasıl olacak acaba söyler misin"), "Merhaba, bugün hava nasıl olacak acaba");
        assert_eq!(first_words(""), "Untitled");
        assert_eq!(first_words("\n \n"), "Untitled");
        let long = first_words("supercalifragilisticexpialidocious-and-then-some-more-letters-to-be-sure again");
        assert_eq!(long.chars().count(), MAX_CHARS);
    }

    #[test]
    fn what_the_model_wrote_is_cleaned_into_a_title() {
        assert_eq!(clean("Fixing the login test").as_deref(), Some("Fixing the login test"));
        assert_eq!(clean("\"Fixing the login test.\"\n").as_deref(), Some("Fixing the login test"));
        assert_eq!(clean("**Title:** “Gökyüzü neden mavi?”").as_deref(), Some("Gökyüzü neden mavi"));
        assert_eq!(clean("# Paris in  three   days\n\nHere is why I chose it.").as_deref(), Some("Paris in three days"));
        assert_eq!(clean("<think>short, in Turkish</think>\n\nBaşlık: Hafta sonu planı!").as_deref(), Some("Hafta sonu planı"));
        assert_eq!(clean("Ratio 16:9 for the film").as_deref(), Some("Ratio 16:9 for the film"), "a colon inside a name stays");
        assert_eq!(clean("日本語の勉強方法。").as_deref(), Some("日本語の勉強方法"));
    }

    #[test]
    fn an_answer_that_is_no_title_is_refused() {
        for raw in ["", "   \n", "\"\"", "...", "-", "<think>still thinking about it", "Title:"] {
            assert_eq!(clean(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_long_answer_is_cut_between_words() {
        let title = clean("A very long sentence about how the tests of the project were repaired over a weekend").unwrap();
        assert!(title.chars().count() <= MAX_CHARS, "{title}");
        assert_eq!(title, "A very long sentence about how the tests of the");
        let one_word = clean(&"x".repeat(90)).unwrap();
        assert_eq!(one_word.chars().count(), MAX_CHARS);
    }

    #[test]
    fn a_model_is_not_moved_for_a_title() {
        // The chat model is in memory: asked, whatever else is there.
        assert_eq!(source(true, true, false, true), From::Loaded);
        assert_eq!(source(true, true, true, false), From::Loaded);
        // Only a picture or video model is in memory: it stays, the first words stay.
        assert_eq!(source(false, true, false, true), From::Nowhere);
        // Nothing is in memory, but a clip is loading or a voice is reading.
        assert_eq!(source(false, false, true, true), From::Nowhere);
        // Nothing is in memory and nothing is at work.
        assert_eq!(source(false, false, false, true), From::Load);
        assert_eq!(source(false, false, false, false), From::Nowhere);
    }

    #[test]
    fn the_model_is_asked_briefly_and_without_thinking() {
        let body = request(&"word ".repeat(2000), "");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert!(body["max_tokens"].as_u64().unwrap() <= 48);
        assert!(body["temperature"].as_f64().unwrap() <= 0.3);
        let shown = body["messages"][1]["content"].as_str().unwrap();
        assert!(shown.chars().count() < SHOWN_TEXT + 80, "what is shown is capped");
        assert!(!shown.contains("The answer began"));
        assert!(request("hi", "Hello there").to_string().contains("The answer began"));
    }
}
