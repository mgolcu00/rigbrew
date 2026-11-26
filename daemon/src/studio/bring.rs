//! Bringing a video file into a film. The file is read with the packaged ffmpeg, cut into shots
//! where its scenes change (and at a fixed length where they do not), and every shot is written
//! into the film's folder at the film's shape, with its own sound. What comes in are ordinary
//! shots: they are trimmed, moved, dubbed and subtitled like made ones, and a new shot can
//! carry on from the last picture of one.
//!
//!   POST /v1/studio/projects/{id}/import   {"path"}: start bringing a video file in
//!   GET  /v1/studio/projects/{id}/import   where that is: {"state", "stage", "done", "of", "shots", "error"}

use std::path::{Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::Modality;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{job_of, job_running, load, look_at, new_id, set_job, store, ExportOptions, Shot, Take, MAX_SHOTS};
use crate::{fail, App, Shared};

/// The shortest and the longest shot a video is cut into, in seconds, and the most shots one file gives.
const SHORTEST: f32 = 1.0;
const LONGEST: f32 = 8.0;
const MOST: usize = 120;
/// How different two pictures have to be for a cut to lie between them, 0 to 1.
const SCENE: f32 = 0.3;

/// What ffmpeg says about a video file.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub seconds: f32,
    /// As it is shown: a file that is stored on its side is already turned here.
    pub width: u32,
    pub height: u32,
    pub has_sound: bool,
}

/// Reads what `ffmpeg -i <file>` prints. `None` for a file without pictures or without a length.
pub fn source(listing: &str) -> Option<Source> {
    let duration = listing.split("Duration:").nth(1)?.split(',').next()?.trim();
    let mut parts = duration.split(':');
    let (h, m, s) = (parts.next()?.parse::<f32>().ok()?, parts.next()?.parse::<f32>().ok()?, parts.next()?.parse::<f32>().ok()?);
    let seconds = h * 3600.0 + m * 60.0 + s;
    let video = listing.lines().find(|l| l.contains("Stream #") && l.contains("Video:") && !l.contains("attached pic"))?;
    let size = regex::Regex::new(r"[ ,](\d{2,5})x(\d{2,5})[ ,\[]").ok()?.captures(video)?;
    let (mut width, mut height) = (size[1].parse::<u32>().ok()?, size[2].parse::<u32>().ok()?);
    // A phone holds an upright video on its side and says so: it is shown turned.
    if ["rotation of -90", "rotation of 90", "rotation of 270", "rotation of -270"].iter().any(|r| listing.contains(r)) {
        (width, height) = (height, width);
    }
    (seconds > 0.0 && width > 0 && height > 0).then(|| Source {
        seconds,
        width,
        height,
        has_sound: listing.lines().any(|l| l.contains("Stream #") && l.contains("Audio:")),
    })
}

/// The moments a scene changes, out of what ffmpeg's `showinfo` printed for the pictures that
/// differ enough from the one before them.
pub fn scene_times(log: &str) -> Vec<f32> {
    log.lines()
        .filter(|l| l.contains("showinfo"))
        .filter_map(|l| l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse::<f32>().ok())
        .filter(|t| t.is_finite() && *t > 0.0)
        .collect()
}

/// Where a video of `seconds` is cut: at its scene changes, never leaving a shot shorter than
/// `shortest`; a scene longer than `longest` in equal parts; at most `most` shots. Answers with
/// each shot's start and length, and whether the end of the video was left out.
pub fn cut_plan(seconds: f32, scenes: &[f32], shortest: f32, longest: f32, most: usize) -> (Vec<(f32, f32)>, bool) {
    if !(seconds.is_finite() && seconds > 0.0) || most == 0 {
        return (Vec::new(), false);
    }
    let mut cuts: Vec<f32> = scenes.iter().copied().filter(|t| t.is_finite() && *t > 0.0 && *t < seconds).collect();
    cuts.sort_by(f32::total_cmp);
    let mut bounds = vec![0.0f32];
    for cut in cuts {
        if cut - bounds[bounds.len() - 1] >= shortest && seconds - cut >= shortest {
            bounds.push(cut);
        }
    }
    bounds.push(seconds);
    let mut out: Vec<(f32, f32)> = Vec::new();
    for pair in bounds.windows(2) {
        let (from, length) = (pair[0], pair[1] - pair[0]);
        let parts = (length / longest).ceil().max(1.0);
        for n in 0..parts as usize {
            out.push((from + length * n as f32 / parts, length / parts));
        }
    }
    let left_out = out.len() > most;
    out.truncate(most);
    (out, left_out)
}

