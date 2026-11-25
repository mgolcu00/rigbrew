//! Verify before claiming: after a change the model is told which command checks this project,
//! and an answer that says the work is good when nothing was run after the last change gets a
//! plain line under it.
//!
//! The rule is deliberately simple. "Checked" means a command ended well after the last file
//! change of the request; which command it was is not judged.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::{Step, Task};

pub const NOT_RUN: &str = "Not checked: no test or build ran after the last change.";
pub const NOT_PASSED: &str = "Not checked: the last command after the change did not end well.";

fn has(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

fn text_of(root: &Path, name: &str) -> String {
    // Project files are small; anything large is not one.
    match std::fs::metadata(root.join(name)) {
        Ok(m) if m.is_file() && m.len() < 512 * 1024 => std::fs::read_to_string(root.join(name)).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Python test files at the top of the project or in its test folder.
fn python_tests(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for dir in [".", "tests", "test"] {
        for entry in std::fs::read_dir(root.join(dir)).into_iter().flatten().flatten().take(500) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py")) {
                out.push(entry.path());
            }
        }
    }
    out
}

/// The commands that test or build the project in this folder, the most telling first.
pub fn commands(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if has(root, "Cargo.toml") {
        out.push("cargo test".into());
    }
    if has(root, "go.mod") {
        out.push("go test ./...".into());
    }
    if let Ok(package) = serde_json::from_str::<serde_json::Value>(&text_of(root, "package.json")) {
        let runner = if has(root, "pnpm-lock.yaml") {
            "pnpm"
        } else if has(root, "yarn.lock") {
            "yarn"
        } else if has(root, "bun.lockb") || has(root, "bun.lock") {
            "bun"
        } else {
            "npm"
        };
        for script in ["test", "check", "build"] {
            // The script npm writes into a new project only prints that there is no test.
            if package["scripts"][script].as_str().is_some_and(|s| !s.trim().is_empty() && !s.contains("no test specified")) {
                out.push(if script == "test" { format!("{runner} test") } else { format!("{runner} run {script}") });
            }
        }
    }
    let pytest_named = ["pyproject.toml", "setup.cfg", "tox.ini", "requirements.txt", "requirements-dev.txt"]
        .iter()
        .any(|name| text_of(root, name).contains("pytest"));
    let tests = python_tests(root);
    if pytest_named || has(root, "pytest.ini") || has(root, "conftest.py") || has(root, "tests/conftest.py") {
        out.push("python3 -m pytest".into());
    } else if !tests.is_empty() {
        let unittest = tests.iter().take(20).any(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("unittest")));
        out.push(if unittest { "python3 -m unittest".into() } else { "python3 -m pytest".into() });
    }
    let makefile = text_of(root, "Makefile");
    for target in ["test", "check", "build"] {
        if makefile
            .lines()
            .any(|l| l.strip_prefix(target).is_some_and(|rest| rest.trim_start().starts_with(':') && !rest.trim_start().starts_with(":=")))
        {
            out.push(format!("make {target}"));
        }
    }
    out.truncate(2);
    out
}

/// What is added to the result of a change that was applied.
pub fn reminder(root: &Path) -> String {
    match commands(root).as_slice() {
        [] => "Now check it if you can: run the project's tests or its build, and say what really happened.".into(),
        [one] => format!("Now check it: run `{one}` and say what it really printed."),
        [one, two, ..] => format!("Now check it: run `{one}` (or `{two}`) and say what it really printed."),
    }
}

