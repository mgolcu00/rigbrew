//! What is said in a film, made heard and read: the dub, the lines written down from a video
//! that was brought in, and the subtitles.
//!
//!   POST /v1/studio/projects/{id}/dub        {"language"?}: the voice reads every spoken line; the readings go on
//!                                            a voice track of that language, at each shot's place in the film. A
//!                                            language other than the film's own has the chat model translate first.
//!   GET  /v1/studio/projects/{id}/dub        where that is: {"state", "stage", "done", "of", "language", "error"}
//!   POST /v1/studio/projects/{id}/listen     {"shots"?}: the listening model writes down what is said in the clips
//!                                            of the shots that have no lines yet
//!   GET  /v1/studio/projects/{id}/listen     where that is
//!   GET  /v1/studio/projects/{id}/subtitles  the subtitles as the export holds them: {"cues", "key", "width",
//!                                            "height", "language", "drawn"}
//!   PUT  /v1/studio/projects/{id}/captions?key=&n=&of=&w=&h=   one subtitle as a picture (raw RGBA, w*h*4 bytes),
//!                                            drawn by the app, for an export that burns the subtitles in

use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rigbrew_core::manifest::Modality;
use serde::Deserialize;
use serde_json::{json, Value};

use super::sound::{self, Region, Track};
use super::words::{self, Cue, Scene, Spoken};
use super::{ask, clip_path, exported, has_sound, job_of, job_running, load, need, new_id, own_port, set_job, store, Project, Shot};
use crate::{fail, App, Shared};

/// Where each shot of a film starts and how long it is on the timeline: every shot, made or
/// not, with its trims, a dissolve laying its start over the end of the shot before it. The
/// same arithmetic as the app's timeline, counted in frames.
pub fn film_times(shots: &[Shot]) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(shots.len());
    let mut before: Option<(f32, u32)> = None;
    for s in shots {
        let made = s.state == "done" || s.state == "changed";
        let whole = s.length();
        let kept = if made { (whole - s.trim_start.max(0.0) - s.trim_end.max(0.0)).max(0.1) } else { whole };
        let frames = ((kept * 24.0).round() as u32).max(1);
        let start = match before {
            None => 0.0,
            Some((end, frames_before)) => {
                let dissolves = !s.continues && s.join == "dissolve";
                let asked = (s.join_seconds.clamp(0.25, 2.0) * 24.0).round() as u32;
                let shared = if dissolves { asked.min(frames_before / 2).min(frames / 2) } else { 0 };
                end - shared as f32 / 24.0
            }
        };
        let length = frames as f32 / 24.0;
        out.push((start, length));
        before = Some((start + length, frames));
    }
    out
}

/// The language a film's lines are written in.
pub fn own_language(p: &Project) -> String {
    let named = words::language(&p.language);
    if named.is_empty() {
        "English".into()
    } else {
        named
    }
}

/// The language that is heard and read now: the dub that was chosen, else the film's own.
pub fn shown_language(p: &Project) -> String {
    let chosen = words::language(&p.dub);
    if chosen.is_empty() {
        own_language(p)
    } else {
        chosen
    }
}

/// The languages the installed voice reads. A line in another language is not read aloud: the
/// voice leaves out the words it does not know, and what is left is no reading.
pub const SPOKEN: [&str; 2] = ["English", "Chinese"];

pub fn voice_reads(language: &str) -> bool {
    SPOKEN.iter().any(|l| l.eq_ignore_ascii_case(language.trim()))
}

/// The language whose voice track is heard: the one that is shown when it has a track, else the
/// film's own (a film shown in a language the voice does not read is heard as it was written).
pub fn heard_language(p: &Project) -> String {
    let shown = shown_language(p);
    if p.tracks.iter().any(|t| t.kind == "voice" && t.lang.eq_ignore_ascii_case(&shown)) {
        shown
    } else {
        own_language(p)
    }
}

/// One language is heard at a time: its voice track is on, the others are muted.
pub fn hear_one(p: &mut Project) {
    let heard = heard_language(p);
    if p.tracks.iter().any(|t| t.kind == "voice" && t.lang.eq_ignore_ascii_case(&heard)) {
        for t in p.tracks.iter_mut().filter(|t| t.kind == "voice") {
            t.mute = !t.lang.eq_ignore_ascii_case(&heard);
        }
    }
}

