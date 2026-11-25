//! What is said in a film: the spoken lines of its shots, where a dub puts them in time, the
//! subtitles made of them, and how another language is asked for. Nothing here runs anything.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The most lines one shot holds, and the longest one line is.
pub const MAX_LINES: usize = 8;
pub const MAX_LINE: usize = 300;
/// The longest row of a subtitle, and how many rows are on screen at once.
const ROW: usize = 42;
const ROWS: usize = 2;
/// Seconds between two lines that are said one after another.
const GAP: f32 = 0.12;
/// Seconds the shots' own sound takes to go down before a line and to come back after it.
pub const DUCK_RAMP: f64 = 0.25;

/// One spoken line of a shot.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Spoken {
    pub id: String,
    /// Who says it: the id of a cast member, or empty for the narrator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub who: String,
    pub text: String,
    /// The same line in other languages, by the language's name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub said: BTreeMap<String, String>,
}

impl Spoken {
    /// The line in a language: its translation when there is one, else as it was written.
    pub fn text_in(&self, language: &str) -> &str {
        self.said
            .iter()
            .find(|(l, t)| l.eq_ignore_ascii_case(language) && !t.trim().is_empty())
            .map_or(self.text.as_str(), |(_, t)| t.as_str())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpokenIn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub who: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub said: BTreeMap<String, String>,
}

/// One line of text as it is kept: on one row, no longer than a line may be.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LINE).collect()
}

/// A language as the film names it: "turkish " is "Turkish". Empty when nothing was said.
pub fn language(name: &str) -> String {
    let name: String = name.trim().chars().filter(|c| c.is_alphabetic() || *c == ' ' || *c == '-').take(24).collect();
    let mut letters = name.trim().chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// The three letters a film file names a language with; "und" for one this list does not know.
pub fn language_code(name: &str) -> &'static str {
    match name.trim().to_lowercase().as_str() {
        "english" => "eng",
        "turkish" | "türkçe" => "tur",
        "german" | "deutsch" => "deu",
        "french" | "français" => "fra",
        "spanish" | "español" => "spa",
        "italian" | "italiano" => "ita",
        "portuguese" | "português" => "por",
        "dutch" => "nld",
        "polish" => "pol",
        "russian" => "rus",
        "arabic" => "ara",
        "hindi" => "hin",
        "japanese" => "jpn",
        "korean" => "kor",
        "chinese" => "zho",
        _ => "und",
    }
}

/// The lines of a shot as they are saved: empty ones dropped, a speaker who is not in the cast
/// is the narrator, a line that keeps its id keeps it.
pub fn merge_lines(new: Vec<SpokenIn>, cast: &[String], mut id: impl FnMut() -> String) -> Vec<Spoken> {
    let valid = |i: &str| (1..=40).contains(&i.len()) && i.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let mut out: Vec<Spoken> = Vec::new();
    for line in new {
        let text = tidy(&line.text);
        if text.is_empty() || out.len() >= MAX_LINES {
            continue;
        }
        let line_id = line.id.filter(|i| valid(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(&mut id);
        let said =
            line.said.into_iter().map(|(l, t)| (language(&l), tidy(&t))).filter(|(l, t)| !l.is_empty() && !t.is_empty()).take(12).collect();
        out.push(Spoken { id: line_id, who: if cast.contains(&line.who) { line.who } else { String::new() }, text, said });
    }
    out
}

/// What a region on a voice track is the reading of: "<shot>/<line>".
pub fn tag(shot: &str, line: &str) -> String {
    format!("{shot}/{line}")
}

/// Where the lines of one shot start when they are said one after another: a little into the
/// shot, never before the line before them has ended (`free` is where that was).
pub fn lay(shot_start: f32, lengths: &[f32], free: f32) -> Vec<f32> {
    let mut at = (shot_start + 0.15).max(free);
    lengths
        .iter()
        .map(|length| {
            let start = at;
            at += length + GAP;
            start
        })
        .collect()
}

/// A shot in the film's time, with what is said in it.
pub struct Scene<'a> {
    pub id: &'a str,
    pub start: f32,
    pub length: f32,
    pub lines: &'a [Spoken],
}

/// One subtitle.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cue {
    pub start: f32,
    pub end: f32,
    pub text: String,
    pub shot: String,
}

