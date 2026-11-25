//! The storyboard: what the chat model is asked for, and how its answer is read.

/// One shot as the chat model wrote it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Line {
    pub prompt: String,
    /// The shot carries on from the one before it (same scene); otherwise it starts with a cut.
    pub continues: bool,
    /// What is said during the shot: who (a name of the cast, or empty for the narrator) and the words.
    pub said: Vec<(String, String)>,
}

/// What a film brings to the writing of its shots.
#[derive(Debug, Clone, Default)]
pub struct Wants<'a> {
    /// The names of the cast. The writer names them and never says how they look.
    pub cast: &'a [String],
    /// A few words on who each of them is ("a small grey cat"), in the order of the names, so
    /// the writer knows whom a name stands for. May be shorter than the cast, or empty.
    pub who: &'a [String],
    /// The look every shot shares, so the writer does not write it into each one.
    pub look: &'a str,
    /// Whether the shots get spoken lines, and in which language.
    pub speech: bool,
    pub language: &'a str,
}

/// How many shots the chat model is asked for in one answer. A long film is written in parts.
pub const PART: usize = 12;

/// What the chat model is told for a film with a cast, a look or spoken lines.
pub fn system_prompt_for(count: usize, wants: &Wants) -> String {
    let mut text = format!(
        "You plan films for a text-to-video model. A film is told in scenes. A scene is one place and one moment, shown in 2 to 4 shots that follow each other without a cut. \
         Write exactly {count} shots as a numbered list, one line each. \
         Start each line with (cut) when the shot begins a new scene, or with (continues) when it carries on from the shot before it in the same scene. The first shot of a film is (cut). \
         After the marker, describe one continuous shot of about five seconds in English: who or what is seen, what moves, the place, the camera, the light, and the sound. \
         Describe the same places with the same words in every shot so they look alike. No titles, no text on screen."
    );
    if !wants.cast.is_empty() {
        let cast: Vec<String> = wants
            .cast
            .iter()
            .enumerate()
            .map(|(i, name)| match wants.who.get(i).filter(|w| !w.is_empty()) {
                Some(who) => format!("{name} ({who})"),
                None => name.clone(),
            })
            .collect();
        text.push_str(&format!(
            "\nThe cast of this film: {}. Call them by exactly these names, the name alone, every time they are in a shot: write \"{}\", never \"the woman\" or \"her cat\". \
             Never say how they look: their age, face, hair and clothes are added for you.",
            cast.join(", "),
            wants.cast[0]
        ));
    }
    if !wants.look.trim().is_empty() {
        text.push_str("\nThe style, the colours and the lens of the film are added for you: do not write them into the shots.");
    }
    let example_names: (&str, &str) = match wants.cast {
        [] => ("A red fox", "The red fox"),
        [one, ..] => (one.as_str(), one.as_str()),
    };
    if wants.speech {
        let language = if wants.language.trim().is_empty() { "English" } else { wants.language.trim() };
        let speakers =
            if wants.cast.is_empty() { "NARRATOR".to_string() } else { format!("NARRATOR or one of the cast ({})", wants.cast.join(", ")) };
        text.push_str(&format!(
            "\nUnder a shot you may write what is said during it, each on a line of its own that starts with the speaker and a colon: {speakers}. \
             One short sentence a line, 14 words at most, in {language}: it has to be said in five seconds. Most shots have one line; a shot may have none."
        ));
    } else {
        text.push_str("\nNo dialogue: nothing is said in this film.");
    }
    text.push_str(&format!(
        "\nExample:\n\
         1. (cut) {} trots through fresh snow between birch trees, the camera follows from the side, soft morning light, quiet wind.\n",
        example_names.0
    ));
    if wants.speech {
        text.push_str("NARRATOR: Nobody had walked here since the snow fell.\n");
    }
    text.push_str(&format!(
        "2. (continues) {} stops between the birch trees and looks up as snow falls from a branch, the camera pushes in slowly, a soft thump of snow.\n\
         3. (cut) A paper boat floats down a narrow stream between snowy banks, the camera moves with the boat, low sun, gentle running water.",
        example_names.1
    ));
    text
}

