//! Clips the app's webview can play. The video engine writes WebM with VP8 pictures and PCM sound.
//! WebKit on macOS plays neither PCM in WebM nor decodes it for Web Audio, so on macOS every made
//! clip gets a copy next to it, `<stem>.play.mp4`: H.264 from the hardware encoder at a constant
//! quality that looks like the clip, AAC at 48 kHz, its index first so it starts at once. It is
//! made right after the clip, or on the first request for a clip from before.
//!
//! The WebM stays the master: exports, waveforms, last pictures and listening read it, so nothing
//! the film is made of goes through a second encoding. On Windows the webview is Chromium and on
//! Linux WebKitGTK plays through GStreamer; both play the WebM as it is, and no copy is made.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::{App, Shared};

/// Whether this platform's webview needs a copy of a made clip.
pub const NEEDED: bool = cfg!(target_os = "macos");

/// Copies made at the same time, across all files: the hardware encoder has few sessions.
static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// One lock for each copy being made: a second request for the same clip waits for the first.
static MAKING: Mutex<Vec<(PathBuf, Arc<tokio::sync::Mutex<()>>)>> = Mutex::new(Vec::new());

/// `<dir>/<stem>.play.mp4` for `<dir>/<stem>.webm`. Any other file plays as it is.
pub fn copy_of(clip: &Path) -> Option<PathBuf> {
    let stem = clip.file_name()?.to_str()?.strip_suffix(".webm")?;
    Some(clip.with_file_name(format!("{stem}.play.mp4")))
}

/// What a clip file is, by its ending.
pub fn kind(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()) {
        Some("mp4") => "video/mp4",
        Some("webp") => "image/webp",
        _ => "video/webm",
    }
}

/// A copy that is there and not older than its clip.
fn fresh(clip: &Path, copy: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((modified(clip), modified(copy)), (Some(clip), Some(copy)) if copy >= clip)
}

/// What ffmpeg is told: the first picture and sound streams, H.264 in hardware at constant quality
/// 85 (about 48 dB PSNR against the clip; a keyframe every half second, so the viewer seeks fast),
/// AAC 48 kHz stereo at 192 kbit/s, the index at the front.
pub fn args(clip: &Path, out: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-i"].iter().map(Into::into).collect();
    args.push(clip.into());
    args.extend(["-map", "0:v:0", "-map", "0:a:0?"].iter().map(Into::into));
    args.extend(
        ["-c:v", "h264_videotoolbox", "-q:v", "85", "-profile:v", "high", "-pix_fmt", "yuv420p", "-g", "12"].iter().map(Into::into),
    );
    args.extend(["-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2"].iter().map(Into::into));
    args.extend(["-movflags", "+faststart", "-f", "mp4"].iter().map(Into::into));
    args.push(out.into());
    args
}

