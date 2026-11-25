//! Crash reports. When the service or the desktop shell panics, a short record is left in
//! `<data dir>/crashes/`; the service turns each one into a `crash` event the next time it starts.
//!
//! A record says what failed and where in the code: the panic message, the source location, the
//! program, its version and the thread. It never holds what was typed or made, and every string
//! is short enough to pass as basic usage data. Home folders in paths are replaced with `~`.

use std::path::{Path, PathBuf};

const KEEP: usize = 120;

fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join("crashes")
}

fn short(text: &str) -> String {
    let mut text = text.replace('\n', " ");
    if let Some(home) = dirs::home_dir().and_then(|h| h.to_str().map(str::to_owned)).filter(|h| h.len() > 1) {
        text = text.replace(&home, "~");
    }
    // A dependency's path is long and the same on every machine up to the crate's name.
    if let Some(at) = text.find("/registry/src/") {
        if let Some(rest) = text[at + 14..].split_once('/') {
            text = rest.1.to_string();
        }
    }
    text.chars().take(KEEP).collect()
}

/// Leaves a record in `<data_dir>/crashes/` whenever this program panics, then lets the panic go on.
pub fn install(data_dir: PathBuf, program: &'static str, version: &'static str) {
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message =
            info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned());
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
        let record = serde_json::json!({
            "program": program,
            "version": version,
            "message": short(message.as_deref().unwrap_or("panic")),
            "location": short(location.as_deref().unwrap_or("")),
            "thread": short(std::thread::current().name().unwrap_or("unnamed")),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "at_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        });
        let dir = folder(&data_dir);
        if std::fs::create_dir_all(&dir).is_ok() {
            let name = format!("{}-{}-{}.json", program, record["at_ms"], std::process::id());
            std::fs::write(dir.join(name), record.to_string()).ok();
        }
        earlier(info);
    }));
}

/// The records left by earlier runs, oldest first; each is removed as it is read. At most 20 are
/// returned: a program that crashes in a loop does not flood the queue.
pub fn take(data_dir: &Path) -> Vec<serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(folder(data_dir)) else { return vec![] };
    let mut files: Vec<PathBuf> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let mut out = vec![];
    for path in files {
        if out.len() < 20 {
            if let Some(v) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .filter(|v| v.is_object())
            {
                out.push(v);
            }
        }
        std::fs::remove_file(&path).ok();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_leaves_a_record_that_is_read_once() {
        let dir = std::env::temp_dir().join(format!("rigbrew-crash-{}", uuid::Uuid::now_v7()));
        install(dir.clone(), "test", "0.0.0");
        let result = std::thread::Builder::new().name("worker".into()).spawn(|| panic!("the kettle boiled dry")).unwrap().join();
        // Other tests panic on purpose too; give the hook back before looking.
        let _ = std::panic::take_hook();
        assert!(result.is_err());
        let found = take(&dir);
        let mine: Vec<_> = found.iter().filter(|r| r["message"] == "the kettle boiled dry").collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0]["program"], "test");
        assert_eq!(mine[0]["thread"], "worker");
        assert!(mine[0]["location"].as_str().unwrap().contains("crash.rs"));
        assert!(take(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paths_lose_the_home_folder_and_long_text_is_cut() {
        let home = dirs::home_dir().unwrap();
        let s = short(&format!("{}/Library/x.rs:3 {}", home.display(), "y".repeat(400)));
        assert!(s.starts_with("~/Library/x.rs:3"));
        assert_eq!(s.chars().count(), KEEP);
        assert_eq!(
            short("/Users/a/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.40.0/src/runtime/x.rs:9"),
            "tokio-1.40.0/src/runtime/x.rs:9"
        );
    }
}
