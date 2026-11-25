//! The brief: what a film is before its shots are written. The person writes a few plain words;
//! the chat model answers with a title, a shape, a length, a look, the cast and whether the film
//! is told by a voice. The answer is read line by line, and whatever is missing gets a plain
//! default, so a small model that forgets a field still gives a film. Nothing here runs anything.

use serde::Serialize;

/// The lengths a film is offered in, in seconds.
pub const LENGTHS: [u32; 5] = [15, 30, 60, 120, 300];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Brief {
    pub title: String,
    /// "landscape", "portrait" or "square".
    pub shape: String,
    pub seconds: u32,
    /// One line every shot shares: style, colours, light, lens.
    pub look: String,
    /// Whether the shots get spoken lines.
    pub speech: bool,
    /// The language of what is said.
    pub language: String,
    /// Name and fixed look of each character.
    pub cast: Vec<(String, String)>,
}

pub fn system_prompt() -> String {
    "You are the director of a short film that a text-to-video model will make. Read what the person wants and fill in this form. Answer with the form only, one field a line:\n\
     TITLE: two to five words\n\
     SHAPE: wide, tall or square. tall for phones, stories and reels; square only when asked for; otherwise wide\n\
     LENGTH: the seconds the film lasts: 15, 30, 60 or 120. 30 unless the person says or the story needs otherwise\n\
     LOOK: one line every shot shares: the style, the colours, the light, the lens\n\
     NARRATION: yes when a voice tells the story or somebody speaks, no for pictures and sound alone\n\
     LANGUAGE: the language the person wrote in\n\
     CAST:\n\
     - Name: how this character looks in every shot: age, build, face, hair, clothes and their colours\n\
     One line for each character who is seen more than once, four at most. Write CAST: none for a film without characters.\n\
     \n\
     Example:\n\
     TITLE: The Paper Boat\n\
     SHAPE: wide\n\
     LENGTH: 30\n\
     LOOK: soft watercolour animation, muted winter colours, low morning light, shallow depth of field\n\
     NARRATION: no\n\
     LANGUAGE: English\n\
     CAST:\n\
     - Fox: a small red fox with a white-tipped tail, bright amber eyes and one torn ear\n\
     - Mira: a girl of about eight in a yellow raincoat and red rubber boots, short dark hair, round face"
        .to_string()
}

/// A name for the film out of the first words of what the person wrote.
pub fn title_of(idea: &str) -> String {
    let words: String = idea.chars().map(|c| if c.is_alphanumeric() || c == '\'' || c == '-' { c } else { ' ' }).collect();
    let title: String = words.split_whitespace().take(6).collect::<Vec<_>>().join(" ").chars().take(48).collect();
    let mut letters = title.trim().chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => "Film".into(),
    }
}

/// The shape a field of the form names: "wide", "tall (9:16)", "square".
fn shape_word(value: &str) -> Option<&'static str> {
    let value = value.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| value.contains(w));
    if has(&["tall", "portrait", "vertical", "9:16"]) {
        Some("portrait")
    } else if has(&["square", "1:1"]) {
        Some("square")
    } else if has(&["wide", "landscape", "horizontal", "16:9"]) {
        Some("landscape")
    } else {
        None
    }
}

/// The shape the person asks for in so many words, if they do. Only words that mean a shape and
/// nothing else: "a tall woman by a wide river" asks for none.
pub fn shape_asked(idea: &str) -> Option<&'static str> {
    let idea = idea.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| idea.contains(w));
    if has(&["vertical", "9:16", "tiktok", "reels", "instagram reel", "youtube shorts", "for phones", "for my phone"]) {
        Some("portrait")
    } else if has(&["square video", "square film", "square format", "1:1"]) {
        Some("square")
    } else if has(&["16:9", "widescreen", "horizontal video", "landscape format"]) {
        Some("landscape")
    } else {
        None
    }
}

/// The seconds a field such as "30", "45 seconds" or "2 min" says, between 5 seconds and 5 minutes.
fn seconds_said(value: &str) -> Option<u32> {
    let digits: String = value.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f32 = digits.parse().ok()?;
    let rest = value.to_lowercase();
    let seconds = if rest.contains("min") { n * 60.0 } else { n };
    (seconds >= 1.0).then(|| (seconds.round() as u32).clamp(5, 300))
}