/// The readings on the voice track that is heard: which line, where it starts, how long it is.
fn readings(p: &Project) -> Vec<(String, f32, f32)> {
    let heard = heard_language(p);
    p.tracks
        .iter()
        .filter(|t| t.kind == "voice" && t.lang.eq_ignore_ascii_case(&heard))
        .flat_map(|t| t.regions.iter().filter(|r| !r.line.is_empty()).map(|r| (r.line.clone(), r.start, r.length)))
        .collect()
}

/// The subtitles of a film in the language that is shown. With `only_made`, as the export holds
/// them: the shots without a clip are not in the film, and what follows them comes earlier.
pub fn subtitles(p: &Project, only_made: bool) -> Vec<Cue> {
    let language = shown_language(p);
    let shots: Vec<Shot> = if only_made { exported(p).into_iter().cloned().collect() } else { p.shots.clone() };
    let times = film_times(&shots);
    let scenes: Vec<Scene> =
        shots.iter().zip(&times).map(|(s, (start, length))| Scene { id: &s.id, start: *start, length: *length, lines: &s.lines }).collect();
    // A reading that runs past the last shot is cut off there, and so is its subtitle.
    let end = times.last().map_or(0.0, |(start, length)| start + length);
    let mut cues = words::cues(&scenes, &readings(p), &language);
    cues.retain(|c| c.start < end - 0.05);
    cues.iter_mut().for_each(|c| c.end = c.end.min(end));
    cues
}

fn hash(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.bytes().chain([0]) {
            hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

/// How many voices the installed voice has: its file holds one block of numbers for each.
pub fn voices(app: &App) -> u32 {
    let Some(package) = app.manifest.packages.iter().find(|pk| pk.modality == Modality::Voice && !app.installed(pk).is_empty()) else {
        return 0;
    };
    let bundle = app.installed(package).into_iter().next().and_then(|v| v.files.iter().find(|f| rigbrew_core::bundle::is_bundle(f)));
    let Some(bundle) = bundle else { return 0 };
    let bytes = std::fs::metadata(rigbrew_core::bundle::dir(&app.packages_dir(), bundle).join("voices.bin")).map(|m| m.len()).unwrap_or(0);
    // 510 lengths of 256 numbers, four bytes each.
    ((bytes / (510 * 256 * 4)) as u32).clamp(1, 999)
}

#[derive(Deserialize, Default)]
pub(super) struct DubBody {
    #[serde(default)]
    language: Option<String>,
}

pub(super) async fn dub_state(Path(id): Path<String>) -> Json<Value> {
    Json(job_of(&id, "dub"))
}

pub(super) async fn dub(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<DubBody>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    if p.shots.iter().all(|s| s.lines.is_empty()) {
        return fail(StatusCode::CONFLICT, "This film has no spoken lines yet. Write a line under a shot first.");
    }
    let own = own_language(&p);
    let language = body.and_then(|b| b.0.language).map(|l| words::language(&l)).filter(|l| !l.is_empty()).unwrap_or_else(|| own.clone());
    let translated = !language.eq_ignore_ascii_case(&own);
    // A language the voice does not read is translated for the subtitles, and needs no voice.
    let voice = match need(&app, Modality::Voice) {
        Ok(package) => package.id.clone(),
        Err(_) if translated && !voice_reads(&language) => String::new(),
        Err(refusal) => return *refusal,
    };
    if translated {
        if let Err(refusal) = need(&app, Modality::Chat) {
            return *refusal;
        }
    }
    if job_running(&id, "dub") {
        return fail(StatusCode::CONFLICT, "This film is being dubbed already.");
    }
    set_job(&id, "dub", json!({"state": "working", "stage": if translated { "translating" } else { "reading" }, "language": language}));
    let app2 = app.clone();
    let answer = Json(job_of(&id, "dub"));
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match run_dub(&app2, &id, &language, translated, &voice).await {
            Ok((lines, seconds)) => {
                app2.record(
                    "studio_dubbed",
                    json!({"lines": lines, "audio_seconds": seconds, "seconds": started.elapsed().as_secs_f32(), "translated": translated}),
                );
                set_job(&id, "dub", json!({"state": "done", "lines": lines, "language": language, "spoken": lines > 0}));
            }
            Err(e) => {
                app2.record("studio_dub_failed", json!({"translated": translated}));
                set_job(&id, "dub", json!({"state": "failed", "error": e, "language": language}));
            }
        }
    });
    (StatusCode::ACCEPTED, answer).into_response()
}

