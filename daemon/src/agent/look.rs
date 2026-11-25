//! `look_at_page`: a page of the app the task is building, as a browser shows it: its title,
//! the text on it, the errors its scripts wrote, the requests that failed, and a picture.
//!
//! Only an address on this computer is ever opened (`web::local_url`).
//!
//! The service has no browser of its own; the app has one. So a look is a job the app's window
//! takes: it waits for one at `GET /v1/agent/looks/next`, opens the address in a hidden window
//! of its own (`app/src-tauri/src/look.rs`), and hands back what it saw with `POST
//! /v1/agent/looks/{id}`. When no window is waiting (the service runs alone, or the app is too
//! old), the page's HTML is fetched and read instead, and the result says "not rendered":
//! scripts did not run, and there is no picture.
//!
//! Pictures (of a page, or of a phone's screen, see `devices`) are kept as PNG files next to
//! the task, `<data dir>/agent/shots/<task>/<name>.png`, and go with the task when it is removed.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::{files, web};

/// How long one wait for a job lasts before the window asks again.
const POLL: Duration = Duration::from_secs(20);
/// How long a window has to answer a job it took.
const ANSWER: Duration = Duration::from_secs(40);
/// How long a job waits to be taken before the page is read without a browser.
const TAKEN: Duration = Duration::from_secs(4);
const FETCH: Duration = Duration::from_secs(10);
const MAX_HTML_BYTES: usize = 1024 * 1024;
const TEXT_CHARS: usize = 2500;
const MAX_LINES: usize = 10;
const LINE_CHARS: usize = 240;
pub const MAX_PICTURE_BYTES: usize = 8 * 1024 * 1024;
/// Pictures kept for one task; the oldest go first.
const MAX_SHOTS: usize = 30;
/// Clicks and typing one look may ask for.
const MAX_ACTIONS: usize = 10;

/// What a look found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub url: String,
    pub title: String,
    pub text: String,
    /// What the page's scripts wrote as errors, and errors nothing caught.
    pub errors: Vec<String>,
    /// Requests that did not work: "GET /api/items 404".
    pub failed: Vec<String>,
    /// False when only the HTML was read.
    pub rendered: bool,
    pub picture: Option<Vec<u8>>,
    /// Something the model and the person should know about this look: why there is no picture,
    /// a click that found nothing.
    pub note: String,
}

/// The last look of a task, as it is kept with it and shown under the preview.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Page {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
    #[serde(default)]
    pub rendered: bool,
    /// The picture's name under the task's shots.
    #[serde(default)]
    pub shot: Option<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub at_ms: i64,
}

fn short(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let line: String = line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(LINE_CHARS).collect();
        if !line.is_empty() && !out.contains(&line) && out.len() < MAX_LINES {
            out.push(line);
        }
    }
    out
}

impl Seen {
    pub fn page(&self, shot: Option<String>) -> Page {
        Page {
            url: self.url.clone(),
            title: self.title.clone(),
            errors: self.errors.clone(),
            failed: self.failed.clone(),
            rendered: self.rendered,
            shot,
            note: self.note.clone(),
            at_ms: super::now_ms(),
        }
    }

