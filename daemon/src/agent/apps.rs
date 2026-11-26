//! `start_app`: the project's own app (a dev server, a static page, a small web service) as a
//! background process the task owns, so the model and the person can look at what was made.
//!
//! The service picks a free port, hands it to the command (`{port}` in the command line, and
//! `PORT` in its environment), and waits until something answers on this computer's own
//! address or the process dies. A server that chose another port and printed its address is
//! found by that line. The last lines of output are kept.
//!
//! One task has one app. It is stopped when the task is stopped, fails or is removed, when the
//! person stops it from the preview, when another one is started in its place, and when the
//! service stops. A task that finished keeps its app, so the person can look at the result;
//! at most `MAX_APPS` run at a time, and the oldest of a task that is not working makes room.
//!
//! The command runs like `run_command`: in the folder, and on macOS in the same sandbox, which
//! lets it listen on this computer's own address and nothing more (see `files::Net`).

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

use super::files::{self, Folder, Net};
use super::{verify, Task};
use crate::Shared;

/// Apps running at the same time, over all tasks.
pub const MAX_APPS: usize = 4;
/// How long a new app has to answer.
pub const READY: Duration = Duration::from_secs(90);
const KEPT_LINES: usize = 200;
const LINE_CHARS: usize = 300;
/// Lines of output the model is shown with the result.
const SHOWN_LINES: usize = 12;

struct Running {
    command: String,
    port: u16,
    started_ms: i64,
    pid: Option<u32>,
    lines: Arc<Mutex<VecDeque<String>>>,
    alive: Arc<AtomicBool>,
    /// The piece of work that waits for the process; dropping it drops (and so kills) the process.
    waiter: tokio::task::AbortHandle,
}

impl Running {
    fn last_lines(&self, n: usize) -> Vec<String> {
        let lines = self.lines.lock().unwrap();
        lines.iter().skip(lines.len().saturating_sub(n)).cloned().collect()
    }

    /// Asks everything the command started to end, and makes sure of it a moment later.
    fn stop(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            // Negative pid: every process in the group the shell started.
            unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
            let alive = self.alive.clone();
            std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_millis(1500);
                while alive.load(Ordering::SeqCst) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(50));
                }
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            });
        }
        #[cfg(windows)]
        if let Some(pid) = self.pid {
            // The command and everything under it.
            std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok();
        }
        #[cfg(not(unix))]
        self.waiter.abort();
        // On Unix the waiter sees the process end and says so; it is cut only if that takes long.
        #[cfg(unix)]
        {
            let waiter = self.waiter.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                waiter.abort();
            });
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The apps of this run of the service, by task.
#[derive(Default)]
pub struct Apps {
    running: Mutex<HashMap<String, Running>>,
}

impl Apps {
    /// The port of a task's app while it runs.
    pub fn port(&self, task: &str) -> Option<u16> {
        self.running.lock().unwrap().get(task).filter(|r| r.alive.load(Ordering::SeqCst)).map(|r| r.port)
    }

    /// The app of a task as the API shows it, or `None` when it has none.
    pub fn view(&self, task: &str) -> Option<Value> {
        let running = self.running.lock().unwrap();
        let r = running.get(task)?;
        Some(json!({
            "url": format!("http://localhost:{}/", r.port),
            "port": r.port,
            "command": r.command,
            "alive": r.alive.load(Ordering::SeqCst),
            "started_ms": r.started_ms,
            "lines": r.last_lines(30),
        }))
    }

    /// Stops the app of a task. Says whether there was one.
    pub fn stop(&self, task: &str) -> bool {
        // Dropped outside the lock: stopping takes a moment.
        let gone = self.running.lock().unwrap().remove(task);
        gone.is_some()
    }

    /// Stops every app: the service is closing.
    pub fn stop_all(&self) {
        let gone: Vec<Running> = self.running.lock().unwrap().drain().map(|(_, r)| r).collect();
        #[cfg(unix)]
        for r in &gone {
            if let Some(pid) = r.pid {
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            }
        }
        drop(gone);
    }

    fn count(&self) -> usize {
        self.running.lock().unwrap().values().filter(|r| r.alive.load(Ordering::SeqCst)).count()
    }

    /// The task with the oldest app among `idle`.
    fn oldest(&self, idle: &dyn Fn(&str) -> bool) -> Option<String> {
        let running = self.running.lock().unwrap();
        running.iter().filter(|(id, _)| idle(id)).min_by_key(|(_, r)| r.started_ms).map(|(id, _)| id.clone())
    }
}