/// Puts the lines that have no words in `language` yet into it, a part of the film at a time.
async fn translate(app: &Shared, id: &str, language: &str) -> Result<(), String> {
    let p = load(app, id).ok_or("the film is gone")?;
    let missing: Vec<(String, String, String)> = p
        .shots
        .iter()
        .flat_map(|s| s.lines.iter().map(move |l| (s, l)))
        .filter(|(_, l)| !l.said.keys().any(|k| k.eq_ignore_ascii_case(language)))
        .map(|(s, l)| (s.id.clone(), l.id.clone(), l.text.clone()))
        .collect();
    let mut found: Vec<(String, String, String)> = Vec::new();
    for part in missing.chunks(20) {
        let texts: Vec<&str> = part.iter().map(|(_, _, text)| text.as_str()).collect();
        let answer = ask(app, &words::translate_prompt(language), &words::translate_request(&texts), 80 * part.len() as u32 + 200, 0.3)
            .await
            .map_err(|(_, why)| why)?;
        for ((shot, line, _), said) in part.iter().zip(words::read_translation(&answer, part.len())) {
            if let Some(said) = said {
                found.push((shot.clone(), line.clone(), said));
            }
        }
    }
    if !missing.is_empty() && found.is_empty() {
        return Err("the chat model did not write a translation".into());
    }
    // The film was open while the lines were translated: take it as it is now.
    let mut p = load(app, id).ok_or("the film is gone")?;
    for (shot, line, said) in found {
        if let Some(l) = p.shots.iter_mut().find(|s| s.id == shot).and_then(|s| s.lines.iter_mut().find(|l| l.id == line)) {
            l.said.insert(language.to_string(), said);
        }
    }
    store(&mut p).map_err(|e| e.to_string())
}

/// The speaker number a line is read with: its character's own voice, else the film's.
fn voice_of(p: &Project, line: &Spoken) -> u32 {
    p.cast.iter().find(|m| m.id == line.who).and_then(|m| m.voice).or(p.voice).unwrap_or(0)
}