/// The film shape closest to a video's: the one whose width to height is nearest.
pub fn film_size(width: u32, height: u32, sizes: &[(u32, u32)]) -> (u32, u32) {
    let shape = (width as f32 / height.max(1) as f32).ln();
    sizes
        .iter()
        .copied()
        .min_by(|a, b| {
            let away = |s: &(u32, u32)| ((s.0 as f32 / s.1.max(1) as f32).ln() - shape).abs();
            away(a).total_cmp(&away(b))
        })
        .unwrap_or((640, 384))
}

/// Whether a video has the pictures for a clip of twice the film's width and height.
pub fn is_large(source: &Source, film: (u32, u32)) -> bool {
    source.width as f32 >= film.0 as f32 * 1.8 && source.height as f32 >= film.1 as f32 * 1.8
}

/// The kinds of video file Studio brings in, by the end of their name.
pub fn is_video(path: &FsPath) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "mpg" | "mpeg" | "ts"))
}

/// What ffmpeg is told to find the scene changes of a file: small pictures, only the ones that
/// differ enough from the one before, each printed with its time.
pub fn detect_args(file: &FsPath) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> =
        ["-hide_banner", "-loglevel", "info", "-nostats", "-t", "3600", "-i"].iter().map(Into::into).collect();
    args.push(file.into());
    args.extend(["-map", "0:v:0", "-an", "-vf"].iter().map(Into::into));
    args.push(format!("scale=320:-2,select='gt(scene,{SCENE})',showinfo").into());
    args.extend(["-f", "null", "-"].iter().map(Into::into));
    args
}

/// What ffmpeg is told to write one shot: a stretch of the file at the film's shape (filled, the
/// overhang cut away), 24 pictures a second, in the form the film's player and its export read.
pub fn cut_args(file: &FsPath, start: f32, length: f32, size: (u32, u32), bits: (u32, u32), out: &FsPath) -> Vec<std::ffi::OsString> {
    let (w, h) = size;
    let mut args: Vec<std::ffi::OsString> = ["-hide_banner", "-loglevel", "error", "-y", "-ss"].iter().map(Into::into).collect();
    args.push(format!("{start:.3}").into());
    args.extend(["-t".into(), format!("{length:.3}").into(), "-i".into(), file.into()]);
    args.extend(["-map", "0:v:0", "-map", "0:a:0?", "-vf"].iter().map(Into::into));
    args.push(
        format!("fps=24,scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h},setsar=1,format=yuv420p").into(),
    );
    args.extend(["-c:v", "h264_videotoolbox", "-b:v"].iter().map(Into::into));
    args.push(bits.0.to_string().into());
    args.extend(["-g", "24", "-c:a", "aac", "-b:a"].iter().map(Into::into));
    args.push(bits.1.to_string().into());
    args.extend(["-ar", "48000", "-ac", "2", "-movflags", "+faststart", "-f", "mp4"].iter().map(Into::into));
    args.push(out.into());
    args
}

/// The shapes the video model makes, for a film that takes its shape from a video.
fn shapes(app: &App) -> Vec<(u32, u32)> {
    let found: Vec<(u32, u32)> = app
        .manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video)
        .max_by_key(|pk| !app.installed(pk).is_empty())
        .and_then(|pk| pk.defaults["sizes"].as_object())
        .map(|sizes| sizes.values().filter_map(|s| Some((s[0].as_u64()? as u32, s[1].as_u64()? as u32))).collect())
        .unwrap_or_default();
    let found: Vec<(u32, u32)> = found.into_iter().filter(|(w, h)| super::valid_size(&format!("{w}x{h}"))).collect();
    if found.is_empty() {
        vec![(640, 384), (384, 640), (512, 512)]
    } else {
        found
    }
}

