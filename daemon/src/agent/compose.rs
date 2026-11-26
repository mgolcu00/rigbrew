//! What the person sends besides plain words: files named with `@`, pictures, and messages
//! written while the task works.
//!
//! - **`@path`** in a message that names a file or folder in the task's folder brings its text
//!   (or, for a folder, its listing) with the message, capped. The step shows what was typed.
//! - **Pictures** (PNG, base64) are kept with the task like the pictures its tools take and go
//!   to the model as pictures when it has a vision part; otherwise the model is told one came.
//! - **A message sent while the task works** waits in `queued` and joins the conversation at the
//!   next turn: after the results of the step in progress, before the model is asked again. The
//!   person can take one back until then.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::files::{self, Folder};
use super::{look, Step, Task};

/// Pictures one message may carry.
pub const MAX_PICTURES: usize = 4;
/// Messages that may wait at once.
pub const MAX_QUEUED: usize = 8;
/// Files one message may bring with `@`, and how much of one.
const MAX_MENTIONS: usize = 6;
const MENTION_CHARS: usize = 4000;
/// Pictures of the person's that go to the model on one turn, the newest.
const PICTURES_SENT: usize = 4;

/// A message the person sent while the task was working.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Queued {
    pub id: String,
    /// What the person typed.
    pub text: String,
    /// What the model will be sent: the text with what it names and stands for.
    #[serde(default)]
    pub sent: String,
    /// The pictures it carries: names under the task's shots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// A skill the message's command named, read for the model when the message goes in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
}

/// The paths `@` names in `text` that are in the folder, in order, each once.
pub fn mentions(text: &str, folder: &Folder) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in text.match_indices('@') {
        // An address like me@example.com is no mention: the `@` starts a word.
        if text[..at].chars().next_back().is_some_and(|c| !c.is_whitespace() && !"([{\"'".contains(c)) {
            continue;
        }
        let rest = &text[at + 1..];
        let end = rest.find(|c: char| c.is_whitespace() || ",;:!?)]}\"'`".contains(c)).unwrap_or(rest.len());
        let path = rest[..end].trim_end_matches('.');
        if path.is_empty() || out.iter().any(|p| p == path) || folder.existing(path).is_err() {
            continue;
        }
        out.push(path.to_string());
        if out.len() == MAX_MENTIONS {
            break;
        }
    }
    out
}

/// The message as the model reads it: what was typed, then the text of each file it names with
/// `@` (a folder's listing), within `room` characters together: what the model's context has
/// for one message (see `super::room`).
pub fn with_mentions(text: &str, folder: &Folder, room: usize) -> String {
    let mut out = text.to_string();
    let mut left = room;
    for path in mentions(text, folder) {
        let Ok(real) = folder.existing(&path) else { continue };
        let body = if real.is_dir() {
            files::list(folder, &path, 2)
        } else {
            files::read_text(&real, &path, files::MAX_READ_BYTES).map(|t| files::cap(t.trim_end(), MENTION_CHARS.min(left)))
        };
        let Ok(body) = body else { continue };
        left = left.saturating_sub(body.chars().count());
        out.push_str(&format!("\n\n[{path}, which the person named]\n{body}"));
        if left < 200 {
            break;
        }
    }
    out
}

/// Checks and keeps the pictures of one message with the task, and gives their names.
pub fn keep_pictures(agent_dir: &std::path::Path, task: &str, pictures: &[String]) -> Result<Vec<String>, String> {
    use base64::Engine as _;
    if pictures.len() > MAX_PICTURES {
        return Err(format!("A message can carry {MAX_PICTURES} pictures at most."));
    }
    let mut names = Vec::new();
    for picture in pictures {
        let data = picture.split_once("base64,").map_or(picture.as_str(), |(_, d)| d);
        let png =
            base64::engine::general_purpose::STANDARD.decode(data.trim().as_bytes()).map_err(|_| "That picture could not be read.")?;
        if !png.starts_with(&[0x89, b'P', b'N', b'G']) || png.len() > look::MAX_PICTURE_BYTES {
            return Err("Pictures go as PNG files of at most 8 MB.".into());
        }
        names.push(look::keep_shot(agent_dir, task, &png).ok_or("The picture could not be kept.")?);
    }
    Ok(names)
}

/// One message of the person in the conversation. Its pictures are named here and put in when
/// the conversation is sent (`with_pictures`).
pub fn user_message(sent: &str, images: &[String]) -> Value {
    let mut message = json!({"role": "user", "content": sent});
    if !images.is_empty() {
        message["images"] = json!(images);
    }
    message
}

/// Adds the person's message to the task: the step that shows it, and the message the model reads.
pub fn add(task: &mut Task, shown: &str, sent: &str, images: &[String]) {
    task.steps.push(Step::User { text: shown.to_string() });
    if !images.is_empty() {
        task.steps.push(Step::Pictures { images: images.to_vec() });
    }
    task.messages.push(user_message(sent, images));
}

