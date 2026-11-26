//! Studio: films made of shots, kept in a folder the person chooses.
//!
//!   GET    /v1/studio/projects                    the films this install knows
//!   POST   /v1/studio/projects                    {"folder"?, "name"}: start a film in a folder (created if missing;
//!                                                 without one, a new folder named after the film under Movies/Rigbrew)
//!   GET    /v1/studio/projects/{id}               the film: its shots and what is being made
//!   PUT    /v1/studio/projects/{id}               {"name", "size", "auto_export", "fade_in", "fade_out", "master",
//!                                                 "export": {"scale", "quality", "name"},
//!                                                 "shots": [{"id"?, "prompt", "frames", "continues", "trim_start",
//!                                                 "trim_end", "join", "join_seconds", "volume", "mute"}],
//!                                                 "shot_track": {"volume", "pan", "mute", "solo"},
//!                                                 "tracks": [{"id"?, "name", "color", "volume", "pan", "mute", "solo",
//!                                                 "regions": [{"id"?, "file", "start", "offset", "length", "gain",
//!                                                 "fade_in", "fade_out", "loops"}]}],
//!                                                 "look", "cast": [{"id"?, "name", "look", "voice"}], "seed",
//!                                                 "speech", "language", "voice", "duck", "dub",
//!                                                 "subtitles": {"show", "burn"}}; a shot also takes
//!                                                 "lines": [{"id"?, "who", "text", "said": {language: text}}]
//!                                                 and "from" (a cast member whose portrait it starts from)
//!   DELETE /v1/studio/projects/{id}               forget the film (its folder stays)
//!   POST   /v1/studio/projects/{id}/render        {"shots": [id]?, "model"?}: make the listed shots, or every shot not made yet
//!   DELETE /v1/studio/projects/{id}/render        stop: the shot being made is stopped, the rest leave the queue
//!   POST   /v1/studio/projects/{id}/storyboard    {"idea", "shots", "as_written"?}: the chat model writes the shots in
//!                                                 scenes, for the film's cast, look and speech (not saved); with
//!                                                 "as_written" every line of the idea is one shot as it stands
//!   POST   /v1/studio/brief, /projects/{id}/cast/...           see `studio/direct.rs`: a film directed from plain words,
//!                                                 the look of a character, portraits
//!   POST   /v1/studio/projects/{id}/import        see `studio/bring.rs`: a video file cut into shots
//!   POST   /v1/studio/projects/{id}/dub, /listen; GET /subtitles; PUT /captions    see `studio/dub.rs`
//!   POST   /v1/studio/projects/{id}/export      join the made shots into `<folder>/<name>.mp4`, with the film's
//!                                               sound tracks mixed in ({"music"?, "music_volume"?} lays one more
//!                                               file under all of it; {"scale"?, "quality"?, "name"?} instead of
//!                                               the film's own)
//!   POST   /v1/studio/projects/{id}/reveal      show the exported file in the system's file manager
//!   POST   /v1/studio/projects/{id}/media       an address the app's <video> plays the exported film (or a clip)
//!                                               from, see `studio/play.rs`; the film's answer says in `film`
//!                                               whether the export holds the film as it is now
//!   GET    /v1/studio/projects/{id}/export      where the export is: {"state", "file", "error"}
//!   GET    /v1/studio/projects/{id}/sound?file=  a sound file of the film, to play in the app (up to 200 MB)
//!   GET    /v1/studio/projects/{id}/sounds      the sound files in the film's folder and the ones added to it
//!   POST   /v1/studio/projects/{id}/sounds      {"path"}: add a sound file, or every sound file in a folder
//!   DELETE /v1/studio/projects/{id}/sounds?file=  take an added file out of the list (the file stays)
//!   GET    /v1/studio/projects/{id}/peaks?shot= | ?file=   the small picture of a sound: 50 lowest and highest
//!                                               values a second, kept next to the clips as `.peaks.json`
//!   POST   /v1/studio/projects/{id}/narration   {"text", "at"?, "track"?}: the voice package reads it; the WAV is
//!                                               saved in the film's folder and set on a narration track at `at`
//!   POST   /v1/studio/projects/{id}/shots/{shot}/rewrite       {"hint"?, "prompt"?}: the chat model writes the
//!                                               description of one shot again, in the words of the film (not saved)
//!   POST   /v1/studio/projects/{id}/shots/{shot}/take          {"take": n}: use that take of the shot
//!   DELETE /v1/studio/projects/{id}/shots/{shot}/takes/{n}     delete a take and its clip
//!   GET    /v1/studio/projects/{id}/clips/{shot}.webm          the take in use; `{shot}.{n}.webm` for one take;
//!                                                 on macOS its playable MP4 copy (see `playable.rs`)
//!   GET    /v1/studio/projects/{id}/clips/{shot}.jpg           one small picture of it, for the timeline
//!
//! A film is `<folder>/rigbrew-film.json` plus `<folder>/clips/<shot>.<take>.webm` (and `.jpg`; a film
//! from before takes has `<shot>.webm`, which becomes take 1 when the shot is made again); the list of folders is
//! `<data dir>/studio.json`. Shots are made one after another by the installed video package through
//! the same path as `/v1/videos/generations`, so a long film renders unattended, shot by shot. A film
//! with `auto_export` is exported by the same worker when its last shot is made. How long each shot
//! took is kept in `<data dir>/studio-pace.json`, and a film says from it how long it will still take.
//!
//! Nothing is written or made without the model for it: writing shots needs the video model and
//! the chat model, making them the video model, a dub a voice, a portrait the picture model. Each
//! refuses with 409 and one short sentence (see `missing`). A film has a cast (see `studio/cast.rs`):
//! a shot names its characters, and the fixed look of each one it names goes into what the video
//! model is told, word for word, with the look of the film. What is said in a shot is its `lines`;
//! a dub puts their readings on a voice track and the subtitles follow them (see `studio/words.rs`).

use std::collections::VecDeque;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rigbrew_core::manifest::Modality;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cloud::now_ms;
use crate::{fail, video, App, Shared};

pub mod assist;
mod board;
mod brief;
mod bring;
mod cast;
mod direct;
mod dub;
mod export;
mod pace;
mod play;
mod sound;
mod words;

use cast::Member;
use sound::{Region, Strip, Track};
use words::Spoken;

const FILE: &str = "rigbrew-film.json";
const MAX_SHOTS: usize = 200;
const MAX_PROMPT: usize = 1200;
/// The longest text the voice reads for one narration.
const MAX_NARRATION: usize = 6000;
/// The most shots one storyboard has: five minutes of five-second shots.
const MAX_STORYBOARD: usize = 60;
/// Shots taken out of a film are kept this many deep, so putting one back gives it its clip again.
const MAX_REMOVED: usize = 40;
/// The largest sound file the app is handed to play.
const MAX_SOUND_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shot {
    pub id: String,
    pub prompt: String,
    pub frames: u32,
    /// "empty" (never made), "queued", "making", "done", "failed", or "changed" (made, then the description changed).
    #[serde(default = "empty")]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long the last render took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    /// The frames the clip really has: the video model rounds a length to what it can make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_frames: Option<u32>,
    /// Seconds cut from the start and from the end of the clip when the film is exported or played.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_start: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_end: f32,
    /// Start this shot from the last picture of the shot before it, so the two join without a cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continues: bool,
    /// How the film goes from the shot before into this one: "" (a cut), "dissolve" or "black".
    /// Not looked at when the shot continues the one before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub join: String,
    /// Seconds a dissolve or a fade through black into this shot lasts: 0.25 to 2.
    #[serde(default = "half", skip_serializing_if = "is_half")]
    pub join_seconds: f32,
    /// How loud the shot's own sound is in the film: 1.0 as made, up to 1.5.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mute: bool,
    /// The clips made for this shot, and the number of the one in use (0 before the first take).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<Take>,
    #[serde(default, skip_serializing_if = "is_none_yet")]
    pub take: u32,
    /// A seed the person wants the next clip made with; without one every clip gets a new seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// What the clip in use was made from (see `made_key`), kept once the shot is changed: a
    /// shot that is changed back is made again without making it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_from: Option<String>,
    /// What is said during the shot: by the narrator or by a character of the cast.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<Spoken>,
    /// The id of a cast member whose portrait the shot starts from, when the video model can
    /// start from a picture. Not looked at when the shot continues the one before.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
}

fn is_none_yet(n: &u32) -> bool {
    *n == 0
}

impl Default for Shot {
    /// A shot that is not made, at its own volume.
    fn default() -> Shot {
        Shot {
            id: String::new(),
            prompt: String::new(),
            frames: 121,
            state: empty(),
            error: None,
            seconds: None,
            made_frames: None,
            trim_start: 0.0,
            trim_end: 0.0,
            continues: false,
            join: String::new(),
            join_seconds: 0.5,
            volume: 1.0,
            mute: false,
            takes: Vec::new(),
            take: 0,
            seed: None,
            made_from: None,
            lines: Vec::new(),
            from: String::new(),
        }
    }
}

impl Shot {
    /// The length of the shot in seconds: of its clip when there is one, else what was asked for.
    fn length(&self) -> f32 {
        self.made_frames.filter(|_| self.state == "done" || self.state == "changed").unwrap_or(self.frames) as f32 / 24.0
    }
}

/// One clip of a shot. Making a shot again keeps the clip before as a take.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Take {
    pub n: u32,
    /// The seed it was made with: the same seed and description give the same clip again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    /// The frames that were asked for, and the frames the clip has.
    pub frames: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_frames: Option<u32>,
    /// Whether it started from the last picture of the shot before it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub continued: bool,
    /// How the clip's file ends when it is not a made one: "mp4" for a clip brought in from a video.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ext: String,
    /// The clip has twice the film's width and height (a clip from a video that had the pictures for it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub large: bool,
}

/// A shot keeps this many takes; the oldest one that is not in use goes when another is made.
const MAX_TAKES: usize = 4;

impl Shot {
    /// Notes that making the shot did not work. A shot that has a clip keeps it, and its place in the film.
    fn fail(&mut self, why: String) {
        if self.take == 0 && self.made_frames.is_none() {
            self.state = "failed".into();
        }
        self.error = Some(why);
    }

    /// Keeps a new take and puts it in use. Answers with the takes that have to go.
    fn add_take(&mut self, take: Take) -> Vec<u32> {
        self.take = take.n;
        self.state = "done".into();
        self.error = None;
        self.seconds = take.seconds;
        self.made_frames = take.made_frames;
        self.takes.retain(|t| t.n != take.n);
        self.takes.push(take);
        let mut gone = Vec::new();
        while self.takes.len() > MAX_TAKES {
            let Some(at) = self.takes.iter().position(|t| t.n != self.take) else { break };
            gone.push(self.takes.remove(at).n);
        }
        gone
    }

    /// Puts a take in use: the shot is then as long as that clip and counts as made.
    fn use_take(&mut self, n: u32) -> bool {
        let Some(take) = self.takes.iter().find(|t| t.n == n).cloned() else { return false };
        self.take = n;
        self.frames = take.frames;
        self.seconds = take.seconds;
        self.made_frames = take.made_frames;
        self.state = "done".into();
        self.error = None;
        // Trims that fitted a longer clip leave at least half a second of this one.
        let length = self.length();
        self.trim_start = self.trim_start.clamp(0.0, (length - 0.5).max(0.0));
        self.trim_end = self.trim_end.clamp(0.0, (length - 0.5 - self.trim_start).max(0.0));
        true
    }

    /// Takes a take away. When it was the one in use, the newest one left is; with none left the shot is not made.
    fn remove_take(&mut self, n: u32) -> bool {
        let before = self.takes.len();
        self.takes.retain(|t| t.n != n);
        if self.takes.len() == before {
            return false;
        }
        if self.take == n {
            match self.takes.iter().map(|t| t.n).max() {
                Some(newest) => {
                    self.use_take(newest);
                }
                None => {
                    self.take = 0;
                    self.state = empty();
                    self.seconds = None;
                    self.made_frames = None;
                }
            }
        }
        true
    }
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn half() -> f32 {
    0.5
}

fn is_half(v: &f32) -> bool {
    *v == 0.5
}

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn empty() -> String {
    "empty".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder: String,
    /// "640x384": every shot of a film has the same shape.
    pub size: String,
    pub shots: Vec<Shot>,
    /// Export the film by itself when its last shot is made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_export: bool,
    /// From before sound tracks: a sound file (its full path) that plays under the whole film.
    /// A film that has one gets a track named "Music" with one region when it is opened.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sound: String,
    /// 0 to 1: how loud the sound file is next to the shots' own sound.
    #[serde(default = "half")]
    pub sound_volume: f32,
    /// The sound file plays once (a narration); otherwise it repeats until the film ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sound_once: bool,
    /// The film comes up from black at its start, and goes down to black at its end.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fade_in: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fade_out: bool,
    /// How the film is written to its MP4.
    #[serde(default, skip_serializing_if = "ExportOptions::is_usual")]
    pub export: ExportOptions,
    /// The sound tracks: music, narration, effects, each with its regions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
    /// The channel strip of the shots' own sound as a whole.
    #[serde(default, skip_serializing_if = "Strip::is_usual")]
    pub shot_track: Strip,
    /// How loud the whole film is: 1.0 as mixed, up to 2.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub master: f32,
    /// Sound files from outside the film's folder that the person added to its library.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<String>,
    /// Shots that were taken out of the film and had a clip: one that is put back has it again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Shot>,
    /// One line every shot shares: the style, the colours, the light, the lens. Put behind the
    /// description of every shot when it is made.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub look: String,
    /// The characters of the film, each with a fixed look.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast: Vec<Member>,
    /// A seed every shot without one of its own is made with, so the shots start alike.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// Whether the shot writer writes spoken lines for the shots.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub speech: bool,
    /// The language the lines are written in, by its name; English when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// The speaker number of the voice that reads the narrator's lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<u32>,
    #[serde(default, skip_serializing_if = "Subtitles::is_usual")]
    pub subtitles: Subtitles,
    /// How far the shots' own sound goes down while a line is said: 0 not at all, 1 to silence.
    #[serde(default = "usual_duck", skip_serializing_if = "is_usual_duck")]
    pub duck: f32,
    /// The language that is heard and read when it is not the film's own: the dub that was chosen.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dub: String,
    pub updated_ms: i64,
}

fn usual_duck() -> f32 {
    0.6
}

fn is_usual_duck(v: &f32) -> bool {
    *v == 0.6
}

/// How the spoken lines of a film are shown as text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subtitles {
    /// Shown in the viewer, and a subtitle track in the exported file that a player turns on and off.
    #[serde(default = "yes")]
    pub show: bool,
    /// Drawn into the pictures of the exported file, where no player can turn them off.
    #[serde(default)]
    pub burn: bool,
}

fn yes() -> bool {
    true
}

impl Default for Subtitles {
    fn default() -> Subtitles {
        Subtitles { show: true, burn: false }
    }
}

impl Subtitles {
    fn is_usual(&self) -> bool {
        *self == Subtitles::default()
    }
}

impl Default for Project {
    /// An empty film in the usual shape.
    fn default() -> Project {
        Project {
            id: String::new(),
            name: String::new(),
            folder: String::new(),
            size: "640x384".into(),
            shots: Vec::new(),
            auto_export: false,
            sound: String::new(),
            sound_volume: half(),
            sound_once: false,
            fade_in: false,
            fade_out: false,
            export: ExportOptions::default(),
            tracks: Vec::new(),
            shot_track: Strip::default(),
            master: 1.0,
            sounds: Vec::new(),
            removed: Vec::new(),
            look: String::new(),
            cast: Vec::new(),
            seed: None,
            speech: false,
            language: String::new(),
            voice: None,
            subtitles: Subtitles::default(),
            duck: usual_duck(),
            dub: String::new(),
            updated_ms: 0,
        }
    }
}

/// How a film is written to its MP4: how large, how good, under which name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportOptions {
    /// 2 enlarges the picture to twice its width and height (it adds no detail); otherwise as made.
    #[serde(default)]
    pub scale: u32,
    /// "small", "balanced" or "best": how many bits a second the file may use.
    #[serde(default)]
    pub quality: String,
    /// The file's name without its ending; empty for the film's name.
    #[serde(default)]
    pub name: String,
}

impl ExportOptions {
    fn is_usual(&self) -> bool {
        *self == ExportOptions::default()
    }

    /// The same options with only the values that mean something.
    fn tidy(self) -> ExportOptions {
        ExportOptions {
            scale: if self.scale == 2 { 2 } else { 0 },
            quality: match self.quality.as_str() {
                "small" | "best" => self.quality,
                _ => String::new(),
            },
            name: if self.name.trim().is_empty() { String::new() } else { file_stem(&self.name) },
        }
    }

    /// Bits a second for the pictures and for the sound of a film of this size.
    /// Balanced is 8 Mbit/s at 640x384; a larger shape gets as much more as it has more points.
    /// An enlarged picture has four times the points and no more detail: it gets two and a half times.
    fn bitrates(&self, size: &str) -> (u32, u32) {
        let (w, h) = size.split_once('x').and_then(|(w, h)| Some((w.parse::<u64>().ok()?, h.parse::<u64>().ok()?))).unwrap_or((640, 384));
        let points = w * h * if self.scale == 2 { 5 } else { 2 } / 2;
        let (per_million, sound) = match self.quality.as_str() {
            "small" => (14_000_000u64, 128_000),
            "best" => (64_000_000, 256_000),
            _ => (32_500_000, 192_000),
        };
        let video = (points * per_million / 1_000_000).clamp(1_500_000, 60_000_000);
        ((video / 100_000 * 100_000) as u32, sound)
    }
}