/// Reads every line aloud and sets the readings on the voice track of the language.
async fn run_dub(app: &Shared, id: &str, language: &str, translated: bool, voice: &str) -> Result<(usize, f32), String> {
    if translated {
        translate(app, id, language).await?;
    }
    if !voice_reads(language) {
        // The voice does not read this language: the film is read in it (subtitles) and heard as it was.
        let mut p = load(app, id).ok_or("the film is gone")?;
        p.dub = language.to_string();
        hear_one(&mut p);
        store(&mut p).map_err(|e| e.to_string())?;
        return Ok((0, 0.0));
    }
    let p = load(app, id).ok_or("the film is gone")?;
    let port = own_port(app).ok_or("the service does not know its own address")?;
    let dir = FsPath::new(&p.folder).join("voice");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let all: Vec<(&Shot, &Spoken)> = p.shots.iter().flat_map(|s| s.lines.iter().map(move |l| (s, l))).collect();
    // The reading of each line: its file and its length. A line that was read before, in the
    // same words by the same voice, is not read again.
    let mut read: Vec<(String, PathBuf, f32)> = Vec::new();
    for (n, (shot, line)) in all.iter().enumerate() {
        set_job(id, "dub", json!({"state": "working", "stage": "reading", "done": n, "of": all.len(), "language": language}));
        let text = line.text_in(language);
        let speaker = voice_of(&p, line);
        let file = dir.join(format!("{}.wav", hash(&[text, &speaker.to_string()])));
        if !file.is_file() {
            let answer = app
                .http
                .post(format!("http://127.0.0.1:{port}/v1/audio/speech"))
                .bearer_auth(&app.token)
                .json(&json!({"model": voice, "input": text, "voice": speaker}))
                .timeout(Duration::from_secs(600))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !answer.status().is_success() {
                let v: Value = answer.json().await.unwrap_or(Value::Null);
                return Err(v["error"]["message"].as_str().unwrap_or("the voice could not read a line").to_string());
            }
            let wav = answer.bytes().await.map_err(|e| e.to_string())?;
            std::fs::write(&file, &wav).map_err(|e| e.to_string())?;
        }
        let seconds = std::fs::read(&file).ok().and_then(|b| rigbrew_core::voice::wav_seconds(&b)).filter(|s| *s > 0.02);
        if let Some(seconds) = seconds {
            read.push((words::tag(&shot.id, &line.id), file, seconds));
        }
    }
    if read.is_empty() {
        return Err("the voice read nothing".into());
    }
    // The film was open while the voice read: take it as it is now.
    let mut p = load(app, id).ok_or("the film is gone")?;
    let at = match p.tracks.iter().position(|t| t.kind == "voice" && t.lang.eq_ignore_ascii_case(language)) {
        Some(at) => at,
        None if p.tracks.len() >= sound::MAX_TRACKS => return Err("this film has as many sound tracks as it can hold".into()),
        None => {
            p.tracks.push(Track {
                id: new_id(),
                name: format!("{language} voice"),
                kind: "voice".into(),
                lang: language.to_string(),
                color: sound::next_color(p.tracks.len()),
                volume: 1.0,
                pan: 0.0,
                mute: false,
                solo: false,
                regions: Vec::new(),
            });
            p.tracks.len() - 1
        }
    };
    // The readings from before go; what the person set on the track by hand stays.
    p.tracks[at].regions.retain(|r| r.line.is_empty());
    let times = film_times(&p.shots);
    let (mut free, mut total) = (0.0f32, 0.0f32);
    for (shot, (start, _)) in p.shots.clone().iter().zip(&times) {
        let mine: Vec<&(String, PathBuf, f32)> =
            shot.lines.iter().filter_map(|l| read.iter().find(|(tag, _, _)| *tag == words::tag(&shot.id, &l.id))).collect();
        let lengths: Vec<f32> = mine.iter().map(|(_, _, seconds)| *seconds).collect();
        for ((tag, file, seconds), begin) in mine.iter().zip(words::lay(*start, &lengths, free)) {
            p.tracks[at].regions.push(Region {
                id: new_id(),
                file: file.to_string_lossy().to_string(),
                start: begin,
                offset: 0.0,
                length: *seconds,
                gain: 1.0,
                fade_in: 0.0,
                fade_out: 0.0,
                loops: 1,
                line: tag.clone(),
            });
            free = begin + seconds;
            total += seconds;
        }
    }
    if p.tracks[at].regions.len() > sound::MAX_REGIONS {
        return Err("this film has more spoken lines than one track holds".into());
    }
    // One language is heard at a time: the others are muted, and the subtitles follow this one.
    p.dub = if translated { language.to_string() } else { String::new() };
    hear_one(&mut p);
    store(&mut p).map_err(|e| e.to_string())?;
    Ok((read.len(), total))
}

/// The language a listening model names with two letters, by its name.
fn language_named(code: &str) -> &'static str {
    match code {
        "en" => "English",
        "tr" => "Turkish",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "pl" => "Polish",
        "ru" => "Russian",
        "ar" => "Arabic",
        "hi" => "Hindi",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        _ => "",
    }
}

/// Whether a listening model is installed: only then can what is said in a clip be written down.
pub fn can_listen(app: &App) -> bool {
    app.manifest.packages.iter().any(|pk| pk.modality == Modality::Listen && !app.installed(pk).is_empty())
}

#[derive(Deserialize, Default)]
pub(super) struct ListenBody {
    #[serde(default)]
    shots: Option<Vec<String>>,
}

pub(super) async fn listen_state(Path(id): Path<String>) -> Json<Value> {
    Json(job_of(&id, "listen"))
}

