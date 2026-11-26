//! What a project says about itself to whoever works in it, and the short commands the person
//! types for common requests.
//!
//! **Instructions.** `AGENTS.md`, `RIGBREW.md` or `CLAUDE.md` in the task's folder, and in each
//! folder above it up to the top of its git repository, go into the system message, the outer
//! ones first so the nearest has the last word. Each name is read once per folder, the whole is
//! cut at `MAX_CHARS`, and the nearest file keeps its room first.
//!
//! **Commands.** A request that starts with `/init`, `/review`, `/test`, `/explain` or
//! `/<skill>` is sent to the model as the request it stands for (`expand`); the step keeps what
//! the person typed. `/clear`, `/model` and `/mode` never reach the service: the app does them.

use std::path::{Path, PathBuf};

use super::files;
use super::skills::Skill;

/// The names a project's instructions go by, in the order they are read in one folder.
pub const NAMES: &[&str] = &["AGENTS.md", "RIGBREW.md", "CLAUDE.md"];
/// Characters of instructions in the system message, all files together. The model's context is
/// small, and the instructions are sent on every turn.
const MAX_CHARS: usize = 3000;

/// The folders whose instructions count, the outermost first: those from the top of the git
/// repository down to `root`, or `root` alone outside a repository.
fn folders(root: &Path) -> Vec<PathBuf> {
    let mut up: Vec<PathBuf> = Vec::new();
    for dir in root.ancestors().take(12) {
        up.push(dir.to_path_buf());
        if dir.join(".git").exists() {
            up.reverse();
            return up;
        }
    }
    vec![root.to_path_buf()]
}

/// The instruction files that count for `root`, each as (the path as the person sees it from the
/// folder, its text), the outermost first.
pub fn found(root: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for dir in folders(root) {
        let up = root.strip_prefix(&dir).map(|rest| rest.components().count()).unwrap_or(0);
        for name in NAMES {
            let path = dir.join(name);
            // A page of text: anything larger is not instructions.
            if std::fs::metadata(&path).map(|m| !m.is_file() || m.len() > 256 * 1024).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let text = text.trim().to_string();
            // The same text under two names (a link, a copy) counts once.
            if text.is_empty() || out.iter().any(|(_, t)| *t == text) {
                continue;
            }
            out.push((format!("{}{name}", "../".repeat(up)), text));
        }
    }
    out
}

/// The lines of the system message with the project's instructions, or nothing when it has none.
pub fn instructions(root: &Path) -> String {
    let all = found(root);
    if all.is_empty() {
        return String::new();
    }
    // The nearest first gets its room; what is left goes outwards.
    let mut left = MAX_CHARS;
    let mut kept: Vec<String> = Vec::new();
    for (path, text) in all.iter().rev() {
        if left < 200 {
            break;
        }
        let text = files::cap(text, left);
        left = left.saturating_sub(text.chars().count());
        kept.push(format!("From {path}:\n{text}"));
    }
    kept.reverse();
    format!("\n\nThe project's own instructions. Follow them; the person's words come first when they disagree.\n{}", kept.join("\n\n"))
}

/// A request as the model is sent it, when the person typed a command.
#[derive(Debug, PartialEq)]
pub struct Expanded {
    /// What the model is asked.
    pub text: String,
    /// The skill the command named: it is read for the model before the work starts.
    pub skill: Option<String>,
}

/// What `/review` sends: the uncommitted change in the folder, at most `room` characters of
/// it, or why there is none.
fn uncommitted(root: &Path, room: usize) -> Result<String, String> {
    let status = super::undo::git(root, None, &["status", "--porcelain", "--untracked-files=all"], None)
        .map_err(|_| "Nothing to review: this folder is not a git repository.".to_string())?;
    let status = String::from_utf8_lossy(&status).trim().to_string();
    if status.is_empty() {
        return Err("Nothing to review: there are no uncommitted changes.".into());
    }
    let diff = super::undo::git(root, None, &["diff", "HEAD", "--no-color", "--no-ext-diff"], None)
        .or_else(|_| super::undo::git(root, None, &["diff", "--no-color", "--no-ext-diff"], None))
        .unwrap_or_default();
    let diff = String::from_utf8_lossy(&diff);
    let new: Vec<&str> = status.lines().filter_map(|l| l.strip_prefix("?? ")).collect();
    let mut out = format!("git status:\n{status}\n\ngit diff HEAD:\n{}", files::cap(diff.trim(), room));
    if !new.is_empty() {
        out.push_str(&format!("\n\nNew files are not in the diff; read them: {}", new.join(", ")));
    }
    Ok(out)
}

