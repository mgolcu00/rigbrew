//! Skills: a folder with a `SKILL.md` that starts with YAML front matter (`name`, `description`)
//! and goes on with instructions. Only the name and the description go into the system prompt;
//! the model reads the instructions with `use_skill` when it needs them.
//!
//! A skill folder may hold other text files its instructions point to (`reference/x.md`, a
//! template). `use_skill` lists them and `read_skill_file` reads one; nothing outside the
//! skill's own folder can be read that way.
//!
//! The person's own skills live in `<data dir>/skills/<name>/`. Which skills are switched off
//! is kept in `<data dir>/skills.json`. The API for the app's Skills sheet is in `api`, and
//! bringing a skill in from a folder or from GitHub is in `import`.

pub mod api;
mod import;

use std::path::{Path, PathBuf};

use serde_json::json;

/// Longest skill text handed to the model.
const MAX_BODY_CHARS: usize = 6000;
pub const MAX_DESCRIPTION_CHARS: usize = 300;
/// Longest instructions the app's form saves.
pub const MAX_SAVED_BODY_CHARS: usize = 20_000;
/// Other files of one skill that are listed, and how deep they are looked for.
const MAX_FILES: usize = 40;
const MAX_FILE_DEPTH: usize = 3;
/// Endings of the files a skill may carry: text a model can read.
pub const TEXT_ENDINGS: &[&str] = &[
    "md", "markdown", "txt", "json", "yaml", "yml", "toml", "csv", "tsv", "html", "css", "xml", "svg", "py", "js", "mjs", "ts", "sh", "rs",
    "go", "rb", "java", "kt", "swift", "c", "h", "cpp", "sql", "tmpl", "template", "ini", "cfg",
];

/// Where a skill is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// In Code tasks and in the chat.
    Any,
    /// Only in Code: it needs a folder to work on.
    Code,
    /// Only in the chat.
    Chat,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::Any => "any",
            Scope::Code => "code",
            Scope::Chat => "chat",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// "built-in", "yours" (the data folder) or "folder" (the project's `.rigbrew/skills`).
    pub source: &'static str,
    /// The folder it was read from; none for a built-in skill.
    pub dir: Option<PathBuf>,
    /// From `metadata: where:` in the front matter: `code`, `chat`, or anywhere when left out.
    pub scope: Scope,
    /// False when the person switched it off.
    pub enabled: bool,
}

/// The skills that come with Rigbrew.
const BUILT_IN: &[(&str, &str)] = &[
    ("explain-codebase", include_str!("../../skills/explain-codebase/SKILL.md")),
    ("fix-failing-test", include_str!("../../skills/fix-failing-test/SKILL.md")),
    ("write-readme", include_str!("../../skills/write-readme/SKILL.md")),
    ("review-a-change", include_str!("../../skills/review-a-change/SKILL.md")),
    ("write-tests", include_str!("../../skills/write-tests/SKILL.md")),
    ("refactor-safely", include_str!("../../skills/refactor-safely/SKILL.md")),
    ("debug-an-error", include_str!("../../skills/debug-an-error/SKILL.md")),
    ("summarise-a-document", include_str!("../../skills/summarise-a-document/SKILL.md")),
    ("write-an-email", include_str!("../../skills/write-an-email/SKILL.md")),
    ("plan-a-project", include_str!("../../skills/plan-a-project/SKILL.md")),
    ("research-a-topic", include_str!("../../skills/research-a-topic/SKILL.md")),
    ("storyboard-a-film", include_str!("../../skills/storyboard-a-film/SKILL.md")),
];

/// Whether a skill with this name comes with Rigbrew.
pub fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(n, _)| *n == name)
}

/// The text of a built-in skill's file.
fn built_in_text(name: &str) -> Option<&'static str> {
    BUILT_IN.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

