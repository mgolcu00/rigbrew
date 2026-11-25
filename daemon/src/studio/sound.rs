//! The sound tracks of a film: what a track and a region are, how a save is checked, which
//! regions are heard, and the small picture of a sound file the timeline draws (its peaks).

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::export;

pub const MAX_TRACKS: usize = 32;
pub const MAX_REGIONS: usize = 400;
/// Files the person added to a film's library from outside its folder.
pub const MAX_SOUNDS: usize = 200;
/// The longest a region is, and the latest it starts: six hours.
const MAX_SECONDS: f32 = 6.0 * 3600.0;
const MAX_LOOPS: u32 = 1000;
/// Peaks a second, and the samples a second the file is read at for them.
pub const PEAK_RATE: u32 = 50;
pub const READ_RATE: u32 = 8000;
/// The colours a track gets when it names none, in turn.
const COLORS: [&str; 8] = ["#4f8fd9", "#3fae8a", "#c98a3c", "#b06bc9", "#d0605e", "#5fa8b8", "#9aa63f", "#8d7fd6"];

fn one() -> f32 {
    1.0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn once() -> u32 {
    1
}

fn is_once(n: &u32) -> bool {
    *n <= 1
}

/// A stretch of a sound file at a place in the film.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    /// The file: a full path, or a name inside the film's folder.
    pub file: String,
    /// Seconds into the film where it starts.
    pub start: f32,
    /// Seconds into the file where it starts, and how long it plays from there.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f32,
    pub length: f32,
    /// 1.0 as the file is, up to 2.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub gain: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out: f32,
    /// How many times the stretch plays, one after another.
    #[serde(default = "once", skip_serializing_if = "is_once")]
    pub loops: u32,
}

/// A sound track: music, narration or effects, with its regions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    /// "sound": the only kind the person adds.
    #[serde(default = "sound_kind")]
    pub kind: String,
    /// "#rrggbb"
    pub color: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f32,
    /// -1 is all left, 1 all right.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mute: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    #[serde(default)]
    pub regions: Vec<Region>,
}

fn sound_kind() -> String {
    "sound".into()
}

/// The channel strip of the shots' own sound as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Strip {
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
}

impl Default for Strip {
    fn default() -> Strip {
        Strip { volume: 1.0, pan: 0.0, mute: false, solo: false }
    }
}

impl Strip {
    pub fn is_usual(&self) -> bool {
        *self == Strip::default()
    }

    pub fn tidy(self) -> Strip {
        Strip { volume: level(self.volume, 1.0, 2.0), pan: level(self.pan, 0.0, 1.0).max(-1.0), mute: self.mute, solo: self.solo }
    }
}

/// A number the person set, held between `-top` (or 0) and `top`; `usual` when it is no number.
fn level(value: f32, usual: f32, top: f32) -> f32 {
    if value.is_finite() {
        value.clamp(if usual == 0.0 { -top } else { 0.0 }, top)
    } else {
        usual
    }
}

#[derive(Deserialize, Default)]
pub struct RegionIn {
    #[serde(default)]
    pub id: Option<String>,
    pub file: String,
    #[serde(default)]
    pub start: f32,
    #[serde(default)]
    pub offset: f32,
    pub length: f32,
    #[serde(default)]
    pub gain: Option<f32>,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
    #[serde(default)]
    pub loops: Option<u32>,
}

#[derive(Deserialize, Default)]
pub struct TrackIn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub volume: Option<f32>,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub regions: Vec<RegionIn>,
}

fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The kinds of sound file a track takes, by the end of their name.
pub fn is_sound(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(e.to_ascii_lowercase().as_str(), "wav" | "mp3" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" | "aif" | "aiff")
    })
}

/// Where the file of a region is: a full path as it is, a name inside the film's folder. `None`
/// for anything that is not a sound file by name, or that would leave the folder.
pub fn place(folder: &Path, file: &str) -> Option<PathBuf> {
    let file = file.trim();
    if file.is_empty() || file.len() > 1024 {
        return None;
    }
    let path = Path::new(file);
    if !is_sound(path) {
        return None;
    }
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    path.components().all(|c| matches!(c, Component::Normal(_))).then(|| folder.join(path))
}