#[derive(Deserialize)]
pub(super) struct Import {
    path: String,
}

pub(super) async fn state(Path(id): Path<String>) -> Json<Value> {
    Json(job_of(&id, "import"))
}

pub(super) async fn import(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Import>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let path = PathBuf::from(body.path.trim());
    if !path.is_absolute() || body.path.len() > 1024 {
        return fail(StatusCode::BAD_REQUEST, "Give the video file by its full path.");
    }
    if !path.is_file() {
        return fail(StatusCode::NOT_FOUND, "Nothing was found at that path.");
    }
    if !is_video(&path) {
        return fail(StatusCode::BAD_REQUEST, "That is not a video file Studio can bring in. Use mp4, mov, m4v, mkv, webm or avi.");
    }
    if p.shots.len() >= MAX_SHOTS {
        return fail(StatusCode::CONFLICT, format!("A film has at most {MAX_SHOTS} shots."));
    }
    if job_running(&id, "import") {
        return fail(StatusCode::CONFLICT, "A video is being brought into this film already.");
    }
    let ffmpeg = match crate::engine_tool(&app, crate::EngineKind::Ffmpeg, "ffmpeg").await {
        Ok(tool) => tool,
        Err(e) => return fail(StatusCode::SERVICE_UNAVAILABLE, format!("The tool that reads video files could not be set up: {e}")),
    };
    set_job(&id, "import", json!({"state": "working", "stage": "reading"}));
    let app2 = app.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match run(&app2, &id, &path, &ffmpeg).await {
            Ok((shots, left_out, seconds)) => {
                app2.record(
                    "studio_video_imported",
                    json!({"shots": shots, "film_seconds": seconds, "seconds": started.elapsed().as_secs_f32(), "left_out": left_out}),
                );
                set_job(&id, "import", json!({"state": "done", "shots": shots, "left_out": left_out}));
            }
            Err(e) => {
                app2.record("studio_video_import_failed", json!({}));
                set_job(&id, "import", json!({"state": "failed", "error": e}));
            }
        }
    });
    (StatusCode::ACCEPTED, Json(json!({"state": "working", "stage": "reading"}))).into_response()
}

async fn tool(ffmpeg: &FsPath, args: Vec<std::ffi::OsString>) -> Result<std::process::Output, String> {
    tokio::process::Command::new(ffmpeg)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| e.to_string())
}

