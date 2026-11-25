//! The storyboard: what the chat model is asked for, and how its answer is read.

/// One shot as the chat model wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub prompt: String,
    /// The shot carries on from the one before it (same scene); otherwise it starts with a cut.
    pub continues: bool,
}

/// How many shots the chat model is asked for in one answer. A long film is written in parts.
pub const PART: usize = 12;

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
pub fn shot_lines(text: &str, limit: usize) -> Vec<Line> {
    let mut out = Vec::new();
    let (mut scenes, mut fresh) = (false, true);
    for line in text.lines() {
        if out.len() >= limit {
            break;
        }
        let line = line.trim().trim_start_matches(['-', '*', '•', '#']).trim();
        let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
        // Numbered ("3. ...", "3) ...") lines only; headings and chatter are dropped.
        let numbered = rest.len() < line.len() && (rest.starts_with('.') || rest.starts_with(')') || rest.starts_with(':'));
        if !numbered {
            if line.trim_start_matches(['*', '_']).to_ascii_lowercase().starts_with("scene") {
                scenes = true;
                fresh = true;
            }
            continue;
        }
        let (said, body) = marker(rest[1..].trim());
        let body = body.trim().trim_matches(['"', '*', '`']).trim();
        if body.chars().count() < 12 {
            continue;
        }
        let continues = said.unwrap_or(scenes && !fresh);
        fresh = false;
        out.push(Line { prompt: body.chars().take(super::MAX_PROMPT).collect(), continues });
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
    fn a_long_film_is_asked_for_in_parts() {
        assert_eq!(part_request("a fox", &[], 6, 6), "a fox");
        let so_far: Vec<Line> = (0..20).map(|i| Line { prompt: format!("shot number {i} of the film"), continues: i % 3 != 0 }).collect();
        let next = part_request("a fox", &so_far, 12, 60);
        assert!(next.contains("earlier shots left out") && !next.contains("\n8. (") && next.contains("\n9. (continues) shot number 8"));
        assert!(next.contains("Write shots 21 to 32 now") && next.contains("do not end the story yet"));
        assert!(part_request("a fox", &so_far, 4, 24).contains("bring the film to its end"));
    }
}