/// At a turn boundary: the messages that waited join the conversation. Gives the skills they
/// named, to be read for the model; `None` when nothing waited.
pub fn deliver(task: &mut Task) -> Option<Vec<String>> {
    if task.extra.queued.is_empty() {
        return None;
    }
    let mut skills = Vec::new();
    for q in std::mem::take(&mut task.extra.queued) {
        let sent = if q.sent.is_empty() { q.text.clone() } else { q.sent.clone() };
        add(task, &q.text, &sent, &q.images);
        skills.extend(q.skill);
    }
    Some(skills)
}

/// The conversation as the engine takes it. The newest pictures of the person go in as pictures
/// when the model sees; otherwise each is a line that says a picture came. Nothing else changes.
pub fn with_pictures(mut messages: Vec<Value>, sees: bool, read: impl Fn(&str) -> Option<Vec<u8>>) -> Vec<Value> {
    use base64::Engine as _;
    let mut budget = PICTURES_SENT;
    for message in messages.iter_mut().rev() {
        let Some(names) = message.as_object_mut().and_then(|m| m.remove("images")) else { continue };
        let names: Vec<String> = serde_json::from_value(names).unwrap_or_default();
        let text = message["content"].as_str().unwrap_or("").to_string();
        let mut parts = vec![json!({"type": "text", "text": text})];
        let mut left_out = 0;
        for name in &names {
            match read(name).filter(|_| sees && budget > 0) {
                Some(png) => {
                    budget -= 1;
                    let data = base64::engine::general_purpose::STANDARD.encode(png);
                    parts.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{data}")}}));
                }
                None => left_out += 1,
            }
        }
        if left_out > 0 {
            let why = if sees { "left out to save space" } else { "you cannot see pictures; ask what they show if it matters" };
            parts[0] = json!({"type": "text", "text": format!("{text}\n\n[{left_out} picture(s) the person sent: {why}]")});
        }
        message["content"] = if parts.len() == 1 { parts[0]["text"].clone() } else { json!(parts) };
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_named_with_at_come_with_the_message() {
        let root = files::scratch("mentions");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("README.md"), "# Hello\n").unwrap();
        let folder = Folder::open(root.to_str().unwrap()).unwrap();
        let text =
            "Why does @src/main.rs not print? See @README.md, and @src. Mail me@example.com, not @missing.txt or @src/main.rs again.";
        assert_eq!(mentions(text, &folder), ["src/main.rs", "README.md", "src"]);
        let sent = with_mentions(text, &folder, 12_000);
        assert!(sent.starts_with(text));
        assert!(
            sent.contains("\n\n[src/main.rs, which the person named]\nfn main() {}\n\n[README.md, which the person named]\n# Hello"),
            "{sent}"
        );
        assert!(sent.contains("[src, which the person named]\nsrc/\nmain.rs (13 B)"), "{sent}");
        assert_eq!(with_mentions("No files here.", &folder, 12_000), "No files here.");
        let small = with_mentions("@src/main.rs and @README.md", &folder, 20);
        assert!(small.contains("[src/main.rs, which the person named]\nfn main() {}") && !small.contains("[README.md"), "{small}");
        assert!(mentions("@../outside @/etc/passwd", &folder).is_empty());
    }

    #[test]
    fn pictures_go_as_pictures_to_a_model_that_sees() {
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
        let messages = vec![
            json!({"role": "user", "content": "old", "images": ["a"]}),
            json!({"role": "assistant", "content": "ok"}),
            user_message("What is wrong here?", &["b".into(), "c".into()]),
        ];
        let read = |name: &str| (name != "c").then(|| png.to_vec());
        let seen = with_pictures(messages.clone(), true, read);
        assert_eq!(seen[2]["content"][0]["text"], "What is wrong here?\n\n[1 picture(s) the person sent: left out to save space]");
        assert!(seen[2]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,iVBORw"));
        assert_eq!(seen[0]["content"][1]["type"], "image_url");
        assert!(seen.iter().all(|m| m.get("images").is_none()), "the names never reach the engine");
        let blind = with_pictures(messages, false, read);
        assert_eq!(
            blind[2]["content"],
            "What is wrong here?\n\n[2 picture(s) the person sent: you cannot see pictures; ask what they show if it matters]"
        );
        assert_eq!(blind[1]["content"], "ok");
    }

    #[test]
    fn pictures_are_checked_before_they_are_kept() {
        use base64::Engine as _;
        let dir = files::scratch("pictures");
        let task = "0".repeat(32);
        let png = base64::engine::general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G', 0, 0]);
        let names = keep_pictures(&dir, &task, &[format!("data:image/png;base64,{png}"), png.clone()]).unwrap();
        assert_eq!(names.len(), 2);
        assert!(look::read_shot(&dir, &task, &names[0]).is_some());
        assert!(keep_pictures(&dir, &task, &[base64::engine::general_purpose::STANDARD.encode(b"GIF89a")]).is_err());
        assert!(keep_pictures(&dir, &task, &["not base64!".into()]).is_err());
        assert!(keep_pictures(&dir, &task, &vec![png; MAX_PICTURES + 1]).is_err());
    }
}