/// The subtitles of a film in one language. A line that was read aloud is on screen while its
/// reading plays (`read`: the tag of a voice region, its start and its length); the others share
/// the time of their shot by how long they are. A line too long for two rows becomes several.
pub fn cues(scenes: &[Scene], read: &[(String, f32, f32)], language: &str) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    for scene in scenes {
        let unread: Vec<&Spoken> = scene.lines.iter().filter(|l| !read.iter().any(|(t, _, _)| *t == tag(scene.id, &l.id))).collect();
        let letters: usize = unread.iter().map(|l| l.text_in(language).chars().count().max(1)).sum();
        let mut at = scene.start;
        for line in scene.lines {
            let text = line.text_in(language);
            let (start, end) = match read.iter().find(|(t, _, _)| *t == tag(scene.id, &line.id)) {
                Some((_, start, length)) => (*start, start + length),
                None => {
                    let share = scene.length * text.chars().count().max(1) as f32 / letters.max(1) as f32;
                    let span = (at, at + share);
                    at += share;
                    span
                }
            };
            let parts = pages(text);
            let total: usize = parts.iter().map(|p| p.chars().count().max(1)).sum();
            let mut from = start;
            for part in parts {
                let length = (end - start) * part.chars().count().max(1) as f32 / total.max(1) as f32;
                out.push(Cue { start: from, end: from + length, text: part, shot: scene.id.to_string() });
                from += length;
            }
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    // Two subtitles are never on screen at once: one ends where the next begins.
    for i in 1..out.len() {
        let next = out[i].start;
        if out[i - 1].end > next {
            out[i - 1].end = next.max(out[i - 1].start);
        }
    }
    out.retain(|c| c.end - c.start >= 0.05 && !c.text.is_empty());
    out
}

/// A text broken into rows of at most `ROW` letters, at spaces. A word longer than a row stands alone.
pub fn wrap(text: &str) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match rows.last_mut() {
            Some(row) if row.chars().count() + 1 + word.chars().count() <= ROW => {
                row.push(' ');
                row.push_str(word);
            }
            _ => rows.push(word.to_string()),
        }
    }
    rows
}

/// A text as the subtitles that show it one after another: two rows each.
fn pages(text: &str) -> Vec<String> {
    wrap(text).chunks(ROWS).map(|rows| rows.join("\n")).collect()
}

/// "00:01:02,345"
fn srt_time(seconds: f32) -> String {
    let ms = (seconds.max(0.0) as f64 * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// The subtitles as a SubRip file.
pub fn srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (n, cue) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", n + 1, srt_time(cue.start), srt_time(cue.end), cue.text));
    }
    out
}

/// What the subtitles of a film are, as one short word: pictures of them drawn for one set of
/// subtitles are not laid over another. FNV-1a, the same in every build.
pub fn cues_key(cues: &[Cue], width: u32, height: u32) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes.iter().chain(&[0]) {
            hash = (hash ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    };
    feed(format!("{width}x{height}").as_bytes());
    for cue in cues {
        feed(format!("{:.3}-{:.3}", cue.start, cue.end).as_bytes());
        feed(cue.text.as_bytes());
    }
    format!("{hash:016x}")
}

