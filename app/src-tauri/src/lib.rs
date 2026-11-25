//! The native side of the Rigbrew app.
//!
//! It has one job besides hosting the window: make sure `rigbrewd` (the local service) is
//! running, tell the frontend where it is, restart it when it stops answering, and stop it
//! again when the app quits. Everything else happens between the frontend and the service over
//! HTTP on 127.0.0.1, except two things that need the system:
//!
//! - saving a file the person asked to keep (`save_image`, `save_text`): a save dialog and a write;
//! - signing in through the system browser: `account.rs`, only present in official builds (feature `account`).

#[cfg(feature = "account")]
mod account;

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{Manager, RunEvent};

/// What `rigbrewd` writes to `<data dir>/Rigbrew/daemon.json` when it starts.
#[derive(Deserialize)]
struct DaemonFile {
    port: u16,
    token: String,
    #[serde(default)]
    pid: u32,
}

#[derive(Serialize)]
struct DaemonInfo {
    port: u16,
    token: String,
}

/// The service process, when this app started it. `None` when it attached to one that was
/// already running: that one is not ours to stop.
#[derive(Default)]
struct Daemon(Mutex<Option<Child>>);

/// Process id of the service this app started, for the signal handler. 0 when there is none.
static DAEMON_PID: AtomicI32 = AtomicI32::new(0);

/// When the app itself is told to stop (Ctrl-C in `tauri dev`, `kill`), the normal exit path
/// does not run. Pass the request on to the service so it does not outlive the app.
#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    let pid = DAEMON_PID.load(Ordering::SeqCst);
    // SAFETY: `kill` and `_exit` are async-signal-safe.
    unsafe {
        if pid > 0 {
            libc::kill(pid, libc::SIGINT);
        }
        libc::_exit(0);
    }
}

/// The service's data folder, as `rigbrewd` picks it (core `default_data_dir`): `$RIGBREW_DATA_DIR`
/// when set, so a run against a fresh folder finds the service it started.
fn info_path() -> PathBuf {
    std::env::var_os("RIGBREW_DATA_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Rigbrew"))
        .join("daemon.json")
}

/// When the shell panics, a short record goes where the service looks for them at its next start
/// (`<data dir>/crashes/`, the same form as `rigbrew_core::crash`): the message, the place in the
/// code, the version. Nothing that was typed or made; home folders in paths become `~`.
fn report_panics() {
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let home = dirs::home_dir().and_then(|h| h.to_str().map(str::to_owned)).unwrap_or_default();
        let short = |text: String| -> String {
            let text = if home.len() > 1 { text.replace(&home, "~") } else { text };
            text.replace('\n', " ").chars().take(120).collect()
        };
        let message =
            info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned());
        let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let record = serde_json::json!({
            "program": "app",
            "version": env!("CARGO_PKG_VERSION"),
            "message": short(message.unwrap_or_else(|| "panic".into())),
            "location": short(info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default()),
            "thread": short(std::thread::current().name().unwrap_or("unnamed").to_string()),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "at_ms": at_ms,
        });
        if let Some(dir) = info_path().parent().map(|d| d.join("crashes")) {
            if std::fs::create_dir_all(&dir).is_ok() {
                std::fs::write(dir.join(format!("app-{at_ms}-{}.json", std::process::id())), record.to_string()).ok();
            }
        }
        earlier(info);
    }));
}

fn read_info() -> Option<DaemonFile> {
    serde_json::from_str(&std::fs::read_to_string(info_path()).ok()?).ok()
}

/// `daemon.json` counts as live only when something accepts connections on its port.
fn live_info() -> Option<DaemonFile> {
    let info = read_info()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;
    Some(info)
}

fn daemon_binary() -> PathBuf {
    if let Ok(path) = std::env::var("RIGBREW_DAEMON") {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) { "rigbrewd.exe" } else { "rigbrewd" };
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).unwrap_or_else(|| PathBuf::from(name))
}