/// What is waiting to be made and what is being made now. One film renders at a time.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<(String, String)>,
    /// (project, shot, video job)
    now: Option<(String, String, String)>,
    working: bool,
    model: Option<String>,
    /// Stop was asked for before the shot being made had a job in the engine.
    stop_now: bool,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);

fn with_queue<T>(f: impl FnOnce(&mut Queue) -> T) -> T {
    let mut guard = QUEUE.lock().unwrap();
    f(guard.get_or_insert_with(Queue::default))
}

fn index_path(app: &App) -> PathBuf {
    app.root.join("studio.json")
}

/// How long shots took on this machine, newest last.
fn paces(app: &App) -> Vec<pace::Sample> {
    std::fs::read_to_string(app.root.join("studio-pace.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The measurements a film's times are read from: this machine's, or the film's own takes when
/// this machine has made nothing since measurements are kept.
fn paces_for(app: &App, p: &Project) -> Vec<pace::Sample> {
    let seen = paces(app);
    if !seen.is_empty() {
        return seen;
    }
    let mut own = Vec::new();
    for s in &p.shots {
        for t in &s.takes {
            pace::remember(&mut own, pace::Sample { frames: t.frames, continued: t.continued, seconds: t.seconds.unwrap_or(0.0) });
        }
        if s.takes.is_empty() {
            pace::remember(&mut own, pace::Sample { frames: s.frames, continued: s.continues, seconds: s.seconds.unwrap_or(0.0) });
        }
    }
    own
}

/// About how long the film still needs: `all` for every described shot that is not made, `left`
/// for the shots that wait or are being made now (less the time the one being made has had).
fn time_left(p: &Project, seen: &[pace::Sample], waiting: &[String], now: Option<(&str, f32)>) -> (f32, f32) {
    let (mut all, mut left) = (0.0, 0.0);
    for (i, s) in p.shots.iter().enumerate() {
        let needs = pace::estimate(seen, s.frames, s.continues && i > 0);
        let making = now.filter(|(id, _)| *id == s.id);
        if s.state != "done" && !s.prompt.is_empty() {
            all += needs;
        }
        if let Some((_, elapsed)) = making {
            // A shot that runs longer than shots like it did is still not done: a little is always left.
            left += (needs - elapsed).max(needs * 0.05);
        } else if waiting.contains(&s.id) {
            left += needs;
        }
    }
    (all, left)
}

/// id -> folder
fn index(app: &App) -> Vec<(String, String)> {
    std::fs::read_to_string(index_path(app)).ok().and_then(|t| serde_json::from_str::<Vec<(String, String)>>(&t).ok()).unwrap_or_default()
}

fn write_index(app: &App, list: &[(String, String)]) -> std::io::Result<()> {
    std::fs::write(index_path(app), serde_json::to_string_pretty(list).unwrap_or_default())
}

fn load(app: &App, id: &str) -> Option<Project> {
    let folder = index(app).into_iter().find(|(i, _)| i == id)?.1;
    let mut p: Project = serde_json::from_str(&std::fs::read_to_string(FsPath::new(&folder).join(FILE)).ok()?).ok()?;
    // The folder may have been moved and added again: where it is now wins.
    p.folder = folder;
    p.id = id.to_string();
    Some(p)
}

fn store(p: &mut Project) -> std::io::Result<()> {
    p.updated_ms = now_ms();
    let path = FsPath::new(&p.folder).join(FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(p).unwrap_or_default())?;
    std::fs::rename(tmp, path)
}

/// The clip of a shot: of the take in use, or the one clip of a shot from before takes.
fn clip_path(p: &Project, shot: &str) -> PathBuf {
    let ext = take_in_use(p, shot).map_or("webm", |t| clip_ext(t));
    FsPath::new(&p.folder).join("clips").join(format!("{}.{ext}", clip_stem(p, shot)))
}

/// The take a shot has in use.
fn take_in_use<'a>(p: &'a Project, shot: &str) -> Option<&'a Take> {
    let s = p.shots.iter().find(|s| s.id == shot)?;
    s.takes.iter().find(|t| t.n == s.take)
}

/// How the file of a take's clip ends: "webm" as the video model writes it, unless the take says otherwise.
fn clip_ext(take: &Take) -> &str {
    match take.ext.as_str() {
        "mp4" => "mp4",
        _ => "webm",
    }
}

/// "<shot>.<take>" for the take in use; "<shot>" for a shot from before takes.
fn clip_stem(p: &Project, shot: &str) -> String {
    match p.shots.iter().find(|s| s.id == shot).map(|s| s.take).filter(|n| *n > 0) {
        Some(n) => format!("{shot}.{n}"),
        None => shot.to_string(),
    }
}

/// The clip of one take, and its small picture.
fn take_files(p: &Project, shot: &str, n: u32) -> (PathBuf, PathBuf) {
    let dir = FsPath::new(&p.folder).join("clips");
    let ext = p.shots.iter().find(|s| s.id == shot).and_then(|s| s.takes.iter().find(|t| t.n == n)).map_or("webm", clip_ext);
    (dir.join(format!("{shot}.{n}.{ext}")), dir.join(format!("{shot}.{n}.jpg")))
}

fn poster_path(p: &Project, shot: &str) -> PathBuf {
    FsPath::new(&p.folder).join("clips").join(format!("{}.jpg", clip_stem(p, shot)))
}

fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn valid_size(size: &str) -> bool {
    size.split_once('x')
        .is_some_and(|(w, h)| [w, h].iter().all(|v| v.parse::<u32>().is_ok_and(|n| (256..=1280).contains(&n) && n % 32 == 0)))
}

/// Where films go when the person does not choose a folder: `Movies/Rigbrew` in their home.
fn films_home() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(if cfg!(target_os = "macos") { "Movies" } else { "Videos" }).join("Rigbrew"))
}

/// A folder for a new film named `name` inside `home`: the first of "Name", "Name 2", ... that holds no film yet.
fn new_folder(home: &FsPath, name: &str) -> PathBuf {
    let stem = file_stem(name);
    (1..)
        .map(|n| home.join(if n == 1 { stem.clone() } else { format!("{stem} {n}") }))
        .find(|folder| !folder.join(FILE).exists())
        .unwrap_or_else(|| home.join(&stem))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Long work on a film that is not making shots or exporting: bringing a video in ("import"),
/// dubbing ("dub"), listening to clips ("listen"). Each is "working", "done" or "failed".
static JOBS: Mutex<Vec<(String, &'static str, Value)>> = Mutex::new(Vec::new());

fn job_of(id: &str, kind: &str) -> Value {
    JOBS.lock()
        .unwrap()
        .iter()
        .find(|(i, k, _)| i == id && *k == kind)
        .map(|(_, _, v)| v.clone())
        .unwrap_or_else(|| json!({"state": "none"}))
}

fn set_job(id: &str, kind: &'static str, value: Value) {
    let mut all = JOBS.lock().unwrap();
    all.retain(|(i, k, _)| !(i == id && *k == kind));
    all.push((id.to_string(), kind, value));
}

fn job_running(id: &str, kind: &str) -> bool {
    job_of(id, kind)["state"] == "working"
}

/// What Studio says when a model it needs for something is not installed.
fn missing(modality: Modality) -> &'static str {
    match modality {
        Modality::Video => "Studio needs the video model. Add it under Models.",
        Modality::Chat => "Studio needs the chat model to write. Add it under Models.",
        Modality::Voice => "Studio needs a voice to read lines aloud. Add it under Models.",
        Modality::Image => "Portraits need the picture model. Add it under Models.",
        Modality::Listen => "No listening model is installed. Type the lines instead.",
    }
}

/// The installed package of a kind, or why the work that needs it cannot start.
fn installed_one(app: &App, modality: Modality) -> Result<&rigbrew_core::manifest::Package, (StatusCode, String)> {
    app.manifest
        .packages
        .iter()
        .find(|pk| pk.modality == modality && !app.installed(pk).is_empty())
        .ok_or_else(|| (StatusCode::CONFLICT, missing(modality).to_string()))
}

/// The same as a refusal the app is answered with.
fn need(app: &App, modality: Modality) -> Result<&rigbrew_core::manifest::Package, Box<Response>> {
    installed_one(app, modality).map_err(|(status, why)| Box::new(fail(status, why)))
}

/// Asks the chat model one thing and answers with what it wrote.
async fn ask(app: &Shared, system: &str, user: &str, max_tokens: u32, temperature: f32) -> Result<String, (StatusCode, String)> {
    let chat = installed_one(app, Modality::Chat)?;
    let port = own_port(app).ok_or((StatusCode::INTERNAL_SERVER_ERROR, "the service does not know its own address".to_string()))?;
    let request = json!({
        "model": chat.id,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "max_tokens": max_tokens,
        "temperature": temperature,
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let answer = app
        .http
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(&app.token)
        .json(&request)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    if !answer.status().is_success() {
        let v: Value = answer.json().await.unwrap_or(Value::Null);
        return Err((StatusCode::BAD_GATEWAY, v["error"]["message"].as_str().unwrap_or("the chat model could not answer").to_string()));
    }
    let v: Value = answer.json().await.unwrap_or(Value::Null);
    Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string())
}

/// Whether the installed video model can start a shot from a picture.
fn can_continue(app: &App) -> bool {
    app.manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video)
        .any(|pk| app.installed(pk).iter().any(|v| v.files.iter().any(|f| f.role == "mmproj")))
}

/// The film as the app sees it: the stored film plus what the queue says right now.
fn view(app: &App, mut p: Project) -> Value {
    let (now, waiting) =
        with_queue(|q| (q.now.clone(), q.waiting.iter().filter(|(pid, _)| *pid == p.id).map(|(_, s)| s.clone()).collect::<Vec<_>>()));
    let mut progress = Value::Null;
    let mut elapsed = None;
    for shot in &mut p.shots {
        if now.as_ref().is_some_and(|(pid, sid, _)| *pid == p.id && *sid == shot.id) {
            shot.state = "making".into();
            if let Some((_, _, job)) = &now {
                if let Some(j) = app.videos.lock().unwrap().get(job) {
                    progress = json!({"shot": shot.id, "stage": j.stage, "step": j.step, "steps": j.steps, "elapsed_ms": j.elapsed_ms});
                    elapsed = Some(j.elapsed_ms as f32 / 1000.0);
                }
            }
        } else if waiting.contains(&shot.id) {
            shot.state = "queued".into();
        }
    }
    let made = p.shots.iter().filter(|s| s.state == "done").count();
    let seconds: f32 = p.shots.iter().map(Shot::length).sum();
    let continues = can_continue(app);
    let export = export_of(&p.id);
    let film = play::film(&p);
    let mine = now.as_ref().filter(|(pid, _, _)| *pid == p.id).map(|(_, sid, _)| (sid.as_str(), elapsed.unwrap_or(0.0)));
    let (all, left) = time_left(&p, &paces_for(app, &p), &waiting, mine);
    // The shots that were taken out are the service's own business.
    p.removed.clear();
    // Which shots name which character, the subtitles as the timeline holds them, and what is heard.
    let appears: serde_json::Map<String, Value> = p
        .cast
        .iter()
        .map(|m| {
            let shots: Vec<&str> =
                p.shots.iter().filter(|s| cast::named(std::slice::from_ref(m), &s.prompt).len() == 1).map(|s| s.id.as_str()).collect();
            (m.id.clone(), json!(shots))
        })
        .collect();
    let cues = dub::subtitles(&p, false);
    // The languages the film can be shown in: its own, every one with a voice track, every one its lines are translated into.
    let mut languages = vec![dub::own_language(&p)];
    let named = p
        .tracks
        .iter()
        .filter(|t| t.kind == "voice")
        .map(|t| &t.lang)
        .chain(p.shots.iter().flat_map(|s| s.lines.iter().flat_map(|l| l.said.keys())));
    for language in named {
        if !language.is_empty() && !languages.iter().any(|l| l.eq_ignore_ascii_case(language)) {
            languages.push(language.clone());
        }
    }
    let installed = |modality: Modality| installed_one(app, modality).is_ok();
    let jobs = json!({"import": job_of(&p.id, "import"), "dub": job_of(&p.id, "dub"), "listen": job_of(&p.id, "listen")});
    json!({"project": p, "progress": progress, "rendering": mine.is_some() || !waiting.is_empty(), "made": made, "seconds": seconds,
           "can_continue": continues, "export": export, "film": film, "estimate": {"all": all, "left": left},
           "cast_in": appears, "cues": cues, "language": dub::shown_language(&p), "own_language": dub::own_language(&p),
           "languages": languages, "spoken": dub::SPOKEN, "can_listen": dub::can_listen(app), "voices": dub::voices(app), "jobs": jobs,
           "has": {"video": installed(Modality::Video), "chat": installed(Modality::Chat), "voice": installed(Modality::Voice),
                   "image": installed(Modality::Image)}})
}

async fn list(State(app): State<Shared>) -> Json<Value> {
    let films: Vec<Value> = index(&app)
        .into_iter()
        .map(|(id, folder)| match load(&app, &id) {
            Some(p) => json!({"id": id, "name": p.name, "folder": folder, "shots": p.shots.len(), "made": p.shots.iter().filter(|s| s.state == "done").count(), "updated_ms": p.updated_ms}),
            None => json!({"id": id, "name": "", "folder": folder, "missing": true}),
        })
        .collect();
    // What a five-second shot takes here, for the start screen to say how long a film will take.
    let seen = paces(&app);
    let pace = json!({"frames": 121, "plain": pace::estimate(&seen, 121, false), "continuing": pace::estimate(&seen, 121, true), "measured": !seen.is_empty()});
    Json(
        json!({"projects": films, "home": films_home().map(|h| h.to_string_lossy().to_string()), "pace": pace, "can_continue": can_continue(&app),
               "voices": dub::voices(&app)}),
    )
}

#[derive(Deserialize)]
struct NewProject {
    #[serde(default)]
    folder: String,
    #[serde(default)]
    name: String,
}

async fn create(State(app): State<Shared>, Json(body): Json<NewProject>) -> Response {
    let folder = if body.folder.trim().is_empty() {
        match films_home() {
            Some(home) => new_folder(&home, body.name.trim()),
            None => return fail(StatusCode::BAD_REQUEST, "choose a folder for the film"),
        }
    } else {
        PathBuf::from(body.folder.trim())
    };
    if !folder.is_absolute() || body.folder.len() > 1024 {
        return fail(StatusCode::BAD_REQUEST, "choose a folder by its full path");
    }
    if let Err(e) = std::fs::create_dir_all(folder.join("clips")) {
        return fail(StatusCode::BAD_REQUEST, format!("that folder cannot be used: {e}"));
    }
    let folder_text = folder.to_string_lossy().to_string();
    let mut list = index(&app);
    // A folder that already holds a film is opened, not overwritten.
    let existing = std::fs::read_to_string(folder.join(FILE)).ok().and_then(|t| serde_json::from_str::<Project>(&t).ok());
    let id = list
        .iter()
        .find(|(_, f)| *f == folder_text)
        .map(|(i, _)| i.clone())
        .or_else(|| existing.as_ref().map(|p| p.id.clone()).filter(|i| valid_id(i)))
        .unwrap_or_else(new_id);
    let mut project = existing.unwrap_or_default();
    project.id = id.clone();
    project.folder = folder_text.clone();
    if project.name.is_empty() {
        let fallback = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Film".into());
        project.name = if body.name.trim().is_empty() { fallback } else { body.name.trim().chars().take(80).collect() };
    }
    if let Err(e) = store(&mut project) {
        return fail(StatusCode::BAD_REQUEST, format!("the film could not be saved there: {e}"));
    }
    list.retain(|(i, f)| *i != id && *f != folder_text);
    list.insert(0, (id, folder_text));
    write_index(&app, &list).ok();
    app.record("studio_project_opened", json!({"shots": project.shots.len()}));
    let project = with_tracks(&app, project).await;
    Json(view(&app, project)).into_response()
}

async fn read(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match load(&app, &id) {
        Some(p) => Json(view(&app, with_tracks(&app, p).await)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone"),
    }
}

/// A film from before sound tracks has one sound file under all of it. It becomes one region on
/// a track named "Music": as long as the file, repeated to the end of the film unless it played
/// once. A file that cannot be read stays as it was, and the export says so as before.
async fn with_tracks(app: &Shared, mut p: Project) -> Project {
    let file = p.sound.trim().to_string();
    if file.is_empty() {
        return p;
    }
    let path = PathBuf::from(&file);
    if !path.is_absolute() || !sound::is_sound(&path) || !path.is_file() {
        return p;
    }
    let Some(seconds) = peaks_of(app, &p, &path, None).await.map(|(_, seconds)| seconds).filter(|s| *s >= 0.05) else {
        return p;
    };
    let film: f32 = p.shots.iter().map(Shot::length).sum();
    let loops = if p.sound_once { 1 } else { ((film / seconds).ceil() as u32).clamp(1, 1000) };
    let region = Region {
        id: new_id(),
        file,
        start: 0.0,
        offset: 0.0,
        length: seconds,
        gain: p.sound_volume.clamp(0.0, 2.0),
        fade_in: 0.0,
        fade_out: 0.0,
        loops,
        line: String::new(),
    };
    p.tracks.insert(
        0,
        Track {
            id: new_id(),
            name: "Music".into(),
            kind: "sound".into(),
            lang: String::new(),
            color: sound::next_color(0),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            regions: vec![region],
        },
    );
    p.sound.clear();
    p.sound_once = false;
    p.sound_volume = half();
    store(&mut p).ok();
    p
}

#[derive(Deserialize, Default)]
struct ShotIn {
    #[serde(default)]
    id: Option<String>,
    prompt: String,
    #[serde(default)]
    frames: Option<u32>,
    #[serde(default)]
    trim_start: f32,
    #[serde(default)]
    trim_end: f32,
    #[serde(default)]
    continues: bool,
    #[serde(default)]
    join: String,
    #[serde(default)]
    join_seconds: Option<f32>,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    seed: Option<i64>,
    /// Without a list the shot keeps the lines it has.
    #[serde(default)]
    lines: Option<Vec<words::SpokenIn>>,
    #[serde(default)]
    from: Option<String>,
}

/// A value that may be left out (keep what is there), or be null (take it away).
fn maybe<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize, Default)]
struct Save {
    #[serde(default)]
    look: Option<String>,
    #[serde(default)]
    cast: Option<Vec<cast::MemberIn>>,
    #[serde(default, deserialize_with = "maybe")]
    seed: Option<Option<i64>>,
    #[serde(default)]
    speech: Option<bool>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default, deserialize_with = "maybe")]
    voice: Option<Option<u32>>,
    #[serde(default)]
    subtitles: Option<Subtitles>,
    #[serde(default)]
    duck: Option<f32>,
    #[serde(default)]
    dub: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    shots: Option<Vec<ShotIn>>,
    #[serde(default)]
    auto_export: Option<bool>,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    sound_volume: Option<f32>,
    #[serde(default)]
    sound_once: Option<bool>,
    #[serde(default)]
    fade_in: Option<bool>,
    #[serde(default)]
    fade_out: Option<bool>,
    #[serde(default)]
    export: Option<ExportOptions>,
    #[serde(default)]
    tracks: Option<Vec<sound::TrackIn>>,
    #[serde(default)]
    shot_track: Option<Strip>,
    #[serde(default)]
    master: Option<f32>,
}