/// The stretches of the film in which something is said, each widened by the time the shots'
/// sound takes to go down and come back, and joined where they touch.
pub fn duck_spans(said: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = said.iter().filter(|(a, b)| b > a).map(|(a, b)| ((a - DUCK_RAMP).max(0.0), b + DUCK_RAMP)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            // Lines that follow each other closely are one stretch: the sound stays down between them.
            Some(last) if a <= last.1 + DUCK_RAMP => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// What the chat model is told when it puts the lines of a film into another language.
pub fn translate_prompt(language: &str) -> String {
    format!(
        "You translate the spoken lines of a film into {language}. Answer with the same numbered list, one line each, each line translated into natural spoken {language}, nothing else. \
         Keep names as they are. Keep each line about as long as it was: it is read aloud in the same time."
    )
}

pub fn translate_request(lines: &[&str]) -> String {
    lines.iter().enumerate().map(|(i, l)| format!("{}. {l}\n", i + 1)).collect()
}

/// The translated lines out of what the chat model wrote, by their numbers: `None` for a line it left out.
pub fn read_translation(text: &str, count: usize) -> Vec<Option<String>> {
    let mut out = vec![None; count];
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•', '#']).trim();
        let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
        let Some(n) = digits.parse::<usize>().ok().filter(|n| (1..=count).contains(n)) else { continue };
        let rest = line[digits.len()..].trim_start();
        let Some(rest) = rest.strip_prefix(['.', ')', ':']) else { continue };
        let said = tidy(rest.trim().trim_matches(['"', '*', '`', '\u{201c}', '\u{201d}']));
        if !said.is_empty() && out[n - 1].is_none() {
            out[n - 1] = Some(said);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, who: &str, text: &str) -> Spoken {
        Spoken { id: id.into(), who: who.into(), text: text.into(), said: BTreeMap::new() }
    }

    #[test]
    fn lines_are_tidied_and_keep_their_ids() {
        let mut n = 0;
        let new = vec![
            SpokenIn { id: Some("a1".into()), who: "mira".into(), text: "  Look,\n a boat!  ".into(), said: BTreeMap::new() },
            SpokenIn { id: Some("a1".into()), who: "nobody".into(), text: "It is made of paper.".into(), said: BTreeMap::new() },
            SpokenIn { id: None, who: String::new(), text: "   ".into(), said: BTreeMap::new() },
            SpokenIn {
                id: Some("BAD/..".into()),
                who: String::new(),
                text: "The stream took it.".into(),
                said: BTreeMap::from([("turkish".into(), " Dere onu aldı. ".into()), ("".into(), "x".into())]),
            },
        ];
        let out = merge_lines(new, &["mira".to_string()], || {
            n += 1;
            format!("new{n}")
        });
        assert_eq!(out.len(), 3);
        assert_eq!((out[0].id.as_str(), out[0].who.as_str(), out[0].text.as_str()), ("a1", "mira", "Look, a boat!"));
        // An id that is taken, or not an id, is replaced; a speaker who is not in the cast is the narrator.
        assert_eq!((out[1].id.as_str(), out[1].who.as_str()), ("new1", ""));
        assert_eq!(out[2].id, "new2");
        assert_eq!(out[2].said, BTreeMap::from([("Turkish".to_string(), "Dere onu aldı.".to_string())]));
        assert_eq!(out[2].text_in("turkish"), "Dere onu aldı.");
        assert_eq!(out[2].text_in("German"), "The stream took it.");
        let many: Vec<SpokenIn> = (0..20).map(|i| SpokenIn { text: format!("line {i}"), ..SpokenIn::default() }).collect();
        assert_eq!(merge_lines(many, &[], || "x".into()).len(), MAX_LINES);
    }

    #[test]
    fn a_language_has_one_name_and_a_code() {
        assert_eq!(language("  tURKISH "), "Turkish");
        assert_eq!(language("pt-br"), "Pt-br");
        assert_eq!(language(" 12 "), "");
        assert_eq!((language_code("Turkish"), language_code("english"), language_code("Klingon")), ("tur", "eng", "und"));
    }

    #[test]
    fn lines_are_said_one_after_another() {
        // Two lines in a shot that starts at 5 s: the second waits for the first.
        assert_eq!(lay(5.0, &[2.0, 1.5], 0.0), [5.15, 7.27]);
        // The line before ran over into this shot: this one starts when that has ended.
        assert_eq!(lay(5.0, &[1.0], 6.4), [6.4]);
        assert!(lay(5.0, &[], 0.0).is_empty());
    }

    #[test]
    fn subtitles_follow_the_shots_and_the_readings() {
        let first = [line("l1", "", "A fox walks."), line("l2", "", "It finds a paper boat in the snow.")];
        let second = [line("l3", "mira", "Come back!")];
        let scenes = [
            Scene { id: "s1", start: 0.0, length: 5.0, lines: &first },
            Scene { id: "s2", start: 5.0, length: 5.0, lines: &[] },
            Scene { id: "s3", start: 10.0, length: 4.0, lines: &second },
        ];
        // Nothing was read aloud: the lines of a shot share its time by their length.
        let plain = cues(&scenes, &[], "English");
        assert_eq!(plain.len(), 3);
        assert_eq!((plain[0].start, plain[0].shot.as_str()), (0.0, "s1"));
        assert!((plain[0].end - plain[1].start).abs() < 1e-4 && (plain[1].end - 5.0).abs() < 1e-4);
        assert!(plain[0].end < 2.0, "the short line gets the smaller share");
        assert_eq!((plain[2].start, plain[2].end, plain[2].text.as_str()), (10.0, 14.0, "Come back!"));
        // A line that was read is on screen while its reading plays.
        let read = [("s1/l2".to_string(), 1.9, 2.6), ("s3/l3".to_string(), 10.15, 0.9)];
        let dubbed = cues(&scenes, &read, "English");
        assert_eq!((dubbed[1].start, dubbed[1].end), (1.9, 4.5));
        assert!((dubbed[2].start - 10.15).abs() < 1e-4 && (dubbed[2].end - 11.05).abs() < 1e-4);
        // The unread line has the shot to itself, but ends where the read one begins.
        assert_eq!((dubbed[0].start, dubbed[0].end), (0.0, 1.9));
    }

    #[test]
    fn a_long_line_is_wrapped_and_shown_in_turns() {
        let text =
            "The little red fox had never seen anything like the paper boat that came down the stream that cold morning, turning slowly.";
        let rows = wrap(text);
        assert!(rows.len() > 2 && rows.iter().all(|r| r.chars().count() <= 42));
        assert_eq!(rows.join(" "), text);
        let long = [line("l1", "", text)];
        let shown = cues(&[Scene { id: "s1", start: 2.0, length: 6.0, lines: &long }], &[], "");
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().all(|c| c.text.lines().count() <= 2));
        assert_eq!((shown[0].start, shown[1].end), (2.0, 8.0));
        assert_eq!(shown[0].end, shown[1].start);
        assert_eq!(
            wrap("Supercalifragilisticexpialidocious-and-then-some-more-of-it yes"),
            ["Supercalifragilisticexpialidocious-and-then-some-more-of-it", "yes"]
        );
    }

    #[test]
    fn subtitles_are_written_as_subrip() {
        let list = [
            Cue { start: 0.0, end: 1.9, text: "A fox walks.".into(), shot: "s1".into() },
            Cue { start: 3661.5, end: 3663.25, text: "It finds a paper boat\nin the snow.".into(), shot: "s1".into() },
        ];
        assert_eq!(
            srt(&list),
            "1\n00:00:00,000 --> 00:00:01,900\nA fox walks.\n\n2\n01:01:01,500 --> 01:01:03,250\nIt finds a paper boat\nin the snow.\n\n"
        );
        assert_eq!(srt(&[]), "");
        // The pictures of one set of subtitles are not used for another, or at another size.
        let key = cues_key(&list, 640, 384);
        assert_eq!(key, cues_key(&list, 640, 384));
        assert_ne!(key, cues_key(&list, 1280, 768));
        assert_ne!(key, cues_key(&list[..1], 640, 384));
    }

    #[test]
    fn the_shots_sound_goes_down_while_something_is_said() {
        assert_eq!(duck_spans(&[]), []);
        // Two lines close to each other are one stretch; one far away is its own.
        assert_eq!(duck_spans(&[(5.0, 6.0), (1.0, 2.0), (2.3, 3.0), (4.0, 4.0)]), [(0.75, 3.25), (4.75, 6.25)]);
        assert_eq!(duck_spans(&[(0.1, 1.0)]), [(0.0, 1.25)]);
    }

    #[test]
    fn a_translation_is_read_by_its_numbers() {
        assert!(translate_prompt("Turkish").contains("into Turkish"));
        assert_eq!(translate_request(&["A fox walks.", "Come back!"]), "1. A fox walks.\n2. Come back!\n");
        let answer = "Here you go:\n1. Bir tilki yürüyor.\n3) \"Geri gel!\"\n7. too many\n- 2: **İkinci satır.**\n1. again";
        assert_eq!(
            read_translation(answer, 3),
            [Some("Bir tilki yürüyor.".to_string()), Some("İkinci satır.".to_string()), Some("Geri gel!".to_string())]
        );
        assert_eq!(read_translation("no list here", 2), [None, None]);
    }
}