    /// What the model reads. `sees` says whether the picture goes to it as well.
    pub fn text(&self, sees: bool) -> String {
        let list = |name: &str, lines: &[String]| {
            if lines.is_empty() {
                format!("{name}: none")
            } else {
                format!("{name} ({}):\n{}", lines.len(), lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n"))
            }
        };
        let mut out = vec![format!("Looked at {}{}", self.url, if self.rendered { "" } else { " (not rendered)" })];
        if !self.title.is_empty() {
            out.push(format!("Title: {}", self.title));
        }
        if self.rendered {
            out.push(list("Console errors", &self.errors));
        }
        out.push(list("Failed requests", &self.failed));
        out.push(if self.text.trim().is_empty() {
            "Text on the page: none. The page is empty.".to_string()
        } else {
            format!("Text on the page:\n{}", files::cap(self.text.trim(), TEXT_CHARS))
        });
        if !self.note.is_empty() {
            out.push(format!("[{}]", self.note));
        }
        out.push(match (self.rendered, self.picture.is_some(), sees) {
            (false, _, _) => {
                "[Not rendered: the Rigbrew window is not open, so only the HTML was read. Scripts did not run and there is no picture.]"
                    .to_string()
            }
            (true, true, true) => {
                "[A screenshot of the page comes with this result. Look at it: is anything missing, broken or out of place?]".into()
            }
            (true, true, false) => {
                "[A screenshot was taken and shown to the person. You cannot see pictures: go by the text and the errors.]".into()
            }
            (true, false, _) => "[No screenshot could be taken here: go by the text and the errors.]".into(),
        });
        out.join("\n")
    }
}

/// One click or one piece of typing on the page, checked and in the form the window takes.
pub fn actions(args: &Value) -> Result<Vec<Value>, String> {
    let Some(list) = args.get("do").filter(|d| !d.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or("\"do\" is a list, for example [{\"click\": \"Add to cart\"}].")?;
    if list.len() > MAX_ACTIONS {
        return Err(format!("\"do\" takes at most {MAX_ACTIONS} steps."));
    }
    let word = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 300).map(str::to_string);
    list.iter()
        .map(|step| {
            if let Some(target) = step.get("click").and_then(word) {
                Ok(json!({"click": target}))
            } else if let Some(target) = step.get("type").and_then(word) {
                let text = step["text"].as_str().unwrap_or("");
                Ok(json!({"type": target, "text": text.chars().take(500).collect::<String>()}))
            } else {
                Err("Each step of \"do\" is {\"click\": \"text or CSS selector\"} or {\"type\": \"CSS selector or label\", \"text\": \"what to type\"}.".to_string())
            }
        })
        .collect()
}

/// The size of the window a look opens: "phone", "tablet" or "desktop".
pub fn size(name: Option<&str>) -> Result<(u32, u32), String> {
    match name.map(str::trim).unwrap_or("") {
        "" | "desktop" => Ok((1280, 800)),
        "tablet" => Ok((820, 1180)),
        "phone" => Ok((390, 844)),
        other => Err(format!("\"size\" is \"phone\", \"tablet\" or \"desktop\", not \"{other}\".")),
    }
}

struct Job {
    id: String,
    url: String,
    width: u32,
    height: u32,
    actions: Vec<Value>,
}

/// The looks that wait for a window, and the ones a window is working on.
#[derive(Default)]
pub struct Looks {
    queue: Mutex<VecDeque<Job>>,
    waiting: Mutex<HashMap<String, oneshot::Sender<Seen>>>,
    wake: tokio::sync::Notify,
    /// Windows that are waiting for a job right now.
    polling: AtomicUsize,
    last_poll: Mutex<Option<Instant>>,
}

impl Looks {
    /// Whether a window of the app is there to take a job.
    fn window(&self) -> bool {
        self.polling.load(Ordering::SeqCst) > 0 || self.last_poll.lock().unwrap().is_some_and(|at| at.elapsed() < Duration::from_secs(5))
    }

    /// Waits for a job, at most `wait`. `None` when none came.
    async fn next(&self, wait: Duration) -> Option<Value> {
        struct Polling<'a>(&'a Looks);
        impl Drop for Polling<'_> {
            fn drop(&mut self) {
                self.0.polling.fetch_sub(1, Ordering::SeqCst);
                *self.0.last_poll.lock().unwrap() = Some(Instant::now());
            }
        }
        self.polling.fetch_add(1, Ordering::SeqCst);
        let _polling = Polling(self);
        let until = Instant::now() + wait;
        loop {
            let woken = self.wake.notified();
            if let Some(job) = self.queue.lock().unwrap().pop_front() {
                // A job nobody waits for any more is skipped.
                if self.waiting.lock().unwrap().contains_key(&job.id) {
                    return Some(json!({"id": job.id, "url": job.url, "width": job.width, "height": job.height, "actions": job.actions}));
                }
                continue;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() || tokio::time::timeout(left, woken).await.is_err() {
                return None;
            }
        }
    }