/// What a clip was made from, as one short word: the film's shape, the description, the length,
/// and for a shot that continues another the shot it stood behind. FNV-1a, the same in every build.
fn made_key(size: &str, prompt: &str, frames: u32, before: Option<&str>) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let frames = frames.to_string();
    for part in [size, prompt, &frames, before.unwrap_or("")] {
        for b in part.bytes().chain([0]) {
            hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

/// Whether a shot has a clip in the folder to keep.
fn has_clip(shot: &Shot) -> bool {
    shot.state == "done" || shot.state == "changed" || !shot.takes.is_empty()
}

/// What the clips of a film are made from besides each shot's own description: the film's
/// shape, its look and its cast.
struct Film<'a> {
    size: &'a str,
    look: &'a str,
    cast: &'a [Member],
}

impl Film<'_> {
    /// What the clip of a shot of this film is made from (see `made_key`): what the video model
    /// is told (the description, the look of every character it names, the look of the film),
    /// the length, and the picture it starts from: the shot before it, or a portrait.
    fn key(&self, prompt: &str, frames: u32, before: Option<&str>, from: &str) -> String {
        let told = cast::told(self.look, self.cast, prompt);
        let portrait = self.cast.iter().find(|m| !from.is_empty() && m.id == from && m.portrait > 0);
        let starts = match (before, portrait) {
            (Some(shot), _) => shot.to_string(),
            (None, Some(m)) => format!("portrait:{}:{}", m.id, m.portrait),
            (None, None) => String::new(),
        };
        made_key(self.size, &told, frames, Some(&starts))
    }
}

/// The new list of shots: a shot that keeps its id and what its clip is made from keeps its
/// clip; one whose description, length, shape, or the look of a character it names changed is
/// marked "changed" until it is made again.
/// It also answers with the shots that are out of the film now and had a clip (`bin`, newest
/// last): a shot that comes back by its id, after an undo, has its clip and its takes again.
fn merge(old: &[Shot], bin: &[Shot], new: Vec<ShotIn>, films: (&Film, &Film)) -> Result<(Vec<Shot>, Vec<Shot>), String> {
    if new.len() > MAX_SHOTS {
        return Err(format!("a film has at most {MAX_SHOTS} shots"));
    }
    let (was, now) = films;
    // What the clip of a shot of the film as it was would be made from, as the shot stands.
    let stands = |at: usize| {
        let o = &old[at];
        let before = at.checked_sub(1).filter(|_| o.continues).map(|i| old[i].id.as_str());
        was.key(&o.prompt, o.frames, before, &o.from)
    };
    // What the clip of a shot in the film was made from: for a made shot, the shot as it stands.
    let was_key = |at: usize| if old[at].state == "done" { Some(stands(at)) } else { old[at].made_from.clone() };
    let cast_ids: Vec<String> = now.cast.iter().map(|m| m.id.clone()).collect();
    let mut out: Vec<Shot> = Vec::with_capacity(new.len());
    for s in new {
        let prompt: String = s.prompt.trim().chars().take(MAX_PROMPT).collect();
        let frames = s.frames.unwrap_or(121);
        if !(5..=241).contains(&frames) {
            return Err("a shot is between 5 and 241 frames".into());
        }
        let id = s.id.filter(|i| valid_id(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(new_id);
        // The first shot has nothing to continue.
        let continues = s.continues && !out.is_empty();
        let from = s.from.filter(|f| cast_ids.contains(f)).unwrap_or_default();
        let key = now.key(&prompt, frames, out.last().filter(|_| continues).map(|l| l.id.as_str()), &from);
        let again = |made_from: &Option<String>| if made_from.as_deref() == Some(key.as_str()) { "done" } else { "changed" };
        let mut shot = match (old.iter().position(|o| o.id == id), bin.iter().find(|o| o.id == id)) {
            (Some(at), _) => {
                let o = &old[at];
                // Nothing its clip is made from changed. A shot that continues another was made
                // from that shot's last picture: after a move it stands behind a different one,
                // and that is a change too.
                let same = stands(at) == key && o.continues == continues;
                let made_from = was_key(at);
                let state = if same {
                    o.state.clone()
                } else if o.state == "empty" || o.state == "failed" {
                    empty()
                } else {
                    // Changed back to what its clip was made from: it is made.
                    again(&made_from).to_string()
                };
                Shot { prompt, frames, state, error: if same { o.error.clone() } else { None }, continues, made_from, ..o.clone() }
            }
            // Back in the film after it was taken out.
            (None, Some(o)) => Shot { prompt, frames, state: again(&o.made_from).into(), error: None, continues, ..o.clone() },
            (None, None) => Shot { id, prompt, frames, state: empty(), continues, ..Shot::default() },
        };
        shot.from = from;
        // What is said in a shot, how it joins the film and how loud it is change nothing about its clip.
        if let Some(lines) = s.lines {
            shot.lines = words::merge_lines(lines, &cast_ids, new_id);
        } else {
            shot.lines.iter_mut().filter(|l| !cast_ids.contains(&l.who)).for_each(|l| l.who.clear());
        }
        shot.join = match s.join.as_str() {
            "dissolve" | "black" => s.join,
            _ => String::new(),
        };
        shot.join_seconds = s.join_seconds.filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.25, 2.0);
        shot.volume = s.volume.filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.5);
        shot.mute = s.mute;
        shot.seed = s.seed.filter(|seed| (0..=u32::MAX as i64).contains(seed));
        // Trims leave at least half a second of the shot.
        let length = shot.length();
        shot.trim_start = if s.trim_start.is_finite() { s.trim_start } else { 0.0 }.clamp(0.0, (length - 0.5).max(0.0));
        shot.trim_end = if s.trim_end.is_finite() { s.trim_end } else { 0.0 }.clamp(0.0, (length - 0.5 - shot.trim_start).max(0.0));
        out.push(shot);
    }
    let mut kept: Vec<Shot> = bin.iter().filter(|b| !out.iter().any(|o| o.id == b.id)).cloned().collect();
    for (at, o) in old.iter().enumerate().filter(|(_, o)| has_clip(o) && !out.iter().any(|n| n.id == o.id)) {
        kept.push(Shot { made_from: was_key(at), error: None, ..o.clone() });
    }
    let extra = kept.len().saturating_sub(MAX_REMOVED);
    kept.drain(..extra);
    Ok((out, kept))
}

#[cfg(test)]
fn merge_shots(old: &[Shot], new: Vec<ShotIn>, size_changed: bool) -> Result<Vec<Shot>, String> {
    merge(old, &[], new, (&film("640x384"), &film(if size_changed { "384x640" } else { "640x384" }))).map(|(shots, _)| shots)
}

/// A film of a shape, without a look and without a cast.
#[cfg(test)]
fn film(size: &str) -> Film<'_> {
    Film { size, look: "", cast: &[] }
}

async fn save(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Save>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    if let Err(e) = apply_save(&app, &mut p, body) {
        return fail(StatusCode::BAD_REQUEST, e);
    }
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    Json(view(&app, p)).into_response()
}

/// The shot as it stands, as the workstation sends it back: its lines are kept as they are.
fn shot_in(s: &Shot) -> ShotIn {
    ShotIn {
        id: Some(s.id.clone()),
        prompt: s.prompt.clone(),
        frames: Some(s.frames),
        trim_start: s.trim_start,
        trim_end: s.trim_end,
        continues: s.continues,
        join: s.join.clone(),
        join_seconds: Some(s.join_seconds),
        volume: Some(s.volume),
        mute: s.mute,
        seed: s.seed,
        lines: None,
        from: Some(s.from.clone()),
    }
}

/// One change to a film, as the workstation sends it: what `body` has replaces what the film
/// has, checked the same way whoever sends it (the person's edits, the assistant's). Nothing is
/// stored here; on an error the film is left half changed and must not be stored.
fn apply_save(app: &App, p: &mut Project, body: Save) -> Result<(), String> {
    if let Some(name) = body.name.map(|n| n.trim().chars().take(80).collect::<String>()).filter(|n| !n.is_empty()) {
        p.name = name;
    }
    let was = (p.size.clone(), p.look.clone(), p.cast.clone());
    if let Some(size) = body.size {
        if !valid_size(&size) {
            return Err("size must be WIDTHxHEIGHT, each 256 to 1280 and a multiple of 32".into());
        }
        p.size = size;
    }
    if let Some(look) = body.look {
        p.look = look.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect();
    }
    if let Some(members) = body.cast {
        p.cast = cast::merge_cast(&p.cast, members, new_id)?;
    }
    if let Some(seed) = body.seed {
        p.seed = seed.filter(|seed| (0..=u32::MAX as i64).contains(seed));
    }
    if let Some(voice) = body.voice {
        p.voice = voice.filter(|v| *v < 1000);
    }
    p.speech = body.speech.unwrap_or(p.speech);
    if let Some(language) = body.language {
        p.language = words::language(&language);
    }
    if let Some(subtitles) = body.subtitles {
        p.subtitles = subtitles;
    }
    if let Some(duck) = body.duck.filter(|v| v.is_finite()) {
        p.duck = duck.clamp(0.0, 1.0);
    }
    // Without a list of shots the film keeps the ones it has, in a shape that may be new.
    let shots = body.shots.unwrap_or_else(|| p.shots.iter().map(shot_in).collect());
    let films = (Film { size: &was.0, look: &was.1, cast: &was.2 }, Film { size: &p.size, look: &p.look, cast: &p.cast });
    (p.shots, p.removed) = merge(&p.shots, &p.removed, shots, (&films.0, &films.1))?;
    if let Some(tracks) = body.tracks {
        let (before, regions_before) = (p.tracks.len(), p.tracks.iter().map(|t| t.regions.len()).sum::<usize>());
        p.tracks = sound::merge_tracks(FsPath::new(&p.folder), &p.tracks, tracks, new_id, &p.sounds)?;
        let regions = p.tracks.iter().map(|t| t.regions.len()).sum::<usize>();
        if p.tracks.len() > before {
            app.record("studio_track_added", json!({"tracks": p.tracks.len()}));
        }
        if regions > regions_before {
            app.record("studio_region_added", json!({"kind": "sound", "regions": regions}));
        }
    }
    if let Some(strip) = body.shot_track {
        p.shot_track = strip.tidy();
    }
    if let Some(chosen) = body.dub {
        // One language is heard at a time: choosing it unmutes its voice track and mutes the others.
        let chosen = words::language(&chosen);
        let chosen = if chosen.eq_ignore_ascii_case(&dub::own_language(p)) { String::new() } else { chosen };
        // Only when the choice changes: after that the mutes are the person's own again.
        if !chosen.eq_ignore_ascii_case(&p.dub) {
            p.dub = chosen;
            dub::hear_one(p);
        }
    }
    if let Some(master) = body.master.filter(|v| v.is_finite()) {
        p.master = master.clamp(0.0, 2.0);
    }
    if let Some(auto) = body.auto_export {
        p.auto_export = auto;
    }
    if let Some(sound) = body.sound {
        let sound = sound.trim();
        if !sound.is_empty() && (!FsPath::new(sound).is_absolute() || sound.len() > 1024) {
            return Err("give the sound file by its full path".into());
        }
        p.sound = sound.to_string();
    }
    if let Some(volume) = body.sound_volume.filter(|v| v.is_finite()) {
        p.sound_volume = volume.clamp(0.0, 1.0);
    }
    p.sound_once = body.sound_once.unwrap_or(p.sound_once);
    p.fade_in = body.fade_in.unwrap_or(p.fade_in);
    p.fade_out = body.fade_out.unwrap_or(p.fade_out);
    if let Some(options) = body.export {
        p.export = options.tidy();
    }
    Ok(())
}

async fn forget(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let mut list = index(&app);
    let before = list.len();
    let folder = list.iter().find(|(i, _)| *i == id).map(|(_, folder)| PathBuf::from(folder));
    list.retain(|(i, _)| *i != id);
    if list.len() == before {
        return fail(StatusCode::NOT_FOUND, "no such film");
    }
    with_queue(|q| q.waiting.retain(|(pid, _)| *pid != id));
    write_index(&app, &list).ok();
    // The clips stay in the folder; the copies made for the app go.
    if let Some(folder) = folder {
        crate::playable::remove_all(&folder.join("clips"));
    }
    Json(json!({"forgotten": id})).into_response()
}

#[derive(Deserialize, Default)]
struct Render {
    #[serde(default)]
    shots: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
}

async fn render(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Render>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let body = body.map(|b| b.0).unwrap_or_default();
    let Some(package) = app
        .manifest
        .packages
        .iter()
        .filter(|pk| pk.modality == Modality::Video && !app.installed(pk).is_empty())
        .find(|pk| body.model.as_deref().is_none_or(|m| m == pk.id))
    else {
        return fail(StatusCode::CONFLICT, missing(Modality::Video));
    };
    let wanted: Vec<String> = match body.shots {
        Some(ids) => p.shots.iter().filter(|s| ids.contains(&s.id) && !s.prompt.is_empty()).map(|s| s.id.clone()).collect(),
        None => p.shots.iter().filter(|s| s.state != "done" && !s.prompt.is_empty()).map(|s| s.id.clone()).collect(),
    };
    if wanted.is_empty() {
        return fail(StatusCode::CONFLICT, "Every shot is made already. Change a shot or pick the ones to make again.");
    }
    let start = with_queue(|q| {
        for shot in &wanted {
            let key = (id.clone(), shot.clone());
            let making = q.now.as_ref().is_some_and(|(pid, sid, _)| *pid == key.0 && *sid == key.1);
            if !making && !q.waiting.contains(&key) {
                q.waiting.push_back(key);
            }
        }
        q.model = Some(package.id.clone());
        !std::mem::replace(&mut q.working, true)
    });
    app.record("studio_render_started", json!({"package": package.id, "shots": wanted.len()}));
    if start {
        tokio::spawn(work(app.clone()));
    }
    Json(view(&app, p)).into_response()
}