/// What a command of this task may reach: the web when the task has it, and its own app.
pub fn net_of(app: &Shared, task: &Task) -> Net {
    Net { web: task.web, listen: false, app_port: app.agent.apps.port(&task.id) }
}

fn has(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

/// The command that starts this project's app, with `{port}` where the port goes, or `None`
/// when the folder does not say. The model can always give its own.
pub fn detect(root: &Path) -> Option<String> {
    let text = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    if let Ok(package) = serde_json::from_str::<Value>(&text("package.json")) {
        let runner = verify::node_runner(root);
        for script in ["dev", "start", "serve", "preview"] {
            let Some(line) = package["scripts"][script].as_str().filter(|s| !s.trim().is_empty()) else { continue };
            // Arguments after the script's name: npm needs "--" before them, the others do not.
            let pass = if runner == "npm" { " --" } else { "" };
            let words: Vec<&str> = line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).collect();
            let flag = if words.contains(&"next") {
                format!("{pass} -p {{port}}")
            } else if ["vite", "astro", "nuxt", "svelte-kit", "http-server", "serve"].iter().any(|k| words.contains(k)) {
                format!("{pass} --port {{port}}")
            } else {
                // Anything else is told through PORT in its environment.
                String::new()
            };
            return Some(format!("{runner} run {script}{flag}"));
        }
    }
    if has(root, "manage.py") {
        return Some("python3 manage.py runserver 127.0.0.1:{port}".into());
    }
    if has(root, "pubspec.yaml") && text("pubspec.yaml").contains("flutter") && root.join("web").is_dir() {
        return Some("flutter run -d web-server --web-hostname 127.0.0.1 --web-port {port}".into());
    }
    if has(root, "Cargo.toml") && (has(root, "src/main.rs") || text("Cargo.toml").contains("[[bin]]")) {
        return Some("cargo run".into());
    }
    if has(root, "go.mod") && has(root, "main.go") {
        return Some("go run .".into());
    }
    for (dir, shown) in [(".", ""), ("public", " --directory public"), ("dist", " --directory dist")] {
        if has(&root.join(dir), "index.html") {
            let python = if cfg!(windows) { "python" } else { "python3" };
            return Some(format!("{python} -m http.server {{port}} --bind 127.0.0.1{shown}"));
        }
    }
    None
}

/// Whether a command line starts a server that runs until it is stopped: `run_command` waits
/// for a command to end, so such a command is the work of `start_app`.
pub fn serves(command: &str) -> bool {
    static SERVES: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let serves = SERVES.get_or_init(|| {
        regex::Regex::new(
            r"(?x)(^|[;&|(]\s*)(
                python3?\s+-m\s+http\.server | (npx\s+)?(http-server|live-server|serve)(\s|$) | php\s+-S\s |
                (python3?\s+)?manage\.py\s+runserver | flask\s+run | (bin/)?rails\s+s(erver)?(\s|$) |
                (npm|pnpm|yarn|bun)\s+(run\s+)?(dev|start|serve|preview)(\s|$) | vite(\s+(dev|serve|preview))?(\s+-|\s*$)
            )",
        )
        .expect("static pattern")
    });
    // Sent to the background, it is not waited for.
    !command.trim_end().ends_with('&') && serves.is_match(command)
}

/// What `run_command` answers instead of starting a server.
pub const SERVES: &str = "Not run: this starts a server, and run_command waits for a command to end. Call start_app instead: leave out \"command\" to let Rigbrew find how this project starts, or give the command with {port} where the port goes.";

/// The command line with the port put in.
pub fn with_port(command: &str, port: u16) -> String {
    command.replace("{port}", &port.to_string()).replace("$PORT", &port.to_string()).replace("${PORT}", &port.to_string())
}

/// A port nothing listens on right now.
pub fn free_port() -> Option<u16> {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?.local_addr().ok().map(|a| a.port())
}

/// Whether something accepts connections on this computer's own address at that port.
pub async fn answers(port: u16) -> bool {
    for addr in [SocketAddr::from((Ipv4Addr::LOCALHOST, port)), SocketAddr::from((Ipv6Addr::LOCALHOST, port))] {
        if matches!(tokio::time::timeout(Duration::from_millis(300), tokio::net::TcpStream::connect(addr)).await, Ok(Ok(_))) {
            return true;
        }
    }
    false
}