/// Cuts the file into shots and puts them at the end of the film. Answers with how many shots
/// came in, whether the end of the video was left out, and the seconds that came in.
async fn run(app: &Shared, id: &str, file: &FsPath, ffmpeg: &FsPath) -> Result<(usize, bool, f32), String> {
    let listed = tool(ffmpeg, vec!["-hide_banner".into(), "-i".into(), file.into()]).await?;
    let src = source(&String::from_utf8_lossy(&listed.stderr)).ok_or("that file has no pictures Studio can read")?;
    let mut p = load(app, id).ok_or("the film is gone")?;
    // An empty film takes the shape of the video it starts from.
    if p.shots.is_empty() {
        let (w, h) = film_size(src.width, src.height, &shapes(app));
        p.size = format!("{w}x{h}");
        store(&mut p).map_err(|e| e.to_string())?;
    }
    let film = p.size.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).unwrap_or((640, 384));
    let large = is_large(&src, film);
    let size = if large { (film.0 * 2, film.1 * 2) } else { film };
    // As good as a balanced export of that size, and no more than a camera writes.
    let bits = ExportOptions::default().bitrates(&format!("{}x{}", size.0, size.1));
    let bits = (bits.0.min(16_000_000), bits.1);
    set_job(id, "import", json!({"state": "working", "stage": "looking"}));
    let seen = tool(ffmpeg, detect_args(file)).await?;
    let scenes = scene_times(&String::from_utf8_lossy(&seen.stderr));
    let room = MAX_SHOTS.saturating_sub(p.shots.len()).min(MOST);
    let (plan, left_out) = cut_plan(src.seconds.min(3600.0), &scenes, SHORTEST, LONGEST, room);
    if plan.is_empty() {
        return Err("that video is too short to bring in".into());
    }
    let dir = FsPath::new(&p.folder).join("clips");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut shots: Vec<Shot> = Vec::new();
    let mut last_error = String::new();
    for (n, (start, length)) in plan.iter().enumerate() {
        set_job(id, "import", json!({"state": "working", "stage": "cutting", "done": n, "of": plan.len()}));
        let shot = new_id();
        let (clip, picture) = (dir.join(format!("{shot}.1.mp4")), dir.join(format!("{shot}.1.jpg")));
        let cut = tool(ffmpeg, cut_args(file, *start, *length, size, bits, &clip)).await?;
        let frames = if cut.status.success() { look_at(app, &clip, &picture).await } else { None };
        let Some(frames) = frames.filter(|f| *f >= 5) else {
            last_error = String::from_utf8_lossy(&cut.stderr).lines().last().unwrap_or("").chars().take(200).collect();
            std::fs::remove_file(&clip).ok();
            std::fs::remove_file(&picture).ok();
            continue;
        };
        let take = Take { n: 1, frames, made_frames: Some(frames), ext: "mp4".into(), large, ..Take::default() };
        shots.push(Shot {
            id: shot,
            frames,
            state: "done".into(),
            made_frames: Some(frames),
            takes: vec![take],
            take: 1,
            ..Shot::default()
        });
    }
    if shots.is_empty() {
        return Err(if last_error.is_empty() { "the video could not be cut into shots".into() } else { last_error });
    }
    // The film was open while the video was cut: take it as it is now.
    let mut p = load(app, id).ok_or("the film is gone")?;
    let (count, seconds) = (shots.len(), shots.iter().map(Shot::length).sum());
    if large && p.shots.iter().all(|s| s.takes.is_empty()) {
        // The clips hold twice the film's width and height: the export keeps them.
        p.export.scale = 2;
    }
    p.shots.extend(shots);
    store(&mut p).map_err(|e| e.to_string())?;
    Ok((count, left_out, seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'trip.mov':\n  Metadata:\n    major_brand     : qt  \n  Duration: 00:01:02.50, start: 0.000000, bitrate: 8123 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 7990 kb/s, 30 fps, 30 tbr, 15360 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 128 kb/s (default)\nAt least one output file must be specified";

    #[test]
    fn a_video_file_says_what_it_is() {
        assert_eq!(source(LISTING), Some(Source { seconds: 62.5, width: 1920, height: 1080, has_sound: true }));
        // A phone's upright video is stored on its side.
        let phone = LISTING
            .replace("Audio: aac", "Data: none")
            .replace("(default)\nAt", "(default)\n      displaymatrix: rotation of -90.00 degrees\nAt");
        assert_eq!(source(&phone), Some(Source { seconds: 62.5, width: 1080, height: 1920, has_sound: false }));
        assert_eq!(source("Input #0, mp3, from 'song.mp3':\n  Duration: 00:03:00.00\n  Stream #0:0: Audio: mp3, 44100 Hz"), None);
        assert_eq!(source("trip.mov: No such file or directory"), None);
        assert!(is_video(FsPath::new("/a/Trip.MOV")) && !is_video(FsPath::new("/a/song.mp3")) && !is_video(FsPath::new("/a/mp4")));
    }

    #[test]
    fn scene_changes_are_read_from_what_the_tool_prints() {
        let log = "[Parsed_showinfo_2 @ 0x1] config in time_base: 1/15360, frame_rate: 30/1\n\
                   [Parsed_showinfo_2 @ 0x1] n:   0 pts:  61440 pts_time:4       duration:    512 fmt:yuv420p\n\
                   [Parsed_showinfo_2 @ 0x1] n:   1 pts: 190464 pts_time:12.4    duration:    512 fmt:yuv420p\n\
                   frame=    2 fps=0.0 q=-0.0 Lsize=N/A time=00:00:12.43 bitrate=N/A speed= 120x\n";
        assert_eq!(scene_times(log), [4.0, 12.4]);
        assert!(scene_times("nothing").is_empty());
        let args: Vec<String> = detect_args(FsPath::new("/a/trip.mov")).iter().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"scale=320:-2,select='gt(scene,0.3)',showinfo".to_string()) && args.contains(&"/a/trip.mov".to_string()));
    }

    #[test]
    fn a_video_is_cut_at_its_scenes() {
        // Cuts at 4 and 12.4; the one at 12.9 would leave a shot of half a second, the one at 19.6 an end too short.
        let (plan, left_out) = cut_plan(20.0, &[12.4, 4.0, 12.9, 19.6, 25.0], 1.0, 8.0, 120);
        assert!(!left_out);
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0], (0.0, 4.0));
        // The scene from 4 to 12.4 is longer than a shot may be: two equal parts.
        assert!((plan[1].0 - 4.0).abs() < 1e-4 && (plan[1].1 - 4.2).abs() < 1e-4 && (plan[2].0 - 8.2).abs() < 1e-4);
        assert!((plan[3].0 - 12.4).abs() < 1e-4 && (plan[3].1 - 7.6).abs() < 1e-4);
        // The shots follow each other without a gap and cover the video.
        let end = plan.iter().fold(0.0, |at, (start, length)| {
            assert!((start - at).abs() < 1e-3);
            start + length
        });
        assert!((end - 20.0f32).abs() < 1e-3);
    }

    #[test]
    fn a_video_without_scene_changes_is_cut_at_a_fixed_length() {
        let (plan, left_out) = cut_plan(60.0, &[], 1.0, 8.0, 120);
        assert_eq!(plan.len(), 8);
        assert!(plan.iter().all(|(_, length)| (*length - 7.5).abs() < 1e-4));
        assert!(!left_out);
        // No more shots than the film has room for: the rest of the video is left out, and that is said.
        let (plan, left_out) = cut_plan(600.0, &[], 1.0, 8.0, 10);
        assert_eq!((plan.len(), left_out), (10, true));
        assert_eq!(cut_plan(0.4, &[], 1.0, 8.0, 10).0, [(0.0, 0.4)]);
        assert!(cut_plan(0.0, &[], 1.0, 8.0, 10).0.is_empty() && cut_plan(f32::NAN, &[], 1.0, 8.0, 10).0.is_empty());
    }

    #[test]
    fn a_film_takes_the_shape_closest_to_the_video() {
        let sizes = [(640, 384), (384, 640), (512, 512)];
        assert_eq!(film_size(1920, 1080, &sizes), (640, 384));
        assert_eq!(film_size(1080, 1920, &sizes), (384, 640));
        assert_eq!(film_size(1000, 1000, &sizes), (512, 512));
        assert_eq!(film_size(1200, 1080, &sizes), (512, 512));
        assert_eq!(film_size(1920, 1080, &[]), (640, 384));
        let hd = Source { seconds: 1.0, width: 1920, height: 1080, has_sound: true };
        assert!(is_large(&hd, (640, 384)) && !is_large(&Source { width: 854, height: 480, ..hd }, (640, 384)));
    }

    #[test]
    fn a_shot_is_cut_in_the_form_the_film_reads() {
        let args: Vec<String> =
            cut_args(FsPath::new("/a/trip.mov"), 4.0, 4.2, (1280, 768), (20_000_000, 256_000), FsPath::new("/f/clips/s1.1.mp4"))
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect();
        let line = args.join(" ");
        assert!(line.starts_with("-hide_banner -loglevel error -y -ss 4.000 -t 4.200 -i /a/trip.mov -map 0:v:0 -map 0:a:0? -vf fps=24,scale=1280:768:force_original_aspect_ratio=increase:flags=lanczos,crop=1280:768,setsar=1,format=yuv420p"));
        assert!(line.ends_with(
            "-c:v h264_videotoolbox -b:v 20000000 -g 24 -c:a aac -b:a 256000 -ar 48000 -ac 2 -movflags +faststart -f mp4 /f/clips/s1.1.mp4"
        ));
    }
}