/// Returns the connection details of a running service, starting one if there is none.
fn ensure_daemon(daemon: &Daemon) -> Result<DaemonInfo, String> {
    // Held for the whole call so the setup thread and the frontend cannot both spawn one.
    let mut slot = daemon.0.lock().map_err(|_| "service state is poisoned".to_string())?;
    if let Some(info) = live_info() {
        return Ok(DaemonInfo { port: info.port, token: info.token });
    }
    if let Some(mut dead) = slot.take() {
        dead.kill().ok();
        dead.wait().ok();
    }
    // Whatever is in the file now belongs to a service that is gone.
    std::fs::remove_file(info_path()).ok();

    let binary = daemon_binary();
    let mut command = Command::new(&binary);
    if let Ok(manifest) = std::env::var("RIGBREW_MANIFEST") {
        command.arg(manifest);
    }
    // The service watches this process id and stops by itself when the app is gone, so a
    // crashed or force-quit app never leaves it (and a loaded model) behind.
    command.env("RIGBREW_PARENT_PID", std::process::id().to_string());
    command.stdin(Stdio::null()).stdout(Stdio::null());
    let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", binary.display()))?;

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(info) = live_info().filter(|i| i.pid == child.id()) {
            DAEMON_PID.store(child.id() as i32, Ordering::SeqCst);
            *slot = Some(child);
            return Ok(DaemonInfo { port: info.port, token: info.token });
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "{} stopped right after starting ({status}). Can it reach models.rigbrew.dev, or is RIGBREW_MANIFEST valid?",
                binary.display()
            ));
        }
        if Instant::now() > deadline {
            child.kill().ok();
            child.wait().ok();
            return Err(format!("{} did not come up within 20 seconds", binary.display()));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

/// Stops the service this app started. It is asked politely first so it can shut its model
/// engine down and remove `daemon.json`; it is killed if it does not go.
fn stop_daemon(daemon: &Daemon) {
    let Some(mut child) = daemon.0.lock().ok().and_then(|mut slot| slot.take()) else {
        return;
    };
    DAEMON_PID.store(0, Ordering::SeqCst);
    #[cfg(unix)]
    {
        // SAFETY: plain signal to a process id we own.
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    child.kill().ok();
    child.wait().ok();
}

/// Where the service is. Called again by the frontend whenever the service stops answering:
/// a dead one is replaced, and the new port and token are read from the new `daemon.json`.
#[tauri::command]
async fn daemon_info(app: tauri::AppHandle) -> Result<DaemonInfo, String> {
    tauri::async_runtime::spawn_blocking(move || ensure_daemon(&app.state::<Daemon>())).await.map_err(|e| e.to_string())?
}

/// Asks where to save a generated image (a PNG, base64-encoded) and writes it there.
/// Returns the chosen path, or `None` when the dialog was cancelled.
#[tauri::command]
async fn save_image(app: tauri::AppHandle, name: String, data: String) -> Result<Option<String>, String> {
    use base64::Engine as _;
    use tauri_plugin_dialog::DialogExt;

    let bytes =
        base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| format!("the image data is not valid: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(target) =
            app.dialog().file().set_title("Save image").set_file_name(&name).add_filter("PNG image", &["png"]).blocking_save_file()
        else {
            return Ok(None);
        };
        let path = target.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Asks where to save a clip and writes it there. Returns the chosen path, or `None` when the
/// dialog was cancelled.
#[tauri::command]
async fn save_video(app: tauri::AppHandle, name: String, data: String) -> Result<Option<String>, String> {
    use base64::Engine as _;
    use tauri_plugin_dialog::DialogExt;

    let bytes =
        base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|e| format!("the clip data is not valid: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(target) =
            app.dialog().file().set_title("Save video").set_file_name(&name).add_filter("WebM video", &["webm"]).blocking_save_file()
        else {
            return Ok(None);
        };
        let path = target.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Asks where to save a text file (the account export, a chat artifact) and writes it there.
/// The dialog offers the kind of file the name ends in. Returns the chosen path, or `None` when
/// the dialog was cancelled.
#[tauri::command]
async fn save_text(app: tauri::AppHandle, name: String, text: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    tauri::async_runtime::spawn_blocking(move || {
        let ending = std::path::Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty() && e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("txt")
            .to_ascii_lowercase();
        let Some(target) = app
            .dialog()
            .file()
            .set_title("Save")
            .set_file_name(&name)
            .add_filter(ending.to_ascii_uppercase(), &[ending.as_str()])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = target.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(unix)]
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: installs a handler that only calls async-signal-safe functions.
        unsafe { libc::signal(signal, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t) };
    }
    report_panics();
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_dialog::init()).manage(Daemon::default());
    // Sign-in lives in a module that is not part of the public source; without it the app has no accounts.
    #[cfg(feature = "account")]
    let builder = builder.manage(account::SignIn::default()).invoke_handler(tauri::generate_handler![
        daemon_info,
        save_image,
        save_video,
        save_text,
        account::signin_listen,
        account::signin_wait,
        account::signin_cancel
    ]);
    #[cfg(not(feature = "account"))]
    let builder = builder.invoke_handler(tauri::generate_handler![daemon_info, save_image, save_video, save_text]);
    builder
        .setup(|app| {
            // Start the service while the window is still loading.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if let Err(e) = ensure_daemon(&handle.state::<Daemon>()) {
                    eprintln!("rigbrew: {e}");
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Rigbrew app")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                stop_daemon(&app.state::<Daemon>());
            }
        });
}