async fn stop(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let job = with_queue(|q| {
        q.waiting.retain(|(pid, _)| *pid != id);
        let job = q.now.as_ref().filter(|(pid, _, _)| *pid == id).map(|(_, _, job)| job.clone());
        if job.as_deref() == Some("") {
            q.stop_now = true;
        }
        job.filter(|j| !j.is_empty())
    });
    if let Some(job) = job {
        if let Some(j) = app.videos.lock().unwrap().get_mut(&job) {
            j.cancel = true;
        }
    }
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

fn set_shot(app: &App, project: &str, shot: &str, f: impl FnOnce(&mut Shot)) {
    if let Some(mut p) = load(app, project) {
        if let Some(s) = p.shots.iter_mut().find(|s| s.id == shot) {
            f(s);
            store(&mut p).ok();
        }
    }
}

/// Makes the waiting shots one after another until none is left.
async fn work(app: Shared) {
    loop {
        let Some((project, shot)) = with_queue(|q| {
            let next = q.waiting.pop_front();
            match &next {
                // The shot counts as being made from this moment, before the engine has a job for it.
                Some((project, shot)) => q.now = Some((project.clone(), shot.clone(), String::new())),
                None => {
                    q.working = false;
                    q.now = None;
                }
            }
            next
        }) else {
            return;
        };
        let Some(p) = load(&app, &project) else { continue };
        let Some(s) = p.shots.iter().find(|s| s.id == shot).cloned() else { continue };
        let model = with_queue(|q| q.model.clone()).unwrap_or_default();
        // A shot that continues the one before it starts from that shot's last picture.
        let first = if s.continues {
            let before = p
                .shots
                .iter()
                .position(|x| x.id == shot)
                .and_then(|i| i.checked_sub(1))
                .map(|i| clip_path(&p, &p.shots[i].id))
                .filter(|c| c.is_file());
            match before {
                Some(clip) => last_picture(&app, &clip, &p.size).await,
                None => None,
            }
        } else {
            portrait_start(&app, &p, &s)
        };
        // The seed the person asked for, for this shot or for the whole film, else a new one: it is kept with the take.
        let seed = s.seed.or(p.seed).unwrap_or_else(|| uuid::Uuid::new_v4().as_u128() as u32 as i64);
        // What the model is told: the description, the look of every character it names, the look of the film.
        let told = cast::told(&p.look, &p.cast, &s.prompt);
        // Another clip (from the chat) may be in the making: wait for the engine to be free.
        let job = loop {
            let resp = video::generate(
                State(app.clone()),
                Json(video::VideoBody::new(&model, &told, &p.size, s.frames).with_seed(seed).starting_from(first.clone())),
            )
            .await;
            let status = resp.status();
            let body = axum::body::to_bytes(resp.into_body(), 64 * 1024).await.unwrap_or_default();
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if let Some(job) = v["job"].as_str() {
                break Ok(job.to_string());
            }
            if status != StatusCode::CONFLICT {
                break Err(v["error"]["message"].as_str().unwrap_or("the shot could not be started").to_string());
            }
            // Stopped while waiting for a free engine.
            if !with_queue(|q| q.working) {
                break Err("stopped".into());
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        };
        let job = match job {
            Ok(job) => job,
            Err(e) => {
                set_shot(&app, &project, &shot, |s| s.fail(e));
                continue;
            }
        };
        let stop = with_queue(|q| {
            q.now = Some((project.clone(), shot.clone(), job.clone()));
            std::mem::take(&mut q.stop_now)
        });
        if stop {
            if let Some(j) = app.videos.lock().unwrap().get_mut(&job) {
                j.cancel = true;
            }
        }
        let (state, error, seconds, file) = loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let seen = app.videos.lock().unwrap().get(&job).map(|j| (j.state.clone(), j.error.clone(), j.elapsed_ms, j.video.clone()));
            match seen {
                Some((state, error, ms, video)) if state != "loading" && state != "running" => {
                    break (state, error, ms as f32 / 1000.0, video)
                }
                None => break ("failed".into(), Some("the shot was lost".into()), 0.0, None),
                _ => {}
            }
        };
        with_queue(|q| q.now = None);
        let from = file
            .as_ref()
            .and_then(|v| v["path"].as_str())
            .and_then(|path| path.rsplit('/').next())
            .map(|name| app.root.join("videos").join(name));
        let made = match from.filter(|_| state == "done") {
            Some(from) => {
                keep_take(
                    &app,
                    &project,
                    &shot,
                    &from,
                    Take {
                        n: 0,
                        seed: Some(seed),
                        seconds: Some(seconds),
                        frames: s.frames,
                        made_frames: None,
                        continued: first.is_some(),
                        ..Take::default()
                    },
                )
                .await
            }
            None => false,
        };
        if !made {
            set_shot(&app, &project, &shot, |s| {
                if state == "cancelled" {
                    // Stopped by the person: the shot is as it was before.
                    s.error = None;
                } else {
                    s.fail(error.unwrap_or_else(|| "the clip could not be saved in the film's folder".into()));
                }
            });
        }
        if made {
            app.record("studio_shot_made", json!({"frames": s.frames, "seconds": seconds, "continues": s.continues}));
            let mut seen = paces(&app);
            pace::remember(&mut seen, pace::Sample { frames: s.frames, continued: first.is_some(), seconds });
            std::fs::write(app.root.join("studio-pace.json"), serde_json::to_string(&seen).unwrap_or_default()).ok();
            stale_after(&app, &project, &shot);
        }
        // The last waiting shot of a film that exports by itself: join it now, with the screen open or not.
        if made && !with_queue(|q| q.waiting.iter().any(|(pid, _)| *pid == project)) {
            export_when_complete(&app, &project).await;
        }
    }
}

/// A made shot that continues `shot` started from a last picture that is not the last picture any more.
fn stale_after(app: &App, project: &str, shot: &str) {
    if let Some(mut p) = load(app, project) {
        let after = p.shots.iter().position(|x| x.id == shot).map(|i| i + 1);
        if let Some(next) = after.and_then(|i| p.shots.get_mut(i)).filter(|n| n.continues && n.state == "done") {
            next.state = "changed".into();
            store(&mut p).ok();
        }
    }
}

/// Puts a finished clip into the film's folder as a new take of its shot and puts it in use.
/// The clip the shot had stays as a take; a film from before takes has its one clip named as take 1.
async fn keep_take(app: &Shared, project: &str, shot: &str, from: &FsPath, mut take: Take) -> bool {
    let Some(p) = load(app, project) else { return false };
    let Some(now) = p.shots.iter().find(|s| s.id == shot) else { return false };
    let dir = FsPath::new(&p.folder).join("clips");
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let old = (dir.join(format!("{shot}.webm")), dir.join(format!("{shot}.jpg")));
    let adopted = (now.takes.is_empty() && old.0.is_file()).then(|| {
        let (clip, picture) = take_files(&p, shot, 1);
        crate::playable::rename(&old.0, &clip);
        std::fs::rename(&old.0, clip).ok();
        std::fs::rename(&old.1, picture).ok();
        Take { n: 1, seconds: now.seconds, frames: now.frames, made_frames: now.made_frames, continued: now.continues, ..Take::default() }
    });
    take.n = now.takes.iter().map(|t| t.n).max().unwrap_or(0).max(adopted.as_ref().map_or(0, |t| t.n)) + 1;
    let (clip, picture) = take_files(&p, shot, take.n);
    if std::fs::copy(from, &clip).is_err() {
        return false;
    }
    // What the clip really holds, and a small picture of it for the timeline.
    take.made_frames = look_at(app, &clip, &picture).await;
    // The copy the app plays, made while the next shot is being made.
    crate::playable::soon(app, clip);
    let mut gone = Vec::new();
    set_shot(app, project, shot, |s| {
        s.takes.extend(adopted);
        gone = s.add_take(take);
    });
    for n in gone {
        let (clip, picture) = take_files(&p, shot, n);
        crate::playable::remove(&clip);
        std::fs::remove_file(clip).ok();
        std::fs::remove_file(picture).ok();
    }
    true
}

#[derive(Deserialize)]
struct UseTake {
    take: u32,
}

async fn use_take(State(app): State<Shared>, Path((id, shot)): Path<(String, String)>, Json(body): Json<UseTake>) -> Response {
    let mut found = false;
    set_shot(&app, &id, &shot, |s| found = s.use_take(body.take));
    if !found {
        return fail(StatusCode::NOT_FOUND, "no such take");
    }
    stale_after(&app, &id, &shot);
    app.record("studio_take_used", json!({"take": body.take}));
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

async fn delete_take(State(app): State<Shared>, Path((id, shot, n)): Path<(String, String, u32)>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    if with_queue(|q| q.now.as_ref().is_some_and(|(pid, sid, _)| *pid == id && *sid == shot)) {
        return fail(StatusCode::CONFLICT, "This shot is being made. Stop it or wait for it first.");
    }
    let (mut found, mut used) = (false, false);
    set_shot(&app, &id, &shot, |s| {
        used = s.take == n;
        found = s.remove_take(n);
    });
    if !found {
        return fail(StatusCode::NOT_FOUND, "no such take");
    }
    let (clip, picture) = take_files(&p, &shot, n);
    crate::playable::remove(&clip);
    std::fs::remove_file(clip).ok();
    std::fs::remove_file(picture).ok();
    if used {
        stale_after(&app, &id, &shot);
    }
    app.record("studio_take_deleted", json!({"take": n}));
    match load(&app, &id) {
        Some(p) => Json(view(&app, p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film"),
    }
}

/// Exports a film that asked for it once every described shot is made.
async fn export_when_complete(app: &Shared, project: &str) {
    let Some(p) = load(app, project).filter(|p| p.auto_export) else { return };
    if p.shots.iter().any(|s| s.state != "done" && !s.prompt.is_empty()) {
        return;
    }
    if let Err((_, why)) = begin_export(app, p, Export::default()).await {
        set_export(project, json!({"state": "failed", "error": why}));
    }
}

/// The last picture of a clip as base64 JPEG, or `None` when the tool for it is missing or fails
/// (the shot is then made without it, with a cut).
async fn last_picture(app: &Shared, clip: &FsPath, size: &str) -> Option<String> {
    use base64::Engine as _;
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await.ok()?;
    let out = std::env::temp_dir().join(format!("rigbrew-last-{}.jpg", uuid::Uuid::new_v4().simple()));
    let status = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", "-0.2", "-i"])
        .arg(clip)
        // A clip brought in from a video can be larger than the film: the picture is the film's size.
        .args(["-frames:v", "1", "-update", "1", "-vf", &format!("scale={}", size.replace('x', ":")), "-q:v", "2"])
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .ok()?;
    let bytes = std::fs::read(&out).ok();
    std::fs::remove_file(&out).ok();
    status.success().then_some(())?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes?))
}

/// Where the newest portrait of a cast member is kept.
fn portrait_path(p: &Project, member: &Member) -> PathBuf {
    FsPath::new(&p.folder).join("cast").join(format!("{}.{}.png", member.id, member.portrait))
}

/// The portrait a shot starts from, as base64: the shot names a member for it, that member has
/// one, and the installed video model can start a clip from a picture.
fn portrait_start(app: &App, p: &Project, shot: &Shot) -> Option<String> {
    use base64::Engine as _;
    let member = p.cast.iter().find(|m| !shot.from.is_empty() && m.id == shot.from && m.portrait > 0)?;
    can_continue(app).then_some(())?;
    Some(base64::engine::general_purpose::STANDARD.encode(std::fs::read(portrait_path(p, member)).ok()?))
}

/// The number after the last "frame=" in what ffmpeg printed while it read a clip.
fn frames_read(text: &str) -> Option<u32> {
    let rest = text.rsplit("frame=").next().filter(|r| r.len() < text.len())?;
    rest.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok().filter(|n| *n > 0)
}

/// Reads a clip once: writes a small picture from its middle to `poster` and answers how many
/// frames it has. `None` when the tool for it is missing or cannot read the clip.
async fn look_at(app: &Shared, clip: &FsPath, poster: &FsPath) -> Option<u32> {
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await.ok()?;
    let counted = tokio::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-stats", "-i"])
        .arg(clip)
        .args(["-map", "0:v:0", "-f", "null", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let frames = frames_read(&String::from_utf8_lossy(&counted.stderr));
    let middle = format!("{:.3}", frames.unwrap_or(0) as f32 / 48.0);
    tokio::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &middle, "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-update", "1", "-vf", "scale=320:-2", "-q:v", "4"])
        .arg(poster)
        .stdin(std::process::Stdio::null())
        .status()
        .await
        .ok();
    frames
}

/// "<shot>.webm", "<shot>.jpg", or with a take: "<shot>.<n>.webm": the shot, the take, and whether it is the picture.
fn clip_name(name: &str) -> Option<(&str, Option<u32>, bool)> {
    let (rest, picture) = match name.rsplit_once('.')? {
        (rest, "webm") => (rest, false),
        (rest, "jpg") => (rest, true),
        _ => return None,
    };
    let (shot, take) = match rest.split_once('.') {
        Some((shot, n)) => (shot, Some(n.parse::<u32>().ok().filter(|n| (1..100_000).contains(n))?)),
        None => (rest, None),
    };
    valid_id(shot).then_some((shot, take, picture))
}

async fn clip(State(app): State<Shared>, Path((id, name)): Path<(String, String)>) -> Response {
    let Some((shot, take, picture)) = clip_name(&name) else {
        return fail(StatusCode::BAD_REQUEST, "bad clip name");
    };
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    let (clip, poster) = match take {
        Some(n) => take_files(&p, shot, n),
        None => (clip_path(&p, shot), poster_path(&p, shot)),
    };
    // A clip is asked for as ".webm" whatever it is: one brought in from a video is an MP4, and says
    // so; a made one is handed over as its playable copy where the webview needs one.
    let (file, kind) = if picture { (poster, "image/jpeg") } else { crate::playable::served(&app, &clip).await };
    // A film made before pictures were kept gets them when they are first asked for.
    if picture && !file.is_file() && clip.is_file() {
        let frames = look_at(&app, &clip, &file).await;
        if take.is_none() {
            set_shot(&app, &id, shot, |s| s.made_frames = s.made_frames.or(frames));
        }
    }
    match tokio::fs::read(file).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, kind)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "that shot has no clip yet"),
    }
}

/// The kinds of sound file the app plays under a film, by the end of their name.
fn sound_kind(path: &FsPath) -> Option<&'static str> {
    Some(match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" | "mp4" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "aif" | "aiff" => "audio/aiff",
        _ => return None,
    })
}

#[derive(Deserialize)]
struct SoundQuery {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    shot: Option<String>,
}

/// The file behind a name the app asks for: a sound file in the film's folder, one the person
/// added to the film, or one a region plays. Nothing else is read for a film.
fn sound_of(p: &Project, file: &str) -> Option<PathBuf> {
    let file = file.trim();
    let folder = FsPath::new(&p.folder);
    let path = sound::place(folder, file)?;
    let inside = path.starts_with(folder) && path.components().all(|c| !matches!(c, std::path::Component::ParentDir));
    let known = p.sounds.iter().any(|s| s == file) || p.tracks.iter().any(|t| t.regions.iter().any(|r| r.file == file));
    (inside || known).then_some(path)
}

async fn sound(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<SoundQuery>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film");
    };
    // Without a name: the one sound file of a film from before sound tracks.
    let path = match q.file.as_deref() {
        Some(file) => sound_of(&p, file),
        None => Some(PathBuf::from(&p.sound)).filter(|path| path.is_absolute()),
    };
    let Some((path, kind)) = path.and_then(|path| sound_kind(&path).map(|kind| (path, kind))) else {
        return fail(StatusCode::NOT_FOUND, "this film has no such sound file");
    };
    match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.len() > MAX_SOUND_BYTES => {
            return fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "This sound file is larger than 200 MB. It is in the exported film, but the app does not play it.",
            )
        }
        Ok(_) => {}
        Err(_) => return fail(StatusCode::NOT_FOUND, "the sound file was not found"),
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, kind)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => fail(StatusCode::NOT_FOUND, "the sound file was not found"),
    }
}