fn lock_for(copy: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let mut all = MAKING.lock().unwrap();
    if let Some((_, lock)) = all.iter().find(|(p, _)| p == copy) {
        return lock.clone();
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    all.push((copy.to_path_buf(), lock.clone()));
    lock
}

/// Forgets the locks nobody holds or waits for.
fn tidy_locks() {
    MAKING.lock().unwrap().retain(|(_, lock)| Arc::strong_count(lock) > 1);
}

/// The playable copy of a clip, made first when it is missing or older than the clip. `None` when
/// none is needed here, or it could not be made (the clip is then played as it is).
pub async fn make(app: &App, clip: &Path) -> Option<PathBuf> {
    if !NEEDED {
        return None;
    }
    let copy = copy_of(clip)?;
    let lock = lock_for(&copy);
    let made = {
        let _mine = lock.lock().await;
        if fresh(clip, &copy) {
            Some(copy)
        } else if clip.is_file() {
            write(app, clip, &copy).await.then_some(copy)
        } else {
            None
        }
    };
    drop(lock);
    tidy_locks();
    made
}

/// Writes the copy under a temporary name and renames it, so a copy that is there is whole.
async fn write(app: &App, clip: &Path, copy: &Path) -> bool {
    let Ok(_slot) = SLOTS.acquire().await else { return false };
    let Ok(ffmpeg) = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await else { return false };
    let Some(name) = copy.file_name().and_then(|n| n.to_str()) else { return false };
    let tmp = copy.with_file_name(format!(".{name}.tmp"));
    let started = std::time::Instant::now();
    let out = tokio::process::Command::new(ffmpeg)
        .args(args(clip, &tmp))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await;
    let why = match out {
        Ok(o) if o.status.success() => match std::fs::rename(&tmp, copy) {
            Ok(()) => return true,
            Err(e) => e.to_string(),
        },
        Ok(o) => String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("ffmpeg failed").chars().take(200).collect(),
        Err(e) => e.to_string(),
    };
    std::fs::remove_file(&tmp).ok();
    app.record("clip_copy_failed", json!({"error": why, "seconds": started.elapsed().as_secs_f32()}));
    false
}

/// Makes the copy of a clip that was just made, in the background.
pub fn soon(app: &Shared, clip: PathBuf) {
    if NEEDED {
        let app = app.clone();
        tokio::spawn(async move { make(&app, &clip).await });
    }
}

/// The file to hand the app for a clip, and its content type: the copy where one is needed, the
/// clip itself elsewhere or when the copy cannot be made.
pub async fn served(app: &App, clip: &Path) -> (PathBuf, &'static str) {
    match make(app, clip).await {
        Some(copy) => (copy, "video/mp4"),
        None => (clip.to_path_buf(), kind(clip)),
    }
}

/// Removes the copy of a clip that goes.
pub fn remove(clip: &Path) {
    if let Some(copy) = copy_of(clip) {
        std::fs::remove_file(copy).ok();
    }
}

/// Moves the copy along with a clip that is renamed.
pub fn rename(from: &Path, to: &Path) {
    if let (Some(from), Some(to)) = (copy_of(from), copy_of(to)) {
        if from.is_file() {
            std::fs::rename(from, to).ok();
        }
    }
}

/// Removes every copy in a folder of clips (a film that is forgotten keeps its clips, not its copies).
pub fn remove_all(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_str().is_some_and(|n| n.ends_with(".play.mp4")) {
            std::fs::remove_file(entry.path()).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_clip_has_its_copy_next_to_it() {
        assert_eq!(copy_of(Path::new("/f/clips/a1.2.webm")), Some(PathBuf::from("/f/clips/a1.2.play.mp4")));
        assert_eq!(copy_of(Path::new("/d/videos/0123abcd.webm")), Some(PathBuf::from("/d/videos/0123abcd.play.mp4")));
        // A clip brought in from a video is an MP4 already; a picture is not a clip.
        assert_eq!(copy_of(Path::new("/f/clips/a1.1.mp4")), None);
        assert_eq!(copy_of(Path::new("/d/videos/x.webp")), None);
        assert_eq!(kind(Path::new("/f/clips/a1.1.mp4")), "video/mp4");
        assert_eq!(kind(Path::new("/f/clips/a1.1.webm")), "video/webm");
    }

    #[test]
    #[cfg(unix)]
    fn the_copy_is_h264_and_aac_with_its_index_first() {
        let line: Vec<String> = args(Path::new("/f/clips/a1.1.webm"), Path::new("/f/clips/.a1.1.play.mp4.tmp"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let line = line.join(" ");
        assert!(line.starts_with("-hide_banner -loglevel error -y -i /f/clips/a1.1.webm -map 0:v:0 -map 0:a:0?"), "{line}");
        for part in
            ["-c:v h264_videotoolbox -q:v 85", "-pix_fmt yuv420p", "-c:a aac -b:a 192k -ar 48000 -ac 2", "-movflags +faststart -f mp4"]
        {
            assert!(line.contains(part), "{part} missing in {line}");
        }
        assert!(line.ends_with("/f/clips/.a1.1.play.mp4.tmp"));
    }

    #[test]
    fn a_copy_older_than_its_clip_is_made_again() {
        let dir = std::env::temp_dir().join(format!("rigbrew-playable-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let (clip, copy) = (dir.join("a.1.webm"), dir.join("a.1.play.mp4"));
        std::fs::write(&clip, b"clip").unwrap();
        assert!(!fresh(&clip, &copy), "no copy yet");
        std::fs::write(&copy, b"copy").unwrap();
        assert!(fresh(&clip, &copy));
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&clip).unwrap().set_modified(later).unwrap();
        assert!(!fresh(&clip, &copy), "the clip changed after its copy");
        // Renamed with its clip, and gone with the folder's other copies.
        rename(&clip, &dir.join("b.1.webm"));
        assert!(!copy.exists() && dir.join("b.1.play.mp4").is_file());
        remove_all(&dir);
        assert!(!dir.join("b.1.play.mp4").exists() && clip.is_file());
        std::fs::remove_dir_all(&dir).ok();
    }
}