fn plain(line: &str) -> String {
    static ANSI: OnceLock<Regex> = OnceLock::new();
    let ansi = ANSI.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|[\x00-\x08\x0b-\x1f]").expect("static pattern"));
    ansi.replace_all(line, "").trim_end().chars().take(LINE_CHARS).collect()
}

/// The ports of the addresses on this computer that the output names: "Local: http://localhost:5173/".
pub fn printed_ports(lines: &[String]) -> Vec<u16> {
    static URL: OnceLock<Regex> = OnceLock::new();
    let url =
        URL.get_or_init(|| Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{2,5})\b").expect("static pattern"));
    let mut out: Vec<u16> = Vec::new();
    for line in lines {
        for found in url.captures_iter(line) {
            if let Ok(port) = found[1].parse::<u16>() {
                if port > 0 && !out.contains(&port) {
                    out.push(port);
                }
            }
        }
    }
    out
}

async fn keep_lines(pipe: impl tokio::io::AsyncRead + Unpin, lines: Arc<Mutex<VecDeque<String>>>) {
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(pipe);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = plain(&String::from_utf8_lossy(&buf));
                if line.is_empty() {
                    continue;
                }
                let mut kept = lines.lock().unwrap();
                if kept.len() == KEPT_LINES {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
        }
    }
}

/// What a started app is, for the model and the step.
#[derive(Debug)]
pub struct Started {
    pub port: u16,
    pub command: String,
    pub lines: Vec<String>,
}

impl Started {
    pub fn text(&self) -> String {
        let mut out = format!("Started the app on http://localhost:{}/ with `{}`.", self.port, self.command);
        if !self.lines.is_empty() {
            out.push_str("\nIts last lines:\n");
            out.push_str(&self.lines.join("\n"));
        }
        out.push_str(&format!(
            "\nNow call look_at_page with http://localhost:{}/ to see it. The app keeps running; start_app again starts it afresh.",
            self.port
        ));
        out
    }
}

/// Starts `command` for a task and waits until it answers. An app the task already had is
/// stopped first. `changed` is called when the process ends by itself.
pub async fn start(
    apps: &Apps,
    task: &str,
    folder: &Folder,
    command: &str,
    web: bool,
    ready: Duration,
    changed: impl FnOnce() + Send + 'static,
) -> Result<Started, String> {
    use std::process::Stdio;
    apps.stop(task);
    let port = free_port().ok_or("No free port was found on this computer.")?;
    let command = with_port(command.trim(), port);
    let env = [("PORT", port.to_string()), ("HOST", "127.0.0.1".to_string()), ("BROWSER", "none".to_string())];
    let mut cmd = files::shell(folder, &command, Net { web, listen: true, app_port: None }, &env);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("The command could not be started: {e}"))?;
    let pid = child.id();
    let lines: Arc<Mutex<VecDeque<String>>> = Default::default();
    tokio::spawn(keep_lines(child.stdout.take().expect("piped"), lines.clone()));
    tokio::spawn(keep_lines(child.stderr.take().expect("piped"), lines.clone()));
    let alive = Arc::new(AtomicBool::new(true));
    let exit: Arc<Mutex<Option<String>>> = Default::default();
    let (alive_w, exit_w) = (alive.clone(), exit.clone());
    let waiter = tokio::spawn(async move {
        let status = child.wait().await;
        *exit_w.lock().unwrap() = Some(status.ok().and_then(|s| s.code()).map(|c| c.to_string()).unwrap_or_else(|| "none (killed)".into()));
        alive_w.store(false, Ordering::SeqCst);
        changed();
    });
    let mut running =
        Running { command: command.clone(), port, started_ms: super::now_ms(), pid, lines, alive, waiter: waiter.abort_handle() };
    let shown = |r: &Running| {
        let lines = r.last_lines(SHOWN_LINES);
        if lines.is_empty() {
            "(no output)".to_string()
        } else {
            lines.join("\n")
        }
    };
    let until = Instant::now() + ready;
    let found = loop {
        if !running.alive.load(Ordering::SeqCst) {
            // The output may still be on its way.
            tokio::time::sleep(Duration::from_millis(150)).await;
            let code = exit.lock().unwrap().clone().unwrap_or_default();
            return Err(format!("`{command}` ended at once (exit code {code}) and nothing is running.\n{}", shown(&running)));
        }
        let mut ports = vec![port];
        ports.extend(printed_ports(&running.last_lines(KEPT_LINES)));
        let mut found = None;
        for p in ports {
            if answers(p).await {
                found = Some(p);
                break;
            }
        }
        if found.is_some() {
            break found;
        }
        if Instant::now() >= until {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    };
    let Some(found) = found else {
        let said = shown(&running);
        drop(running);
        return Err(format!(
            "`{command}` did not answer on port {port} within {} seconds, so it was stopped. Make it listen on the port in $PORT (or write {{port}} in the command).\n{said}",
            ready.as_secs()
        ));
    };
    let started = Started { port: found, command, lines: running.last_lines(SHOWN_LINES) };
    running.port = found;
    apps.running.lock().unwrap().insert(task.to_string(), running);
    Ok(started)
}

