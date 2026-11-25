//! Skills: a folder with a `SKILL.md` that starts with YAML front matter (`name`, `description`)
//! and goes on with instructions. Only the name and the description go into the system prompt;
//! the model reads the instructions with `use_skill` when it needs them.

use std::path::Path;

/// Longest skill text handed to the model.
const MAX_BODY_CHARS: usize = 6000;
const MAX_DESCRIPTION_CHARS: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    /// "built-in", "yours" (the data folder) or "folder" (the project's `.rigbrew/skills`).
    pub source: &'static str,
}

/// The skills that come with Rigbrew.
const BUILT_IN: &[(&str, &str)] = &[
    ("explain-codebase", include_str!("../../skills/explain-codebase/SKILL.md")),
    ("fix-failing-test", include_str!("../../skills/fix-failing-test/SKILL.md")),
    ("write-readme", include_str!("../../skills/write-readme/SKILL.md")),
];

fn unquote(value: &str) -> &str {
    let v = value.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

fn valid_name(name: &str) -> bool {
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
    Some(Skill { name, description, body: super::files::cap(body.trim(), MAX_BODY_CHARS), source })
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
        if let Some(skill) = parse(&text, &folder_name, source) {
            // A later place wins: the project's skill replaces yours, yours replaces the built-in one.
            into.retain(|s| s.name != skill.name);
            into.push(skill);
        }
    }
}

/// Every skill a task in `folder` can use: built in, then `<data dir>/skills`, then
/// `<folder>/.rigbrew/skills`.
pub fn load(data_dir: &Path, folder: Option<&Path>) -> Vec<Skill> {
    let mut out: Vec<Skill> = BUILT_IN.iter().filter_map(|(name, text)| parse(text, name, "built-in")).collect();
    load_dir(&data_dir.join("skills"), "yours", &mut out);
    if let Some(folder) = folder {
        load_dir(&folder.join(".rigbrew").join("skills"), "folder", &mut out);
    }
    out
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
    fn the_three_built_in_skills_parse() {
        let all = load(Path::new("/nonexistent-rigbrew-data"), None);
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["explain-codebase", "fix-failing-test", "write-readme"]);
        assert!(all.iter().all(|s| s.source == "built-in" && !s.body.is_empty() && s.description.len() > 20));
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
        let all = load(&data, Some(&project));
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().find(|s| s.name == "mine").unwrap().source, "yours");
        let readme = all.iter().find(|s| s.name == "write-readme").unwrap();
        assert_eq!((readme.source, readme.body.as_str()), ("folder", "Ours."));
    }
}