pub(super) async fn listen(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<ListenBody>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    if let Err(refusal) = need(&app, Modality::Listen) {
        return *refusal;
    }
    let asked = body.and_then(|b| b.0.shots);
    let wanted: Vec<String> = exported(&p)
        .into_iter()
        .filter(|s| asked.as_ref().map_or(s.lines.is_empty(), |ids| ids.contains(&s.id)))
        .map(|s| s.id.clone())
        .collect();
    if wanted.is_empty() {
        return fail(StatusCode::CONFLICT, "There is no clip without lines to listen to.");
    }
    if job_running(&id, "listen") {
        return fail(StatusCode::CONFLICT, "This film is being listened to already.");
    }
    let ffmpeg = match crate::engine_tool(&app, crate::EngineKind::Ffmpeg, "ffmpeg").await {
        Ok(tool) => tool,
        Err(e) => return fail(StatusCode::SERVICE_UNAVAILABLE, format!("The tool that reads the clips could not be set up: {e}")),
    };
    set_job(&id, "listen", json!({"state": "working", "done": 0, "of": wanted.len()}));
    let app2 = app.clone();
    let answer = Json(job_of(&id, "listen"));
    tokio::spawn(async move {
        match run_listen(&app2, &id, &wanted, &ffmpeg).await {
            Ok(lines) => {
                app2.record("studio_listened", json!({"shots": wanted.len(), "lines": lines}));
                set_job(&id, "listen", json!({"state": "done", "lines": lines, "of": wanted.len()}));
            }
            Err(e) => set_job(&id, "listen", json!({"state": "failed", "error": e})),
        }
    });
    (StatusCode::ACCEPTED, answer).into_response()
}

/// Writes down what is said in one sound file, with the installed listening model.
async fn hear(app: &Shared, wav: &FsPath) -> Result<rigbrew_core::listen::Transcript, String> {
    let package = app
        .manifest
        .packages
        .iter()
        .find(|pk| pk.modality == Modality::Listen && !app.installed(pk).is_empty())
        .ok_or("no listening model is installed")?;
    let variant = app.installed(package).into_iter().next().ok_or("no listening model is installed")?;
    let bundle = variant.files.iter().find(|f| rigbrew_core::bundle::is_bundle(f)).ok_or("the listening model has no model bundle")?;
    let binary = crate::engine_tool(app, crate::EngineKind::Sherpa, rigbrew_core::listen::PROGRAM).await?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
    let model = rigbrew_core::listen::ListenModel { binary, dir: rigbrew_core::bundle::dir(&app.packages_dir(), bundle), threads };
    let _turn = app.listen.lock().await;
    rigbrew_core::listen::transcribe(&model, wav, None).await.map_err(|e| e.to_string())
}

/// Listens to the clip of each shot and writes what is said under it as one line.
async fn run_listen(app: &Shared, id: &str, shots: &[String], ffmpeg: &FsPath) -> Result<usize, String> {
    let p = load(app, id).ok_or("the film is gone")?;
    let dir = app.root.join("tmp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut heard: Vec<(String, String)> = Vec::new();
    let mut language = String::new();
    for (n, shot) in shots.iter().enumerate() {
        set_job(id, "listen", json!({"state": "working", "done": n, "of": shots.len()}));
        let clip = clip_path(&p, shot);
        if !clip.is_file() || !has_sound(ffmpeg, &clip).await {
            continue;
        }
        let wav = dir.join(format!("{}.wav", uuid::Uuid::new_v4().simple()));
        let cut = tokio::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&clip)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-f", "wav"])
            .arg(&wav)
            .stdin(std::process::Stdio::null())
            .status()
            .await;
        let said = if cut.is_ok_and(|s| s.success()) { hear(app, &wav).await } else { Err("the sound of a clip could not be read".into()) };
        std::fs::remove_file(&wav).ok();
        let said = said?;
        if language.is_empty() {
            language = said.language.as_deref().map(language_named).unwrap_or("").to_string();
        }
        let text: String = said.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.chars().filter(|c| c.is_alphanumeric()).count() >= 2 {
            heard.push((shot.clone(), text));
        }
    }
    // The film was open while its clips were listened to: take it as it is now.
    let mut p = load(app, id).ok_or("the film is gone")?;
    let mut lines = 0;
    for (shot, text) in heard {
        if let Some(s) = p.shots.iter_mut().find(|s| s.id == shot) {
            let new = words::merge_lines(vec![words::SpokenIn { text, ..words::SpokenIn::default() }], &[], new_id);
            lines += new.len();
            s.lines = new;
        }
    }
    if p.language.is_empty() && !language.is_empty() {
        p.language = language;
    }
    store(&mut p).map_err(|e| e.to_string())?;
    Ok(lines)
}