/// The request a command stands for. `None` when `text` is no command the service knows: it is
/// sent as it is. `Err` is said to the person, and nothing is sent. `room` is what the model's
/// context has for one message.
pub fn expand(text: &str, root: &Path, skills: &[Skill], room: usize) -> Option<Result<Expanded, String>> {
    let rest = text.strip_prefix('/')?;
    let (name, extra) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(n, e)| (n, e.trim()));
    let also = |line: &str| if extra.is_empty() { line.to_string() } else { format!("{line}\n\nAlso: {extra}") };
    let plain = |text: String| Some(Ok(Expanded { text, skill: None }));
    match name {
        "init" => {
            let there = NAMES.iter().find(|n| root.join(n).is_file());
            let start = match there {
                Some(name) => format!("{name} is already there: read it, keep what is still true, and improve the rest."),
                None => "Then write AGENTS.md at the top of the folder.".into(),
            };
            plain(also(&format!(
                "Explore this folder and write the instructions for people and coding agents who work in it. This is only about \
                 writing them: change no other file, and fix nothing, even what is broken; say it in the instructions instead. \
                 Read before you write: the layout, the build and test commands, the conventions the code follows. {start} Keep it \
                 short and specific to this project: what it is, how it is laid out, how to build, run and test it, and the rules \
                 to follow."
            )))
        }
        "review" => Some(uncommitted(root, room).map(|change| Expanded {
            text: format!(
                "{}\n\n{change}",
                also(
                    "Review the uncommitted changes below. Look for bugs, missing cases, and anything that does not fit the code \
                     around it; read the files when you need more of them. Do not change files. Answer with a short list: the file \
                     and line, what is wrong, and what to do. If everything looks right, say so."
                )
            ),
            skill: None,
        })),
        "test" => plain(also(
            "Run the project's checks (check_project, or its own test command) and fix what fails. Then run them again, until \
             they pass. Change the code, not the tests, unless a test is plainly wrong; then say why.",
        )),
        "explain" => {
            let what = if extra.is_empty() { "this codebase" } else { extra };
            plain(format!(
                "Explain {what}: what it does and how its parts fit together. Read the code before you explain, name the files \
                 and functions you mean, and do not change anything."
            ))
        }
        _ => {
            let skill = skills.iter().find(|s| s.name == name)?;
            let text =
                if extra.is_empty() { format!("Do what the skill {} is for, in this folder.", skill.name) } else { extra.to_string() };
            Some(Ok(Expanded { text, skill: Some(skill.name.clone()) }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_come_from_the_folder_and_the_repository_above_it() {
        let top = files::scratch("project");
        let inner = top.join("crates/app");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(instructions(&inner), "", "no instructions anywhere");
        std::fs::write(top.join("AGENTS.md"), "Run just test before you commit.\n").unwrap();
        // Outside a repository only the folder itself counts.
        assert_eq!(instructions(&inner), "");
        std::fs::create_dir_all(top.join(".git")).unwrap();
        std::fs::write(inner.join("CLAUDE.md"), "This crate is the app.").unwrap();
        std::fs::write(inner.join("RIGBREW.md"), "This crate is the app.").unwrap();
        let found = found(&inner);
        assert_eq!(
            found.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            ["../../AGENTS.md", "RIGBREW.md"],
            "the same text counts once"
        );
        let said = instructions(&inner);
        assert!(
            said.contains("From ../../AGENTS.md:\nRun just test before you commit.\n\nFrom RIGBREW.md:\nThis crate is the app."),
            "{said}"
        );
        // Too much: the nearest file keeps its room, the outer one is cut.
        std::fs::write(top.join("AGENTS.md"), "x".repeat(10_000)).unwrap();
        std::fs::write(inner.join("RIGBREW.md"), "near ".repeat(100)).unwrap();
        let said = instructions(&inner);
        assert!(said.contains(&"near ".repeat(99)) && said.chars().count() < MAX_CHARS + 400, "{}", said.len());
    }

    #[test]
    fn commands_stand_for_a_request() {
        let root = files::scratch("commands");
        let skill = Skill {
            name: "write-tests".into(),
            description: "d".into(),
            body: "b".into(),
            source: "built-in",
            dir: None,
            scope: super::super::skills::Scope::Any,
            enabled: true,
        };
        let skills = [skill];
        assert_eq!(expand("Fix the test", &root, &skills, 9000), None);
        assert_eq!(expand("/nothing here", &root, &skills, 9000), None, "an unknown command is sent as it is");
        let init = expand("/init", &root, &skills, 9000).unwrap().unwrap();
        assert!(init.text.contains("write AGENTS.md") && init.skill.is_none(), "{}", init.text);
        std::fs::write(root.join("AGENTS.md"), "old").unwrap();
        let again = expand("/init mention the release steps", &root, &skills, 9000).unwrap().unwrap();
        assert!(again.text.contains("AGENTS.md is already there") && again.text.ends_with("Also: mention the release steps"));
        assert!(expand("/explain src/parse.rs", &root, &skills, 9000).unwrap().unwrap().text.starts_with("Explain src/parse.rs:"));
        assert!(expand("/test", &root, &skills, 9000).unwrap().unwrap().text.contains("check_project"));
        let skilled = expand("/write-tests for the parser", &root, &skills, 9000).unwrap().unwrap();
        assert_eq!(skilled, Expanded { text: "for the parser".into(), skill: Some("write-tests".into()) });
        assert_eq!(
            expand("/write-tests", &root, &skills, 9000).unwrap().unwrap().text,
            "Do what the skill write-tests is for, in this folder."
        );
        // Not a repository: nothing to review, and the person is told.
        assert!(expand("/review", &root, &skills, 9000).unwrap().unwrap_err().contains("not a git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn review_sends_the_uncommitted_change() {
        if crate::connectors::find_program("git").is_none() {
            return;
        }
        let root = files::scratch("review");
        let git = |args: &[&str]| super::super::undo::git(&root, None, args, None).unwrap();
        git(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "first"]);
        assert!(expand("/review", &root, &[], 9000).unwrap().unwrap_err().contains("no uncommitted changes"));
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("b.txt"), "new\n").unwrap();
        let review = expand("/review the edge cases", &root, &[], 9000).unwrap().unwrap().text;
        assert!(review.contains("Also: the edge cases") && review.contains("+two") && review.contains("?? b.txt"), "{review}");
        assert!(review.ends_with("New files are not in the diff; read them: b.txt"), "{review}");
    }
}