pub fn system_prompt(count: usize) -> String {
    format!(
        "You plan films for a text-to-video model. A film is told in scenes. A scene is one place and one moment, shown in 2 to 4 shots that follow each other without a cut. \
         Write exactly {count} shots as a numbered list, one line each, nothing else. \
         Start each line with (cut) when the shot begins a new scene, or with (continues) when it carries on from the shot before it in the same scene. The first shot of a film is (cut). \
         After the marker, describe one continuous shot of about five seconds in English: who or what is seen, what moves, the place, the light, the camera, and the sound. \
         Describe the same characters and places with the same words in every shot so they look alike. No dialogue, no titles, no text on screen.\n\
         Example:\n\
         1. (cut) A red fox trots through fresh snow between birch trees, soft morning light, the camera follows from the side, quiet wind.\n\
         2. (continues) The red fox stops between the birch trees and looks up as snow falls from a branch, the camera pushes in slowly, a soft thump of snow.\n\
         3. (cut) A paper boat floats down a narrow stream between snowy banks, low sun, the camera moves with the boat, gentle running water."
    )
}

/// What the chat model is told for one part of the film: the idea, the shots so far, which to write now.
pub fn part_request(idea: &str, so_far: &[Line], count: usize, total: usize) -> String {
    if so_far.is_empty() && count >= total {
        return idea.to_string();
    }
    let mut text = format!("The film: {idea}\n\n");
    if !so_far.is_empty() {
        // The last shots are enough to carry on from; the idea holds the rest.
        let from = so_far.len().saturating_sub(PART);
        text.push_str(if from > 0 { "The film so far (earlier shots left out):\n" } else { "The film so far:\n" });
        for (i, line) in so_far.iter().enumerate().skip(from) {
            text.push_str(&format!("{}. ({}) {}\n", i + 1, if line.continues { "continues" } else { "cut" }, line.prompt));
        }
        text.push('\n');
    }
    let (first, last) = (so_far.len() + 1, so_far.len() + count);
    text.push_str(&format!("The whole film has {total} shots. Write shots {first} to {last} now, numbered from {first}."));
    if last >= total {
        text.push_str(" These are the last shots: bring the film to its end.");
    } else {
        text.push_str(" The film goes on after these: do not end the story yet.");
    }
    text
}

/// The first words of a look, as a hint at who a name stands for: "a small grey tabby cat".
pub fn who_is(look: &str) -> String {
    let first = look.split([',', '.', ';']).next().unwrap_or("");
    first.split_whitespace().take(7).collect::<Vec<_>>().join(" ")
}

/// What the chat model is told when it writes one description again in a film with a cast.
pub fn rewrite_prompt_for(cast: &[String]) -> String {
    let mut text = rewrite_prompt();
    if !cast.is_empty() {
        text.push_str(&format!(
            " The cast of this film: {}. Call them by exactly these names and never say how they look: that is added for you.",
            cast.join(", ")
        ));
    }
    text
}

/// What the chat model is told when it writes one description again.
pub fn rewrite_prompt() -> String {
    "You improve one shot description for a text-to-video model. Answer with the improved description only: one line, no numbering, no quotes, no notes. \
     Describe one continuous shot of about five seconds in English: who or what is seen, what moves, the place, the light, the camera, and the sound. \
     Keep what happens in the shot. Describe the same characters and places with the same words as the other shots of the film, so they look alike. \
     No dialogue, no titles, no text on screen."
        .to_string()
}

/// The shot to write again, with the shots around it and what the person wants different.
pub fn rewrite_request(before: &[&str], after: Option<&str>, shot: &str, hint: &str, continues: bool) -> String {
    let mut text = String::new();
    if !before.is_empty() {
        text.push_str("The shots before it in the film:\n");
        for line in before {
            text.push_str(&format!("- {line}\n"));
        }
        text.push('\n');
    }
    if let Some(next) = after {
        text.push_str(&format!("The shot after it:\n- {next}\n\n"));
    }
    if continues {
        text.push_str("This shot carries on from the shot before it without a cut: same place, same moment.\n\n");
    }
    if shot.is_empty() {
        text.push_str("The shot has no description yet. Write one that fits between the others.");
    } else {
        text.push_str(&format!("The shot to improve:\n{shot}"));
    }
    if !hint.is_empty() {
        text.push_str(&format!("\n\nWhat should be different: {hint}"));
    }
    text
}

