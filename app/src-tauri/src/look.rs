//! Looking at a page for a Code task: the service has no browser, this app has one.
//!
//! `serve` waits at the service for a page to look at. Each one is opened in a hidden window of
//! its own, an address on this computer and nothing else; when the page has loaded, what a
//! person would find there goes back to the service: the title, the text, what the page's
//! scripts wrote as errors, the requests that failed and, on macOS, a picture of it.
//!
//! The hidden window is not the app's window: it is named in no capability, so the page in it
//! can call nothing of this app, and it may only show addresses on this computer. It keeps no
//! cookies or storage between looks.

use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, Url, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "look";
/// How long a page has to finish loading.
const LOAD: Duration = Duration::from_secs(15);
/// What a page gets after loading to draw itself and fetch its data.
const SETTLE: Duration = Duration::from_millis(900);
const ANSWER: Duration = Duration::from_secs(6);
/// How long an empty page is given to draw itself, and how long one click or piece of typing may take.
const DRAW: Duration = Duration::from_secs(3);
const STEP: Duration = Duration::from_secs(4);
/// The widest picture, in pixels.
#[cfg(target_os = "macos")]
const PICTURE_WIDTH: f64 = 1024.0;

/// Runs in the page before its own scripts: keeps what goes wrong, and can click and type.
const INIT: &str = include_str!("look.js");

#[derive(Serialize, Default, serde::Deserialize)]
pub struct Seen {
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
    /// The PNG, base64. Missing where no picture can be taken.
    #[serde(default)]
    picture: Option<String>,
    #[serde(default)]
    note: String,
    /// The steps asked for are still going. Not part of the answer to the service.
    #[serde(default, skip_serializing)]
    busy: bool,
}

/// Whether an address is on this computer: http or https to `localhost`, a name under it, or a
/// loopback address. The service checks the same before it asks.
fn local(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str().map(|h| h.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase()) else { return false };
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".localhost"),
    }
}

/// The page's own report: the script's answer comes back as JSON text inside a JSON string.
fn read_report(answer: &str) -> Option<Seen> {
    let inner: String = serde_json::from_str(answer).ok()?;
    serde_json::from_str(&inner).ok()
}

/// A picture of what the window shows, as a PNG.
#[cfg(target_os = "macos")]
fn picture(window: &tauri::WebviewWindow, width: f64) -> Option<Vec<u8>> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSNumber};
    use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

    fn png_of(image: &NSImage) -> Option<Vec<u8>> {
        let tiff = image.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
        // SAFETY: an empty dictionary holds no property of a wrong type.
        let png = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
        Some(png.to_vec())
    }

    // On a display with two pixels to the point the picture is twice as wide as asked for.
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let points = width.min(PICTURE_WIDTH) / scale;
    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    let failed = tx.clone();
    let asked = window.with_webview(move |webview| {
        let Some(mtm) = MainThreadMarker::new() else {
            tx.send(None).ok();
            return;
        };
        let done = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            // SAFETY: WebKit hands over a valid image or null.
            let png = unsafe { image.as_ref() }.and_then(png_of);
            tx.send(png).ok();
        });
        // SAFETY: `inner` is this window's WKWebView, alive for the length of this call, and we
        // are on the main thread, where WebKit wants to be called.
        unsafe {
            let view: &WKWebView = &*(webview.inner() as *const WKWebView);
            let config = WKSnapshotConfiguration::new(mtm);
            config.setSnapshotWidth(Some(&NSNumber::new_f64(points)));
            view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
        }
    });
    if asked.is_err() {
        failed.send(None).ok();
    }
    rx.recv_timeout(Duration::from_secs(8)).ok().flatten()
}

#[cfg(not(target_os = "macos"))]
fn picture(_window: &tauri::WebviewWindow, _width: f64) -> Option<Vec<u8>> {
    None
}