/// The size of the film as it is exported.
pub fn export_size(p: &Project, scale: u32) -> (u32, u32) {
    let (w, h) = p.size.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).unwrap_or((640, 384));
    let by = if scale == 2 { 2 } else { 1 };
    (w * by, h * by)
}

/// Where the pictures of one set of subtitles are kept.
fn captions_dir(p: &Project, key: &str) -> PathBuf {
    FsPath::new(&p.folder).join(".captions").join(key)
}

/// What the app drew for a set of subtitles: how many pictures, how wide and how high each is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Drawn {
    pub count: usize,
    pub width: u32,
    pub height: u32,
}

/// The pictures of the subtitles `cues` at this size, when the app drew all of them.
pub fn drawn(p: &Project, cues: &[Cue], size: (u32, u32)) -> Option<(PathBuf, Drawn)> {
    let dir = captions_dir(p, &words::cues_key(cues, size.0, size.1));
    let note: Drawn = serde_json::from_str(&std::fs::read_to_string(dir.join("drawn.json")).ok()?).ok()?;
    let whole = note.count == cues.len() && note.width == size.0 && (2..=size.1).contains(&note.height);
    (whole && (0..note.count).all(|n| dir.join(format!("{n}.pam")).is_file())).then_some((dir, note))
}

/// The list ffmpeg reads the pictures from as one stream: each subtitle for as long as it is on
/// screen, an empty picture in between.
pub fn caption_list(cues: &[Cue], total: f32) -> String {
    let mut list = String::from("ffconcat version 1.0\n");
    let mut at = 0.0f32;
    let gap = |list: &mut String, seconds: f32| {
        if seconds > 0.001 {
            list.push_str(&format!("file 'blank.pam'\nduration {seconds:.3}\n"));
        }
    };
    for (n, cue) in cues.iter().enumerate() {
        gap(&mut list, cue.start - at);
        list.push_str(&format!("file '{n}.pam'\nduration {:.3}\n", (cue.end - cue.start).max(0.04)));
        at = cue.end;
    }
    gap(&mut list, (total - at).max(0.04));
    // The last picture is named once more: a list ends with a file, not with a length.
    list.push_str("file 'blank.pam'\n");
    list
}

/// A picture as ffmpeg reads it without any codec: a few lines of text, then the points.
pub fn pam(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = format!("P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n").into_bytes();
    out.extend_from_slice(rgba);
    out
}

pub(super) async fn subtitles_get(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let cues = subtitles(&p, true);
    let (width, height) = export_size(&p, p.export.scale);
    let key = words::cues_key(&cues, width, height);
    let have = drawn(&p, &cues, (width, height)).is_some();
    Json(json!({"cues": cues, "key": key, "width": width, "height": height, "language": shown_language(&p), "drawn": have})).into_response()
}

#[derive(Deserialize)]
pub(super) struct CaptionQuery {
    key: String,
    n: usize,
    of: usize,
    w: u32,
    h: u32,
}