fn unquote(value: &str) -> &str {
    let v = value.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Reads one `SKILL.md`. `folder_name` is used when the front matter has no name. Returns
/// nothing when there is no front matter, no description, or the name is not `a-z`, `0-9`, `-`.
pub fn parse(text: &str, folder_name: &str, source: &'static str) -> Option<Skill> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = text.strip_prefix("---\n")?;
    let (front, body) = match rest.split_once("\n---\n") {
        Some(parts) => parts,
        None => (rest.strip_suffix("\n---")?, ""),
    };
    let mut name = String::new();
    let mut description = String::new();
    let mut scope = Scope::Any;
    // The key a continuation line (indented, under `key: >` or `key: |`) belongs to.
    let mut open: Option<&str> = None;
    for line in front.lines() {
        if line.starts_with([' ', '\t']) {
            if open == Some("description") && !line.trim().is_empty() {
                if !description.is_empty() {
                    description.push(' ');
                }
                description.push_str(line.trim());
            }
            if open == Some("metadata") {
                if let Some(("where", value)) = line.split_once(':').map(|(k, v)| (k.trim(), unquote(v))) {
                    scope = match value {
                        "code" => Scope::Code,
                        "chat" => Scope::Chat,
                        _ => Scope::Any,
                    };
                }
            }
            continue;
        }
        open = None;
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        let block = matches!(value, ">" | "|" | ">-" | "|-" | "");
        match key.trim() {
            "name" => name = unquote(value).to_string(),
            "description" => {
                if block {
                    open = Some("description");
                } else {
                    description = unquote(value).to_string();
                }
            }
            "metadata" if block => open = Some("metadata"),
            _ => {}
        }
    }
    if name.is_empty() {
        name = folder_name.to_string();
    }
    if !valid_name(&name) || description.is_empty() {
        return None;
    }
    let description: String = description.chars().take(MAX_DESCRIPTION_CHARS).collect();
    Some(Skill { name, description, body: super::files::cap(body.trim(), MAX_BODY_CHARS), source, dir: None, scope, enabled: true })
}

/// The front matter of a skill file, without the lines around it.
fn front_of(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    rest.split_once("\n---\n").map(|(front, _)| front).or_else(|| rest.strip_suffix("\n---"))
}

/// The lines of a skill file's front matter that are neither the name nor the description
/// (a licence, metadata): kept as they are when the app saves the skill again.
fn other_front_lines(text: &str) -> Vec<String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut out = Vec::new();
    let mut ours = false;
    for line in front_of(&text).unwrap_or("").lines() {
        if !line.starts_with([' ', '\t']) {
            ours = matches!(line.split_once(':').map(|(k, _)| k.trim()), Some("name" | "description"));
        }
        if !ours && !line.trim().is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// What the front matter says the licence is, when it says so.
pub fn licence_of(text: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    front_of(&text)?
        .lines()
        .filter(|l| !l.starts_with([' ', '\t']))
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| matches!(k.trim(), "license" | "licence"))
        .map(|(_, v)| unquote(v).chars().filter(|c| !c.is_control()).take(120).collect::<String>())
        .filter(|v| !v.is_empty())
}

/// The whole instructions of a skill file, not capped: what the app's form edits.
pub fn full_body(text: &str) -> String {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")).map(|(_, body)| body.trim().to_string()).unwrap_or_default()
}

/// A skill file: the name, the description on one line, the lines to keep, the instructions.
pub fn compose(name: &str, description: &str, keep: &[String], body: &str) -> String {
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text = format!("---\nname: {name}\ndescription: >-\n  {description}\n");
    for line in keep {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("---\n\n");
    text.push_str(body.trim());
    text.push('\n');
    text
}

/// The names of the skills the person switched off.
pub fn off(data_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(data_dir.join("skills.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["off"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()))
        .unwrap_or_default()
}

/// Switches one skill on or off.
pub fn set_enabled(data_dir: &Path, name: &str, enabled: bool) -> Result<(), String> {
    let mut list = off(data_dir);
    list.retain(|n| n != name);
    if !enabled {
        list.push(name.to_string());
    }
    list.sort();
    let tmp = data_dir.join(".skills.json.tmp");
    std::fs::write(&tmp, json!({"v": 1, "off": list}).to_string())
        .and_then(|()| std::fs::rename(&tmp, data_dir.join("skills.json")))
        .map_err(|e| format!("skills.json cannot be written: {e}"))
}

/// The folder the person's own skills are in.
pub fn own_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("skills")
}

/// Whether a file's name ends like text a skill may carry.
pub fn text_ending(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ending)) => !stem.is_empty() && TEXT_ENDINGS.contains(&ending.to_ascii_lowercase().as_str()),
        // A licence file has no ending.
        None => matches!(name.to_ascii_uppercase().as_str(), "LICENSE" | "LICENCE" | "NOTICE" | "COPYING"),
    }
}

