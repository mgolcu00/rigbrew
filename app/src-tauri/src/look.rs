//! Looking at a page for a Code task: the service has no browser, this app has one.
//!
//! `look_page` opens an address on this computer in a hidden window of its own, waits until the
//! page has loaded, and gives back what a person would find there: the title, the text, what the
//! page's scripts wrote as errors, the requests that failed and, on macOS, a picture of it. The
//! frontend asks the service for such jobs and hands the answers back (`src/lib/look.ts`).
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
/// The widest picture, in pixels.
#[cfg(target_os = "macos")]
const PICTURE_WIDTH: f64 = 1024.0;

/// Runs in the page before its own scripts: keeps what goes wrong, and can click and type.
const INIT: &str = include_str!("look.js");

/// One look at a time: they share the window's name.
#[derive(Default)]
pub struct Looking(std::sync::Mutex<()>);

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
        std::thread::sleep(Duration::from_millis(600 + 450 * actions.len() as u64));
    }
    let (report_tx, report) = mpsc::channel::<String>();
    // Without the script (a page the browser itself made, like an error page) the title and text still count.
    let ask = "JSON.stringify(window.__rigbrewLook ? window.__rigbrewLook.report() : \
               {url: location.href, title: document.title, text: (document.body && document.body.innerText || '').slice(0, 6000)})";
    let asked = window.eval_with_callback(ask, move |answer| {
        report_tx.send(answer).ok();
    });
    let mut seen = match asked.ok().and_then(|_| report.recv_timeout(ANSWER).ok()).as_deref().and_then(read_report) {
        Some(seen) => seen,
        None => {
            notes.push("The page did not answer when it was asked what it shows.".into());
            Seen { url: url.to_string(), ..Default::default() }
        }
    };
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

/// Opens one page on this computer in a hidden window and says what is on it.
#[tauri::command]
pub async fn look_page(app: tauri::AppHandle, url: String, width: f64, height: f64, actions: Vec<Value>) -> Result<Seen, String> {
    let url = Url::parse(url.trim()).map_err(|_| "That is not a web address.".to_string())?;
    if !local(&url) {
        return Err("Only pages on this computer are opened.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let looking = app.state::<Looking>();
        let _one = looking.0.lock().map_err(|_| "The preview window is not available.".to_string())?;
        look(&app, url, width, height, actions)
    })
    .await
    .map_err(|e| e.to_string())?
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
        assert!(read_report("\"not json\"").is_none());
        // The script is plain JavaScript an old engine reads too: no arrow functions, no let.
        assert!(!INIT.contains("=>") && !INIT.contains("let ") && INIT.contains("__rigbrewLook"));
    }
}