fn look(app: &tauri::AppHandle, url: Url, width: f64, height: f64, actions: Vec<Value>) -> Result<Seen, String> {
    use base64::Engine as _;
    if let Some(old) = app.get_webview_window(LABEL) {
        old.destroy().ok();
        std::thread::sleep(Duration::from_millis(150));
    }
    let (loaded_tx, loaded) = mpsc::channel::<()>();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url.clone()))
        .title("Rigbrew preview")
        .inner_size(width.clamp(320.0, 1920.0), height.clamp(320.0, 1600.0))
        .visible(false)
        .focused(false)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .incognito(true)
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .initialization_script(INIT)
        // The page may go where it likes on this computer, and nowhere else.
        .on_navigation(|to| local(to) || to.scheme() == "about")
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Finished {
                loaded_tx.send(()).ok();
            }
        })
        .build()
        .map_err(|e| format!("The page could not be opened: {e}"))?;
    let mut notes: Vec<String> = Vec::new();
    if loaded.recv_timeout(LOAD).is_err() {
        notes.push(format!("The page did not finish loading in {} seconds.", LOAD.as_secs()));
    }
    std::thread::sleep(SETTLE);
    if !actions.is_empty() {
        let steps = serde_json::to_string(&actions).unwrap_or_else(|_| "[]".into());
        window.eval(format!("window.__rigbrewLook && window.__rigbrewLook.act({steps})")).ok();
    }
    // Without the script (a page the browser itself made, like an error page) the title and text still count.
    let ask = "JSON.stringify(window.__rigbrewLook ? window.__rigbrewLook.report() : \
               {url: location.href, title: document.title, text: (document.body && document.body.innerText || '').slice(0, 6000)})";
    let report = || {
        let (tx, rx) = mpsc::channel::<String>();
        let asked = window.eval_with_callback(ask, move |answer| {
            tx.send(answer).ok();
        });
        asked.ok().and_then(|_| rx.recv_timeout(ANSWER).ok()).as_deref().and_then(read_report)
    };
    // Asked again while the steps are still going, and for a moment while the page is still
    // empty: an app that draws itself needs a little longer than a page that is just there.
    let until = std::time::Instant::now() + STEP * actions.len() as u32 + DRAW;
    let mut seen = loop {
        let now = report();
        let waiting = now.as_ref().is_some_and(|s| s.busy || (s.text.trim().is_empty() && s.errors.is_empty()));
        if !waiting || std::time::Instant::now() >= until {
            break now;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    .unwrap_or_else(|| {
        notes.push("The page did not answer when it was asked what it shows.".into());
        Seen { url: url.to_string(), ..Default::default() }
    });
    if seen.busy {
        notes.push("Not every step was done in time.".into());
    }
    seen.picture = picture(&window, width).map(|png| base64::engine::general_purpose::STANDARD.encode(png));
    if seen.picture.is_none() && cfg!(not(target_os = "macos")) {
        notes.push("No picture: Rigbrew takes pictures of pages on macOS only for now.".into());
    }
    window.destroy().ok();
    if !notes.is_empty() {
        seen.note = format!("{} {}", seen.note, notes.join(" ")).trim().to_string();
    }
    Ok(seen)
}

/// What the service answered: the status and the body.
fn read_answer(raw: &[u8]) -> Option<(u16, String)> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
    let status: u16 = head.lines().next()?.split_whitespace().nth(1)?.parse().ok()?;
    let mut body = &raw[end + 4..];
    if !head.contains("transfer-encoding: chunked") {
        return Some((status, String::from_utf8_lossy(body).into_owned()));
    }
    // In pieces: each has its length in hex on a line of its own.
    let mut whole = Vec::new();
    loop {
        let line = body.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(String::from_utf8_lossy(&body[..line]).split(';').next()?.trim(), 16).ok()?;
        let from = line + 2;
        if size == 0 {
            break;
        }
        whole.extend_from_slice(body.get(from..from + size)?);
        body = body.get(from + size + 2..)?;
    }
    Some((status, String::from_utf8_lossy(&whole).into_owned()))
}

/// One request to the service on this computer. Small on purpose: the app has no other use
/// for an HTTP client, and the service is only ever at 127.0.0.1.
fn request(port: u16, token: &str, method: &str, path: &str, body: &str, wait: Duration) -> Option<(u16, String)> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(wait)).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(20))).ok()?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(body.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    read_answer(&raw)
}