/// The one description out of what the chat model wrote: its first real line, without a number,
/// a marker, a label or quotes around it.
pub fn one_description(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•', '#', '>']).trim();
        let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
        let rest = if rest.len() < line.len() { rest.trim_start_matches(['.', ')', ':']).trim() } else { line };
        let (_, body) = marker(rest);
        // "Improved description: ..." says nothing about the shot.
        let body = match body.split_once(':') {
            Some((label, said))
                if label.len() <= 28
                    && ["description", "shot", "improved", "rewritten", "version"]
                        .iter()
                        .any(|w| label.to_ascii_lowercase().contains(w)) =>
            {
                said
            }
            _ => body,
        };
        let body = body.trim_matches(|c: char| c.is_whitespace() || ['"', '*', '`', '\u{201c}', '\u{201d}'].contains(&c));
        if body.chars().count() >= 12 {
            return Some(body.chars().take(super::MAX_PROMPT).collect());
        }
    }
    None
}

/// A leading "(cut)" or "(continues)", with or without brackets: what it says and the rest of the line.
fn marker(body: &str) -> (Option<bool>, &str) {
    let body = body.trim_start_matches(['*', '"', '`', ' ']);
    let (word, rest) = match body.chars().next() {
        Some(open @ ('(' | '[')) => {
            let close = if open == '(' { ')' } else { ']' };
            match body.find(close) {
                Some(end) => (&body[1..end], &body[end + 1..]),
                None => return (None, body),
            }
        }
        // "CUT: a fox ..." or "Continues - the fox ..."
        _ => match body.find([':', '-', '–', '—']) {
            Some(end) if end <= 12 => (&body[..end], &body[end..]),
            _ => return (None, body),
        },
    };
    let said = match word.trim().trim_end_matches('.').to_ascii_lowercase().as_str() {
        "cut" | "cut to" | "new scene" | "scene" | "new" => false,
        "continues" | "continue" | "continued" | "continuing" | "cont" | "same scene" | "same" => true,
        _ => return (None, body),
    };
    (Some(said), rest.trim_start_matches(['*', ':', '-', '–', '—', ' ']))
}

/// The shots out of what the chat model wrote: numbered lines, each with an optional leading marker.
/// Without markers, a "Scene" heading starts a scene and the shots under it carry on from each other.
#[cfg(test)]
pub fn shot_lines(text: &str, limit: usize) -> Vec<Line> {
    shots_and_lines(text, limit, None)
}

/// Who a line under a shot is said by, and its words: "NARRATOR: ..." or "Mira: ...". `None` for
/// a line that is not said by the narrator or one of `cast`.
fn spoken(line: &str, cast: &[String]) -> Option<(String, String)> {
    let line = line.trim().trim_start_matches(['-', '*', '•', '>', ' ']);
    let (who, said) = line.split_once(':')?;
    let who = who.trim_matches(|c: char| c.is_whitespace() || ['*', '_', '"', '(', ')', '[', ']'].contains(&c)).to_lowercase();
    let who = match who.as_str() {
        "narrator" | "narration" | "voice" | "voice-over" | "voice over" | "voiceover" | "vo" => String::new(),
        name => cast.iter().find(|c| c.to_lowercase() == name)?.clone(),
    };
    let said = said.trim_matches(|c: char| c.is_whitespace() || ['"', '*', '`', '\u{201c}', '\u{201d}'].contains(&c));
    (said.chars().count() >= 2)
        .then(|| (who, said.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(super::words::MAX_LINE).collect()))
}