    /// Hands a job to a window and waits for what it saw. `None` when no window took it or
    /// answered in time.
    async fn ask(&self, url: &str, (width, height): (u32, u32), actions: Vec<Value>) -> Option<Seen> {
        if !self.window() {
            return None;
        }
        struct Asked<'a>(&'a Looks, String);
        impl Drop for Asked<'_> {
            fn drop(&mut self) {
                self.0.waiting.lock().unwrap().remove(&self.1);
                self.0.queue.lock().unwrap().retain(|j| j.id != self.1);
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tx, mut rx) = oneshot::channel();
        self.waiting.lock().unwrap().insert(id.clone(), tx);
        let _asked = Asked(self, id.clone());
        self.queue.lock().unwrap().push_back(Job { id: id.clone(), url: url.to_string(), width, height, actions });
        self.wake.notify_waiters();
        // Not taken soon: the window went away between its questions.
        if let Ok(seen) = tokio::time::timeout(TAKEN, &mut rx).await {
            return seen.ok();
        }
        if self.queue.lock().unwrap().iter().any(|j| j.id == id) {
            return None;
        }
        tokio::time::timeout(ANSWER, rx).await.ok().and_then(Result::ok)
    }

    /// Takes a window's answer to a job. False when nobody waits for it.
    fn answer(&self, id: &str, seen: Seen) -> bool {
        match self.waiting.lock().unwrap().remove(id) {
            Some(tx) => tx.send(seen).is_ok(),
            None => false,
        }
    }
}

fn title_of(html: &str) -> String {
    static TITLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let title = TITLE.get_or_init(|| regex::Regex::new(r"(?is)<title\b[^>]*>(.*?)</title\s*>").expect("static pattern"));
    title.captures(html).map(|c| web::decode_entities(c[1].trim())).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The page without a browser: its HTML is fetched from this computer and read as text.
pub async fn fetch(url: &reqwest::Url) -> Seen {
    use futures_util::StreamExt;
    let mut seen = Seen { url: url.to_string(), ..Default::default() };
    // A redirect is followed only while it stays on this computer.
    let stay = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > 5 || web::local_url(attempt.url().as_str()).is_err() {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut builder = reqwest::Client::builder().no_proxy().redirect(stay).timeout(FETCH);
    // A name under "localhost" is this computer, whatever a name server says.
    if let (Some(url::Host::Domain(name)), Some(port)) = (url.host(), url.port_or_known_default()) {
        let here = [std::net::SocketAddr::from(([127, 0, 0, 1], port)), std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port))];
        builder = builder.resolve_to_addrs(name, &here);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => {
            seen.failed.push(format!("GET {url}: {e}"));
            return seen;
        }
    };
    let answer = match client.get(url.clone()).header("Accept", "text/html,*/*;q=0.5").send().await {
        Ok(a) => a,
        Err(e) => {
            let why = if e.is_timeout() {
                "it took too long to answer"
            } else if e.is_connect() {
                "nothing answers there (is the app running? start_app starts it)"
            } else {
                "it could not be reached"
            };
            seen.failed.push(format!("GET {url}: {why}"));
            return seen;
        }
    };
    let status = answer.status();
    if !status.is_success() {
        seen.failed.push(format!("GET {url} {}", status.as_u16()));
    }
    let mut body = Vec::new();
    let mut stream = answer.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        let room = MAX_HTML_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= MAX_HTML_BYTES {
            break;
        }
    }
    let html = String::from_utf8_lossy(&body);
    seen.title = title_of(&html);
    seen.text = if html.contains('<') { web::readable(&html) } else { html.trim().to_string() };
    seen
}

/// Looks at one page: through the app's window when one is there, without a browser otherwise.
pub async fn look(looks: &Looks, url: &reqwest::Url, size: (u32, u32), actions: Vec<Value>) -> Seen {
    let wanted = !actions.is_empty();
    if let Some(mut seen) = looks.ask(url.as_str(), size, actions).await {
        seen.rendered = true;
        seen.errors = short(std::mem::take(&mut seen.errors));
        seen.failed = short(std::mem::take(&mut seen.failed));
        seen.title = seen.title.chars().take(200).collect();
        seen.note = seen.note.chars().take(400).collect();
        seen.picture = seen.picture.filter(|p| p.starts_with(&[0x89, b'P', b'N', b'G']) && p.len() <= MAX_PICTURE_BYTES);
        if seen.url.is_empty() {
            seen.url = url.to_string();
        }
        return seen;
    }
    let mut seen = fetch(url).await;
    if wanted {
        seen.note = "Nothing was clicked or typed: that needs the Rigbrew window.".into();
    }
    seen
}

// --- pictures kept with a task ------------------------------------------------------------------

fn shots_dir(agent_dir: &Path, task: &str) -> PathBuf {
    agent_dir.join("shots").join(task)
}