/// Keeps one subtitle picture. When the last one of a set is in, the set is noted as whole.
pub(super) async fn caption_put(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<CaptionQuery>, body: Bytes) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let key_ok = q.key.len() == 16 && q.key.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if !key_ok || q.of == 0 || q.of > 2000 || q.n >= q.of {
        return fail(StatusCode::BAD_REQUEST, "bad subtitle picture");
    }
    if !(16..=2560).contains(&q.w) || !(2..=1024).contains(&q.h) || body.len() != q.w as usize * q.h as usize * 4 {
        return fail(StatusCode::BAD_REQUEST, "a subtitle picture is w*h*4 bytes of RGBA");
    }
    let dir = captions_dir(&p, &q.key);
    // Pictures of subtitles that are no longer the film's go.
    if let Ok(others) = std::fs::read_dir(dir.parent().unwrap_or(&dir)) {
        for other in others.flatten().filter(|o| o.path() != dir) {
            std::fs::remove_dir_all(other.path()).ok();
        }
    }
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(format!("{}.pam", q.n)), pam(q.w, q.h, &body)))
        .and_then(|()| std::fs::write(dir.join("blank.pam"), pam(q.w, q.h, &vec![0u8; body.len()])));
    if let Err(e) = written {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film's folder cannot be written to: {e}"));
    }
    let whole = (0..q.of).all(|n| dir.join(format!("{n}.pam")).is_file());
    if whole {
        let note = Drawn { count: q.of, width: q.w, height: q.h };
        std::fs::write(dir.join("drawn.json"), serde_json::to_string(&note).unwrap_or_default()).ok();
    }
    Json(json!({"kept": q.n, "whole": whole})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: &str, frames: u32, state: &str) -> Shot {
        Shot { id: id.into(), frames, state: state.into(), ..Shot::default() }
    }

    #[test]
    fn every_shot_has_its_place_in_the_film() {
        let mut shots = vec![shot("a", 120, "done"), shot("b", 96, "empty"), shot("c", 120, "done"), shot("d", 48, "done")];
        shots[0].made_frames = Some(121);
        shots[2].made_frames = Some(120);
        shots[2].trim_start = 1.0;
        shots[2].join = "dissolve".into();
        shots[3].made_frames = Some(48);
        shots[3].join = "dissolve".into();
        shots[3].continues = true;
        let times = film_times(&shots);
        // A made shot is as long as its clip; one that is not made, as long as it was asked for.
        assert_eq!(times[0], (0.0, 121.0 / 24.0));
        assert_eq!(times[1], (121.0 / 24.0, 4.0));
        // A dissolve of half a second starts that much before the shot before it ends; trims shorten.
        let third = 121.0 / 24.0 + 4.0 - 0.5;
        assert!((times[2].0 - third).abs() < 1e-4 && (times[2].1 - 4.0).abs() < 1e-4);
        // A shot that continues the one before it joins with a cut, whatever its join says.
        assert!((times[3].0 - (third + 4.0)).abs() < 1e-4);
        assert!(film_times(&[]).is_empty());
    }

    #[test]
    fn subtitles_follow_the_chosen_language_and_its_readings() {
        let mut p = Project { shots: vec![shot("a", 120, "empty"), shot("b", 120, "empty")], ..Project::default() };
        p.shots[0].lines = vec![Spoken {
            id: "l1".into(),
            who: String::new(),
            text: "A fox walks.".into(),
            said: [("Turkish".to_string(), "Bir tilki yürüyor.".to_string())].into(),
        }];
        p.shots[1].lines = vec![Spoken { id: "l2".into(), who: String::new(), text: "It stops.".into(), said: Default::default() }];
        assert_eq!((own_language(&p), shown_language(&p)), ("English".to_string(), "English".to_string()));
        let plain = subtitles(&p, false);
        assert_eq!(
            plain.iter().map(|c| (c.start, c.end, c.text.as_str())).collect::<Vec<_>>(),
            [(0.0, 5.0, "A fox walks."), (5.0, 10.0, "It stops.")]
        );
        // No shot is made: the export holds no film, and no subtitles.
        assert!(subtitles(&p, true).is_empty());
        // A Turkish dub: its readings say when, its words are what is read. A line without a translation stays as written.
        let region = Region {
            id: "r1".into(),
            file: "/f/voice/x.wav".into(),
            start: 0.15,
            offset: 0.0,
            length: 1.5,
            gain: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            loops: 1,
            line: "a/l1".into(),
        };
        let voice = |lang: &str, mute: bool, regions: Vec<Region>| Track {
            id: format!("t{lang}"),
            name: format!("{lang} voice"),
            kind: "voice".into(),
            lang: lang.into(),
            color: "#4f8fd9".into(),
            volume: 1.0,
            pan: 0.0,
            mute,
            solo: false,
            regions,
        };
        p.tracks = vec![voice("English", false, vec![Region { start: 3.0, ..region.clone() }]), voice("Chinese", true, vec![region])];
        // Turkish is shown, and the voice does not read it: the film is heard in English, and
        // the Turkish words are on screen while the English reading plays.
        p.dub = "turkish".into();
        assert_eq!((shown_language(&p), heard_language(&p)), ("Turkish".to_string(), "English".to_string()));
        let read = subtitles(&p, false);
        assert_eq!((read[0].start, read[0].end, read[0].text.as_str()), (3.0, 4.5, "Bir tilki yürüyor."));
        assert_eq!(read[1].text, "It stops.");
        // A language with a voice track of its own is heard and read: one track on, the others muted.
        p.dub = "chinese".into();
        hear_one(&mut p);
        assert_eq!(p.tracks.iter().map(|t| t.mute).collect::<Vec<_>>(), [true, false]);
        let dubbed = subtitles(&p, false);
        assert_eq!((dubbed[0].start, dubbed[0].end), (0.15, 1.65));
        p.dub.clear();
        hear_one(&mut p);
        assert_eq!(p.tracks.iter().map(|t| t.mute).collect::<Vec<_>>(), [false, true]);
        assert!(voice_reads(" english") && voice_reads("Chinese") && !voice_reads("Turkish"));
        // A reading that runs past the end of the film: its subtitle ends with the film.
        p.tracks[0].regions[0] = Region { start: 9.5, length: 3.0, line: "b/l2".into(), ..p.tracks[0].regions[0].clone() };
        let last = subtitles(&p, false).pop().unwrap();
        assert_eq!((last.start, last.end), (9.5, 10.0));
    }

    #[test]
    fn subtitle_pictures_are_one_stream_for_the_whole_film() {
        let cues = [
            Cue { start: 0.5, end: 2.0, text: "A fox walks.".into(), shot: "a".into() },
            Cue { start: 2.0, end: 3.25, text: "It stops.".into(), shot: "a".into() },
            Cue { start: 6.0, end: 7.0, text: "Snow falls.".into(), shot: "b".into() },
        ];
        assert_eq!(
            caption_list(&cues, 10.0),
            "ffconcat version 1.0\nfile 'blank.pam'\nduration 0.500\nfile '0.pam'\nduration 1.500\nfile '1.pam'\nduration 1.250\n\
             file 'blank.pam'\nduration 2.750\nfile '2.pam'\nduration 1.000\nfile 'blank.pam'\nduration 3.000\nfile 'blank.pam'\n"
        );
        let picture = pam(2, 1, &[255, 0, 0, 255, 0, 0, 0, 0]);
        assert!(
            picture.starts_with(b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n")
                && picture.ends_with(&[255, 0, 0, 255, 0, 0, 0, 0])
        );
        let p = Project { size: "640x384".into(), ..Project::default() };
        assert_eq!((export_size(&p, 0), export_size(&p, 2)), ((640, 384), (1280, 768)));
    }

    #[test]
    fn the_pictures_of_other_subtitles_are_not_used() {
        let dir = std::env::temp_dir().join(format!("rigbrew-captions-{}", uuid::Uuid::new_v4().simple()));
        let p = Project { folder: dir.to_string_lossy().to_string(), ..Project::default() };
        let cues = [Cue { start: 0.0, end: 1.0, text: "A fox walks.".into(), shot: "a".into() }];
        assert!(drawn(&p, &cues, (640, 384)).is_none());
        let kept = captions_dir(&p, &words::cues_key(&cues, 640, 384));
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::write(kept.join("0.pam"), pam(640, 2, &vec![0; 640 * 2 * 4])).unwrap();
        std::fs::write(kept.join("drawn.json"), "{\"count\":1,\"width\":640,\"height\":2}").unwrap();
        assert_eq!(drawn(&p, &cues, (640, 384)).map(|(_, note)| note), Some(Drawn { count: 1, width: 640, height: 2 }));
        // Other words, another size, or a picture gone: not these pictures.
        let other = [Cue { text: "A fox runs.".into(), ..cues[0].clone() }];
        assert!(drawn(&p, &other, (640, 384)).is_none() && drawn(&p, &cues, (1280, 768)).is_none());
        std::fs::remove_file(kept.join("0.pam")).unwrap();
        assert!(drawn(&p, &cues, (640, 384)).is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