/// The shots as `shot_lines` reads them, and with `cast` given also what is said under each:
/// the lines between two numbered shots that start with the narrator or a name of the cast.
pub fn shots_and_lines(text: &str, limit: usize, cast: Option<&[String]>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let (mut scenes, mut fresh) = (false, true);
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•', '#']).trim();
        let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
        // Numbered ("3. ...", "3) ...") lines only; headings and chatter are dropped.
        let numbered = rest.len() < line.len() && (rest.starts_with('.') || rest.starts_with(')') || rest.starts_with(':'));
        if !numbered {
            if line.trim_start_matches(['*', '_']).to_ascii_lowercase().starts_with("scene") {
                scenes = true;
                fresh = true;
            } else if let (Some(cast), Some(shot)) = (cast, out.last_mut()) {
                if let Some(said) = spoken(line, cast).filter(|_| shot.said.len() < super::words::MAX_LINES) {
                    shot.said.push(said);
                }
            }
            continue;
        }
        // The lines under the last shot are still read; the shot after it is not.
        if out.len() >= limit {
            break;
        }
        let (said, body) = marker(rest[1..].trim());
        let body = body.trim().trim_matches(['"', '*', '`']).trim();
        if body.chars().count() < 12 {
            continue;
        }
        let continues = said.unwrap_or(scenes && !fresh);
        fresh = false;
        out.push(Line { prompt: body.chars().take(super::MAX_PROMPT).collect(), continues, said: Vec::new() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_storyboard_is_read_from_numbered_lines_only() {
        let text = "Here are your shots:\n1. A red fox trots through fresh snow between birch trees, soft light.\n2) The fox stops and looks up as snow falls from a branch.\n\n**Notes**: keep it calm.\n3. \"Close on the fox's face, breath in the cold air, quiet wind.\"";
        let shots = shot_lines(text, 10);
        assert_eq!(shots.len(), 3);
        assert!(shots[0].prompt.starts_with("A red fox") && shots[2].prompt.starts_with("Close on"));
        assert!(shots.iter().all(|s| !s.continues));
        assert_eq!(shot_lines(text, 2).len(), 2);
    }

    #[test]
    fn markers_say_which_shots_carry_on() {
        let text = "1. (cut) A red fox trots through fresh snow between birch trees.\n\
                    2. (continues) The red fox stops and looks up at a branch.\n\
                    3. **(Continues)** The red fox shakes snow from its fur.\n\
                    4. [CUT] A paper boat floats down a narrow stream.\n\
                    5. Continues: the paper boat turns in an eddy near the bank.\n\
                    6. CUT - A wide view of the forest at dusk, snow falling.\n\
                    7. (wide shot) The red fox walks away over the hill.\n\
                    8. (cont.) The red fox is a small dot on the white hill.";
        let shots = shot_lines(text, 20);
        assert_eq!(shots.iter().map(|s| s.continues).collect::<Vec<_>>(), [false, true, true, false, true, false, false, true]);
        assert_eq!(shots[0].prompt, "A red fox trots through fresh snow between birch trees.");
        assert_eq!(shots[2].prompt, "The red fox shakes snow from its fur.");
        assert_eq!(shots[4].prompt, "the paper boat turns in an eddy near the bank.");
        assert_eq!(shots[5].prompt, "A wide view of the forest at dusk, snow falling.");
        // A bracket that is not a marker belongs to the description.
        assert!(shots[6].prompt.starts_with("(wide shot)"));
    }

    #[test]
    fn scene_headings_stand_in_for_missing_markers() {
        let text = "## Scene 1: the forest\n1. A red fox trots through fresh snow.\n2. The red fox stops and looks up.\n\n**Scene 2**\n3. A paper boat floats down a stream.\n4. (cut) The forest from above at dusk.\n5. Snow falls on the dark trees below.";
        let shots = shot_lines(text, 20);
        assert_eq!(shots.iter().map(|s| s.continues).collect::<Vec<_>>(), [false, true, false, false, true]);
    }

    #[test]
    fn one_description_is_read_from_the_answer() {
        assert_eq!(
            one_description("A red fox trots through fresh snow, soft light.").as_deref(),
            Some("A red fox trots through fresh snow, soft light.")
        );
        assert_eq!(
            one_description(
                "Sure!\n\n**Improved description:** \"A red fox trots through fresh snow between birch trees.\"\n\nNotes: calm."
            )
            .as_deref(),
            Some("A red fox trots through fresh snow between birch trees.")
        );
        assert_eq!(
            one_description("1. (continues) The red fox stops and looks up at a branch.").as_deref(),
            Some("The red fox stops and looks up at a branch.")
        );
        // A colon inside the description stays.
        assert_eq!(
            one_description("A wide view of the forest at dusk: snow falling, quiet wind.").as_deref(),
            Some("A wide view of the forest at dusk: snow falling, quiet wind.")
        );
        assert_eq!(one_description("OK.\n\n"), None);
        let asked = rewrite_request(&["a fox walks", "the fox stops"], Some("a boat"), "fox looks up", "closer", true);
        assert!(asked.contains("- the fox stops\n") && asked.contains("The shot after it:\n- a boat") && asked.contains("without a cut"));
        assert!(asked.ends_with("The shot to improve:\nfox looks up\n\nWhat should be different: closer"));
        assert!(rewrite_request(&[], None, "", "a storm", false).starts_with("The shot has no description yet."));
    }

    #[test]
    fn a_long_film_is_asked_for_in_parts() {
        assert_eq!(part_request("a fox", &[], 6, 6), "a fox");
        let so_far: Vec<Line> =
            (0..20).map(|i| Line { prompt: format!("shot number {i} of the film"), continues: i % 3 != 0, said: Vec::new() }).collect();
        let next = part_request("a fox", &so_far, 12, 60);
        assert!(next.contains("earlier shots left out") && !next.contains("\n8. (") && next.contains("\n9. (continues) shot number 8"));
        assert!(next.contains("Write shots 21 to 32 now") && next.contains("do not end the story yet"));
        assert!(part_request("a fox", &so_far, 4, 24).contains("bring the film to its end"));
    }

    #[test]
    fn the_writer_is_given_the_cast_and_must_name_them() {
        let cast = ["Mira".to_string(), "Fox".to_string()];
        let who = [who_is("a girl of about eight, in a yellow raincoat"), who_is("")];
        assert_eq!(who[0], "a girl of about eight");
        let asked = system_prompt_for(6, &Wants { cast: &cast, who: &who, look: "soft watercolour", speech: false, language: "" });
        assert!(asked.contains("exactly 6 shots"));
        assert!(
            asked.contains("The cast of this film: Mira (a girl of about eight), Fox. Call them by exactly these names, the name alone")
                && asked.contains("write \"Mira\", never")
                && asked.contains("Never say how they look")
        );
        assert!(asked.contains("do not write them into the shots") && asked.contains("nothing is said in this film"));
        // The example uses a name of the cast and no words for how anyone looks.
        assert!(asked.contains("1. (cut) Mira trots") && !asked.contains("red fox") && !asked.contains("NARRATOR"));
        let plain = system_prompt_for(3, &Wants::default());
        assert!(!plain.contains("The cast of this film") && !plain.contains("added for you") && plain.contains("A red fox trots"));
        let said = system_prompt_for(3, &Wants { cast: &cast, speech: true, language: "Turkish", ..Wants::default() });
        assert!(
            said.contains("NARRATOR or one of the cast (Mira, Fox)")
                && said.contains("in Turkish")
                && said.contains("\nNARRATOR: Nobody had walked here")
        );
        assert!(rewrite_prompt_for(&cast).contains("Mira, Fox") && rewrite_prompt_for(&[]) == rewrite_prompt());
    }

    #[test]
    fn what_is_said_is_read_from_under_its_shot() {
        let cast = ["Mira".to_string(), "Old Tom".to_string()];
        let text = "NARRATOR: said before any shot, dropped\n\
                    1. (cut) Mira kneels by the stream and sets a paper boat on the water.\n\
                    NARRATOR: \"She had folded it that morning.\"\n\
                    - **Mira:** Go on, little boat!\n\
                    Note: keep it calm.\n\
                    2. (continues) The paper boat turns once and slides away under the bridge.\n\
                    3. (cut) Old Tom looks up from his fishing rod on the far bank.\n\
                       old tom: Well, look at that.\n\
                    Stranger: I am not in the cast.\n\
                    4. (cut) This shot is past the limit and so is its line.\n\
                    NARRATOR: Not read.";
        let shots = shots_and_lines(text, 3, Some(&cast));
        assert_eq!(shots.len(), 3);
        assert_eq!(
            shots[0].said,
            [(String::new(), "She had folded it that morning.".to_string()), ("Mira".to_string(), "Go on, little boat!".to_string())]
        );
        assert!(shots[1].said.is_empty());
        assert_eq!(shots[2].said, [("Old Tom".to_string(), "Well, look at that.".to_string())]);
        // Without a cast to read for, nothing under a shot is a line.
        assert!(shot_lines(text, 3).iter().all(|s| s.said.is_empty()));
    }
}