/// Adds the reminder to the result of `edit_file` or `write_file` when the change went in.
pub fn after_change(root: &Path, outcome: (String, &'static str)) -> (String, &'static str) {
    match outcome {
        (text, "done") => (format!("{text}\n{}", reminder(root)), "done"),
        other => other,
    }
}

/// Whether a text says that the work is good: that something works, passes or is fixed. A
/// sentence that also says no ("does not work yet", "could not be fixed") does not count.
pub fn claims_success(text: &str) -> bool {
    static WORDS: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (yes, no) = WORDS.get_or_init(|| {
        (
            Regex::new(r"(?i)\b(works?|working|pass(es|ed|ing)?|fixed|fixes|succeed(s|ed)?|successful(ly)?|resolved|solved|verified|green)\b")
                .expect("pattern"),
            Regex::new(r"(?i)(\b(not|never|cannot|fail(s|ed|ing|ure)?|unable|without|should|might|may|if|until|to see (if|whether)|untested|unverified)\b|n't|n’t)")
                .expect("pattern"),
        )
    });
    text.split(['.', '!', '?', '\n', ';']).any(|sentence| yes.is_match(sentence) && !no.is_match(sentence))
}

/// How the current request stands: `None` when nothing needs a check (no file was changed) or
/// a command ended well after the last change; otherwise the line that says what is missing.
pub fn unchecked(steps: &[Step]) -> Option<&'static str> {
    let start = steps.iter().rposition(|s| matches!(s, Step::User { .. })).map_or(0, |i| i + 1);
    let request = &steps[start..];
    let changed =
        |s: &Step| matches!(s, Step::Tool { name, status, .. } if status == "done" && (name == "edit_file" || name == "write_file"));
    let last_change = request.iter().rposition(changed)?;
    let mut ran = false;
    for step in &request[last_change + 1..] {
        if let Step::Tool { name, status, .. } = step {
            if name == "run_command" && status == "done" {
                return None;
            }
            ran |= name == "run_command" && status == "failed";
        }
    }
    Some(if ran { NOT_PASSED } else { NOT_RUN })
}

/// Called when the model gives its final answer: says so under it when the answer claims more
/// than was checked.
pub fn note_unchecked(task: &mut Task, answer: &str) {
    if !claims_success(answer) {
        return;
    }
    if let Some(line) = unchecked(&task.steps) {
        task.steps.push(Step::Note { text: line.into() });
    }
}

#[cfg(test)]
mod tests {
    use super::super::files;
    use super::*;
    use serde_json::json;