/// Keeps a picture with the task and gives its name.
pub fn keep_shot(agent_dir: &Path, task: &str, png: &[u8]) -> Option<String> {
    let dir = shots_dir(agent_dir, task);
    std::fs::create_dir_all(&dir).ok()?;
    let name = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
    std::fs::write(dir.join(format!("{name}.png")), png).ok()?;
    // The oldest make room.
    let mut all: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    all.sort();
    for (_, path) in all.iter().take(all.len().saturating_sub(MAX_SHOTS)) {
        std::fs::remove_file(path).ok();
    }
    Some(name)
}

pub fn valid_shot(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub fn read_shot(agent_dir: &Path, task: &str, name: &str) -> Option<Vec<u8>> {
    if !valid_shot(name) || !super::valid_id(task) {
        return None;
    }
    std::fs::read(shots_dir(agent_dir, task).join(format!("{name}.png"))).ok()
}

/// Removes the pictures of a task that is forgotten.
pub fn forget(agent_dir: &Path, task: &str) {
    if super::valid_id(task) {
        std::fs::remove_dir_all(shots_dir(agent_dir, task)).ok();
    }
}

// --- the picture the model is looking at ---------------------------------------------------------

/// The newest picture of a task: it goes to the model with the result of the step that took it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Eye {
    /// The step that took it, and the id the model gave that call.
    pub step: String,
    #[serde(default)]
    pub call: String,
    pub shot: String,
    /// "the page http://localhost:5173/", "the screen of iPhone 16 Pro".
    pub what: String,
    /// How often it was sent. A picture is heavy: after a few turns it is left out.
    #[serde(default)]
    pub sends: u8,
}

/// Turns the model gets to see one picture for.
pub const EYE_SENDS: u8 = 3;

/// The conversation with the picture put in after the result it belongs to, in the form the
/// engine takes pictures: a message whose content is a text part and an `image_url` part with
/// the PNG as a data address. Nothing changes when that result is no longer in the conversation.
pub fn with_picture(mut messages: Vec<Value>, eye: &Eye, png: &[u8]) -> Vec<Value> {
    use base64::Engine as _;
    let Some(at) = messages.iter().position(|m| m["role"] == "tool" && m["tool_call_id"] == eye.call.as_str()) else { return messages };
    // Results of calls made together stay together.
    let after = (at + 1..messages.len()).find(|i| messages[*i]["role"] != "tool").unwrap_or(messages.len());
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    messages.insert(
        after,
        json!({"role": "user", "content": [
            {"type": "text", "text": format!("{}This is the screenshot of {}.", super::recall::FROM_SERVICE, eye.what)},
            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{data}")}},
        ]}),
    );
    messages
}

/// Whether an engine error says that it could not take the picture.
pub fn picture_refused(error: &str) -> bool {
    let e = error.to_lowercase();
    ["image", "mmproj", "multimodal", "vision"].iter().any(|w| e.contains(w))
}

// --- the API --------------------------------------------------------------------------------------

