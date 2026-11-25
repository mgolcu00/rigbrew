//! The cast of a film: characters with a name and a fixed look. A shot names its characters and
//! never describes them; when the shot is made, the look of each one it names is put into what
//! the video model is told, word for word, the same in every shot. Nothing here runs anything.

use serde::{Deserialize, Serialize};

pub const MAX_CAST: usize = 12;
const MAX_NAME: usize = 40;
pub const MAX_LOOK: usize = 400;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// The fixed words that describe them: age, build, face, hair, clothes, colours.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub look: String,
    /// The speaker number of the voice that says their lines; the film's own voice without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<u32>,
    /// How many portraits were made of them: 0 for none. The newest is `<folder>/cast/<id>.<n>.png`.
    #[serde(default, skip_serializing_if = "is_none_yet")]
    pub portrait: u32,
}

fn is_none_yet(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemberIn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub look: String,
    #[serde(default)]
    pub voice: Option<u32>,
}

fn one_row(text: &str, most: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(most).collect()
}

/// The cast as it is saved. A member that keeps its id keeps its portrait; two members cannot
/// have the same name, since a shot names them by it.
pub fn merge_cast(old: &[Member], new: Vec<MemberIn>, mut id: impl FnMut() -> String) -> Result<Vec<Member>, String> {
    if new.len() > MAX_CAST {
        return Err(format!("a film has at most {MAX_CAST} characters"));
    }
    let valid = |i: &str| (1..=40).contains(&i.len()) && i.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let mut out: Vec<Member> = Vec::with_capacity(new.len());
    for m in new {
        let name = one_row(&m.name, MAX_NAME);
        if name.is_empty() {
            return Err("a character needs a name".into());
        }
        if out.iter().any(|o| o.name.to_lowercase() == name.to_lowercase()) {
            return Err(format!("two characters are named {name}"));
        }
        let member_id = m.id.filter(|i| valid(i) && !out.iter().any(|o| o.id == *i)).unwrap_or_else(&mut id);
        let portrait = old.iter().find(|o| o.id == member_id).map_or(0, |o| o.portrait);
        out.push(Member { id: member_id, name, look: one_row(&m.look, MAX_LOOK), voice: m.voice.filter(|v| *v < 1000), portrait });
    }
    Ok(out)
}

/// Whether `name` stands in `text` as a word of its own, whatever the case: "Mira" is in
/// "mira's boat" and not in "admiral".
fn names(text: &str, name: &str) -> bool {
    let (text, name) = (text.to_lowercase(), name.to_lowercase());
    if name.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(&name) {
        let (start, end) = (from + at, from + at + name.len());
        let before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// The members a description names, in the order of the cast.
pub fn named<'a>(cast: &'a [Member], prompt: &str) -> Vec<&'a Member> {
    cast.iter().filter(|m| names(prompt, &m.name)).collect()
}

/// What the video model is told for a shot: its description, then the look of every character
/// it names, word for word, then the look of the film. A shot that names nobody in a film
/// without a look is told its description and nothing else.
pub fn told(look: &str, cast: &[Member], prompt: &str) -> String {
    let prompt = prompt.trim();
    let look = look.trim().trim_end_matches('.');
    let seen: Vec<&Member> = named(cast, prompt).into_iter().filter(|m| !m.look.trim().is_empty()).collect();
    if prompt.is_empty() || (seen.is_empty() && look.is_empty()) {
        return prompt.to_string();
    }
    let mut text = prompt.to_string();
    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
    for m in seen {
        text.push_str(&format!(" {}: {}.", m.name, m.look.trim().trim_end_matches('.')));
    }
    if !look.is_empty() {
        text.push_str(&format!(" {look}."));
    }
    text
}

/// What the chat model is told when it writes the look of one character from a few words.
pub fn look_prompt() -> String {
    "You write how one film character looks, for a text-to-video model. Answer with one line only, 20 to 40 words: \
     age, build, face, hair, clothes and their colours, one thing that sets them apart. \
     Start with \"a\" or \"an\". No name, no story, nothing they do, no quotes, no notes."
        .to_string()
}

pub fn look_request(name: &str, words: &str, film_look: &str) -> String {
    let mut text = format!("The character is called {name}.");
    if !words.trim().is_empty() {
        text.push_str(&format!("\nA few words about them: {}", words.trim()));
    }
    if !film_look.trim().is_empty() {
        text.push_str(&format!("\nThe look of the film: {}", film_look.trim()));
    }
    text
}

/// The one line out of what the chat model wrote: its first real line, without a label, a
/// number or quotes around it.
pub fn one_line(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '•', '#', '>']).trim();
        let line = match line.split_once(':') {
            Some((label, said)) if label.len() <= 24 && !label.contains(',') => said,
            _ => line,
        };
        let line = line.trim_matches(|c: char| c.is_whitespace() || ['"', '*', '`', '\u{201c}', '\u{201d}'].contains(&c));
        if line.chars().count() >= 12 {
            return Some(one_row(line, MAX_LOOK));
        }
    }
    None
}

/// What the image model is told for a portrait.
pub fn portrait_prompt(member: &Member, film_look: &str) -> String {
    let look = member.look.trim().trim_end_matches('.');
    let mut text = format!(
        "Portrait of {}. Head and shoulders, facing the camera, plain background, soft even light.",
        if look.is_empty() { &member.name } else { look }
    );
    if !film_look.trim().is_empty() {
        text.push_str(&format!(" {}.", film_look.trim().trim_end_matches('.')));
    }
    text
}