/// One job from the service, looked at. Anything that goes wrong is said in the answer.
fn take(app: &tauri::AppHandle, job: &Value) -> Seen {
    let address = job["url"].as_str().unwrap_or("");
    let refused = |why: &str| Seen { url: address.to_string(), note: why.to_string(), ..Default::default() };
    let Ok(url) = Url::parse(address.trim()) else { return refused("That is not a web address.") };
    if !local(&url) {
        return refused("Only pages on this computer are opened.");
    }
    let (width, height) = (job["width"].as_f64().unwrap_or(1280.0), job["height"].as_f64().unwrap_or(800.0));
    let actions = job["actions"].as_array().cloned().unwrap_or_default();
    look(app, url, width, height, actions).unwrap_or_else(|why| refused(&why))
}

/// Takes looks for the service for as long as the app runs: waits at the service for a page
/// to open (`GET /v1/agent/looks/next`), looks, and hands back what was seen. This runs here
/// and not in the window's page, whose scripts are put to sleep when the window is hidden or
/// the screen is locked, while a task may well go on. `service` says where the service is.
pub fn serve(app: tauri::AppHandle, service: impl Fn() -> Option<(u16, String)> + Send + 'static) {
    std::thread::spawn(move || loop {
        let Some((port, token)) = service() else {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        match request(port, &token, "GET", "/v1/agent/looks/next", "", Duration::from_secs(45)) {
            Some((200, body)) => {
                let job = serde_json::from_str::<Value>(&body).ok().map(|v| v["look"].clone()).filter(|l| l.is_object());
                let Some(job) = job else { continue };
                let seen = serde_json::to_string(&take(&app, &job)).unwrap_or_else(|_| "{}".into());
                let path = format!("/v1/agent/looks/{}", job["id"].as_str().unwrap_or(""));
                request(port, &token, "POST", &path, &seen, Duration::from_secs(20));
            }
            // A service too old to ask. A newer one may take its place.
            Some((404 | 405, _)) => std::thread::sleep(Duration::from_secs(60)),
            _ => std::thread::sleep(Duration::from_secs(2)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_computer_is_opened() {
        for ok in [
            "http://localhost:5173/",
            "http://127.0.0.1:8000/a?b=1",
            "http://[::1]:3000/",
            "https://localhost:8443/",
            "http://shop.localhost:5173/",
        ] {
            assert!(local(&Url::parse(ok).unwrap()), "{ok}");
        }
        for refused in [
            "https://example.com/",
            "http://localhost.example.com/",
            "http://user:pw@localhost:5173/",
            "http://192.168.1.20:5173/",
            "http://169.254.169.254/",
            "file:///etc/passwd",
            "tauri://localhost/",
            "about:blank",
        ] {
            assert!(!local(&Url::parse(refused).unwrap()), "{refused}");
        }
    }

    #[test]
    fn the_report_of_a_page_is_read() {
        let inner = r#"{"url":"http://localhost:5173/","title":"Shop","text":"Hello","errors":["boom"],"failed":[],"note":""}"#;
        let seen = read_report(&serde_json::to_string(inner).unwrap()).unwrap();
        assert_eq!((seen.title.as_str(), seen.errors.len(), seen.picture.is_none()), ("Shop", 1, true));
        assert!(read_report("null").is_none());
        // The service's answers, whole or in pieces.
        let whole = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 13\r\n\r\n{\"look\":null}";
        assert_eq!(read_answer(whole).unwrap(), (200, "{\"look\":null}".to_string()));
        let pieces = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"look\"\r\n6\r\n:null}\r\n0\r\n\r\n";
        assert_eq!(read_answer(pieces).unwrap(), (200, "{\"look\":null}".to_string()));
        assert_eq!(read_answer(b"HTTP/1.1 404 Not Found\r\n\r\n").unwrap().0, 404);
        assert!(read_answer(b"not http").is_none());
        assert!(read_report("\"not json\"").is_none());
        // The script is plain JavaScript an old engine reads too: no arrow functions, no let.
        assert!(!INIT.contains("=>") && !INIT.contains("let ") && INIT.contains("__rigbrewLook"));
    }
}