/// The sound files directly in a folder, by name.
fn sound_files_in(dir: &FsPath) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).map(|entries| entries.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    found.retain(|path| sound::is_sound(path) && path.is_file() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
    found.sort();
    found
}

/// The film's library of sounds: the files in its folder (and one folder down, the clips aside),
/// the ones added from elsewhere, and any other file a region plays.
fn library(p: &Project) -> Value {
    let folder = FsPath::new(&p.folder);
    let mut files: Vec<(PathBuf, &str)> = sound_files_in(folder).into_iter().map(|f| (f, "folder")).collect();
    let mut inner: Vec<PathBuf> = std::fs::read_dir(folder).map(|e| e.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    inner.retain(|d| d.is_dir() && !d.file_name().is_some_and(|n| n == "clips" || n.to_string_lossy().starts_with('.')));
    inner.sort();
    for dir in inner.iter().take(24) {
        files.extend(sound_files_in(dir).into_iter().map(|f| (f, "folder")));
    }
    for file in p.sounds.iter().chain(p.tracks.iter().flat_map(|t| t.regions.iter().map(|r| &r.file))) {
        if let Some(path) = sound::place(folder, file).filter(|path| !files.iter().any(|(f, _)| f == path)) {
            files.push((path, if p.sounds.contains(file) { "added" } else { "used" }));
        }
    }
    let list: Vec<Value> = files
        .into_iter()
        .take(1000)
        .map(|(path, place)| {
            let bytes = std::fs::metadata(&path).map(|m| m.len()).ok();
            json!({"file": path.to_string_lossy(), "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                   "bytes": bytes.unwrap_or(0), "from": place, "missing": bytes.is_none()})
        })
        .collect();
    json!({"sounds": list})
}

async fn sounds(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match load(&app, &id) {
        Some(p) => Json(library(&p)).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone"),
    }
}

#[derive(Deserialize)]
struct AddSound {
    path: String,
}

/// Adds a sound file to the film's library, or every sound file directly in a folder.
async fn add_sound(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<AddSound>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let path = PathBuf::from(body.path.trim());
    if !path.is_absolute() || body.path.len() > 1024 {
        return fail(StatusCode::BAD_REQUEST, "Give the sound file by its full path.");
    }
    let found = if path.is_dir() {
        sound_files_in(&path)
    } else if path.is_file() && sound::is_sound(&path) {
        vec![path]
    } else if path.is_file() {
        return fail(StatusCode::BAD_REQUEST, "That is not a sound file Studio can use. Use wav, mp3, m4a, aac, flac, ogg, opus or aiff.");
    } else {
        return fail(StatusCode::NOT_FOUND, "Nothing was found at that path.");
    };
    if found.is_empty() {
        return fail(StatusCode::NOT_FOUND, "There are no sound files in that folder.");
    }
    let folder = PathBuf::from(&p.folder);
    let mut added = Vec::new();
    for file in found {
        let text = file.to_string_lossy().to_string();
        // A file in the film's own folder is in the library already.
        if !file.starts_with(&folder) && !p.sounds.contains(&text) && p.sounds.len() < sound::MAX_SOUNDS {
            p.sounds.push(text.clone());
        }
        added.push(text);
    }
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    app.record("studio_sound_added", json!({"files": added.len()}));
    let mut answer = library(&p);
    answer["added"] = json!(added);
    Json(answer).into_response()
}

async fn remove_sound(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<SoundQuery>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let file = q.file.unwrap_or_default();
    p.sounds.retain(|s| *s != file);
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    Json(library(&p)).into_response()
}

/// Reads the sound of a file once, as ffmpeg decodes it, into its peaks and its length.
/// A file without sound has no peaks and no length; `None` when the tool is missing or cannot start.
async fn read_peaks(app: &Shared, file: &FsPath) -> Option<(Vec<i8>, f32)> {
    use tokio::io::AsyncReadExt;
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg").await.ok()?;
    let mut child = tokio::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-t", "21600", "-i"])
        .arg(file)
        .args(["-vn", "-map", "0:a:0?", "-ac", "1", "-ar", &sound::READ_RATE.to_string(), "-f", "s16le", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut peaks = sound::Peaks::default();
    let mut piece = vec![0u8; 64 * 1024];
    loop {
        match out.read(&mut piece).await {
            Ok(0) | Err(_) => break,
            Ok(n) => peaks.feed(&piece[..n]),
        }
    }
    child.wait().await.ok();
    Some(peaks.done())
}

/// The peaks of a clip's sound (`shot`) or of a sound file, read once and then kept next to the clips.
async fn peaks_of(app: &Shared, p: &Project, file: &FsPath, shot: Option<&str>) -> Option<(Vec<i8>, f32)> {
    let dir = FsPath::new(&p.folder).join("clips");
    let name = match shot {
        Some(shot) => format!("{}.peaks.json", clip_stem(p, shot)),
        None => {
            let meta = std::fs::metadata(file).ok()?;
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
            sound::peaks_name(file, meta.len(), modified)
        }
    };
    let kept = dir.join(name);
    let read = std::fs::read_to_string(&kept).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| {
        let pairs: Vec<i8> = v["peaks"].as_array()?.iter().filter_map(|n| n.as_i64().map(|n| n as i8)).collect();
        Some((pairs, v["seconds"].as_f64()? as f32))
    });
    if read.is_some() {
        return read;
    }
    if !file.is_file() {
        return None;
    }
    let (pairs, seconds) = read_peaks(app, file).await?;
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(kept, json!({"rate": sound::PEAK_RATE, "seconds": seconds, "peaks": pairs}).to_string()).ok();
    Some((pairs, seconds))
}

async fn peaks(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<SoundQuery>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let found = match (q.shot.as_deref(), q.file.as_deref()) {
        (Some(shot), _) if valid_id(shot) && p.shots.iter().any(|s| s.id == shot) => {
            peaks_of(&app, &p, &clip_path(&p, shot), Some(shot)).await
        }
        (None, Some(file)) => match sound_of(&p, file) {
            Some(path) => peaks_of(&app, &p, &path, None).await,
            None => return fail(StatusCode::NOT_FOUND, "this film has no such sound file"),
        },
        _ => return fail(StatusCode::BAD_REQUEST, "say which sound: \"shot\" or \"file\""),
    };
    match found {
        Some((pairs, seconds)) => Json(json!({"rate": sound::PEAK_RATE, "seconds": seconds, "peaks": pairs})).into_response(),
        None => fail(StatusCode::NOT_FOUND, "that sound was not found, or it could not be read"),
    }
}

/// The port this service listens on, for the requests it makes to its own chat and voice routes.
fn own_port(app: &App) -> Option<u64> {
    std::fs::read_to_string(app.root.join("daemon.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["port"].as_u64())
}

#[derive(Deserialize)]
struct Idea {
    idea: String,
    #[serde(default)]
    shots: Option<u32>,
    /// Every line of the idea is one shot, as it is written: nothing is asked of the chat model.
    #[serde(default)]
    as_written: bool,
}

/// The shots of a film, written by the chat model for the film as it is saved: with its cast
/// (named, never described), without its look (that is added when a shot is made), and with
/// spoken lines when the film has speech. Nothing is saved.
async fn storyboard(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Idea>) -> Response {
    let idea: String = body.idea.trim().chars().take(4000).collect();
    if idea.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "say what the film is about in \"idea\"");
    }
    let count = body.shots.unwrap_or(6).clamp(1, MAX_STORYBOARD as u32) as usize;
    // Shots are written to be made: without the video model nothing is written.
    if let Err(refusal) = need(&app, Modality::Video) {
        return *refusal;
    }
    let p = load(&app, &id).unwrap_or_default();
    let names: Vec<String> = p.cast.iter().map(|m| m.name.clone()).collect();
    let answer = |shots: Vec<board::Line>| {
        let list: Vec<Value> = shots
            .into_iter()
            .map(|s| {
                let lines: Vec<Value> = s
                    .said
                    .into_iter()
                    .map(|(who, text)| json!({"who": p.cast.iter().find(|m| m.name == who).map_or("", |m| m.id.as_str()), "text": text}))
                    .collect();
                json!({"prompt": s.prompt, "continues": s.continues, "lines": lines})
            })
            .collect();
        Json(json!({"shots": list})).into_response()
    };
    if body.as_written {
        let shots: Vec<board::Line> =
            brief::as_written(&idea, count).into_iter().map(|prompt| board::Line { prompt, ..board::Line::default() }).collect();
        if shots.is_empty() {
            return fail(StatusCode::BAD_REQUEST, "Write at least one shot, one on each line.");
        }
        app.record("studio_storyboard_written", json!({"shots": shots.len(), "scenes": shots.len(), "asked": count, "as_written": true}));
        return answer(shots);
    }
    if let Err(refusal) = need(&app, Modality::Chat) {
        return *refusal;
    }
    let language = dub::own_language(&p);
    let who: Vec<String> = p.cast.iter().map(|m| board::who_is(&m.look)).collect();
    let wants = board::Wants { cast: &names, who: &who, look: &p.look, speech: p.speech, language: &language };
    let plain = names.is_empty() && p.look.is_empty() && !p.speech;
    // A long film is written in parts, each part told what came before it.
    let mut shots: Vec<board::Line> = Vec::new();
    for _ in 0..count / board::PART + 3 {
        let wanted = (count - shots.len()).min(board::PART);
        if wanted == 0 {
            break;
        }
        let system = if plain { board::system_prompt(wanted) } else { board::system_prompt_for(wanted, &wants) };
        let tokens = (if p.speech { 190 } else { 140 }) * wanted as u32 + 200;
        let text = match ask(&app, &system, &board::part_request(&idea, &shots, wanted, count), tokens, 0.7).await {
            Ok(text) => text,
            // What is written so far is still a film.
            Err(_) if !shots.is_empty() => break,
            Err((status, why)) => return fail(status, why),
        };
        let part = board::shots_and_lines(&text, wanted, p.speech.then_some(names.as_slice()));
        if part.is_empty() {
            break;
        }
        shots.extend(part);
    }
    if shots.is_empty() {
        return fail(StatusCode::BAD_GATEWAY, "The chat model did not write a list of shots. Try again, or say more about the film.");
    }
    // A film starts with a cut.
    shots[0].continues = false;
    let scenes = shots.iter().filter(|s| !s.continues).count();
    let lines: usize = shots.iter().map(|s| s.said.len()).sum();
    app.record(
        "studio_storyboard_written",
        json!({"shots": shots.len(), "scenes": scenes, "asked": count, "cast": names.len(), "lines": lines, "look": !p.look.is_empty()}),
    );
    answer(shots)
}

#[derive(Deserialize, Default)]
struct Rewrite {
    /// What the person wants different: "closer", "at night".
    #[serde(default)]
    hint: String,
    /// The description as it stands on the screen, when it is not saved yet.
    #[serde(default)]
    prompt: Option<String>,
}

/// The chat model writes the description of one shot again, in the words of the shots around it.
async fn rewrite(State(app): State<Shared>, Path((id, shot)): Path<(String, String)>, body: Option<Json<Rewrite>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let Some(at) = p.shots.iter().position(|s| s.id == shot) else {
        return fail(StatusCode::NOT_FOUND, "no such shot");
    };
    let body = body.map(|b| b.0).unwrap_or_default();
    let now: String = body.prompt.unwrap_or_else(|| p.shots[at].prompt.clone()).trim().chars().take(MAX_PROMPT).collect();
    let hint: String = body.hint.trim().chars().take(400).collect();
    if now.is_empty() && hint.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "Write a few words about the shot first.");
    }
    // The shots around it say who and where: up to three before it and one after.
    let before: Vec<&str> = p.shots[at.saturating_sub(3)..at].iter().map(|s| s.prompt.as_str()).filter(|t| !t.is_empty()).collect();
    let after = p.shots.get(at + 1).map(|s| s.prompt.as_str()).filter(|t| !t.is_empty());
    let names: Vec<String> = p.cast.iter().map(|m| m.name.clone()).collect();
    let asked = board::rewrite_request(&before, after, &now, &hint, p.shots[at].continues && at > 0);
    let text = match ask(&app, &board::rewrite_prompt_for(&names), &asked, 320, 0.6).await {
        Ok(text) => text,
        Err((status, why)) => return fail(status, why),
    };
    match board::one_description(&text) {
        Some(prompt) => {
            app.record("studio_shot_rewritten", json!({"hint": !hint.is_empty(), "chars": prompt.chars().count()}));
            Json(json!({"prompt": prompt})).into_response()
        }
        None => fail(StatusCode::BAD_GATEWAY, "The chat model did not write a description. Try again."),
    }
}

/// The export of each film: "working", "done" or "failed", with the file or what went wrong.
static EXPORTS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());

fn export_of(id: &str) -> Value {
    EXPORTS.lock().unwrap().iter().find(|(i, _)| i == id).map(|(_, v)| v.clone()).unwrap_or_else(|| json!({"state": "none"}))
}

fn set_export(id: &str, value: Value) {
    let mut all = EXPORTS.lock().unwrap();
    all.retain(|(i, _)| i != id);
    all.push((id.to_string(), value));
}

#[derive(Deserialize, Default)]
struct Export {
    /// A sound file to lay under the whole film (a full path), mixed with the shots' own sound.
    #[serde(default)]
    music: Option<String>,
    /// 0 to 1; how loud the music is next to the shots' sound. Default 0.5.
    #[serde(default)]
    music_volume: Option<f32>,
    /// For this export only, instead of the film's own options.
    #[serde(default)]
    scale: Option<u32>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Where a film is exported to: its folder, under the name chosen for the file or the film's own.
fn export_path(p: &Project, options: &ExportOptions) -> PathBuf {
    let name = if options.name.is_empty() { file_stem(&p.name) } else { file_stem(&options.name) };
    FsPath::new(&p.folder).join(format!("{name}.mp4"))
}

/// A file name made of the film's name: letters, digits, spaces, dashes.
fn file_stem(name: &str) -> String {
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { ' ' }).collect();
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    if stem.is_empty() {
        "film".into()
    } else {
        stem.chars().take(80).collect()
    }
}

/// The made shots of a film whose clip is in its folder, in order: what an export holds.
fn exported(p: &Project) -> Vec<&Shot> {
    p.shots.iter().filter(|s| (s.state == "done" || s.state == "changed") && clip_path(p, &s.id).is_file()).collect()
}

/// The concat list ffmpeg reads for a film of plain cuts: every shot with its trims.
fn concat_list(p: &Project, shots: &[&Shot]) -> String {
    let mut list = String::from("ffconcat version 1.0\n");
    for s in shots {
        let length = s.length();
        list.push_str(&format!("file '{}'\n", clip_path(p, &s.id).to_string_lossy().replace('\'', "'\\''")));
        if s.trim_start > 0.0 {
            list.push_str(&format!("inpoint {:.3}\n", s.trim_start));
        }
        if s.trim_end > 0.0 {
            list.push_str(&format!("outpoint {:.3}\n", (length - s.trim_end).max(s.trim_start + 0.1)));
        }
    }
    list
}

/// The shots as the pieces of the film: what is kept of each clip, how it joins, how loud it is.
fn pieces(shots: &[&Shot], sounds: &[bool], fit: Option<(u32, u32)>) -> Vec<export::Piece> {
    shots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let whole = s.length();
            let start = s.trim_start.clamp(0.0, (whole - 0.1).max(0.0));
            let kept = (whole - start - s.trim_end.max(0.0)).max(0.1);
            export::Piece {
                start,
                frames: ((kept * 24.0).round() as u32).max(1),
                // A shot that continues the one before it joins it with nothing in between.
                join: if i == 0 || s.continues { export::Join::Cut } else { export::Join::named(&s.join) },
                volume: if s.mute { 0.0 } else { s.volume.clamp(0.0, 1.5) },
                has_sound: sounds.get(i).copied().unwrap_or(true),
                join_frames: ((s.join_seconds.clamp(0.25, 2.0) * 24.0).round() as u32).max(2),
                fit,
            }
        })
        .collect()
}

/// Whether a clip has sound in it, as ffmpeg lists its streams.
async fn has_sound(ffmpeg: &FsPath, clip: &FsPath) -> bool {
    let listed =
        tokio::process::Command::new(ffmpeg).args(["-hide_banner", "-i"]).arg(clip).stdin(std::process::Stdio::null()).output().await;
    listed.map(|o| String::from_utf8_lossy(&o.stderr).contains("Audio:")).unwrap_or(true)
}

async fn export(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Export>>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    match begin_export(&app, p, body.map(|b| b.0).unwrap_or_default()).await {
        Ok(count) => (StatusCode::ACCEPTED, Json(json!({"state": "working", "shots": count}))).into_response(),
        Err((status, why)) => fail(status, why),
    }
}