/// Whether a path names a file inside a skill folder and nothing else: relative, forward
/// slashes, no `..`, no hidden parts.
pub fn inside_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(|c| c.is_control())
        && path.split('/').all(|part| !part.is_empty() && !part.starts_with('.'))
}

/// The other text files of a skill folder: path relative to it and size, sorted.
pub fn other_files(dir: &Path) -> Vec<(String, u64)> {
    fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, u64)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if out.len() >= MAX_FILES {
                return;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            // Links are not followed: a skill is what is in its folder.
            if kind.is_symlink() {
                continue;
            }
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if !inside_path(&rel) {
                continue;
            }
            if kind.is_dir() {
                if depth < MAX_FILE_DEPTH {
                    walk(base, &path, depth + 1, out);
                }
            } else if rel != "SKILL.md" && text_ending(&rel) {
                out.push((rel, entry.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 1, &mut out);
    out.sort();
    out
}

/// What the model gets from `use_skill`: the instructions, and the names of the skill's other files.
pub fn render(skill: &Skill) -> String {
    let mut text = format!("Skill {}:\n{}", skill.name, skill.body);
    let files = skill.dir.as_deref().map(other_files).unwrap_or_default();
    if !files.is_empty() {
        text.push_str("\n\nOther files of this skill. Read one with read_skill_file when the instructions point to it:\n");
        for (path, size) in files {
            text.push_str(&format!("- {path} ({size} bytes)\n"));
        }
    }
    text
}

/// One of a skill's other files, for `read_skill_file`. Only text, only from inside the skill's
/// own folder, and no longer than a tool result.
pub fn read_file(skill: &Skill, path: &str) -> Result<String, String> {
    const NOT_OURS: &str = "That is not a file of this skill. Use a path from the list use_skill gave.";
    let path = path.trim();
    let path = path.strip_prefix("./").unwrap_or(path);
    let Some(dir) = skill.dir.as_deref() else { return Err(format!("The skill {} has no other files.", skill.name)) };
    if !inside_path(path) || !text_ending(path) {
        return Err(NOT_OURS.into());
    }
    // Links are followed here, so the place the path really leads to is what is checked.
    let (Ok(root), Ok(file)) = (std::fs::canonicalize(dir), std::fs::canonicalize(dir.join(path))) else {
        return Err(format!("The skill {} has no file {path}.", skill.name));
    };
    if !file.starts_with(&root) || !file.is_file() {
        return Err(NOT_OURS.into());
    }
    if std::fs::metadata(&file).map(|m| m.len() > 256 * 1024).unwrap_or(true) {
        return Err("That file is too large to read.".into());
    }
    let bytes = std::fs::read(&file).map_err(|_| format!("The skill {} has no file {path}.", skill.name))?;
    if bytes.contains(&0) {
        return Err("That file is not text.".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "That file is not text.".to_string())?;
    Ok(super::files::cap(text.trim(), super::files::RESULT_CHARS))
}

/// What `use_skill` answers, and a record that the skill was read: its name when it comes with
/// Rigbrew, "custom" otherwise.
pub fn used(app: &crate::App, skill: &Skill, from: &str) -> String {
    let name = if skill.source == "built-in" { skill.name.as_str() } else { "custom" };
    app.record("skill_used", json!({"skill": name, "from": from}));
    render(skill)
}

fn load_dir(dir: &Path, source: &'static str, into: &mut Vec<Skill>) {
    let mut names: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    names.sort();
    for path in names.into_iter().take(50) {
        let file = path.join("SKILL.md");
        // A skill file is a page of text; anything larger is not read.
        if std::fs::metadata(&file).map(|m| m.len() > 256 * 1024).unwrap_or(true) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let folder_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(mut skill) = parse(&text, &folder_name, source) {
            skill.dir = Some(path);
            // A later place wins: the project's skill replaces yours, yours replaces the built-in one.
            into.retain(|s| s.name != skill.name);
            into.push(skill);
        }
    }
}

/// The person's own skills, as they are in `<data dir>/skills`.
fn own(data_dir: &Path) -> Vec<Skill> {
    let mut list = Vec::new();
    load_dir(&own_dir(data_dir), "yours", &mut list);
    list
}

/// Every skill there is: built in, then `<data dir>/skills`, then `<folder>/.rigbrew/skills`,
/// each marked with whether it is switched on.
pub fn load_all(data_dir: &Path, folder: Option<&Path>) -> Vec<Skill> {
    let mut out: Vec<Skill> = BUILT_IN.iter().filter_map(|(name, text)| parse(text, name, "built-in")).collect();
    load_dir(&own_dir(data_dir), "yours", &mut out);
    if let Some(folder) = folder {
        load_dir(&folder.join(".rigbrew").join("skills"), "folder", &mut out);
    }
    let off = off(data_dir);
    for skill in &mut out {
        skill.enabled = !off.contains(&skill.name);
    }
    out
}

/// The skills a Code task in `folder` can use: the ones that are switched on and not only for
/// the chat.
pub fn load(data_dir: &Path, folder: Option<&Path>) -> Vec<Skill> {
    load_all(data_dir, folder).into_iter().filter(|s| s.enabled && s.scope != Scope::Chat).collect()
}

/// The tool that reads one of a skill's other files, offered next to `use_skill`.
fn file_tool() -> serde_json::Value {
    let text = |d: &str| json!({"type": "string", "description": d});
    super::tool(
        "read_skill_file",
        "Read one of the other files of a skill, by the path use_skill listed.",
        json!({"name": text("Name of the skill"), "path": text("Path of the file inside the skill")}),
        &["name", "path"],
    )
}

/// `read_skill_file`, offered only when one of the skills has other files to read.
pub fn extra_tools(skills: &[Skill]) -> Vec<serde_json::Value> {
    if skills.iter().any(|s| s.dir.as_deref().is_some_and(|d| !other_files(d).is_empty())) {
        vec![file_tool()]
    } else {
        Vec::new()
    }
}

/// Runs `read_skill_file`; nothing for any other tool.
pub fn dispatch(app: &crate::App, folder: &Path, name: &str, args: &serde_json::Value) -> Option<(String, &'static str)> {
    if name != "read_skill_file" {
        return None;
    }
    let wanted = args["name"].as_str().unwrap_or("").trim();
    let Some(skill) = load(&app.root, Some(folder)).into_iter().find(|s| s.name == wanted) else {
        return Some((format!("There is no skill named {wanted}."), "failed"));
    };
    Some(match read_file(&skill, args["path"].as_str().unwrap_or("")) {
        Ok(text) => (text, "done"),
        Err(why) => (why, "failed"),
    })
}

/// Saves one of the person's own skills as `<data dir>/skills/<name>/SKILL.md`. `was` is the
/// name it had when it is being changed: its folder, its other files and the other lines of its
/// front matter are kept. A built-in skill that is changed becomes the person's own copy.
pub fn save(data_dir: &Path, was: Option<&str>, name: &str, description: &str, body: &str) -> Result<(), String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("A skill name is 1 to 64 lowercase letters, digits or dashes, like tidy-imports.".into());
    }
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if description.is_empty() {
        return Err("Say when the skill should be used.".into());
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!("\"When to use it\" can be {MAX_DESCRIPTION_CHARS} characters at most."));
    }
    if body.trim().is_empty() {
        return Err("Write the instructions.".into());
    }
    if body.chars().count() > MAX_SAVED_BODY_CHARS {
        return Err(format!("The instructions can be {MAX_SAVED_BODY_CHARS} characters at most."));
    }
    let mine = own(data_dir);
    let old = was.and_then(|w| mine.iter().find(|s| s.name == w)).and_then(|s| s.dir.clone());
    if (old.is_none() || was != Some(name)) && mine.iter().any(|s| s.name == name) {
        return Err(format!("You already have a skill named {name}."));
    }
    // The lines to keep: from the person's own file, or from the built-in skill being copied.
    let keep = match &old {
        Some(dir) => std::fs::read_to_string(dir.join("SKILL.md")).ok().map(|t| other_front_lines(&t)),
        None => was.and_then(built_in_text).map(other_front_lines),
    }
    .unwrap_or_default();
    let dir = match old {
        Some(dir) => dir,
        None => {
            let dir = own_dir(data_dir).join(name);
            if dir.exists() {
                return Err(format!("There is already a folder named {name} in your skills folder."));
            }
            dir
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("The skill could not be saved: {e}"))?;
    std::fs::write(dir.join("SKILL.md"), compose(name, &description, &keep, body))
        .map_err(|e| format!("The skill could not be saved: {e}"))?;
    // A switch that was set for the old name follows the skill.
    if let Some(was) = was.filter(|w| *w != name && !is_built_in(w)) {
        if off(data_dir).iter().any(|n| n == was) {
            set_enabled(data_dir, was, true).ok();
            set_enabled(data_dir, name, false).ok();
        }
    }
    Ok(())
}

/// Copies the other files of one skill folder into another (for Duplicate).
pub fn copy_files(from: &Path, to: &Path) {
    for (path, _) in other_files(from) {
        let target = to.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(from.join(&path), target).ok();
    }
}

/// Deletes one of the person's own skills with its folder. False when there is none by that name.
pub fn delete(data_dir: &Path, name: &str) -> Result<bool, String> {
    let Some(dir) = own(data_dir).into_iter().find(|s| s.name == name).and_then(|s| s.dir) else { return Ok(false) };
    // Only ever a folder directly inside the skills folder.
    if dir.parent() != Some(own_dir(data_dir).as_path()) {
        return Ok(false);
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("The skill could not be deleted: {e}"))?;
    if !is_built_in(name) {
        set_enabled(data_dir, name, true).ok();
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_gives_name_and_description() {
        let s = parse(
            "---\nname: tidy-imports\ndescription: \"Sort the imports: one group per kind.\"\nlicense: MIT\n---\n\n# Steps\n\nDo it.\n",
            "x",
            "yours",
        )
        .unwrap();
        assert_eq!(s.name, "tidy-imports");
        assert_eq!(s.description, "Sort the imports: one group per kind.");
        assert_eq!(s.body, "# Steps\n\nDo it.");
        assert_eq!(s.source, "yours");
    }

    #[test]
    fn folded_descriptions_and_windows_line_ends_are_read() {
        let s = parse("---\r\nname: a-b\r\ndescription: >\r\n  First part\r\n  second part.\r\n---\r\nBody\r\n", "x", "folder").unwrap();
        assert_eq!(s.description, "First part second part.");
        assert_eq!(s.body, "Body");
    }

    #[test]
    fn the_folder_names_a_skill_without_a_name() {
        assert_eq!(parse("---\ndescription: Does a thing.\n---\nBody", "from-folder", "yours").unwrap().name, "from-folder");
    }

    #[test]
    fn broken_skills_are_left_out() {
        assert!(parse("# No front matter\n", "x", "yours").is_none());
        assert!(parse("---\nname: x\n---\nBody", "x", "yours").is_none(), "no description");
        assert!(parse("---\nname: Not Valid\ndescription: d\n---\n", "x", "yours").is_none());
        assert!(parse("---\nname: x\ndescription: d\n", "x", "yours").is_none(), "front matter never ends");
    }

    #[test]
    fn long_bodies_are_capped() {
        let text = format!("---\nname: long\ndescription: d\n---\n{}", "word ".repeat(5000));
        let s = parse(&text, "x", "yours").unwrap();
        assert!(s.body.contains("[truncated"));
        assert!(s.body.chars().count() < MAX_BODY_CHARS + 100);
    }

    #[test]
    fn the_twelve_built_in_skills_parse_and_are_short() {
        let all = load_all(Path::new("/nonexistent-rigbrew-data"), None);
        assert_eq!(all.len(), 12);
        for (skill, (name, text)) in all.iter().zip(BUILT_IN) {
            assert_eq!(skill.name, *name);
            assert!(skill.source == "built-in" && skill.enabled && skill.dir.is_none());
            assert!(skill.description.len() > 40 && skill.description.len() <= 240, "{name}: one line that says when to use it");
            assert!(text.chars().count() < 3500, "{name} is too long for a small model");
            assert!(skill.body.len() > 400 && !skill.body.contains("[truncated"), "{name}");
            assert!(!text.contains('\u{2014}'), "{name}: no em dashes");
        }
        let chat: Vec<&str> = all.iter().filter(|s| s.scope != Scope::Code).map(|s| s.name.as_str()).collect();
        assert_eq!(
            chat,
            ["debug-an-error", "summarise-a-document", "write-an-email", "plan-a-project", "research-a-topic", "storyboard-a-film"]
        );
        let code = load(Path::new("/nonexistent-rigbrew-data"), None);
        assert!(code.iter().all(|s| s.scope != Scope::Chat) && code.iter().any(|s| s.name == "fix-failing-test"));
        assert!(!code.iter().any(|s| s.name == "storyboard-a-film"), "a film is not planned in a code folder");
    }

    #[test]
    fn metadata_says_where_a_skill_is_offered() {
        let at = |meta: &str| parse(&format!("---\nname: x\ndescription: d\n{meta}---\nBody"), "x", "yours").unwrap().scope;
        assert_eq!(at(""), Scope::Any);
        assert_eq!(at("metadata:\n  where: code\n"), Scope::Code);
        assert_eq!(at("metadata:\n  author: me\n  where: \"chat\"\n"), Scope::Chat);
        assert_eq!(at("metadata:\n  where: moon\n"), Scope::Any);
        assert_eq!(at("where: code\n"), Scope::Any, "only under metadata");
    }

    #[test]
    fn the_licence_is_read_from_the_front_matter() {
        assert_eq!(licence_of("---\nname: x\ndescription: d\nlicense: \"MIT\"\n---\nBody").as_deref(), Some("MIT"));
        assert_eq!(licence_of("---\nname: x\nlicence: Apache-2.0\ndescription: d\n---\n").as_deref(), Some("Apache-2.0"));
        assert_eq!(licence_of("---\nname: x\ndescription: d\n---\nlicense: no"), None);
        assert_eq!(licence_of("no front matter"), None);
    }

    #[test]
    fn a_project_skill_replaces_one_with_the_same_name() {
        let base = super::super::files::scratch("skills");
        let (data, project) = (base.join("data"), base.join("project"));
        std::fs::create_dir_all(data.join("skills/mine")).unwrap();
        std::fs::create_dir_all(project.join(".rigbrew/skills/write-readme")).unwrap();
        std::fs::write(data.join("skills/mine/SKILL.md"), "---\nname: mine\ndescription: My own.\n---\nMine.").unwrap();
        std::fs::write(project.join(".rigbrew/skills/write-readme/SKILL.md"), "---\nname: write-readme\ndescription: Ours.\n---\nOurs.")
            .unwrap();
        let all = load_all(&data, Some(&project));
        assert_eq!(all.len(), 13);
        assert_eq!(all.iter().find(|s| s.name == "mine").unwrap().source, "yours");
        let readme = all.iter().find(|s| s.name == "write-readme").unwrap();
        assert_eq!((readme.source, readme.body.as_str()), ("folder", "Ours."));
    }

    #[test]
    fn a_skill_is_saved_changed_switched_off_and_deleted() {
        let data = super::super::files::scratch("skills-own");
        save(&data, None, "tidy-imports", "Sort the imports.\nUse when asked: to tidy.", "# Steps\n\nDo it.").unwrap();
        let file = data.join("skills/tidy-imports/SKILL.md");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("---\nname: tidy-imports\ndescription: >-\n  Sort the imports. Use when asked: to tidy.\n---\n\n# Steps"));
        let all = load_all(&data, None);
        let mine = all.iter().find(|s| s.name == "tidy-imports").unwrap();
        assert_eq!(
            (mine.source, mine.description.as_str(), mine.body.as_str()),
            ("yours", "Sort the imports. Use when asked: to tidy.", "# Steps\n\nDo it.")
        );

        // What is wrong is said, and nothing is written.
        assert!(save(&data, None, "Tidy Imports", "d", "b").is_err());
        assert!(save(&data, None, "x", "  ", "b").is_err());
        assert!(save(&data, None, "x", "d", " \n").is_err());
        assert!(save(&data, None, "x", &"d".repeat(MAX_DESCRIPTION_CHARS + 1), "b").is_err());
        assert!(save(&data, None, "tidy-imports", "d", "b").is_err(), "the name is taken");
        assert!(!data.join("skills/x").exists());

        // Changing it keeps its folder, its other files and the other lines of the front matter.
        std::fs::write(&file, text.replace("---\n\n# Steps", "license: MIT\nmetadata:\n  where: code\n---\n\n# Steps")).unwrap();
        std::fs::create_dir_all(data.join("skills/tidy-imports/reference")).unwrap();
        std::fs::write(data.join("skills/tidy-imports/reference/order.md"), "std first").unwrap();
        save(&data, Some("tidy-imports"), "sort-imports", "Sort them.", "New body.").unwrap();
        let changed = std::fs::read_to_string(&file).unwrap();
        assert!(changed.contains("name: sort-imports") && changed.contains("license: MIT") && changed.contains("  where: code"));
        assert_eq!(full_body(&changed), "New body.");
        let all = load_all(&data, None);
        let mine = all.iter().find(|s| s.name == "sort-imports").unwrap();
        assert_eq!(mine.scope, Scope::Code);
        assert!(render(mine).contains("- reference/order.md (9 bytes)"));
        assert!(!all.iter().any(|s| s.name == "tidy-imports"));

        // Off and on again; a skill that is off is not offered.
        set_enabled(&data, "sort-imports", false).unwrap();
        set_enabled(&data, "write-readme", false).unwrap();
        assert_eq!(off(&data), ["sort-imports", "write-readme"]);
        assert!(!load(&data, None).iter().any(|s| s.name == "sort-imports" || s.name == "write-readme"));
        assert!(load_all(&data, None).iter().any(|s| s.name == "sort-imports" && !s.enabled));
        set_enabled(&data, "write-readme", true).unwrap();
        assert!(load(&data, None).iter().any(|s| s.name == "write-readme"));

        // Your own version of a built-in skill replaces it and keeps where it is offered;
        // deleting yours brings the built-in one back.
        save(&data, Some("write-readme"), "write-readme", "Mine.", "My way.").unwrap();
        let mine = load(&data, None).into_iter().find(|s| s.name == "write-readme").unwrap();
        assert_eq!((mine.source, mine.scope), ("yours", Scope::Code));
        assert!(delete(&data, "write-readme").unwrap());
        assert_eq!(load(&data, None).iter().find(|s| s.name == "write-readme").unwrap().source, "built-in");
        assert!(delete(&data, "sort-imports").unwrap());
        assert!(!delete(&data, "sort-imports").unwrap());
        assert!(!delete(&data, "explain-codebase").unwrap(), "a built-in skill cannot be deleted");
        assert!(off(&data).is_empty());
    }

    #[test]
    fn only_text_inside_the_skill_folder_is_read() {
        let base = super::super::files::scratch("skills-files");
        let dir = base.join("data/skills/report");
        std::fs::create_dir_all(dir.join("reference")).unwrap();
        std::fs::write(dir.join("SKILL.md"), "---\nname: report\ndescription: Write a report.\n---\nSee reference/outline.md.").unwrap();
        std::fs::write(dir.join("reference/outline.md"), "1. Summary\n2. Detail\n").unwrap();
        std::fs::write(dir.join("template.txt"), "x".repeat(9000)).unwrap();
        std::fs::write(dir.join("logo.png"), b"\x89PNG\0\0").unwrap();
        std::fs::write(dir.join("fake.md"), b"text\0with a nul").unwrap();
        std::fs::write(dir.join(".hidden.md"), "no").unwrap();
        std::fs::write(base.join("secret.txt"), "the token").unwrap();
        let all = load_all(&base.join("data"), None);
        let skill = all.iter().find(|s| s.name == "report").unwrap();

        let listed: Vec<String> = other_files(&dir).into_iter().map(|(p, _)| p).collect();
        assert_eq!(listed, ["fake.md", "reference/outline.md", "template.txt"], "no pictures, no hidden files, not SKILL.md");
        let shown = render(skill);
        assert!(shown.starts_with("Skill report:\nSee reference/outline.md.") && shown.contains("- reference/outline.md (21 bytes)"));

        assert_eq!(read_file(skill, "reference/outline.md").unwrap(), "1. Summary\n2. Detail");
        assert_eq!(read_file(skill, "./reference/outline.md").unwrap(), "1. Summary\n2. Detail");
        assert!(read_file(skill, "template.txt").unwrap().contains("[truncated"), "long files are capped");
        let escapes = [
            "../../../secret.txt",
            "reference/../../../../secret.txt",
            "/etc/passwd",
            "..",
            "",
            "reference\\outline.md",
            "C:/x.md",
            ".hidden.md",
        ];
        for outside in escapes {
            assert!(read_file(skill, outside).is_err(), "{outside}");
        }
        assert!(read_file(skill, "logo.png").is_err(), "not text by its name");
        assert!(read_file(skill, "fake.md").is_err(), "not text by its content");
        assert!(read_file(skill, "missing.md").is_err());
        // A link that leads out of the folder is not followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("secret.txt"), dir.join("link.txt")).unwrap();
            assert!(read_file(skill, "link.txt").is_err());
            assert!(!other_files(&dir).iter().any(|(p, _)| p == "link.txt"));
        }
        // A built-in skill has no folder to read from.
        let built_in = all.iter().find(|s| s.name == "write-readme").unwrap();
        assert!(read_file(built_in, "reference/outline.md").is_err());
        assert!(!render(built_in).contains("Other files"));
    }

    #[test]
    fn paths_and_endings() {
        assert!(inside_path("a.md") && inside_path("reference/a.md"));
        let hidden = format!(".{}/config", "git");
        for bad in ["", "/a.md", "../a.md", "a/../b.md", "a//b.md", hidden.as_str(), "a/.env", "a\\b.md", "c:a.md", "a\nb.md"] {
            assert!(!inside_path(bad), "{bad}");
        }
        assert!(text_ending("notes.MD") && text_ending("x/y/template.txt") && text_ending("LICENSE") && text_ending("data.json"));
        assert!(
            !text_ending("logo.png") && !text_ending("archive.zip") && !text_ending("run") && !text_ending(".md") && !text_ending("a.exe")
        );
    }
}