pub mod api {
    use axum::extract::{Path, State};
    use axum::http::{header, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::Json;

    use super::*;
    use crate::{fail, Shared};

    /// `GET /v1/agent/looks/next`: the app's window waits here for a page to open.
    pub async fn next(State(app): State<Shared>) -> Json<Value> {
        Json(json!({"look": app.agent.looks.next(POLL).await}))
    }

    #[derive(Deserialize)]
    pub struct Answer {
        #[serde(default)]
        url: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        errors: Vec<String>,
        #[serde(default)]
        failed: Vec<String>,
        /// The PNG, base64.
        #[serde(default)]
        picture: Option<String>,
        #[serde(default)]
        note: String,
    }

    /// `POST /v1/agent/looks/{id}`: what the window saw.
    pub async fn answer(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Answer>) -> Response {
        use base64::Engine as _;
        let picture = body.picture.and_then(|p| base64::engine::general_purpose::STANDARD.decode(p.as_bytes()).ok());
        let seen = Seen {
            url: body.url,
            title: body.title,
            text: body.text,
            errors: body.errors,
            failed: body.failed,
            rendered: true,
            picture,
            note: body.note,
        };
        if app.agent.looks.answer(&id, seen) {
            Json(json!({"ok": true})).into_response()
        } else {
            fail(StatusCode::NOT_FOUND, "Nobody waits for that look any more.")
        }
    }

    #[derive(Deserialize)]
    pub struct Now {
        /// The address; the task's running app when left out.
        #[serde(default)]
        url: Option<String>,
        /// Keep the result with this task (its preview shows it).
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        size: Option<String>,
    }

    /// `POST /v1/agent/look`: the person looks at a page the way the model does (the preview's reload).
    pub async fn now(State(app): State<Shared>, Json(body): Json<Now>) -> Response {
        use base64::Engine as _;
        let task = body.task.as_deref().filter(|t| !t.is_empty());
        if task.is_some_and(|t| app.agent.get(t).is_none()) {
            return fail(StatusCode::NOT_FOUND, "no such task");
        }
        let address = body
            .url
            .filter(|u| !u.trim().is_empty())
            .or_else(|| task.and_then(|t| app.agent.apps.port(t)).map(|port| format!("http://localhost:{port}/")));
        let Some(address) = address else {
            return fail(StatusCode::BAD_REQUEST, "There is no page to look at: no app is running in this task.");
        };
        let url = match web::local_url(&address) {
            Ok(u) => u,
            Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        };
        let size = match size(body.size.as_deref()) {
            Ok(s) => s,
            Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        };
        let seen = look(&app.agent.looks, &url, size, Vec::new()).await;
        let Some(task) = task else {
            let mut page = serde_json::to_value(seen.page(None)).unwrap_or_default();
            page["text"] = json!(files::cap(seen.text.trim(), TEXT_CHARS));
            page["picture"] = json!(seen.picture.as_ref().map(|p| base64::engine::general_purpose::STANDARD.encode(p)));
            return Json(json!({"page": page})).into_response();
        };
        let shot = seen.picture.as_ref().and_then(|png| keep_shot(&app.agent.dir, task, png));
        let page = seen.page(shot);
        app.agent.change(task, |t| t.extra.page = Some(page.clone()));
        Json(json!({"page": page})).into_response()
    }

    /// `GET /v1/agent/tasks/{id}/shots/{name}`: one picture of a task.
    pub async fn shot(State(app): State<Shared>, Path((id, name)): Path<(String, String)>) -> Response {
        let name = name.strip_suffix(".png").unwrap_or(&name);
        match read_shot(&app.agent.dir, &id, name) {
            Some(png) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, max-age=86400")], png).into_response(),
            None => fail(StatusCode::NOT_FOUND, "no such picture"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn what_was_seen_is_said_in_a_few_lines() {
        let mut seen = Seen {
            url: "http://localhost:5173/".into(),
            title: "Shop".into(),
            text: "Shop\n\nCart (0)".into(),
            errors: vec!["TypeError: cart.items is undefined (http://localhost:5173/src/cart.js:12:9)".into()],
            failed: vec!["GET http://localhost:5173/api/items 404".into()],
            rendered: true,
            picture: Some(vec![0x89, b'P', b'N', b'G']),
            note: String::new(),
        };
        assert_eq!(
            seen.text(true),
            "Looked at http://localhost:5173/\nTitle: Shop\nConsole errors (1):\n- TypeError: cart.items is undefined (http://localhost:5173/src/cart.js:12:9)\n\
             Failed requests (1):\n- GET http://localhost:5173/api/items 404\nText on the page:\nShop\n\nCart (0)\n\
             [A screenshot of the page comes with this result. Look at it: is anything missing, broken or out of place?]"
        );
        assert!(seen
            .text(false)
            .ends_with("[A screenshot was taken and shown to the person. You cannot see pictures: go by the text and the errors.]"));
        seen.picture = None;
        seen.errors.clear();
        seen.failed.clear();
        seen.text = "  ".into();
        seen.note = "Nothing on the page says \"Buy\", so nothing was clicked.".into();
        assert_eq!(
            seen.text(true),
            "Looked at http://localhost:5173/\nTitle: Shop\nConsole errors: none\nFailed requests: none\nText on the page: none. The page is empty.\n\
             [Nothing on the page says \"Buy\", so nothing was clicked.]\n[No screenshot could be taken here: go by the text and the errors.]"
        );
        seen.rendered = false;
        let plain = seen.text(true);
        assert!(plain.starts_with("Looked at http://localhost:5173/ (not rendered)\n") && !plain.contains("Console errors"), "{plain}");
        assert!(plain.ends_with("Scripts did not run and there is no picture.]"));
        // Long, repeated and empty lines do not reach the model.
        let noisy: Vec<String> = (0..30).map(|i| format!("error {}  {}", i % 15, "x".repeat(400))).chain(["  ".to_string()]).collect();
        let kept = short(noisy);
        assert_eq!(kept.len(), MAX_LINES);
        assert!(kept.iter().all(|l| l.chars().count() == LINE_CHARS));
        let page = seen.page(Some("0123456789abcdef".into()));
        assert_eq!((page.url.as_str(), page.shot.as_deref(), page.rendered), ("http://localhost:5173/", Some("0123456789abcdef"), false));
    }

    #[test]
    fn clicks_and_sizes_are_checked() {
        assert_eq!(actions(&json!({})).unwrap(), Vec::<Value>::new());
        assert_eq!(
            actions(&json!({"do": [{"click": " Add to cart "}, {"type": "#name", "text": "Ada"}]})).unwrap(),
            [json!({"click": "Add to cart"}), json!({"type": "#name", "text": "Ada"})]
        );
        assert!(actions(&json!({"do": "click Add"})).is_err());
        assert!(actions(&json!({"do": [{"press": "Enter"}]})).is_err());
        assert!(actions(&json!({"do": [{"click": ""}]})).is_err());
        assert!(actions(&json!({"do": vec![json!({"click": "x"}); MAX_ACTIONS + 1]})).is_err());
        assert_eq!(size(None).unwrap(), (1280, 800));
        assert_eq!(size(Some("phone")).unwrap(), (390, 844));
        assert_eq!(size(Some("tablet")).unwrap(), (820, 1180));
        assert!(size(Some("watch")).is_err());
    }

    /// A small page server on this computer, for the looks below.
    async fn serve(pages: &'static [(&'static str, u16, &'static str)]) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, body) = pages.iter().find(|(p, _, _)| *p == path).map_or((404, "nothing here"), |(_, s, b)| (*s, *b));
                let head = if status == 302 {
                    format!("HTTP/1.1 302 Found\r\nLocation: {body}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                socket.write_all(head.as_bytes()).await.ok();
            }
        });
        port
    }

    #[tokio::test]
    async fn without_a_window_the_html_is_read_and_marked_as_not_rendered() {
        let port = serve(&[
            ("/", 200, "<html><head><title>My  Shop &amp; more</title><script>boom()</script></head><body><h1>Hello</h1><p>Cart (0)</p></body></html>"),
            ("/away", 302, "https://example.com/"),
            ("/home", 302, "/"),
        ])
        .await;
        let looks = Looks::default();
        let url = web::local_url(&format!("http://127.0.0.1:{port}/")).unwrap();
        let seen = look(&looks, &url, (1280, 800), vec![json!({"click": "Hello"})]).await;
        assert!(!seen.rendered && seen.picture.is_none());
        assert_eq!(seen.title, "My Shop & more");
        assert_eq!(seen.text, "Hello\n\nCart (0)");
        assert!(seen.failed.is_empty() && seen.errors.is_empty());
        assert_eq!(seen.note, "Nothing was clicked or typed: that needs the Rigbrew window.");
        let missing = fetch(&web::local_url(&format!("http://127.0.0.1:{port}/nope")).unwrap()).await;
        assert_eq!(missing.failed, [format!("GET http://127.0.0.1:{port}/nope 404")]);
        assert_eq!(missing.text, "nothing here");
        // A redirect on this computer is followed; one that leaves it is not.
        let home = fetch(&web::local_url(&format!("http://127.0.0.1:{port}/home")).unwrap()).await;
        assert_eq!(home.title, "My Shop & more");
        let away = fetch(&web::local_url(&format!("http://127.0.0.1:{port}/away")).unwrap()).await;
        assert!(away.text.is_empty() && away.failed[0].ends_with(" 302"), "{away:?}");
        // Nothing listens there.
        let free = super::super::apps::free_port().unwrap();
        let nothing = fetch(&web::local_url(&format!("http://127.0.0.1:{free}/")).unwrap()).await;
        assert!(nothing.failed[0].contains("nothing answers there"), "{nothing:?}");
    }

    #[tokio::test]
    async fn a_window_that_waits_takes_the_look_and_its_answer_comes_back() {
        let looks = Arc::new(Looks::default());
        assert!(!looks.window());
        assert_eq!(looks.next(Duration::from_millis(50)).await, None, "nothing to take");
        assert!(looks.window(), "a window asked a moment ago");
        // The window: takes the job and answers it.
        let window = looks.clone();
        let taken = tokio::spawn(async move {
            let job = window.next(Duration::from_secs(10)).await.unwrap();
            let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
            let seen = Seen {
                title: "Shop".into(),
                text: "Hello".into(),
                errors: vec!["boom".into(), "boom".into()],
                picture: Some(png),
                ..Default::default()
            };
            assert!(window.answer(job["id"].as_str().unwrap(), seen));
            job
        });
        let url = web::local_url("http://localhost:5173/cart").unwrap();
        let seen = look(&looks, &url, (390, 844), vec![json!({"click": "Add"})]).await;
        let job = taken.await.unwrap();
        assert_eq!(
            (job["url"].as_str().unwrap(), job["width"].as_u64().unwrap(), job["actions"][0]["click"].as_str().unwrap()),
            ("http://localhost:5173/cart", 390, "Add")
        );
        assert!(seen.rendered);
        assert_eq!((seen.url.as_str(), seen.title.as_str(), seen.errors.len()), ("http://localhost:5173/cart", "Shop", 1));
        assert_eq!(seen.picture.unwrap().len(), 7);
        assert!(!looks.answer("0000", Seen::default()), "an answer nobody waits for");
        assert!(looks.waiting.lock().unwrap().is_empty() && looks.queue.lock().unwrap().is_empty());
        // Something that is not a PNG is no picture.
        let window = looks.clone();
        tokio::spawn(async move {
            let job = window.next(Duration::from_secs(10)).await.unwrap();
            window.answer(job["id"].as_str().unwrap(), Seen { picture: Some(b"<html>".to_vec()), ..Default::default() });
        });
        assert!(look(&looks, &url, (1280, 800), Vec::new()).await.picture.is_none());
    }

    #[test]
    fn pictures_are_kept_with_the_task_and_go_with_it() {
        let dir = files::scratch("shots");
        let task = "0".repeat(32);
        let png = [0x89, b'P', b'N', b'G', 9];
        let name = keep_shot(&dir, &task, &png).unwrap();
        assert!(valid_shot(&name));
        assert_eq!(read_shot(&dir, &task, &name).unwrap(), png);
        assert!(read_shot(&dir, &task, "../../../etc/passwd").is_none());
        assert!(read_shot(&dir, "../x", &name).is_none());
        assert!(read_shot(&dir, &task, "0123456789abcdef").is_none());
        for _ in 0..MAX_SHOTS + 5 {
            keep_shot(&dir, &task, &png).unwrap();
        }
        assert_eq!(std::fs::read_dir(dir.join("shots").join(&task)).unwrap().count(), MAX_SHOTS);
        forget(&dir, &task);
        assert!(!dir.join("shots").join(&task).exists());
    }

    #[test]
    fn the_picture_goes_in_after_the_result_it_belongs_to() {
        let messages = vec![
            json!({"role": "user", "content": "make a shop"}),
            json!({"role": "assistant", "content": "", "tool_calls": [{"id": "a"}, {"id": "b"}]}),
            json!({"role": "tool", "tool_call_id": "a", "content": "Looked at http://localhost:5173/"}),
            json!({"role": "tool", "tool_call_id": "b", "content": "ok"}),
            json!({"role": "assistant", "content": "It looks right."}),
        ];
        let eye = Eye { step: "s".into(), call: "a".into(), shot: "x".into(), what: "the page http://localhost:5173/".into(), sends: 0 };
        let out = with_picture(messages.clone(), &eye, &[1, 2, 3]);
        assert_eq!(out.len(), 6);
        assert_eq!(out[4]["role"], "user");
        assert_eq!(
            out[4]["content"][0],
            json!({"type": "text", "text": "[Rigbrew] This is the screenshot of the page http://localhost:5173/."})
        );
        assert_eq!(out[4]["content"][1], json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AQID"}}));
        assert_eq!(out[5], messages[4]);
        // The result left the conversation: no picture either.
        let gone = Eye { call: "z".into(), ..eye };
        assert_eq!(with_picture(messages.clone(), &gone, &[1]), messages);
        assert!(picture_refused("image input is not supported - hint: if this is unexpected, you may need to provide the mmproj"));
        assert!(!picture_refused("the request exceeds the available context size"));
    }
}