/// Starts joining the made shots of a film into its MP4; answers with how many shots go in.
async fn begin_export(app: &Shared, p: Project, body: Export) -> Result<usize, (StatusCode, String)> {
    let mut p = with_tracks(app, p).await;
    let id = p.id.clone();
    if exported(&p).is_empty() {
        return Err((StatusCode::CONFLICT, "No shot is made yet. Make the shots first.".into()));
    }
    // The film's own sound file, unless the request names another.
    let music = match body.music.as_deref().or(Some(p.sound.as_str())).map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if FsPath::new(m).is_absolute() && FsPath::new(m).is_file() => Some(m.to_string()),
        Some(_) => {
            return Err((StatusCode::BAD_REQUEST, "The sound file of this film was not found. Choose it again, or take it out.".into()))
        }
        None => None,
    };
    if EXPORTS.lock().unwrap().iter().any(|(i, v)| *i == id && v["state"] == "working") {
        return Err((StatusCode::CONFLICT, "This film is being exported already.".into()));
    }
    let ffmpeg = crate::engine_tool(app, crate::EngineKind::Ffmpeg, "ffmpeg")
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("The tool that joins the shots could not be set up: {e}")))?;
    set_export(&id, json!({"state": "working", "shots": exported(&p).len()}));
    // A clip from before lengths were kept is measured now: joins are counted in frames.
    let unmeasured: Vec<String> = exported(&p).iter().filter(|s| s.made_frames.is_none()).map(|s| s.id.clone()).collect();
    for shot in unmeasured {
        let frames = look_at(app, &clip_path(&p, &shot), &poster_path(&p, &shot)).await;
        set_shot(app, &id, &shot, |s| s.made_frames = frames);
        if let Some(s) = p.shots.iter_mut().find(|s| s.id == shot) {
            s.made_frames = frames;
        }
    }
    // The film as it is saved now is what the file holds: an edit after this makes it out of date.
    let edited = load(app, &id).map_or(p.updated_ms, |now| now.updated_ms);
    let shots = exported(&p);
    let count = shots.len();
    let clips: Vec<PathBuf> = shots.iter().map(|s| clip_path(&p, &s.id)).collect();
    let mut sounds = Vec::with_capacity(count);
    for clip in &clips {
        sounds.push(has_sound(&ffmpeg, clip).await);
    }
    let options = ExportOptions {
        scale: body.scale.unwrap_or(p.export.scale),
        quality: body.quality.clone().unwrap_or_else(|| p.export.quality.clone()),
        name: body.name.clone().unwrap_or_else(|| p.export.name.clone()),
    }
    .tidy();
    // A film that holds clips brought in from a video holds clips of another kind, and maybe of
    // twice the size: every clip is brought to one size as it is joined. An enlarged export keeps
    // what the larger clips have.
    let takes: Vec<&Take> = shots.iter().filter_map(|s| take_in_use(&p, &s.id)).collect();
    let brought = takes.iter().any(|t| !t.ext.is_empty());
    let keeps_large = options.scale == 2 && takes.iter().any(|t| t.large);
    let fit = brought.then(|| dub::export_size(&p, if keeps_large { 2 } else { 0 }));
    let pieces = pieces(&shots, &sounds, fit);
    // A film of plain cuts is read as one list; anything more is a filter graph over every clip.
    let simple = export::plain(&pieces);
    let film = if simple { export::as_it_is(export::film_frames(&pieces)) } else { export::joined(&pieces) };
    let volume = body.music_volume.unwrap_or(p.sound_volume).clamp(0.0, 1.0);
    let (video_bits, sound_bits) = options.bitrates(&p.size);
    // The files the sound tracks play come after the clips (and the one sound file, when there is one).
    let first_sound = if simple { 1 } else { count } + usize::from(music.is_some());
    let mut sound_files: Vec<PathBuf> = Vec::new();
    let mut missing: Option<String> = None;
    let mix = sound::heard(&p.shot_track, &p.tracks, p.master, p.duck, |file| {
        let path = sound::place(FsPath::new(&p.folder), file).filter(|path| path.is_file());
        let Some(path) = path else {
            missing.get_or_insert_with(|| file.rsplit(['/', '\\']).next().unwrap_or(file).to_string());
            return None;
        };
        let at = sound_files.iter().position(|f| *f == path).unwrap_or_else(|| {
            sound_files.push(path);
            sound_files.len() - 1
        });
        Some(first_sound + at)
    });
    if let Some(name) = missing {
        let why = format!("The sound file {name} was not found. Put it back where it was, or delete its regions from the film.");
        set_export(&id, json!({"state": "failed", "error": why}));
        return Err((StatusCode::BAD_REQUEST, why));
    }
    let (track_count, region_count) = (p.tracks.len(), mix.sounds.len());
    let folder = PathBuf::from(&p.folder);
    let out = export_path(&p, &options);
    // The subtitles: always a file next to the film; drawn into the pictures when that was asked
    // for and the app drew them for this film at this size; else a track a player turns on.
    let cues = dub::subtitles(&p, true);
    let size = dub::export_size(&p, options.scale);
    let burned = if p.subtitles.burn { dub::drawn(&p, &cues, size) } else { None };
    let track = p.subtitles.show && burned.is_none() && !cues.is_empty();
    let srt_path = out.with_extension("srt");
    let captions_input = first_sound + sound_files.len();
    let finish = export::Finish {
        fade_in: p.fade_in,
        fade_out: p.fade_out,
        scale: if keeps_large { 0 } else { options.scale },
        music: music.as_ref().map(|_| (if simple { 1 } else { count }, volume)),
        mix: Some(mix),
        captions: burned.as_ref().map(|(_, note)| (captions_input, size.1.saturating_sub(note.height))),
    };
    let film_seconds = film.frames as f32 / 24.0;
    let graph = export::finished(film, &finish);
    let captions_list = burned.as_ref().map(|(dir, _)| dir.join("list.txt"));
    if let Some(list) = &captions_list {
        std::fs::write(list, dub::caption_list(&cues, film_seconds)).ok();
    }
    if cues.is_empty() {
        std::fs::remove_file(&srt_path).ok();
    } else if let Err(e) = std::fs::write(&srt_path, words::srt(&cues)) {
        set_export(&id, json!({"state": "failed", "error": format!("the film's folder cannot be written to: {e}")}));
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("the film's folder cannot be written to: {e}")));
    }
    let subtitles = if cues.is_empty() {
        "none"
    } else if burned.is_some() {
        "burned"
    } else if track {
        "track"
    } else {
        "file"
    };
    let language = words::language_code(&dub::shown_language(&p));
    let list_path = folder.join(".rigbrew-export.txt");
    let graph_path = folder.join(".rigbrew-export-graph.txt");
    let written = if simple { std::fs::write(&list_path, concat_list(&p, &shots)) } else { Ok(()) }.and_then(|()| {
        if graph.filter.is_empty() {
            Ok(())
        } else {
            std::fs::write(&graph_path, &graph.filter)
        }
    });
    if let Err(e) = written {
        set_export(&id, json!({"state": "failed", "error": format!("the film's folder cannot be written to: {e}")}));
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("the film's folder cannot be written to: {e}")));
    }
    let joins = (
        pieces.iter().skip(1).filter(|x| x.join == export::Join::Dissolve).count(),
        pieces.iter().skip(1).filter(|x| x.join == export::Join::Black).count(),
    );
    let once = p.sound_once;
    let app2 = app.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let tmp = out.with_extension("part.mp4");
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        if simple {
            cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&list_path);
        } else {
            for clip in &clips {
                cmd.arg("-i").arg(clip);
            }
        }
        if let Some(m) = &music {
            // Music repeats under the whole film; a narration is said once. Either ends with the film.
            if !once {
                cmd.args(["-stream_loop", "-1"]);
            }
            cmd.arg("-i").arg(m);
        }
        for file in &sound_files {
            cmd.arg("-i").arg(file);
        }
        if let Some(list) = &captions_list {
            cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(list);
        }
        if track {
            cmd.arg("-i").arg(&srt_path);
        }
        if !graph.filter.is_empty() {
            // The graph of a long film is longer than a command line may be: it is read from a file.
            cmd.arg("-/filter_complex").arg(&graph_path);
        }
        if !graph.filter.is_empty() || track {
            cmd.args(["-map", &graph.video, "-map", &graph.audio]);
        }
        if track {
            // The subtitle file is the last input: one text track, named by its language.
            let input = captions_input + usize::from(captions_list.is_some());
            cmd.args(["-map", &format!("{input}:0"), "-c:s", "mov_text", "-metadata:s:s:0", &format!("language={language}")]);
        }
        // Apple's own encoders: H.264 in hardware and AAC. Plays everywhere.
        cmd.args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            &video_bits.to_string(),
            "-pix_fmt",
            "yuv420p",
            "-r",
            "24",
            "-c:a",
            "aac",
            "-b:a",
            &sound_bits.to_string(),
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(&tmp);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        let result = match cmd.output().await {
            Ok(o) if o.status.success() => std::fs::rename(&tmp, &out).map_err(|e| e.to_string()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr)
                .lines()
                .last()
                .unwrap_or("the shots could not be joined")
                .chars()
                .take(300)
                .collect()),
            Err(e) => Err(e.to_string()),
        };
        std::fs::remove_file(&list_path).ok();
        std::fs::remove_file(&graph_path).ok();
        match result {
            Ok(()) => {
                let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                app2.record(
                    "studio_exported",
                    json!({"shots": count, "seconds": started.elapsed().as_secs_f32(), "bytes": bytes, "music": music.is_some(),
                           "film_seconds": graph.frames as f32 / 24.0, "dissolves": joins.0, "through_black": joins.1, "plain": simple,
                           "tracks": track_count, "regions": region_count, "subtitles": subtitles,
                           "scale": options.scale.max(1), "quality": if options.quality.is_empty() { "balanced" } else { options.quality.as_str() }}),
                );
                set_export(
                    &id,
                    json!({"state": "done", "file": out.to_string_lossy(), "bytes": bytes, "shots": count, "seconds": graph.frames as f32 / 24.0,
                           "subtitles": subtitles, "edited_ms": edited}),
                );
            }
            Err(e) => {
                std::fs::remove_file(&tmp).ok();
                app2.record("studio_export_failed", json!({"shots": count, "plain": simple}));
                set_export(&id, json!({"state": "failed", "error": e}));
            }
        }
    });
    Ok(count)
}

/// Shows a file in the system's file manager: selected in its folder where the system can do
/// that, else the folder it is in.
async fn show_in_folder(path: &FsPath) -> bool {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = tokio::process::Command::new("open");
        c.arg("-R").arg(path);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        c
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut command = {
        let mut c = tokio::process::Command::new("xdg-open");
        c.arg(if path.is_dir() { path } else { path.parent().unwrap_or(path) });
        c
    };
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // Explorer answers with a failure even when it opened the folder: starting it is what counts.
    matches!(tokio::time::timeout(Duration::from_secs(10), command.status()).await, Ok(Ok(_)))
}

/// Shows the film's exported file where the person can pick it up; its folder when there is no file yet.
async fn reveal(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    // The file the last export of this run wrote, else the one the film's options name.
    let last = export_of(&id)["file"].as_str().map(PathBuf::from);
    let file = last.filter(|f| f.is_file()).unwrap_or_else(|| export_path(&p, &p.export));
    let shown = if file.is_file() { file } else { PathBuf::from(&p.folder) };
    if show_in_folder(&shown).await {
        Json(json!({"shown": shown.to_string_lossy()})).into_response()
    } else {
        fail(StatusCode::INTERNAL_SERVER_ERROR, "the folder could not be opened")
    }
}

#[derive(Deserialize)]
struct Narration {
    text: String,
    /// Seconds into the film where the reading starts; its start without one.
    #[serde(default)]
    at: Option<f32>,
    /// The track it goes on; without one, the track named "Narration", made when there is none.
    #[serde(default)]
    track: Option<String>,
}

/// The first of "narration.wav", "narration 2.wav", ... that is not in the folder yet.
fn narration_file(folder: &FsPath) -> PathBuf {
    (1..)
        .map(|n| folder.join(if n == 1 { "narration.wav".to_string() } else { format!("narration {n}.wav") }))
        .find(|file| !file.exists())
        .unwrap_or_else(|| folder.join("narration.wav"))
}

/// The voice package reads a text; the reading is saved in the film's folder and set on a
/// narration track as one region. Answers with the film, and which region it is.
async fn narration(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<Narration>) -> Response {
    let Some(mut p) = load(&app, &id) else {
        return fail(StatusCode::NOT_FOUND, "no such film, or its folder is gone");
    };
    let text: String = body.text.trim().chars().take(MAX_NARRATION).collect();
    if text.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "write what is to be said in \"text\"");
    }
    let voice = match need(&app, Modality::Voice) {
        Ok(package) => package,
        Err(refusal) => return *refusal,
    };
    let Some(port) = own_port(&app) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "the service does not know its own address");
    };
    let answer = app
        .http
        .post(format!("http://127.0.0.1:{port}/v1/audio/speech"))
        .bearer_auth(&app.token)
        .json(&json!({"model": voice.id, "input": text, "voice": p.voice.unwrap_or(0)}))
        .timeout(Duration::from_secs(900))
        .send()
        .await;
    let wav = match answer {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
        },
        Ok(r) => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            return fail(
                StatusCode::BAD_GATEWAY,
                v["error"]["message"].as_str().unwrap_or("the voice could not read the text").to_string(),
            );
        }
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let file = narration_file(FsPath::new(&p.folder));
    if let Err(e) = std::fs::write(&file, &wav) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the narration could not be saved in the film's folder: {e}"));
    }
    let Some(seconds) = peaks_of(&app, &p, &file, None).await.map(|(_, seconds)| seconds).filter(|s| *s > 0.0) else {
        return fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The reading was saved in the film's folder, but it could not be measured. Add it from the Library.",
        );
    };
    // The film was open while the voice read: take it as it is now.
    if let Some(now) = load(&app, &id) {
        p = now;
    }
    let wanted = body.track.as_deref().and_then(|t| p.tracks.iter().position(|x| x.id == t));
    let at = match wanted.or_else(|| p.tracks.iter().position(|t| t.name.eq_ignore_ascii_case("narration"))) {
        Some(at) => at,
        None if p.tracks.len() >= sound::MAX_TRACKS => {
            return fail(StatusCode::CONFLICT, "This film has as many sound tracks as it can hold.")
        }
        None => {
            p.tracks.push(Track {
                id: new_id(),
                name: "Narration".into(),
                kind: "sound".into(),
                lang: String::new(),
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
    let region = Region {
        id: new_id(),
        file: file.to_string_lossy().to_string(),
        start: body.at.filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 6.0 * 3600.0),
        offset: 0.0,
        length: seconds,
        gain: 1.0,
        fade_in: 0.0,
        fade_out: 0.0,
        loops: 1,
        line: String::new(),
    };
    let placed = json!({"track": p.tracks[at].id, "id": region.id, "seconds": seconds});
    p.tracks[at].regions.push(region);
    if let Err(e) = store(&mut p) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("the film could not be saved: {e}"));
    }
    app.record("studio_narration_made", json!({"package": voice.id, "chars": text.chars().count(), "bytes": wav.len()}));
    app.record("studio_region_added", json!({"kind": "narration", "regions": p.tracks.iter().map(|t| t.regions.len()).sum::<usize>()}));
    let mut answer = view(&app, p);
    answer["region"] = placed;
    Json(answer).into_response()
}