/// One line of the answer without the marks a chat model puts around it.
fn bare(line: &str) -> &str {
    line.trim().trim_start_matches(['-', '*', '•', '#', '>', ' ']).trim()
}

/// "KEY: value" out of a line, the key in lower case without marks.
fn field(line: &str) -> Option<(String, &str)> {
    let (key, value) = bare(line).split_once(':')?;
    let key = key.trim_matches(['*', '_', ' ', '`']).to_lowercase();
    (!key.is_empty() && key.len() <= 40).then_some((key, value.trim().trim_start_matches(['*', '_']).trim()))
}

fn unquoted(value: &str) -> String {
    value
        .trim_matches(|c: char| c.is_whitespace() || ['"', '*', '`', '\u{201c}', '\u{201d}'].contains(&c))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The brief out of what the chat model wrote. `idea` is what the person wrote: it names the
/// film when the answer does not, and a shape it asks for in so many words is the shape.
pub fn read(answer: &str, idea: &str) -> Brief {
    let mut brief = Brief {
        title: String::new(),
        shape: String::new(),
        seconds: 0,
        look: String::new(),
        speech: false,
        language: String::new(),
        cast: Vec::new(),
    };
    let mut in_cast = false;
    for line in answer.lines() {
        // A heading such as "### Characters" starts the cast as "CAST:" does.
        let heading = bare(line).trim_matches(['*', '_', ':', ' ']).to_lowercase();
        if heading == "cast" || heading == "characters" {
            in_cast = true;
            continue;
        }
        let Some((key, value)) = field(line) else { continue };
        let known = match key.as_str() {
            "title" | "name" | "film" => {
                brief.title = unquoted(value).chars().take(60).collect();
                true
            }
            "shape" | "format" | "aspect" | "aspect ratio" => {
                brief.shape = shape_word(value).unwrap_or("").to_string();
                true
            }
            "length" | "duration" | "seconds" => {
                brief.seconds = seconds_said(value).unwrap_or(0);
                true
            }
            "look" | "style" | "look and style" => {
                brief.look = unquoted(value).chars().take(300).collect();
                true
            }
            "narration" | "narrator" | "voice" | "speech" => {
                let said = value.to_lowercase();
                brief.speech = said.starts_with("yes") || said.starts_with("true") || said.starts_with("on");
                true
            }
            "language" => {
                brief.language = super::words::language(value);
                true
            }
            "cast" | "characters" => true,
            _ => false,
        };
        if known {
            in_cast = matches!(key.as_str(), "cast" | "characters");
            continue;
        }
        // Under CAST every "Name: look" line is a character.
        let (name, look) = (unquoted(&key), unquoted(value));
        let name_as_written = bare(line).split_once(':').map_or(name.clone(), |(n, _)| unquoted(n));
        let fits = brief.cast.len() < 6 && name.chars().count() <= 30 && look.chars().count() >= 8 && name != "none";
        if in_cast && fits && !brief.cast.iter().any(|(n, _)| n.to_lowercase() == name) {
            brief.cast.push((name_as_written, look.chars().take(super::cast::MAX_LOOK).collect()));
        }
    }
    if brief.title.is_empty() {
        brief.title = title_of(idea);
    }
    // What the person asked for in so many words wins over what the model chose.
    if let Some(shape) = shape_asked(idea) {
        brief.shape = shape.into();
    }
    if brief.shape.is_empty() {
        brief.shape = "landscape".into();
    }
    if brief.seconds == 0 {
        brief.seconds = 30;
    }
    if brief.language.is_empty() {
        brief.language = "English".into();
    }
    brief
}

/// The offered length closest to what the brief asks for.
pub fn offered(seconds: u32) -> u32 {
    LENGTHS.into_iter().min_by_key(|l| l.abs_diff(seconds)).unwrap_or(30)
}

/// The shots of a film that is used as written: every line of the text is one shot, and a text
/// on one line is cut at its sentences.
pub fn as_written(text: &str, most: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(bare).filter(|l| !l.is_empty()).collect();
    let parts: Vec<String> = if lines.len() > 1 {
        lines
            .into_iter()
            .map(|l| l.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')']).trim().to_string())
            .collect()
    } else {
        let mut parts = Vec::new();
        let mut now = String::new();
        for c in text.trim().chars() {
            now.push(c);
            if ['.', '!', '?'].contains(&c) {
                parts.push(std::mem::take(&mut now));
            }
        }
        parts.push(now);
        parts.into_iter().map(|p| p.trim().to_string()).collect()
    };
    parts.into_iter().filter(|p| p.chars().count() >= 3).map(|p| p.chars().take(super::MAX_PROMPT).collect()).take(most).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brief_is_read_from_the_form() {
        let answer = "TITLE: The Paper Boat\nSHAPE: wide\nLENGTH: 60 seconds\nLOOK: soft watercolour animation, muted winter colours\nNARRATION: yes\nLANGUAGE: english\nCAST:\n- Fox: a small red fox with a white-tipped tail and amber eyes\n- Mira: a girl of about eight in a yellow raincoat, short dark hair";
        let brief = read(answer, "a fox finds a paper boat");
        assert_eq!(
            brief,
            Brief {
                title: "The Paper Boat".into(),
                shape: "landscape".into(),
                seconds: 60,
                look: "soft watercolour animation, muted winter colours".into(),
                speech: true,
                language: "English".into(),
                cast: vec![
                    ("Fox".into(), "a small red fox with a white-tipped tail and amber eyes".into()),
                    ("Mira".into(), "a girl of about eight in a yellow raincoat, short dark hair".into()),
                ],
            }
        );
    }

    #[test]
    fn a_small_model_s_answer_is_still_a_brief() {
        // Markdown, other words for the fields, chatter, a field missing, a length in minutes.
        let answer = "Sure! Here is the brief:\n\n**Title:** \"Night Market\"\n**Format:** vertical (9:16)\n**Duration:** 2 minutes\n**Style:** neon-lit handheld documentary, rain on the lens.\n\n### Characters\n* **Lena**: a woman in her thirties with a silver bob and a red umbrella\n* Lena: said twice\n* Old cook: a broad man of sixty in a stained white apron\n* x: short\n\nLet me know if you want changes!";
        let brief = read(answer, "a night market");
        assert_eq!((brief.title.as_str(), brief.shape.as_str(), brief.seconds), ("Night Market", "portrait", 120));
        assert_eq!(brief.look, "neon-lit handheld documentary, rain on the lens.");
        assert!(!brief.speech);
        assert_eq!(brief.language, "English");
        assert_eq!(brief.cast.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Lena", "Old cook"]);
        assert_eq!(brief.cast[0].1, "a woman in her thirties with a silver bob and a red umbrella");
    }

    #[test]
    fn nothing_usable_gives_the_usual_film() {
        let brief = read("I cannot help with that.", "a red fox finds a paper boat in a snowy forest and follows it");
        assert_eq!(brief.title, "A red fox finds a paper");
        assert_eq!((brief.shape.as_str(), brief.seconds, brief.speech, brief.cast.len()), ("landscape", 30, false, 0));
        assert_eq!(read("CAST: none\nLENGTH: soon", "").title, "Film");
        // The shape the person asked for wins over the one the model chose.
        assert_eq!(read("SHAPE: wide", "a reel about my dog for TikTok").shape, "portrait");
        assert_eq!(read("SHAPE: wide", "a tall woman on the phone by a wide river").shape, "landscape");
        assert_eq!(read("SHAPE: square", "my dog").shape, "square");
        assert_eq!(read("LENGTH: 9999", "").seconds, 300);
        assert_eq!(read("LENGTH: 0.5 min", "").seconds, 30);
        assert_eq!((offered(45), offered(20), offered(100), offered(7)), (30, 15, 120, 15));
    }

    #[test]
    fn a_text_used_as_written_is_one_shot_a_line() {
        assert_eq!(
            as_written("1. A fox walks through snow.\n\n- A boat floats by\n3) The fox follows it", 10),
            ["A fox walks through snow.", "A boat floats by", "The fox follows it"]
        );
        assert_eq!(
            as_written("A fox walks through snow. A boat floats by! Does the fox follow", 10),
            ["A fox walks through snow.", "A boat floats by!", "Does the fox follow"]
        );
        assert_eq!(as_written("one\ntwo\nthree", 2).len(), 2);
        assert!(as_written("  ", 5).is_empty());
    }
}