/// Makes room for one more app: the oldest one of a task that is not working goes.
pub fn make_room(app: &Shared) -> Result<(), String> {
    let apps = &app.agent.apps;
    // Apps whose process ended by itself take no room.
    apps.running.lock().unwrap().retain(|_, r| r.alive.load(Ordering::SeqCst));
    while apps.count() >= MAX_APPS {
        let idle = |id: &str| app.agent.get(id).is_none_or(|t| !t.active());
        match apps.oldest(&idle) {
            Some(id) => {
                apps.stop(&id);
            }
            None => return Err(format!("{MAX_APPS} apps are already running in other tasks. Stop one first.")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files_in_it: &[(&str, &str)]) -> std::path::PathBuf {
        let root = files::scratch("apps");
        for (path, text) in files_in_it {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        root
    }

    #[test]
    fn a_command_that_starts_a_server_is_told_apart() {
        for server in [
            "python3 -m http.server 5173",
            "python -m http.server",
            "npx http-server -p 8080",
            "npm run dev",
            "pnpm dev --port 3000",
            "yarn start",
            "cd web && npm start",
            "vite",
            "vite --port 4000",
            "python manage.py runserver",
            "flask run",
            "php -S localhost:8000",
            "npx serve .",
        ] {
            assert!(serves(server), "{server}");
        }
        for not in [
            "npm test",
            "vite build",
            "npm run build",
            "pnpm install",
            "python3 -m pytest",
            "cargo run",
            "python3 -m http.server 8000 &",
            "ls serve",
        ] {
            assert!(!serves(not), "{not}");
        }
    }

    #[test]
    fn the_command_that_starts_a_project_is_found() {
        let found = |files_in_it: &[(&str, &str)]| detect(&project(files_in_it));
        let vite = r#"{"scripts": {"dev": "vite dev", "build": "vite build", "preview": "vite preview"}}"#;
        assert_eq!(found(&[("package.json", vite)]).unwrap(), "npm run dev -- --port {port}");
        assert_eq!(found(&[("package.json", vite), ("pnpm-lock.yaml", "")]).unwrap(), "pnpm run dev --port {port}");
        let next = r#"{"scripts": {"dev": "next dev", "start": "next start"}}"#;
        assert_eq!(found(&[("package.json", next), ("yarn.lock", "")]).unwrap(), "yarn run dev -p {port}");
        assert_eq!(found(&[("package.json", r#"{"scripts": {"start": "node server.js"}}"#)]).unwrap(), "npm run start");
        // A package with nothing to start falls through to what else is in the folder.
        let python = if cfg!(windows) { "python" } else { "python3" };
        assert_eq!(
            found(&[("package.json", r#"{"scripts": {"test": "vitest"}}"#), ("index.html", "<h1>hi</h1>")]).unwrap(),
            format!("{python} -m http.server {{port}} --bind 127.0.0.1")
        );
        assert_eq!(
            found(&[("public/index.html", "<h1>hi</h1>")]).unwrap(),
            format!("{python} -m http.server {{port}} --bind 127.0.0.1 --directory public")
        );
        assert_eq!(found(&[("manage.py", "")]).unwrap(), "python3 manage.py runserver 127.0.0.1:{port}");
        assert_eq!(found(&[("Cargo.toml", "[package]\n"), ("src/main.rs", "fn main() {}")]).unwrap(), "cargo run");
        assert_eq!(found(&[("Cargo.toml", "[package]\n"), ("src/lib.rs", "")]), None, "a library has nothing to start");
        assert_eq!(found(&[("go.mod", "module x\n"), ("main.go", "package main\n")]).unwrap(), "go run .");
        assert_eq!(found(&[("README.md", "hello\n")]), None);
    }

    #[test]
    fn the_port_goes_into_the_command_and_is_read_from_the_output() {
        assert_eq!(with_port("npm run dev -- --port {port}", 5173), "npm run dev -- --port 5173");
        assert_eq!(with_port("serve -l $PORT", 4000), "serve -l 4000");
        assert_eq!(with_port("cargo run", 4000), "cargo run");
        let lines: Vec<String> = [
            "  VITE v6.0.0  ready in 320 ms",
            "  ➜  Local:   http://localhost:5174/",
            "  ➜  Network: http://192.168.1.20:5174/",
            "listening on http://127.0.0.1:8080 and http://[::1]:8081",
            "docs at https://example.com:8443/guide",
        ]
        .iter()
        .map(|l| l.to_string())
        .collect();
        assert_eq!(printed_ports(&lines), [5174, 8080, 8081], "only addresses on this computer");
        assert_eq!(
            plain("\x1b[32m  ➜\x1b[39m  Local:   \x1b[36mhttp://localhost:5173/\x1b[39m\r\n"),
            "  ➜  Local:   http://localhost:5173/"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_app_is_started_answers_and_is_gone_when_stopped() {
        let root = project(&[("index.html", "<title>Shop</title><h1>Hello from the app</h1>")]);
        let folder = Folder::open(root.to_str().unwrap()).unwrap();
        let apps = Apps::default();
        let ended = Arc::new(AtomicBool::new(false));
        let ended_w = ended.clone();
        let command = detect(&root).unwrap();
        // Inside the macOS sandbox, without the web: it may listen on this computer all the same.
        let started = start(&apps, "t1", &folder, &command, false, Duration::from_secs(30), move || ended_w.store(true, Ordering::SeqCst))
            .await
            .unwrap();
        assert_eq!(apps.port("t1"), Some(started.port));
        assert!(started.command.contains(&started.port.to_string()), "{}", started.command);
        assert!(started.text().starts_with(&format!("Started the app on http://localhost:{}/", started.port)));
        let body = reqwest::get(format!("http://127.0.0.1:{}/", started.port)).await.unwrap().text().await.unwrap();
        assert!(body.contains("Hello from the app"));
        let view = apps.view("t1").unwrap();
        assert_eq!(view["alive"], true);
        assert_eq!(view["url"], format!("http://localhost:{}/", started.port));
        // A command of the same task may talk to the app, and to nothing else on the network.
        let net = Net { web: false, listen: false, app_port: apps.port("t1") };
        let ran = files::run(&folder, &format!("curl -s -m 5 http://127.0.0.1:{}/", started.port), Duration::from_secs(10), net).await;
        assert!(ran.text.contains("Hello from the app"), "{}", ran.text);
        // Starting again replaces it: the old port goes quiet.
        let again = start(&apps, "t1", &folder, &command, false, Duration::from_secs(30), || {}).await.unwrap();
        assert_ne!(again.port, 0);
        gone(started.port).await;
        for _ in 0..50 {
            if ended.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(ended.load(Ordering::SeqCst), "the first app ended and said so");
        assert!(apps.stop("t1"));
        assert!(!apps.stop("t1"));
        gone(again.port).await;
        assert_eq!(apps.port("t1"), None);
        assert!(apps.view("t1").is_none());
    }

    #[cfg(unix)]
    async fn gone(port: u16) {
        for _ in 0..60 {
            if !answers(port).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("something still answers on port {port}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_app_that_dies_or_never_answers_is_an_error_with_its_output() {
        let root = project(&[]);
        let folder = Folder::open(root.to_str().unwrap()).unwrap();
        let apps = Apps::default();
        let died = start(&apps, "t1", &folder, "echo cannot find module; exit 3", false, Duration::from_secs(10), || {}).await.unwrap_err();
        assert!(died.contains("ended at once (exit code 3)") && died.contains("cannot find module"), "{died}");
        assert!(apps.view("t1").is_none());
        let started = Instant::now();
        let silent = start(&apps, "t1", &folder, "echo waiting; sleep 30", false, Duration::from_millis(600), || {}).await.unwrap_err();
        assert!(silent.contains("did not answer on port") && silent.contains("waiting"), "{silent}");
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(apps.view("t1").is_none());
        // A server that chose its own port and printed it is found by that line.
        let other = free_port().unwrap();
        let command = format!("echo 'Local: http://localhost:{other}/'; exec python3 -m http.server {other} --bind 127.0.0.1");
        let found = start(&apps, "t2", &folder, &command, false, Duration::from_secs(30), || {}).await.unwrap();
        assert_eq!(found.port, other);
        apps.stop_all();
        gone(other).await;
    }
}