    fn project(files_in_it: &[(&str, &str)]) -> std::path::PathBuf {
        let root = files::scratch("verify");
        for (path, text) in files_in_it {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        root
    }

    #[test]
    fn the_command_of_a_project_is_found() {
        let cmds = |files_in_it: &[(&str, &str)]| commands(&project(files_in_it));
        assert_eq!(cmds(&[("Cargo.toml", "[package]\n")]), ["cargo test"]);
        assert_eq!(cmds(&[("go.mod", "module x\n")]), ["go test ./..."]);
        let scripts = r#"{"scripts": {"build": "vite build", "check": "svelte-check", "test": "vitest run"}}"#;
        assert_eq!(cmds(&[("package.json", scripts)]), ["npm test", "npm run check"]);
        assert_eq!(cmds(&[("package.json", scripts), ("pnpm-lock.yaml", "")]), ["pnpm test", "pnpm run check"]);
        assert_eq!(cmds(&[("package.json", r#"{"scripts": {"build": "tsc"}}"#), ("yarn.lock", "")]), ["yarn run build"]);
        let placeholder = r#"{"scripts": {"test": "echo \"Error: no test specified\" && exit 1"}}"#;
        assert!(cmds(&[("package.json", placeholder)]).is_empty());
        assert!(cmds(&[("package.json", "{not json")]).is_empty());
        assert_eq!(cmds(&[("pyproject.toml", "[tool.pytest.ini_options]\n")]), ["python3 -m pytest"]);
        assert_eq!(cmds(&[("pytest.ini", "")]), ["python3 -m pytest"]);
        assert_eq!(cmds(&[("tests/test_shop.py", "def test_total():\n    assert True\n")]), ["python3 -m pytest"]);
        assert_eq!(cmds(&[("test_shop.py", "import unittest\n")]), ["python3 -m unittest"]);
        assert!(cmds(&[("shop.py", "x = 1\n"), ("pyproject.toml", "[project]\n")]).is_empty());
        assert_eq!(cmds(&[("Makefile", "CC := cc\ntest: build\n\t./run\nbuild:\n\tcc x.c\ntesting:\n")]), ["make test", "make build"]);
        assert!(cmds(&[("Makefile", "test := 1\nall:\n\ttrue\n")]).is_empty());
        assert_eq!(
            cmds(&[("Cargo.toml", ""), ("package.json", scripts), ("Makefile", "test:\n")]),
            ["cargo test", "npm test"],
            "two at most"
        );
        assert!(cmds(&[("README.md", "hello\n")]).is_empty());
    }

    #[test]
    fn a_change_that_went_in_is_followed_by_the_reminder() {
        let root = project(&[("Cargo.toml", "")]);
        let (text, status) = after_change(&root, ("Changed src/lib.rs (10 lines now).".into(), "done"));
        assert_eq!(status, "done");
        assert_eq!(text, "Changed src/lib.rs (10 lines now).\nNow check it: run `cargo test` and say what it really printed.");
        assert_eq!(
            after_change(&root, ("The text to replace is not in src/lib.rs.".into(), "failed")).0,
            "The text to replace is not in src/lib.rs."
        );
        assert_eq!(after_change(&root, ("no".into(), "denied")), ("no".into(), "denied"));
        assert!(reminder(&project(&[])).starts_with("Now check it if you can"));
        assert!(reminder(&project(&[("package.json", r#"{"scripts": {"test": "x", "build": "y"}}"#)]))
            .contains("`npm test` (or `npm run build`)"));
    }

    #[test]
    fn an_answer_that_says_it_is_good_is_told_apart() {
        for yes in [
            "I fixed the bug in total().",
            "The test passes now.",
            "All tests pass.",
            "It works.",
            "The build is successful",
            "Done. The function now works as expected and the tests are green.",
            "The change could not be applied to a.py. The other one is fixed.",
        ] {
            assert!(claims_success(yes), "{yes}");
        }
        for no in [
            "I changed total() to add the tax.",
            "The test still fails.",
            "It does not work yet.",
            "I could not run the tests, so this is not verified.",
            "This should work, but I did not run it.",
            "Run the tests to see if it passes.",
            "The fix isn't working",
            "Here is a summary of the folder.",
            "The network card works differently, it does not pass packets.",
        ] {
            assert!(!claims_success(no), "{no}");
        }
    }

    fn user() -> Step {
        Step::User { text: "fix it".into() }
    }

    fn tool(name: &str, status: &str) -> Step {
        Step::Tool { call: "c".into(), name: name.into(), arguments: json!({}), status: status.into(), result: None, diff: None }
    }

    #[test]
    fn checked_means_a_command_ended_well_after_the_last_change() {
        assert_eq!(unchecked(&[user(), tool("read_file", "done")]), None, "nothing was changed");
        assert_eq!(unchecked(&[user(), tool("edit_file", "denied")]), None, "a change that was refused is no change");
        assert_eq!(unchecked(&[user(), tool("edit_file", "done")]), Some(NOT_RUN));
        assert_eq!(unchecked(&[user(), tool("run_command", "done"), tool("write_file", "done")]), Some(NOT_RUN), "the check came before");
        assert_eq!(unchecked(&[user(), tool("edit_file", "done"), tool("run_command", "denied")]), Some(NOT_RUN));
        assert_eq!(unchecked(&[user(), tool("edit_file", "done"), tool("run_command", "failed")]), Some(NOT_PASSED));
        assert_eq!(unchecked(&[user(), tool("edit_file", "done"), tool("run_command", "failed"), tool("run_command", "done")]), None);
        assert_eq!(
            unchecked(&[
                user(),
                tool("edit_file", "done"),
                tool("run_command", "done"),
                tool("edit_file", "done"),
                tool("read_file", "done")
            ]),
            Some(NOT_RUN),
            "changed again after the check"
        );
        // Only the request being worked on counts.
        assert_eq!(unchecked(&[user(), tool("edit_file", "done"), user(), tool("read_file", "done")]), None);
        assert_eq!(
            unchecked(&[user(), tool("edit_file", "done"), tool("run_command", "done"), user(), tool("edit_file", "done")]),
            Some(NOT_RUN)
        );
    }
}