/// The size of a portrait for a film of `size`: the film's shape, about 1024 on its long side,
/// each side a multiple of 16.
pub fn portrait_size(size: &str) -> (u32, u32) {
    let (w, h) = size.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))).unwrap_or((640.0, 384.0));
    let scale = 1024.0 / w.max(h).max(1.0);
    let side = |v: f32| (((v * scale / 16.0).round() as u32) * 16).clamp(256, 1024);
    (side(w), side(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str, look: &str) -> Member {
        Member { id: id.into(), name: name.into(), look: look.into(), voice: None, portrait: 0 }
    }

    #[test]
    fn a_shot_is_told_the_look_of_who_it_names_word_for_word() {
        let cast = [
            member("m1", "Mira", "a girl of eight in a yellow raincoat and red boots, short dark hair."),
            member("f1", "the Fox", "a small red fox with a white-tipped tail"),
            member("x1", "Ada", ""),
        ];
        let look = "soft watercolour, muted winter colours";
        assert_eq!(
            told(look, &cast, "Mira kneels by the stream and the fox watches from the trees"),
            "Mira kneels by the stream and the fox watches from the trees. \
             Mira: a girl of eight in a yellow raincoat and red boots, short dark hair. \
             the Fox: a small red fox with a white-tipped tail. soft watercolour, muted winter colours."
        );
        // The same words in every shot that names her, and none in a shot that does not.
        let other = told(look, &cast, "MIRA's boat turns in an eddy.");
        assert!(other.contains("Mira: a girl of eight in a yellow raincoat and red boots, short dark hair.") && !other.contains("red fox"));
        assert_eq!(told(look, &cast, "A wide view of the forest."), "A wide view of the forest. soft watercolour, muted winter colours.");
        // A name inside another word names nobody; a member without a look adds nothing.
        assert_eq!(told("", &cast, "The admiral salutes Ada."), "The admiral salutes Ada.");
        // A film without a cast and without a look is told the description as it is.
        assert_eq!(told("", &[], "a fox in snow"), "a fox in snow");
        assert_eq!(told(look, &cast, "  "), "");
        assert_eq!(named(&cast, "Ada meets Mira").iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["m1", "x1"]);
    }

    #[test]
    fn the_cast_is_bounded_and_names_are_its_own() {
        let old = [Member { portrait: 3, ..member("m1", "Mira", "old") }];
        let mut n = 0;
        let mut id = || {
            n += 1;
            format!("new{n}")
        };
        let new = vec![
            MemberIn { id: Some("m1".into()), name: " Mira \n".into(), look: " a girl\n of eight ".into(), voice: Some(4) },
            MemberIn { id: Some("m1".into()), name: "Fox".into(), look: String::new(), voice: Some(5000) },
        ];
        let out = merge_cast(&old, new, &mut id).unwrap();
        assert_eq!(out[0], Member { id: "m1".into(), name: "Mira".into(), look: "a girl of eight".into(), voice: Some(4), portrait: 3 });
        assert_eq!((out[1].id.as_str(), out[1].voice, out[1].portrait), ("new1", None, 0));
        let twice = vec![MemberIn { name: "Mira".into(), ..MemberIn::default() }, MemberIn { name: "mira".into(), ..MemberIn::default() }];
        assert!(merge_cast(&[], twice, &mut id).is_err());
        assert!(merge_cast(&[], vec![MemberIn::default()], &mut id).is_err());
        let many = (0..MAX_CAST + 1).map(|i| MemberIn { name: format!("n{i}"), ..MemberIn::default() }).collect();
        assert!(merge_cast(&[], many, &mut id).is_err());
    }

    #[test]
    fn a_look_and_a_portrait_are_asked_for_plainly() {
        assert_eq!(
            one_line("Sure!\n\n**Look:** \"a tall woman in her thirties with short black hair and a green raincoat\"").as_deref(),
            Some("a tall woman in her thirties with short black hair and a green raincoat")
        );
        assert_eq!(
            one_line("a tall woman, thirties: short black hair, green raincoat").as_deref(),
            Some("a tall woman, thirties: short black hair, green raincoat")
        );
        assert_eq!(one_line("OK."), None);
        let asked = look_request("Mira", "curious kid, raincoat", "watercolour");
        assert!(asked.contains("called Mira") && asked.contains("curious kid") && asked.contains("watercolour"));
        let mira = member("m1", "Mira", "a girl of eight in a yellow raincoat.");
        assert_eq!(
            portrait_prompt(&mira, "soft watercolour"),
            "Portrait of a girl of eight in a yellow raincoat. Head and shoulders, facing the camera, plain background, soft even light. soft watercolour."
        );
        assert!(portrait_prompt(&member("x", "Ada", ""), "").starts_with("Portrait of Ada."));
        assert_eq!(portrait_size("640x384"), (1024, 608));
        assert_eq!(portrait_size("384x640"), (608, 1024));
        assert_eq!(portrait_size("512x512"), (1024, 1024));
        assert_eq!(portrait_size("nonsense"), (1024, 608));
    }
}