async fn export_state(Path(id): Path<String>) -> Json<Value> {
    Json(export_of(&id))
}

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/v1/studio/projects", get(list).post(create))
        .route("/v1/studio/projects/{id}", get(read).put(save).delete(forget))
        .route("/v1/studio/projects/{id}/render", post(render).delete(stop))
        .route("/v1/studio/projects/{id}/storyboard", post(storyboard))
        .route("/v1/studio/projects/{id}/export", get(export_state).post(export))
        .route("/v1/studio/projects/{id}/reveal", post(reveal))
        .route("/v1/studio/projects/{id}/sound", get(sound))
        .route("/v1/studio/projects/{id}/sounds", get(sounds).post(add_sound).delete(remove_sound))
        .route("/v1/studio/projects/{id}/peaks", get(peaks))
        .route("/v1/studio/projects/{id}/shots/{shot}/rewrite", post(rewrite))
        .route("/v1/studio/projects/{id}/narration", post(narration))
        .route("/v1/studio/projects/{id}/shots/{shot}/take", post(use_take))
        .route("/v1/studio/projects/{id}/shots/{shot}/takes/{n}", axum::routing::delete(delete_take))
        .route("/v1/studio/projects/{id}/clips/{name}", get(clip))
        .route("/v1/studio/projects/{id}/media", post(play::media))
        .route("/v1/studio/brief", post(direct::brief))
        .route("/v1/studio/projects/{id}/import", get(bring::state).post(bring::import))
        .route("/v1/studio/projects/{id}/cast/look", post(direct::look))
        .route("/v1/studio/projects/{id}/cast/{member}/portrait", get(direct::portrait).post(direct::make_portrait))
        .route("/v1/studio/projects/{id}/dub", get(dub::dub_state).post(dub::dub))
        .route("/v1/studio/projects/{id}/listen", get(dub::listen_state).post(dub::listen))
        .route("/v1/studio/projects/{id}/subtitles", get(dub::subtitles_get))
        .route(
            "/v1/studio/projects/{id}/captions",
            axum::routing::put(dub::caption_put).layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn shot(id: &str, prompt: &str, state: &str) -> Shot {
        Shot {
            id: id.into(),
            prompt: prompt.into(),
            frames: 121,
            state: state.into(),
            error: None,
            seconds: Some(200.0),
            ..Shot::default()
        }
    }

    #[test]
    fn a_shot_keeps_its_clip_until_its_description_changes() {
        let old = vec![shot("a1", "a fox in snow", "done"), shot("b2", "waves at dusk", "done")];
        let new = vec![
            ShotIn {
                id: Some("b2".into()),
                prompt: "waves at dusk".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
            ShotIn {
                id: Some("a1".into()),
                prompt: "a fox in rain".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
            ShotIn {
                id: None,
                prompt: "a new one".into(),
                frames: None,
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            },
        ];
        let out = merge_shots(&old, new, false).unwrap();
        assert_eq!(out.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["done", "changed", "empty"]);
        assert_eq!(out[0].id, "b2");
        assert!(valid_id(&out[2].id));
        // A new shape means every made shot has to be made again.
        let again = merge_shots(
            &old,
            vec![ShotIn {
                id: Some("a1".into()),
                prompt: "a fox in snow".into(),
                frames: Some(121),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            }],
            true,
        )
        .unwrap();
        assert_eq!(again[0].state, "changed");
    }

    #[test]
    fn shots_are_bounded_and_ids_cannot_repeat_or_leave_the_folder() {
        assert!(merge_shots(
            &[],
            vec![ShotIn {
                id: None,
                prompt: "x".into(),
                frames: Some(999),
                trim_start: 0.0,
                trim_end: 0.0,
                continues: false,
                ..ShotIn::default()
            }],
            false
        )
        .is_err());
        let twice = merge_shots(
            &[],
            vec![
                ShotIn {
                    id: Some("a1".into()),
                    prompt: "one".into(),
                    frames: None,
                    trim_start: 0.0,
                    trim_end: 0.0,
                    continues: false,
                    ..ShotIn::default()
                },
                ShotIn {
                    id: Some("a1".into()),
                    prompt: "two".into(),
                    frames: None,
                    trim_start: 0.0,
                    trim_end: 0.0,
                    continues: false,
                    ..ShotIn::default()
                },
            ],
            false,
        )
        .unwrap();
        assert_ne!(twice[0].id, twice[1].id);
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("A1") && valid_id("a1b2"));
        assert!(valid_size("640x384") && !valid_size("640x385") && !valid_size("64x64") && !valid_size("wide"));
    }

    #[test]
    fn the_export_list_has_made_shots_in_order_with_their_trims() {
        let dir = std::env::temp_dir().join(format!("rigbrew-studio-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        for id in ["a1", "c3"] {
            std::fs::write(dir.join("clips").join(format!("{id}.webm")), b"x").unwrap();
        }
        let mut second = shot("c3", "waves", "done");
        second.trim_start = 0.5;
        second.trim_end = 1.0;
        let p = Project {
            id: "p".into(),
            name: "A film: take 2!".into(),
            folder: dir.to_string_lossy().into(),
            size: "640x384".into(),
            shots: vec![shot("a1", "fox", "done"), shot("b2", "not made", "empty"), second],
            ..Project::default()
        };
        let made = exported(&p);
        assert_eq!(made.len(), 2);
        let list = concat_list(&p, &made);
        let lines: Vec<&str> = list.lines().collect();
        assert!(lines[0].starts_with("ffconcat") && lines[1].ends_with("a1.webm'") && lines[2].ends_with("c3.webm'"));
        assert_eq!(&lines[3..], ["inpoint 0.500", "outpoint 4.042"]);
        assert_eq!(file_stem(&p.name), "A film take 2");
        assert_eq!(file_stem("///"), "film");
        // Trims always leave part of the shot.
        let kept = merge_shots(
            &[],
            vec![ShotIn {
                id: None,
                prompt: "x".into(),
                frames: Some(49),
                trim_start: 9.0,
                trim_end: 9.0,
                continues: false,
                ..ShotIn::default()
            }],
            false,
        )
        .unwrap();
        assert!(kept[0].trim_start <= 49.0 / 24.0 - 0.5 && kept[0].trim_end == 0.0);
        // A clip is as long as the model made it, which is what the trims are held against.
        let mut long = shot("d4", "x", "done");
        long.frames = 49;
        long.made_frames = Some(56);
        assert_eq!(long.length(), 56.0 / 24.0);
        long.state = "empty".into();
        assert_eq!(long.length(), 49.0 / 24.0);
        std::fs::remove_dir_all(dir).ok();
    }

    fn asked(id: &str, prompt: &str, continues: bool) -> ShotIn {
        ShotIn {
            id: Some(id.into()),
            prompt: prompt.into(),
            frames: Some(121),
            trim_start: 0.0,
            trim_end: 0.0,
            continues,
            ..ShotIn::default()
        }
    }

    #[test]
    fn a_shot_that_continues_another_is_made_again_after_a_move() {
        let mut b = shot("b2", "the fox stops", "done");
        b.continues = true;
        let mut d = shot("d4", "the boat turns", "done");
        d.continues = true;
        let old = vec![shot("a1", "a fox", "done"), b, shot("c3", "a boat", "done"), d];
        let states = |order: &[(&str, &str, bool)]| {
            let new = order.iter().map(|(id, prompt, continues)| asked(id, prompt, *continues)).collect();
            merge_shots(&old, new, false).unwrap().into_iter().map(|s| (s.state, s.continues)).collect::<Vec<_>>()
        };
        let (done, changed) = (|c| ("done".to_string(), c), |c| ("changed".to_string(), c));
        // Nothing moved: nothing changes.
        assert_eq!(
            states(&[("a1", "a fox", false), ("b2", "the fox stops", true), ("c3", "a boat", false), ("d4", "the boat turns", true)]),
            [done(false), done(true), done(false), done(true)]
        );
        // The two scenes change places as a whole: each shot still follows the same one.
        assert_eq!(
            states(&[("c3", "a boat", false), ("d4", "the boat turns", true), ("a1", "a fox", false), ("b2", "the fox stops", true)]),
            [done(false), done(true), done(false), done(true)]
        );
        // A continuing shot behind a different shot is made again; so is one that lost its place behind the first.
        assert_eq!(
            states(&[("a1", "a fox", false), ("c3", "a boat", false), ("b2", "the fox stops", true), ("d4", "the boat turns", true)]),
            [done(false), done(false), changed(true), changed(true)]
        );
        // The first shot of a film continues nothing.
        assert_eq!(states(&[("b2", "the fox stops", true), ("a1", "a fox", false)]), [changed(false), done(false)]);
    }

    #[test]
    fn the_frames_of_a_clip_are_read_from_what_the_tool_prints() {
        assert_eq!(frames_read("frame=   12 fps=0.0 q=-0.0 size=N/A\rframe=   56 fps=0.0 q=-0.0 Lsize=N/A time=00:00:02.33"), Some(56));
        assert_eq!(frames_read("frame=121 fps=30"), Some(121));
        assert_eq!(frames_read("clip.webm: Invalid data found when processing input"), None);
        assert_eq!(frames_read(""), None);
    }

    #[test]
    fn the_pieces_of_a_film_follow_its_shots() {
        let mut a = shot("a1", "fox", "done");
        a.made_frames = Some(56);
        a.join = "dissolve".into();
        let mut b = a.clone();
        b.trim_start = 0.5;
        b.trim_end = 0.75;
        b.volume = 1.4;
        let mut c = a.clone();
        c.continues = true;
        c.mute = true;
        let mut d = a.clone();
        d.join = "black".into();
        let made = pieces(&[&a, &b, &c, &d], &[true, true, true, false], None);
        // The first shot has no join; a shot that continues the one before it is always a cut.
        assert_eq!(
            made.iter().map(|x| x.join).collect::<Vec<_>>(),
            [export::Join::Cut, export::Join::Dissolve, export::Join::Cut, export::Join::Black]
        );
        assert_eq!(made.iter().map(|x| x.frames).collect::<Vec<_>>(), [56, 26, 56, 56]);
        assert_eq!(made[1].start, 0.5);
        assert_eq!(made.iter().map(|x| x.volume).collect::<Vec<_>>(), [1.0, 1.4, 0.0, 1.0]);
        assert!(!made[3].has_sound && !export::plain(&made));
        // Joins and volume are kept by a save and leave the clip as it is.
        let kept = merge_shots(
            &[a.clone(), b.clone()],
            vec![
                ShotIn { join: "black".into(), volume: Some(9.0), mute: true, ..asked("a1", "fox", false) },
                ShotIn { join: "wipe".into(), ..asked("b2", "fox", false) },
            ],
            false,
        )
        .unwrap();
        assert_eq!((kept[0].state.as_str(), kept[0].join.as_str(), kept[0].volume, kept[0].mute), ("done", "black", 1.5, true));
        assert_eq!((kept[1].join.as_str(), kept[1].volume), ("", 1.0));
    }

    fn take(n: u32, frames: u32) -> Take {
        Take {
            n,
            seed: Some(n as i64 * 11),
            seconds: Some(n as f32 * 10.0),
            frames,
            made_frames: Some(frames + 7),
            continued: false,
            ..Take::default()
        }
    }

    #[test]
    fn a_shot_keeps_its_last_takes() {
        let mut s = shot("a1", "fox", "changed");
        assert!(s.add_take(take(1, 49)).is_empty());
        assert_eq!((s.take, s.state.as_str(), s.made_frames, s.seconds), (1, "done", Some(56), Some(10.0)));
        for n in 2..=4 {
            assert!(s.add_take(take(n, 49)).is_empty());
        }
        // The fifth take pushes out the oldest; the one in use is never the one to go.
        assert_eq!(s.add_take(take(5, 121)), [1]);
        assert_eq!(s.takes.iter().map(|t| t.n).collect::<Vec<_>>(), [2, 3, 4, 5]);
        assert!(s.use_take(2) && !s.use_take(1));
        assert_eq!((s.take, s.frames, s.made_frames, s.seconds), (2, 49, Some(56), Some(20.0)));
        assert_eq!(s.add_take(take(6, 49)), [2]);
        assert_eq!(s.take, 6);
        // Trims that fitted a long take are held against the shorter one.
        s.use_take(5);
        s.trim_start = 4.0;
        s.use_take(3);
        assert!(s.trim_start <= 56.0 / 24.0 - 0.5);
        // Another try that fails leaves the shot made, with what went wrong next to it.
        s.fail("no memory".into());
        assert_eq!((s.state.as_str(), s.error.as_deref()), ("done", Some("no memory")));
        let mut never = shot("n1", "never made", "empty");
        never.fail("no memory".into());
        assert_eq!(never.state, "failed");
        // Deleting the take in use falls back to the newest one left; deleting the last leaves the shot not made.
        assert!(s.remove_take(3) && !s.remove_take(3));
        assert_eq!(s.take, 6);
        assert!(s.remove_take(4) && s.remove_take(5) && s.remove_take(6));
        assert_eq!((s.take, s.state.as_str(), s.made_frames, s.takes.len()), (0, "empty", None, 0));
    }

    #[test]
    fn clips_are_asked_for_by_shot_and_take() {
        assert_eq!(clip_name("a1b2.webm"), Some(("a1b2", None, false)));
        assert_eq!(clip_name("a1b2.jpg"), Some(("a1b2", None, true)));
        assert_eq!(clip_name("a1b2.3.webm"), Some(("a1b2", Some(3), false)));
        assert_eq!(clip_name("a1b2.12.jpg"), Some(("a1b2", Some(12), true)));
        for bad in ["a1b2", "a1b2.mp4", "a1b2.0.webm", "a1b2.x.webm", "../a1.webm", "a1b2.1.2.webm", ".webm", "A1.webm"] {
            assert_eq!(clip_name(bad), None, "{bad}");
        }
        // The clip of a shot is the take in use; a shot from before takes has one clip.
        let mut with = shot("a1", "fox", "done");
        with.take = 3;
        let p = Project {
            id: "p".into(),
            name: "Film".into(),
            folder: "/films/fox".into(),
            size: "640x384".into(),
            shots: vec![with, shot("b2", "boat", "done")],
            ..Project::default()
        };
        assert_eq!(clip_path(&p, "a1"), FsPath::new("/films/fox/clips/a1.3.webm"));
        assert_eq!(poster_path(&p, "a1"), FsPath::new("/films/fox/clips/a1.3.jpg"));
        assert_eq!(clip_path(&p, "b2"), FsPath::new("/films/fox/clips/b2.webm"));
        assert_eq!(take_files(&p, "b2", 2).0, FsPath::new("/films/fox/clips/b2.2.webm"));
    }

    #[test]
    fn a_film_says_how_long_it_still_needs() {
        let mut second = shot("b2", "the fox stops", "empty");
        second.continues = true;
        let mut third = shot("c3", "a boat", "changed");
        third.frames = 49;
        let p = Project {
            id: "p".into(),
            name: "Film".into(),
            folder: "/films/fox".into(),
            size: "640x384".into(),
            shots: vec![shot("a1", "a fox", "done"), second, third, shot("d4", "", "empty")],
            ..Project::default()
        };
        let seen =
            [pace::Sample { frames: 121, continued: true, seconds: 400.0 }, pace::Sample { frames: 49, continued: false, seconds: 90.0 }];
        // Nothing is waiting: the two shots that are not made, and not the one without a description.
        assert_eq!(time_left(&p, &seen, &[], None), (490.0, 0.0));
        // The second is being made for 100 seconds and the third waits.
        assert_eq!(time_left(&p, &seen, &["c3".into()], Some(("b2", 100.0))), (490.0, 390.0));
        // A shot that takes longer than expected still has a little left.
        assert_eq!(time_left(&p, &seen, &[], Some(("b2", 900.0))).1, 20.0);
        // A made shot that is made again counts as left, and not as missing.
        assert_eq!(time_left(&p, &[], &["a1".into()], None), (121.0 / 24.0 * 80.0 + 49.0 / 24.0 * 25.0, 121.0 / 24.0 * 25.0));
    }

    #[test]
    fn export_options_say_how_large_how_good_and_where() {
        let usual = ExportOptions::default();
        assert_eq!(usual.bitrates("640x384"), (7_900_000, 192_000));
        let small = ExportOptions { quality: "small".into(), ..usual.clone() };
        let best = ExportOptions { quality: "best".into(), scale: 2, ..usual.clone() };
        assert_eq!(small.bitrates("640x384"), (3_400_000, 128_000));
        // Twice as large gets two and a half times the bits; everything stays under what players take.
        assert_eq!(ExportOptions { scale: 2, ..usual.clone() }.bitrates("640x384"), (19_900_000, 192_000));
        assert_eq!(best.bitrates("640x384"), (39_300_000, 256_000));
        assert_eq!(best.bitrates("1280x1280"), (60_000_000, 256_000));
        assert_eq!(small.bitrates("256x256"), (1_500_000, 128_000));
        // Anything unknown is the usual; a name is made safe for a file.
        let odd = ExportOptions { scale: 7, quality: "ultra".into(), name: "  ../Final: cut?  ".into() }.tidy();
        assert_eq!(odd, ExportOptions { scale: 0, quality: String::new(), name: "Final cut".into() });
        assert!(ExportOptions { scale: 1, ..usual.clone() }.tidy().is_usual());
        let mut p = Project {
            id: "p".into(),
            name: "The fox: a film".into(),
            folder: "/films/fox".into(),
            size: "640x384".into(),
            shots: vec![],
            ..Project::default()
        };
        assert_eq!(export_path(&p, &p.export), FsPath::new("/films/fox/The fox a film.mp4"));
        p.export = odd;
        assert_eq!(export_path(&p, &p.export), FsPath::new("/films/fox/Final cut.mp4"));
        // A film without options is stored as before.
        p.export = ExportOptions::default();
        assert!(!serde_json::to_string(&p).unwrap().contains("export"));
    }

    #[test]
    fn only_sound_files_are_played_under_a_film() {
        assert_eq!(sound_kind(FsPath::new("/music/Theme.MP3")), Some("audio/mpeg"));
        assert_eq!(sound_kind(FsPath::new("/film/narration.wav")), Some("audio/wav"));
        assert_eq!(sound_kind(FsPath::new("/home/me/.ssh/id_ed25519")), None);
        assert_eq!(sound_kind(FsPath::new("/home/me/notes.txt")), None);
    }

    #[test]
    fn a_shot_changed_back_is_made_again_without_making_it() {
        let (wide, tall) = (film("640x384"), film("384x640"));
        let size = (&wide, &wide);
        let mut b = shot("b2", "the fox stops", "done");
        b.continues = true;
        let old = vec![shot("a1", "a fox", "done"), b, shot("c3", "a boat", "failed")];
        let ask = |a: &str, b: &str| vec![asked("a1", a, false), asked("b2", b, true), asked("c3", "a boat at sea", false)];
        // The description changes: the clip is kept, the shot is marked, and what it was made from is remembered.
        let (changed, bin) = merge(&old, &[], ask("a fox in rain", "the fox stops"), size).unwrap();
        assert_eq!(changed.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["changed", "done", "empty"]);
        assert!(changed[0].made_from.is_some() && bin.is_empty());
        // Undo: the description is the one the clip was made from, so the shot is made.
        let (back, _) = merge(&changed, &[], ask("a fox", "the fox stops"), size).unwrap();
        assert_eq!(back.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["done", "done", "empty"]);
        // Another shape and back again: the same.
        let (tall_film, _) = merge(&old, &[], ask("a fox", "the fox stops"), (&wide, &tall)).unwrap();
        assert_eq!(tall_film.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["changed", "changed", "empty"]);
        let (again, _) = merge(&tall_film, &[], ask("a fox", "the fox stops"), (&tall, &wide)).unwrap();
        assert_eq!(again.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["done", "done", "empty"]);
        // A film from before this was kept does not know what its changed shots were made from.
        let legacy = vec![shot("a1", "a fox in rain", "changed")];
        let (still, _) = merge(&legacy, &[], vec![asked("a1", "a fox", false)], size).unwrap();
        assert_eq!(still[0].state, "changed");
    }

    #[test]
    fn a_shot_taken_out_of_the_film_comes_back_with_its_clip() {
        let wide = film("640x384");
        let size = (&wide, &wide);
        let mut b = shot("b2", "the fox stops", "done");
        b.continues = true;
        b.takes = vec![take(1, 49), take(2, 49)];
        b.take = 2;
        let old = vec![shot("a1", "a fox", "done"), b, shot("c3", "", "empty")];
        // Two shots are deleted: the one with a clip is kept aside, the empty one is gone.
        let (left, bin) = merge(&old, &[], vec![asked("a1", "a fox", false)], size).unwrap();
        assert_eq!((left.len(), bin.iter().map(|s| s.id.as_str()).collect::<Vec<_>>()), (1, vec!["b2"]));
        // Undo puts it back where it was: made, with its takes.
        let (back, bin) = merge(&left, &bin, vec![asked("a1", "a fox", false), asked("b2", "the fox stops", true)], size).unwrap();
        assert_eq!((back[1].state.as_str(), back[1].take, back[1].takes.len()), ("done", 2, 2));
        assert!(bin.is_empty());
        // Put back somewhere else, a shot that continued another has to be made again; its clip is still its clip.
        let (left, bin) = merge(&back, &bin, vec![asked("a1", "a fox", false)], size).unwrap();
        let (moved, _) = merge(&left, &bin, vec![asked("b2", "the fox stops", false), asked("a1", "a fox", false)], size).unwrap();
        assert_eq!((moved[0].state.as_str(), moved[0].takes.len()), ("changed", 2));
        // Only so many are kept aside, the newest ones.
        let many: Vec<Shot> = (0..MAX_REMOVED + 5).map(|i| shot(&format!("s{i}"), "x", "done")).collect();
        let (_, bin) = merge(&many, &[], vec![], size).unwrap();
        assert_eq!((bin.len(), bin[0].id.as_str()), (MAX_REMOVED, "s5"));
    }

    #[test]
    fn a_join_has_a_length_and_a_film_has_tracks() {
        let made = merge_shots(
            &[],
            vec![
                ShotIn { join: "dissolve".into(), join_seconds: Some(9.0), ..asked("a1", "fox", false) },
                ShotIn { join: "black".into(), join_seconds: Some(0.0), ..asked("b2", "boat", false) },
                ShotIn { join_seconds: Some(f32::NAN), trim_start: f32::NAN, ..asked("c3", "sea", false) },
            ],
            false,
        )
        .unwrap();
        assert_eq!(made.iter().map(|s| s.join_seconds).collect::<Vec<_>>(), [2.0, 0.25, 0.5]);
        assert_eq!(made[2].trim_start, 0.0);
        let mut slow = shot("b2", "boat", "done");
        slow.made_frames = Some(96);
        slow.join = "dissolve".into();
        slow.join_seconds = 1.25;
        assert_eq!(pieces(&[&shot("a1", "fox", "done"), &slow], &[true, true], None)[1].join_frames, 30);
        // A film without tracks is stored as before, and one from before tracks is read as it is.
        let p = Project { id: "p".into(), name: "Film".into(), folder: "/films/fox".into(), ..Project::default() };
        let text = serde_json::to_string(&p).unwrap();
        assert!(!text.contains("tracks") && !text.contains("master") && !text.contains("shot_track") && !text.contains("removed"));
        let old: Project =
            serde_json::from_str(r#"{"id":"p","name":"Film","folder":"/f","size":"640x384","shots":[],"sound":"/m/a.wav","updated_ms":1}"#)
                .unwrap();
        assert_eq!(
            (old.master, old.tracks.len(), old.shot_track.volume, old.sound.as_str(), old.sound_volume),
            (1.0, 0, 1.0, "/m/a.wav", 0.5)
        );
        assert_eq!(narration_file(FsPath::new("/nowhere/at/all")), FsPath::new("/nowhere/at/all/narration.wav"));
    }

    // The paths here are written the Unix way; on Windows they are not full paths.
    #[cfg(unix)]
    #[test]
    fn only_the_sounds_of_a_film_are_read_for_it() {
        let mut p = Project { id: "p".into(), folder: "/films/fox".into(), sounds: vec!["/music/theme.mp3".into()], ..Project::default() };
        p.tracks = sound::merge_tracks(
            FsPath::new("/films/fox"),
            &[],
            vec![sound::TrackIn {
                regions: vec![sound::RegionIn { file: "/fx/rain.flac".into(), length: 2.0, ..sound::RegionIn::default() }],
                ..sound::TrackIn::default()
            }],
            new_id,
            &["/fx/rain.flac".to_string()],
        )
        .unwrap();
        assert_eq!(sound_of(&p, "/films/fox/narration.wav"), Some(PathBuf::from("/films/fox/narration.wav")));
        assert_eq!(sound_of(&p, "narration 2.wav"), Some(PathBuf::from("/films/fox/narration 2.wav")));
        assert_eq!(sound_of(&p, "/music/theme.mp3"), Some(PathBuf::from("/music/theme.mp3")));
        assert_eq!(sound_of(&p, "/fx/rain.flac"), Some(PathBuf::from("/fx/rain.flac")));
        for other in
            ["/music/other.mp3", "/films/fox/../../music/other.mp3", "/films/fox/rigbrew-film.json", "/home/me/.ssh/id_ed25519", ""]
        {
            assert_eq!(sound_of(&p, other), None, "{other}");
        }
        assert_eq!(made_key("640x384", "a fox", 121, None), made_key("640x384", "a fox", 121, None));
        assert_ne!(made_key("640x384", "a fox", 121, None), made_key("640x384", "a fox", 121, Some("a1")));
        assert_ne!(made_key("640x384", "a fox1", 21, None), made_key("640x384", "a fox", 121, None));
    }

    #[test]
    fn a_new_film_gets_a_folder_of_its_own() {
        let home = std::env::temp_dir().join(format!("rigbrew-films-{}", uuid::Uuid::new_v4().simple()));
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film"));
        std::fs::create_dir_all(home.join("The fox a film")).unwrap();
        // An empty folder of that name is taken; one that holds a film is not.
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film"));
        std::fs::write(home.join("The fox a film").join(FILE), "{}").unwrap();
        assert_eq!(new_folder(&home, "The fox: a film"), home.join("The fox a film 2"));
        assert_eq!(new_folder(&home, ""), home.join("film"));
        std::fs::remove_dir_all(home).ok();
    }

    fn member(id: &str, name: &str, look: &str) -> Member {
        Member { id: id.into(), name: name.into(), look: look.into(), voice: None, portrait: 0 }
    }

    #[test]
    fn a_new_look_marks_the_shots_it_is_in() {
        let cast = [member("m1", "Mira", "a girl of eight in a yellow raincoat"), member("f1", "Fox", "a small red fox")];
        let was = Film { size: "640x384", look: "soft watercolour", cast: &cast };
        let old = vec![
            shot("a1", "Mira kneels by the stream", "done"),
            shot("b2", "Fox watches from the trees", "done"),
            shot("c3", "the forest from above", "done"),
        ];
        let same = |list: &[Shot]| list.iter().map(|s| asked(&s.id, &s.prompt, false)).collect::<Vec<_>>();
        // Nothing changed: every shot keeps its clip.
        let (kept, _) = merge(&old, &[], same(&old), (&was, &was)).unwrap();
        assert!(kept.iter().all(|s| s.state == "done"));
        // Mira gets red boots: the shot that names her is changed, the others are not.
        let boots = [member("m1", "Mira", "a girl of eight in a yellow raincoat and red boots"), cast[1].clone()];
        let now = Film { cast: &boots, ..was };
        let (changed, _) = merge(&old, &[], same(&old), (&was, &now)).unwrap();
        assert_eq!(changed.iter().map(|s| s.state.as_str()).collect::<Vec<_>>(), ["changed", "done", "done"]);
        // The look back as it was: the clip is the one that was made from it.
        let (back, _) = merge(&changed, &[], same(&changed), (&now, &was)).unwrap();
        assert!(back.iter().all(|s| s.state == "done"));
        // Another look for the whole film changes every shot.
        let ink = Film { look: "black ink on paper", ..was };
        let (all, _) = merge(&old, &[], same(&old), (&was, &ink)).unwrap();
        assert!(all.iter().all(|s| s.state == "changed"));
        // A film without a cast and without a look is made from what it always was.
        assert_eq!(film("640x384").key("a fox", 121, None, ""), made_key("640x384", "a fox", 121, None));
        assert_eq!(film("640x384").key("a fox", 121, Some("a1"), "m1"), made_key("640x384", "a fox", 121, Some("a1")));
    }

    #[test]
    fn a_shot_keeps_its_lines_and_where_it_starts_from() {
        let mut portrayed = member("m1", "Mira", "a girl of eight");
        portrayed.portrait = 1;
        let cast = [portrayed.clone()];
        let now = Film { size: "640x384", look: "", cast: &cast };
        let said = |text: &str| words::SpokenIn { who: "m1".into(), text: text.into(), ..words::SpokenIn::default() };
        let mut new = asked("a1", "Mira kneels by the stream", false);
        new.lines = Some(vec![
            said("Go on, little boat!"),
            words::SpokenIn { who: "gone".into(), text: "And it went.".into(), ..words::SpokenIn::default() },
        ]);
        new.from = Some("m1".into());
        let old = vec![shot("a1", "Mira kneels by the stream", "done")];
        let (out, _) = merge(&old, &[], vec![new], (&now, &now)).unwrap();
        assert_eq!(
            out[0].lines.iter().map(|l| (l.who.as_str(), l.text.as_str())).collect::<Vec<_>>(),
            [("m1", "Go on, little boat!"), ("", "And it went.")]
        );
        // Starting from the portrait is another picture to start from: the shot is made again.
        assert_eq!((out[0].from.as_str(), out[0].state.as_str()), ("m1", "changed"));
        // A save without lines keeps them; what is said changes nothing about the clip.
        let mut again = asked("a1", "Mira kneels by the stream", false);
        again.from = Some("m1".into());
        let done = vec![Shot { state: "done".into(), ..out[0].clone() }];
        let (kept, _) = merge(&done, &[], vec![again], (&now, &now)).unwrap();
        assert_eq!((kept[0].lines.len(), kept[0].state.as_str()), (2, "done"));
        // A new portrait is another picture again; a member who left the cast is nobody to start from, and says nothing.
        portrayed.portrait = 2;
        let redone = [portrayed];
        let mut third = asked("a1", "Mira kneels by the stream", false);
        third.from = Some("m1".into());
        let (remade, _) = merge(&done, &[], vec![third], (&now, &Film { cast: &redone, ..now })).unwrap();
        assert_eq!(remade[0].state, "changed");
        let mut fourth = asked("a1", "Mira kneels by the stream", false);
        fourth.from = Some("m1".into());
        let (alone, _) = merge(&done, &[], vec![fourth], (&now, &film("640x384"))).unwrap();
        assert!(alone[0].from.is_empty() && alone[0].lines.iter().all(|l| l.who.is_empty()));
    }

    #[test]
    fn a_clip_brought_in_is_found_by_its_own_ending() {
        let mut brought = shot("a1", "", "done");
        brought.takes = vec![
            Take { n: 1, frames: 96, ext: "mp4".into(), large: true, ..Take::default() },
            Take { n: 2, frames: 121, ..Take::default() },
        ];
        brought.take = 1;
        let mut p = Project { folder: "/films/fox".into(), shots: vec![brought, shot("b2", "a fox", "done")], ..Project::default() };
        assert!(clip_path(&p, "a1").ends_with("clips/a1.1.mp4") && clip_path(&p, "b2").ends_with("clips/b2.webm"));
        assert!(take_files(&p, "a1", 1).0.ends_with("a1.1.mp4") && take_files(&p, "a1", 2).0.ends_with("a1.2.webm"));
        assert!(take_files(&p, "a1", 1).1.ends_with("a1.1.jpg"));
        p.shots[0].take = 2;
        assert!(clip_path(&p, "a1").ends_with("clips/a1.2.webm"));
        // Only the endings Studio writes itself are ever looked for.
        p.shots[0].takes[1].ext = "../../etc/passwd".into();
        assert!(clip_path(&p, "a1").ends_with("clips/a1.2.webm"));
    }

    /// A service with nothing in it, or with a video model that counts as installed.
    pub(crate) fn bare_app(video: bool) -> (Shared, PathBuf) {
        use std::collections::HashMap;
        let dir = std::env::temp_dir().join(format!("rigbrewd-studio-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let packages = if video {
            r#"[{"id":"pour-test","name":"Pour","modality":"video","version":1,"engine":"sdcpp","access":"public",
                 "source":{"name":"s","url":"u","license":"l","changes":"c"},"quality_floor":0.5,
                 "variants":[{"id":"q4","accel_mem_mb":1,"backends":[],"quality":1.0,"files":[{"role":"dit","path":"pour-test/dit.gguf","size":1,
                 "sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]}]}]"#
        } else {
            "[]"
        };
        // The one file of the video model is there: that is what "installed" means.
        std::fs::create_dir_all(dir.join("packages").join("pour-test")).unwrap();
        std::fs::write(dir.join("packages").join("pour-test").join("dit.gguf"), "x").unwrap();
        let manifest = format!(r#"{{"schema":1,"generated":"2026-11-20","base_url":"http://127.0.0.1:9/","packages":{packages}}}"#);
        let app = App {
            profile: rigbrew_core::hardware::scan_with_data_dir(&dir),
            manifest: rigbrew_core::Manifest::parse(&manifest).unwrap(),
            community: Mutex::new(rigbrew_core::community::Store::load(&dir)),
            catalog: tokio::sync::Mutex::new(crate::community::CatalogState::default()),
            root: dir.clone(),
            token: "t".into(),
            events: Mutex::new(rigbrew_core::events::EventStore::open(&dir.join("events.db")).unwrap()),
            chats: Mutex::new(rigbrew_core::chats::ChatStore::in_memory().unwrap()),
            jobs: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            wanted_lock: Mutex::new(()),
            engine: tokio::sync::Mutex::new(Vec::new()),
            http: reqwest::Client::new(),
            net: reqwest::Client::new(),
            server: String::new(),
            session: Mutex::new(None),
            sync: Mutex::new(crate::cloud::SyncState::default()),
            sync_now: tokio::sync::Notify::new(),
            telemetry_off: true,
            voice: tokio::sync::Mutex::new(()),
            listen: tokio::sync::Mutex::new(()),
            videos: Mutex::new(HashMap::new()),
            image_progress: Mutex::new(None),
            agent: crate::agent::Agents::load(&dir),
            connectors: crate::connectors::Connectors::new(&dir),
        };
        (std::sync::Arc::new(app), dir)
    }

    async fn said(response: Response) -> (StatusCode, String) {
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, v["error"]["message"].as_str().unwrap_or("").to_string())
    }

    #[tokio::test]
    async fn nothing_is_written_or_made_without_the_model_for_it() {
        let (app, dir) = bare_app(false);
        let folder = dir.join("film").to_string_lossy().to_string();
        let made = create(State(app.clone()), Json(NewProject { folder, name: "Fox".into() })).await;
        assert_eq!(made.status(), StatusCode::OK);
        let id = index(&app)[0].0.clone();
        let saved = save(
            State(app.clone()),
            Path(id.clone()),
            Json(
                serde_json::from_value(json!({
                    "cast": [{"id": "m1", "name": "Mira", "look": "a girl of eight"}],
                    "shots": [{"id": "a1", "prompt": "Mira kneels by the stream", "lines": [{"who": "m1", "text": "Go on, little boat!"}]}],
                }))
                .unwrap(),
            ),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);
        let video = (StatusCode::CONFLICT, missing(Modality::Video).to_string());
        let chat = (StatusCode::CONFLICT, missing(Modality::Chat).to_string());
        let idea = |as_written: bool| Json(Idea { idea: "a fox finds a paper boat".into(), shots: Some(3), as_written });
        // No video model: no brief, no shots written (not even as written), nothing made.
        assert_eq!(
            said(direct::brief(State(app.clone()), Json(serde_json::from_value(json!({"text": "a fox"})).unwrap())).await).await,
            video
        );
        assert_eq!(said(storyboard(State(app.clone()), Path(id.clone()), idea(false)).await).await, video);
        assert_eq!(said(storyboard(State(app.clone()), Path(id.clone()), idea(true)).await).await, video);
        assert_eq!(said(render(State(app.clone()), Path(id.clone()), None).await).await, video);
        // No chat model: no description written again, no look written.
        assert_eq!(said(rewrite(State(app.clone()), Path((id.clone(), "a1".into())), None).await).await, chat);
        let look = Json(serde_json::from_value(json!({"name": "Mira", "words": "curious"})).unwrap());
        assert_eq!(said(direct::look(State(app.clone()), Path(id.clone()), look).await).await, chat);
        // No voice, no picture model, no listening model: no dub, no portrait, nothing written down.
        assert_eq!(
            said(dub::dub(State(app.clone()), Path(id.clone()), None).await).await,
            (StatusCode::CONFLICT, missing(Modality::Voice).to_string())
        );
        assert_eq!(
            said(direct::make_portrait(State(app.clone()), Path((id.clone(), "m1".into()))).await).await,
            (StatusCode::CONFLICT, missing(Modality::Image).to_string())
        );
        assert_eq!(
            said(dub::listen(State(app.clone()), Path(id.clone()), None).await).await,
            (StatusCode::CONFLICT, missing(Modality::Listen).to_string())
        );
        // The film says what it has, so the app can say what is missing before anything is asked.
        let seen = view(&app, load(&app, &id).unwrap());
        assert_eq!(seen["has"], json!({"video": false, "chat": false, "voice": false, "image": false}));
        assert_eq!(seen["cast_in"], json!({"m1": ["a1"]}));
        assert_eq!(seen["cues"][0]["text"], "Go on, little boat!");
        assert_eq!((seen["can_listen"].as_bool(), seen["voices"].as_u64()), (Some(false), Some(0)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn with_the_video_model_the_chat_model_is_asked_for_next() {
        let (app, dir) = bare_app(true);
        let folder = dir.join("film").to_string_lossy().to_string();
        create(State(app.clone()), Json(NewProject { folder, name: "Fox".into() })).await;
        let id = index(&app)[0].0.clone();
        let chat = (StatusCode::CONFLICT, missing(Modality::Chat).to_string());
        assert_eq!(
            said(direct::brief(State(app.clone()), Json(serde_json::from_value(json!({"text": "a fox"})).unwrap())).await).await,
            chat
        );
        let idea =
            |as_written: bool| Json(Idea { idea: "A fox walks through snow.\nA boat floats by.".into(), shots: Some(3), as_written });
        assert_eq!(said(storyboard(State(app.clone()), Path(id.clone()), idea(false)).await).await, chat);
        // Shots used as written ask nothing of the chat model.
        let written = storyboard(State(app.clone()), Path(id.clone()), idea(true)).await;
        assert_eq!(written.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(written.into_body(), 1024 * 1024).await.unwrap()).unwrap();
        assert_eq!(body["shots"].as_array().map(Vec::len), Some(2));
        assert_eq!(body["shots"][1]["prompt"], "A boat floats by.");
        std::fs::remove_dir_all(dir).ok();
    }
}