/// The new tracks of a film. A file that the film did not use before has to be there; one it
/// already used may be gone (it is then silent, and the person is told on the screen).
pub fn merge_tracks(
    folder: &Path,
    old: &[Track],
    new: Vec<TrackIn>,
    mut id: impl FnMut() -> String,
    known: &[String],
) -> Result<Vec<Track>, String> {
    if new.len() > MAX_TRACKS {
        return Err(format!("a film has at most {MAX_TRACKS} sound tracks"));
    }
    let valid_id = |i: &str| (1..=40).contains(&i.len()) && i.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let used: Vec<&str> =
        old.iter().flat_map(|t| t.regions.iter().map(|r| r.file.as_str())).chain(known.iter().map(String::as_str)).collect();
    let mut out: Vec<Track> = Vec::with_capacity(new.len());
    let mut region_ids: Vec<String> = Vec::new();
    for (n, t) in new.into_iter().enumerate() {
        if t.regions.len() > MAX_REGIONS {
            return Err(format!("a track has at most {MAX_REGIONS} regions"));
        }
        let track_id = t.id.filter(|i| valid_id(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(&mut id);
        let name: String = t.name.trim().chars().take(40).collect();
        let mut regions = Vec::with_capacity(t.regions.len());
        for r in t.regions {
            let file = r.file.trim().to_string();
            let Some(path) = place(folder, &file) else {
                return Err("a region needs a sound file: wav, mp3, m4a, aac, flac, ogg, opus or aiff, by its full path".into());
            };
            if !used.contains(&file.as_str()) && !path.is_file() {
                return Err(format!("the sound file was not found: {file}"));
            }
            if !(r.length.is_finite() && r.start.is_finite() && r.offset.is_finite()) {
                return Err("a region needs a start and a length in seconds".into());
            }
            let length = r.length.clamp(0.02, MAX_SECONDS);
            let loops = r.loops.unwrap_or(1).clamp(1, MAX_LOOPS);
            let span = length * loops as f32;
            let fade_in = level(r.fade_in, 0.0, span).max(0.0);
            let region_id = r.id.filter(|i| valid_id(i) && !region_ids.contains(i)).unwrap_or_else(&mut id);
            region_ids.push(region_id.clone());
            regions.push(Region {
                id: region_id,
                file,
                start: r.start.clamp(0.0, MAX_SECONDS),
                offset: r.offset.clamp(0.0, MAX_SECONDS),
                length,
                gain: level(r.gain.unwrap_or(1.0), 1.0, 2.0),
                fade_in,
                // The two fades never cross.
                fade_out: level(r.fade_out, 0.0, span - fade_in).max(0.0),
                loops,
            });
        }
        out.push(Track {
            id: track_id,
            name: if name.is_empty() { format!("Track {}", n + 1) } else { name },
            kind: sound_kind(),
            color: if valid_color(&t.color) { t.color.to_ascii_lowercase() } else { COLORS[n % COLORS.len()].into() },
            volume: level(t.volume.unwrap_or(1.0), 1.0, 2.0),
            pan: level(t.pan, 0.0, 1.0),
            mute: t.mute,
            solo: t.solo,
            regions,
        });
    }
    Ok(out)
}

/// The colour a new track gets in a film that has `count` tracks already.
pub fn next_color(count: usize) -> String {
    COLORS[count % COLORS.len()].into()
}

/// What is heard: for the shots' sound its loudness as a whole, and every region of a track that
/// is not muted (and, when anything is soloed, only of the soloed ones). Each region comes with
/// its file; the caller gives every file an input.
pub fn heard(strip: &Strip, tracks: &[Track], master: f32, mut input_of: impl FnMut(&str) -> Option<usize>) -> export::Mix {
    let soloed = strip.solo || tracks.iter().any(|t| t.solo);
    let on = |mute: bool, solo: bool| !mute && (!soloed || solo);
    let mut sounds = Vec::new();
    for t in tracks.iter().filter(|t| on(t.mute, t.solo)) {
        for r in &t.regions {
            let Some(input) = input_of(&r.file) else { continue };
            sounds.push(export::Sound {
                input,
                offset: r.offset as f64,
                length: r.length as f64,
                loops: r.loops.max(1),
                start: r.start as f64,
                gain: (r.gain * t.volume) as f64,
                pan: t.pan as f64,
                fade_in: r.fade_in as f64,
                fade_out: r.fade_out as f64,
            });
        }
    }
    export::Mix {
        shots_gain: if on(strip.mute, strip.solo) { strip.volume as f64 } else { 0.0 },
        shots_pan: strip.pan as f64,
        sounds,
        master: master as f64,
    }
}

/// The peaks of a sound as it is read: signed 16-bit samples, one channel, `READ_RATE` a second,
/// in pieces of any size. Every `READ_RATE / PEAK_RATE` samples give one lowest and one highest
/// value, each from -127 to 127.
#[derive(Default)]
pub struct Peaks {
    pairs: Vec<i8>,
    low: i16,
    high: i16,
    in_window: u32,
    samples: u64,
    /// The first byte of a sample whose second byte is in the next piece.
    carry: Option<u8>,
}

impl Peaks {
    pub fn feed(&mut self, bytes: &[u8]) {
        let window = READ_RATE / PEAK_RATE;
        let mut bytes = bytes.iter().copied();
        while let Some(first) = self.carry.take().or_else(|| bytes.next()) {
            let Some(second) = bytes.next() else {
                self.carry = Some(first);
                break;
            };
            let sample = i16::from_le_bytes([first, second]);
            self.low = self.low.min(sample);
            self.high = self.high.max(sample);
            self.in_window += 1;
            self.samples += 1;
            if self.in_window == window {
                self.close();
            }
        }
    }

    fn close(&mut self) {
        self.pairs.push((self.low / 258) as i8);
        self.pairs.push((self.high / 258) as i8);
        (self.low, self.high, self.in_window) = (0, 0, 0);
    }

    /// The peaks (lowest, highest, lowest, highest, ...) and the seconds of sound they are of.
    pub fn done(mut self) -> (Vec<i8>, f32) {
        if self.in_window > 0 {
            self.close();
        }
        (self.pairs, self.samples as f32 / READ_RATE as f32)
    }
}

/// The name a file's peaks are kept under: the same file, changed, gets another.
pub fn peaks_name(path: &Path, bytes: u64, modified_ms: u128) -> String {
    // FNV-1a: the same on every machine and in every build.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in path.to_string_lossy().bytes().chain(bytes.to_le_bytes()).chain(modified_ms.to_le_bytes()) {
        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("snd-{hash:016x}.peaks.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("new{n}")
        }
    }

    fn region(file: &str, start: f32, length: f32) -> RegionIn {
        RegionIn { file: file.into(), start, length, ..RegionIn::default() }
    }

    #[test]
    fn tracks_are_checked_when_a_film_is_saved() {
        let dir = std::env::temp_dir().join(format!("rigbrew-sound-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("theme.wav");
        std::fs::write(&file, b"x").unwrap();
        let path = file.to_string_lossy().to_string();
        let wild = RegionIn {
            id: Some("r1".into()),
            start: -4.0,
            offset: -1.0,
            gain: Some(9.0),
            fade_in: 3.0,
            fade_out: 3.0,
            loops: Some(0),
            ..region(&path, 0.0, 2.0)
        };
        let again = RegionIn { id: Some("r1".into()), loops: Some(3), fade_out: 9.0, ..region("theme.wav", 1.0, 2.0) };
        let made = merge_tracks(
            &dir,
            &[],
            vec![
                TrackIn {
                    id: Some("t1".into()),
                    name: "  Music  ".into(),
                    color: "#AABBCC".into(),
                    volume: Some(5.0),
                    pan: -7.0,
                    regions: vec![wild, again],
                    ..TrackIn::default()
                },
                TrackIn { id: Some("t1".into()), color: "red".into(), ..TrackIn::default() },
            ],
            ids(),
            &[],
        )
        .unwrap();
        assert_eq!(
            (made[0].id.as_str(), made[0].name.as_str(), made[0].color.as_str(), made[0].volume, made[0].pan),
            ("t1", "Music", "#aabbcc", 2.0, -1.0)
        );
        // Nothing starts before the film, is louder than twice the file, or fades for longer than it lasts.
        let r = &made[0].regions[0];
        assert_eq!((r.start, r.offset, r.gain, r.fade_in, r.fade_out, r.loops), (0.0, 0.0, 2.0, 2.0, 0.0, 1));
        // An id is used once in a film; a name inside the folder is as good as a full path.
        let r = &made[0].regions[1];
        assert_eq!((r.id.as_str(), r.file.as_str(), r.loops, r.length, r.fade_out), ("new1", "theme.wav", 3, 2.0, 6.0));
        assert_eq!(
            (made[1].id.as_str(), made[1].name.as_str(), made[1].color.as_str(), made[1].volume),
            ("new2", "Track 2", COLORS[1], 1.0)
        );
        // A file that is not there, is no sound file, or lies outside the folder by a relative name is refused.
        for bad in ["/nowhere/at/all.wav", "../theme.wav", "notes.txt", "/etc/passwd", ""] {
            let asked = vec![TrackIn { regions: vec![region(bad, 0.0, 1.0)], ..TrackIn::default() }];
            assert!(merge_tracks(&dir, &[], asked, ids(), &[]).is_err(), "{bad}");
        }
        // A file the film used before may be gone: the film can still be saved.
        std::fs::remove_file(&file).unwrap();
        let kept = merge_tracks(&dir, &made, vec![TrackIn { regions: vec![region(&path, 0.0, 1.0)], ..TrackIn::default() }], ids(), &[]);
        assert!(kept.is_ok());
        assert!(merge_tracks(&dir, &[], (0..=MAX_TRACKS).map(|_| TrackIn::default()).collect(), ids(), &[]).is_err());
        let many = (0..=MAX_REGIONS).map(|_| region(&path, 0.0, 1.0)).collect();
        assert!(merge_tracks(&dir, &made, vec![TrackIn { regions: many, ..TrackIn::default() }], ids(), &[]).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    fn track(id: &str, volume: f32, mute: bool, solo: bool) -> Track {
        Track {
            id: id.into(),
            name: id.into(),
            kind: sound_kind(),
            color: COLORS[0].into(),
            volume,
            pan: 0.5,
            mute,
            solo,
            regions: vec![Region {
                id: format!("{id}r"),
                file: format!("/s/{id}.wav"),
                start: 1.0,
                offset: 0.0,
                length: 2.0,
                gain: 0.5,
                fade_in: 0.0,
                fade_out: 0.0,
                loops: 2,
            }],
        }
    }

    #[test]
    fn mute_and_solo_say_what_is_heard() {
        let files = |file: &str| Some(if file.ends_with("a.wav") { 4 } else { 5 });
        let strip = Strip::default();
        let all = heard(&strip, &[track("a", 0.8, false, false), track("b", 1.0, false, false)], 1.0, files);
        assert_eq!(
            all.sounds.iter().map(|s| (s.input, s.gain, s.pan, s.loops)).collect::<Vec<_>>(),
            [(4, 0.4000000059604645, 0.5, 2), (5, 0.5, 0.5, 2)]
        );
        assert_eq!(all.shots_gain, 1.0);
        // A muted track is not in the mix at all.
        let muted = heard(&strip, &[track("a", 1.0, true, false), track("b", 1.0, false, false)], 1.0, files);
        assert_eq!(muted.sounds.iter().map(|s| s.input).collect::<Vec<_>>(), [5]);
        // A soloed track leaves the others out, the shots' own sound too; mute still wins.
        let solo = heard(&strip, &[track("a", 1.0, false, true), track("b", 1.0, false, false)], 1.0, files);
        assert_eq!((solo.sounds.iter().map(|s| s.input).collect::<Vec<_>>(), solo.shots_gain), (vec![4], 0.0));
        let both = heard(&strip, &[track("a", 1.0, true, true), track("b", 1.0, false, true)], 1.0, files);
        assert_eq!(both.sounds.iter().map(|s| s.input).collect::<Vec<_>>(), [5]);
        // The shots' sound can be the soloed one.
        let shots = heard(&Strip { solo: true, volume: 0.7, ..Strip::default() }, &[track("a", 1.0, false, false)], 1.0, files);
        assert!(shots.sounds.is_empty() && shots.shots_gain == 0.699999988079071);
        // A file that is gone is left out; with nothing to mix the film goes out as it is.
        let gone = heard(&strip, &[track("a", 1.0, false, false)], 1.0, |_| None);
        assert!(gone.is_plain());
        assert!(!heard(&strip, &[], 0.5, files).is_plain());
        assert!(Strip { volume: 9.0, pan: -9.0, ..Strip::default() }.tidy() == Strip { volume: 2.0, pan: -1.0, mute: false, solo: false });
    }

    #[test]
    fn peaks_are_the_lowest_and_highest_of_every_fiftieth_of_a_second() {
        let window = (READ_RATE / PEAK_RATE) as usize;
        // One window of silence, one that swings fully, half a window that goes up a little.
        let mut samples: Vec<i16> = vec![0; window];
        samples.extend((0..window).map(|i| if i % 2 == 0 { i16::MIN } else { i16::MAX }));
        samples.extend(vec![2580; window / 2]);
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut whole = Peaks::default();
        whole.feed(&bytes);
        let (pairs, seconds) = whole.done();
        assert_eq!(pairs, [0, 0, -127, 127, 0, 10]);
        assert_eq!(seconds, 2.5 / PEAK_RATE as f32);
        // The same sound in pieces of odd sizes gives the same peaks.
        let mut pieces = Peaks::default();
        for piece in bytes.chunks(7) {
            pieces.feed(piece);
        }
        assert_eq!(pieces.done().0, pairs);
        assert_eq!(Peaks::default().done(), (vec![], 0.0));
    }

    #[test]
    fn peaks_are_kept_by_file_and_what_it_holds() {
        let a = peaks_name(Path::new("/music/theme.wav"), 1000, 5);
        assert!(a.starts_with("snd-") && a.ends_with(".peaks.json") && a.len() == 4 + 16 + 11);
        assert_eq!(a, peaks_name(Path::new("/music/theme.wav"), 1000, 5));
        assert_ne!(a, peaks_name(Path::new("/music/theme.wav"), 1001, 5));
        assert_ne!(a, peaks_name(Path::new("/music/theme.wav"), 1000, 6));
        assert_ne!(a, peaks_name(Path::new("/music/other.wav"), 1000, 5));
    }

    // The paths here are written the Unix way; on Windows they are not full paths.
    #[cfg(unix)]
    #[test]
    fn a_region_file_is_a_sound_file_in_a_known_place() {
        let folder = Path::new("/films/fox");
        assert_eq!(place(folder, "/music/Theme.MP3"), Some(PathBuf::from("/music/Theme.MP3")));
        assert_eq!(place(folder, "narration 2.wav"), Some(PathBuf::from("/films/fox/narration 2.wav")));
        assert_eq!(place(folder, "sound/rain.flac"), Some(PathBuf::from("/films/fox/sound/rain.flac")));
        for bad in ["../other/theme.wav", "sound/../../x.wav", "/home/me/.ssh/id_ed25519", "film.mp4", "notes.txt", "  "] {
            assert_eq!(place(folder, bad), None, "{bad}");
        }
    }
}
