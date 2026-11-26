//! `check_project`: the project's own checks, found from what is in the folder and run one
//! after the other, with a short answer: what ran, whether it passed, and the first failures
//! with their file and line. Never the whole log.
//!
//! Three kinds are "fast" and run by default: the type check or build, the linter, and the
//! unit tests. Tests that drive the app (a browser, a device) are the "ui" kind and run only
//! when asked for. Nothing is ever installed: a check whose program is missing is reported as
//! not run, with the name of what is missing.
//!
//! The commands are put together here from fixed words, never from what the model wrote, and
//! run the way `run_command` runs one: in the folder, on macOS inside the sandbox.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::files::{self, Folder, Net};
use super::verify;
use crate::connectors::find_program;

/// Failures shown for one check.
const MAX_FAILURES: usize = 8;
/// Lines of the end of the output shown when no failure could be read from it.
const TAIL_LINES: usize = 12;
const LINE_CHARS: usize = 200;
const XCODE_BY_HAND: &str = "an Xcode test needs a scheme and a device: run it with run_command, for example \
`xcodebuild test -scheme <scheme> -destination 'platform=iOS Simulator,name=<device>'` (use_skill ios-simulator says how)";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The compiler or type checker.
    Types,
    Lint,
    /// Unit tests.
    Tests,
    /// Tests that drive the running app: a browser or a device.
    Ui,
}

impl Kind {
    fn timeout(self) -> Duration {
        Duration::from_secs(match self {
            Kind::Types | Kind::Lint => 180,
            Kind::Tests => 300,
            Kind::Ui => 600,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub kind: Kind,
    /// The command line; also how the check is named.
    pub command: String,
    /// Why it cannot run here: what is not installed.
    pub missing: Option<String>,
}

fn has(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

fn text_of(root: &Path, name: &str) -> String {
    match std::fs::metadata(root.join(name)) {
        Ok(m) if m.is_file() && m.len() < 512 * 1024 => std::fs::read_to_string(root.join(name)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn any_file(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|n| has(root, n))
}

/// Whether a folder (one of a few, not deep) holds a file whose name passes `test`.
fn holds(root: &Path, dirs: &[&str], test: &dyn Fn(&str) -> bool) -> bool {
    dirs.iter().any(|dir| {
        std::fs::read_dir(root.join(dir)).into_iter().flatten().flatten().take(500).any(|e| test(&e.file_name().to_string_lossy()))
    })
}

fn not_installed(program: &str) -> Option<String> {
    find_program(program).is_none().then(|| format!("{program} is not installed on this computer"))
}

fn add(out: &mut Vec<Check>, kind: Kind, command: &str, missing: Option<String>) {
    if !out.iter().any(|c| c.command == command) {
        out.push(Check { kind, command: command.to_string(), missing });
    }
}

/// The checks of the project in this folder, in the order they run.
pub fn detect(root: &Path) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    if has(root, "Cargo.toml") {
        let missing = not_installed("cargo");
        add(&mut out, Kind::Types, "cargo check --all-targets --message-format short", missing.clone());
        add(&mut out, Kind::Tests, "cargo test", missing);
    }
    if has(root, "go.mod") {
        let missing = not_installed("go");
        add(&mut out, Kind::Types, "go vet ./...", missing.clone());
        add(&mut out, Kind::Tests, "go test ./...", missing);
    }
    if let Ok(package) = serde_json::from_str::<Value>(&text_of(root, "package.json")) {
        let runner = verify::node_runner(root);
        let script = |name: &str| package["scripts"][name].as_str().filter(|s| !s.trim().is_empty() && !s.contains("no test specified"));
        let needs_packages = ["dependencies", "devDependencies"].iter().any(|k| package[*k].as_object().is_some_and(|o| !o.is_empty()));
        let missing = not_installed(runner).or_else(|| {
            (needs_packages && !root.join("node_modules").is_dir())
                .then(|| format!("the packages of this project are not installed (run `{runner} install`)"))
        });
        let bin = |name: &str| root.join("node_modules/.bin").join(name).exists();
        let local = |name: &str, what: &str| {
            missing.clone().or_else(|| (!bin(name)).then(|| format!("{what} is not installed in this project (run `{runner} install`)")))
        };
        match ["typecheck", "type-check", "check", "tsc"].iter().find(|s| script(s).is_some()) {
            Some(name) => add(&mut out, Kind::Types, &format!("{runner} run {name}"), missing.clone()),
            None if has(root, "tsconfig.json") => add(&mut out, Kind::Types, "npx --no-install tsc --noEmit", local("tsc", "TypeScript")),
            None => {}
        }
        if script("lint").is_some() {
            add(&mut out, Kind::Lint, &format!("{runner} run lint"), missing.clone());
        }
        let node_test = |name: &str| [".test.js", ".test.mjs", ".test.cjs"].iter().any(|end| name.ends_with(end));
        if let Some(line) = script("test") {
            let drives_app = line.contains("playwright") || line.contains("cypress");
            add(&mut out, if drives_app { Kind::Ui } else { Kind::Tests }, &format!("{runner} test"), missing.clone());
        } else if holds(root, &[".", "test", "tests", "src"], &node_test) {
            add(&mut out, Kind::Tests, "node --test", not_installed("node"));
        }
        let config = |tool: &str| ["ts", "js", "mjs", "cjs"].iter().any(|e| has(root, &format!("{tool}.config.{e}")));
        let has_ui = out.iter().any(|c| c.kind == Kind::Ui);
        if let Some(name) = ["test:e2e", "e2e", "test:ui", "test:playwright", "test:cypress"].iter().find(|s| script(s).is_some()) {
            add(&mut out, Kind::Ui, &format!("{runner} run {name}"), missing.clone());
        } else if !has_ui && config("playwright") {
            add(&mut out, Kind::Ui, "npx --no-install playwright test --reporter=line", local("playwright", "Playwright"));
        } else if !has_ui && config("cypress") {
            add(&mut out, Kind::Ui, "npx --no-install cypress run", local("cypress", "Cypress"));
        }
    }
    let pyproject = text_of(root, "pyproject.toml");
    if any_file(root, &["ruff.toml", ".ruff.toml"]) || pyproject.contains("[tool.ruff") {
        add(&mut out, Kind::Lint, "ruff check --output-format concise .", not_installed("ruff"));
    }
    if has(root, "mypy.ini") || pyproject.contains("[tool.mypy") || text_of(root, "setup.cfg").contains("[mypy") {
        add(&mut out, Kind::Types, "mypy .", not_installed("mypy"));
    }
    let own = verify::python(root);
    if let Some(python) = verify::commands(root).iter().find(|c| c.starts_with(&format!("{own} -m "))) {
        let quiet = if python.ends_with("pytest") { " -q" } else { "" };
        let command = format!("{python}{quiet}");
        if own != "python3" {
            add(&mut out, Kind::Tests, &command, None);
        } else {
            let command = if cfg!(windows) { command.replacen("python3", "python", 1) } else { command };
            add(&mut out, Kind::Tests, &command, not_installed(if cfg!(windows) { "python" } else { "python3" }));
        }
    }
    if has(root, "Package.swift") {
        let missing = not_installed("swift");
        // On macOS the check already runs in Rigbrew's sandbox, and Swift's own cannot start inside another.
        let flag = if cfg!(target_os = "macos") { " --disable-sandbox" } else { "" };
        add(&mut out, Kind::Types, &format!("swift build{flag}"), missing.clone());
        if root.join("Tests").is_dir() {
            add(&mut out, Kind::Tests, &format!("swift test{flag}"), missing);
        }
    }
    let pubspec = text_of(root, "pubspec.yaml");
    if !pubspec.is_empty() {
        let tool = if pubspec.contains("flutter") { "flutter" } else { "dart" };
        let missing = not_installed(tool);
        add(&mut out, Kind::Types, &format!("{tool} analyze"), missing.clone());
        if root.join("test").is_dir() {
            add(&mut out, Kind::Tests, &format!("{tool} test"), missing.clone());
        }
        if tool == "flutter" && root.join("integration_test").is_dir() {
            add(&mut out, Kind::Ui, "flutter test integration_test", missing);
        }
    }
    let xcode = |name: &str| name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace");
    if !has(root, "Package.swift") && pubspec.is_empty() && holds(root, &["."], &xcode) {
        // Which scheme and which simulator is the project's own business: the model runs it.
        add(&mut out, Kind::Ui, "xcodebuild test", Some(XCODE_BY_HAND.into()));
    }
    if out.is_empty() {
        for (kind, command) in verify::commands(root).iter().filter_map(|c| match c.as_str() {
            "make test" => Some((Kind::Tests, c)),
            "make check" | "make build" => Some((Kind::Types, c)),
            _ => None,
        }) {
            out.push(Check { kind, command: command.clone(), missing: not_installed("make") });
        }
    }
    out
}

/// Which kinds a call asks for: "fast" (types, lint, unit tests), "ui" or "all".
pub fn wanted(what: &str) -> Result<&'static [Kind], String> {
    match what.trim() {
        "" | "fast" => Ok(&[Kind::Types, Kind::Lint, Kind::Tests]),
        "ui" => Ok(&[Kind::Ui]),
        "all" => Ok(&[Kind::Types, Kind::Lint, Kind::Tests, Kind::Ui]),
        other => Err(format!("\"what\" is \"fast\", \"ui\" or \"all\", not \"{other}\".")),
    }
}

/// The checks a call would run, and those among them that cannot run here.
pub fn chosen(root: &Path, kinds: &[Kind]) -> Vec<Check> {
    detect(root).into_iter().filter(|c| kinds.contains(&c.kind)).collect()
}

/// Whether the project has fast checks that can run on this computer.
pub fn has_fast(root: &Path) -> bool {
    detect(root).iter().any(|c| c.kind != Kind::Ui && c.missing.is_none())
}

#[derive(Debug, PartialEq)]
pub enum State {
    Passed,
    Failed,
    TimedOut,
    NotRun(String),
}

#[derive(Debug)]
pub struct Outcome {
    pub command: String,
    pub state: State,
    /// Tests that passed and failed, when the output says.
    pub counts: Option<(usize, usize)>,
    pub failures: Vec<String>,
    /// The end of the output, kept only when no failure could be read from it.
    pub tail: Vec<String>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static pattern"))
}

fn num(text: &str) -> usize {
    text.parse().unwrap_or(0)
}

/// Tests passed and failed, as the runner's own summary says. `None` when it says nothing.
pub fn counts(output: &str) -> Option<(usize, usize)> {
    static CARGO: OnceLock<Regex> = OnceLock::new();
    static JEST: OnceLock<Regex> = OnceLock::new();
    static VITEST: OnceLock<Regex> = OnceLock::new();
    static NODE: OnceLock<Regex> = OnceLock::new();
    static PYTEST: OnceLock<Regex> = OnceLock::new();
    static UNITTEST: OnceLock<Regex> = OnceLock::new();
    static XCTEST: OnceLock<Regex> = OnceLock::new();
    static DART: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    let cargo = re(&CARGO, r"test result: \w+\. (\d+) passed; (\d+) failed");
    if cargo.is_match(output) {
        return Some(cargo.captures_iter(output).fold((0, 0), |(p, f), c| (p + num(&c[1]), f + num(&c[2]))));
    }
    let word = re(&WORD, r"(\d+) (passed|failed|errors?)\b");
    let tally = |line: &str| {
        let (mut passed, mut failed) = (0, 0);
        for c in word.captures_iter(line) {
            if &c[2] == "passed" {
                passed += num(&c[1]);
            } else {
                failed += num(&c[1]);
            }
        }
        (passed, failed)
    };
    // Jest: "Tests:       2 failed, 14 passed, 16 total". Vitest: "Tests  2 failed | 14 passed (16)".
    for summary in [re(&JEST, r"(?m)^Tests:\s+(.*\d+ total.*)$"), re(&VITEST, r"(?m)^\s*Tests\s{2,}(.*\(\d+\).*)$")] {
        if let Some(c) = summary.captures_iter(output).last() {
            return Some(tally(&c[1]));
        }
    }
    let node = re(&NODE, r"(?m)^(?:#|ℹ) (pass|fail) (\d+)\s*$");
    if node.is_match(output) {
        return Some(node.captures_iter(output).fold((0, 0), |(p, f), c| if &c[1] == "pass" { (num(&c[2]), f) } else { (p, num(&c[2])) }));
    }
    if let Some(c) = re(&XCTEST, r"Executed (\d+) tests?, with (\d+) failures?").captures_iter(output).last() {
        let (total, failed) = (num(&c[1]), num(&c[2]));
        return Some((total.saturating_sub(failed), failed));
    }
    if let Some(c) = re(&DART, r"\+(\d+)(?: ~\d+)?(?: -(\d+))?: (?:All tests passed!|Some tests failed\.)").captures_iter(output).last() {
        return Some((num(&c[1]), c.get(2).map_or(0, |m| num(m.as_str()))));
    }
    if let Some(c) = re(&UNITTEST, r"(?m)^Ran (\d+) tests? in ").captures_iter(output).last() {
        let total = num(&c[1]);
        let failed = output.lines().rev().find(|l| l.starts_with("FAILED (")).map_or(0, |l| {
            static N: OnceLock<Regex> = OnceLock::new();
            re(&N, r"(?:failures|errors)=(\d+)").captures_iter(l).map(|c| num(&c[1])).sum()
        });
        return Some((total.saturating_sub(failed), failed));
    }
    // pytest ("2 failed, 14 passed in 0.12s") and Playwright ("  2 failed", "  14 passed (3.2s)").
    let pytest = re(&PYTEST, r"(?m)^(?:=+ )?((?:\d+ \w+,? ?)+) in [\d.]+ ?s");
    if let Some(c) = pytest.captures_iter(output).last() {
        return Some(tally(&c[1]));
    }
    let lines: Vec<&str> =
        output.lines().rev().take(12).filter(|l| l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit())).collect();
    let (passed, failed) = lines.iter().fold((0, 0), |(p, f), l| {
        let (a, b) = tally(l);
        (p + a, f + b)
    });
    (passed + failed > 0).then_some((passed, failed))
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= LINE_CHARS {
        return text.to_string();
    }
    format!("{}…", text.chars().take(LINE_CHARS - 1).collect::<String>())
}

/// A path as the model can open it: relative to the folder, with forward slashes.
fn near(root: &Path, path: &str) -> String {
    let root = root.to_string_lossy();
    let path = path.strip_prefix(root.as_ref()).unwrap_or(path);
    path.trim_start_matches(['/', '\\']).trim_start_matches("./").replace('\\', "/")
}

/// The first failures in the output of a check, each as "file:line what went wrong" where the
/// output says so. Knows the compilers, linters and test runners `detect` starts.
pub fn failures(output: &str, root: &Path) -> Vec<String> {
    static ANSI: OnceLock<Regex> = OnceLock::new();
    static PLACE: OnceLock<Regex> = OnceLock::new();
    static RUST_AT: OnceLock<Regex> = OnceLock::new();
    static PANIC: OnceLock<Regex> = OnceLock::new();
    static STYLISH: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    static NAMED: OnceLock<Regex> = OnceLock::new();
    static PYTHON: OnceLock<Regex> = OnceLock::new();
    static NODE_AT: OnceLock<Regex> = OnceLock::new();
    static TOOK: OnceLock<Regex> = OnceLock::new();
    // Node's own runner says where a failed test is on the line before its name.
    let node_at = re(&NODE_AT, r"^test at (\S+?):(\d+):\d+$");
    let took = re(&TOOK, r" \([\d.]+ ?m?s\)$");
    // A frame of a Python traceback.
    let python = re(&PYTHON, r#"^\s*File "([^"]+)", line (\d+), in (\w+)$"#);
    let ansi = re(&ANSI, r"\x1b\[[0-9;?]*[A-Za-z]");
    // "src/a.ts:3:5: message", "src/a.ts(3,5): message", "src/a.ts:3:5 - message", "a.py:3: message".
    let place = re(
        &PLACE,
        r"^\s*((?:[A-Za-z]:[\\/]|\.{0,2}[\\/])?[\w@+\-.\\/\[\]]*[\w\-\]]\.[A-Za-z][A-Za-z0-9]{0,5})[:(](\d+)(?:[:,]\d+)?\)?(?::| -|$)\s*(.*)$",
    );
    let rust_at = re(&RUST_AT, r"^\s*--> (\S+?):(\d+):\d+$");
    let panic = re(&PANIC, r"^thread '([^']+)'.* panicked at (\S+?):(\d+):\d+:$");
    let stylish = re(&STYLISH, r"^\s+(\d+):\d+\s+error\s+(.*)$");
    let bare = re(&BARE, r"^(?:[A-Za-z]:[\\/]|[\\/])?[\w@+\-.\\/\[\]]+\.[A-Za-z][A-Za-z0-9]{0,5}$");
    // A test a runner names as failed, without a place.
    let named = re(&NAMED, r"^\s*(?:FAILED |FAIL[: ]\s*|--- FAIL: |not ok \d+ - |[●✕✖✗×] )(.+)$");
    let lines: Vec<String> = output.lines().map(|l| ansi.replace_all(l, "").trim_end().to_string()).collect();
    let bad = |text: &str| {
        let t = text.to_lowercase();
        ["error", "fail", "panicked", "assert", "expected", "exception", "undefined", "cannot", "not found"].iter().any(|w| t.contains(w))
    };
    let test_file = |path: &str| ["_test.", ".test.", ".spec.", "test_", "Tests.", "Test."].iter().any(|k| path.contains(k));
    let mut out: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let push = |list: &mut Vec<String>, line: String| {
        if list.len() < MAX_FAILURES && !list.contains(&line) {
            list.push(line);
        }
    };
    // What the lines before said: an error without a place yet, a file named on a line of its own.
    let (mut error, mut file) = (String::new(), String::new());
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i + 1).map_or("", |l| l.trim());
        if let Some(c) = panic.captures(line) {
            push(&mut out, clip(&format!("{}:{} {}: {next}", near(root, &c[2]), &c[3], &c[1])));
            continue;
        }
        if let Some(c) = node_at.captures(line) {
            if let Some(name) = named.captures(next) {
                let name = took.replace(c_name(&name), "").into_owned();
                // The same test was named before, without its place.
                out.retain(|o| took.replace(o, "") != name);
                push(&mut out, clip(&format!("{}:{} {name}", near(root, &c[1]), &c[2])));
            }
            continue;
        }
        if let Some(c) = python.captures(line) {
            // Only the project's own files: not the library the error passed through.
            let inside = !Path::new(&c[1]).is_absolute() || Path::new(&c[1]).starts_with(root);
            if inside && !c[1].contains("site-packages") {
                push(&mut out, clip(&format!("{}:{} in {}", near(root, &c[1]), &c[2], &c[3])));
            }
            continue;
        }
        if let Some(c) = rust_at.captures(line) {
            if !error.is_empty() {
                push(&mut out, clip(&format!("{}:{} {error}", near(root, &c[1]), &c[2])));
                error.clear();
            }
            continue;
        }
        if line.starts_with("error") && !line.starts_with("error: could not compile") && !line.starts_with("error: test failed") {
            error = line.clone();
            continue;
        }
        if let Some(c) = stylish.captures(line) {
            if !file.is_empty() {
                push(&mut out, clip(&format!("{}:{} {}", near(root, &file), &c[1], &c[2])));
            }
            continue;
        }
        if bare.is_match(line) {
            file = line.clone();
            continue;
        }
        if let Some(c) = place.captures(line) {
            let (path, at, said) = (near(root, &c[1]), &c[2], c[3].trim());
            // A place on a line of its own: what went wrong is on the next line.
            let said = if said.is_empty() && bad(next) { next } else { said };
            if said.is_empty() || path.starts_with("node_modules/") {
                continue;
            }
            let entry = clip(&format!("{path}:{at} {said}"));
            if said.to_lowercase().starts_with("warning") {
                push(&mut warnings, entry);
            } else if bad(said) || test_file(&path) || line.starts_with(|c: char| !c.is_whitespace()) {
                push(&mut out, entry);
            }
            continue;
        }
        if let Some(c) = named.captures(line) {
            let what = c[1].trim();
            // "✖ 2 problems" is a count, not a test.
            let known = out.iter().any(|o| o.ends_with(took.replace(what, "").as_ref()));
            if !known
                && !what.is_empty()
                && !what.ends_with(':')
                && !what.starts_with(|c: char| c == '(' || c.is_ascii_digit())
                && !what.starts_with("errors=")
                && !what.starts_with("failures=")
            {
                push(&mut out, clip(&near(root, what)));
            }
        }
    }
    if out.is_empty() {
        out = warnings;
    }
    out
}

fn c_name<'a>(found: &'a regex::Captures<'a>) -> &'a str {
    found.get(1).map_or("", |m| m.as_str()).trim()
}

/// What is said in place of a failure list when a program the check needs is not there.
fn missing_program(output: &str) -> Option<String> {
    static MODULE: OnceLock<Regex> = OnceLock::new();
    static COMMAND: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&MODULE, r"No module named '?([\w.]+)'?").captures(output) {
        return Some(format!("{} is not installed for this Python", &c[1]));
    }
    let command =
        re(&COMMAND, r"(?m)(?:^|: )([\w.@/-]+): (?:command )?not found|'([\w.@/-]+)' is not recognized as an internal or external command");
    command.captures(output).map(|c| format!("{} is not installed on this computer", c.get(1).or(c.get(2)).map_or("", |m| m.as_str())))
}

/// Reads what a command left behind into the outcome of its check.
pub fn judge(check: &Check, ran: &files::Ran, root: &Path) -> Outcome {
    let output = ran.output.as_str();
    let timed_out = ran.text.starts_with("Stopped after");
    let state = if timed_out {
        State::TimedOut
    } else if ran.ok {
        State::Passed
    } else if let Some(missing) = missing_program(output).filter(|_| output.lines().count() < 12) {
        State::NotRun(missing)
    } else {
        State::Failed
    };
    let failed = matches!(state, State::Failed | State::TimedOut);
    let found = if failed { failures(output, root) } else { Vec::new() };
    let tail = if failed && found.is_empty() {
        let lines: Vec<&str> = output.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
        lines.iter().skip(lines.len().saturating_sub(TAIL_LINES)).map(|l| clip(l)).collect()
    } else {
        Vec::new()
    };
    let counts = matches!(check.kind, Kind::Tests | Kind::Ui).then(|| counts(output)).flatten();
    Outcome { command: check.command.clone(), state, counts, failures: found, tail }
}

/// What the model reads, and whether everything that ran passed.
pub fn report(outcomes: &[Outcome]) -> (String, bool) {
    if outcomes.is_empty() {
        return (
            "Checks: none found. This project has no checks Rigbrew knows how to run. If it has tests, run them with run_command.".into(),
            true,
        );
    }
    let (mut passed, mut failed, mut not_run) = (0, 0, 0);
    for o in outcomes {
        match (&o.state, o.counts) {
            (State::NotRun(_), _) => not_run += 1,
            (_, Some((p, f))) if p + f > 0 => {
                passed += p;
                // A run that failed with no failing test counted (it did not build) still failed.
                failed += if f == 0 && o.state != State::Passed { 1 } else { f };
            }
            (State::Passed, _) => passed += 1,
            _ => failed += 1,
        }
    }
    let mut head =
        if passed + failed == 0 { "Checks: nothing ran".to_string() } else { format!("Checks: {passed} passed, {failed} failed") };
    if not_run > 0 && passed + failed > 0 {
        head.push_str(&format!(", {not_run} not run"));
    }
    let mut lines = vec![head];
    for o in outcomes {
        let counted = o.counts.filter(|(p, f)| p + f > 0).map(|(p, f)| format!(": {p} passed, {f} failed")).unwrap_or_default();
        lines.push(match &o.state {
            State::Passed => format!("ok       {}{counted}", o.command),
            State::Failed => format!("FAILED   {}{counted}", o.command),
            State::TimedOut => format!("FAILED   {}: it did not finish in time and was stopped", o.command),
            State::NotRun(why) => format!("not run  {}: {why}", o.command),
        });
        for line in o.failures.iter().chain(&o.tail) {
            lines.push(format!("  {line}"));
        }
    }
    let ok = failed == 0;
    lines.push(if !ok {
        "Fix what failed, then call check_project again.".into()
    } else if passed == 0 {
        "Nothing could run, so nothing is checked. Say so in your answer; do not install anything unless the person asks.".into()
    } else {
        "Everything that ran passed.".to_string()
    });
    (lines.join("\n"), ok)
}

/// Runs the checks one after the other.
pub async fn run(folder: &Folder, checks: &[Check], net: Net) -> Vec<Outcome> {
    let env = [("CI", "1".to_string()), ("NO_COLOR", "1".to_string()), ("FORCE_COLOR", "0".to_string())];
    let mut out = Vec::new();
    for check in checks {
        if let Some(why) = &check.missing {
            out.push(Outcome {
                command: check.command.clone(),
                state: State::NotRun(why.clone()),
                counts: None,
                failures: Vec::new(),
                tail: Vec::new(),
            });
            continue;
        }
        let ran = files::run_with(folder, &check.command, check.kind.timeout(), net, &env).await;
        out.push(judge(check, &ran, folder.root()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files_in_it: &[(&str, &str)]) -> std::path::PathBuf {
        let root = files::scratch("checks");
        for (path, text) in files_in_it {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        root
    }

    fn found(files_in_it: &[(&str, &str)]) -> Vec<(Kind, String)> {
        detect(&project(files_in_it)).into_iter().map(|c| (c.kind, c.command)).collect()
    }

    fn pairs(list: &[(Kind, &str)]) -> Vec<(Kind, String)> {
        list.iter().map(|(k, c)| (*k, c.to_string())).collect()
    }

    #[test]
    fn the_checks_of_a_project_are_found() {
        use Kind::*;
        assert_eq!(
            found(&[("Cargo.toml", "[package]\n")]),
            pairs(&[(Types, "cargo check --all-targets --message-format short"), (Tests, "cargo test")])
        );
        assert_eq!(found(&[("go.mod", "module x\n")]), pairs(&[(Types, "go vet ./..."), (Tests, "go test ./...")]));
        let web = r#"{"scripts": {"check": "svelte-check", "lint": "eslint .", "test": "vitest", "test:e2e": "playwright test"}}"#;
        assert_eq!(
            found(&[("package.json", web), ("pnpm-lock.yaml", ""), ("playwright.config.ts", "")]),
            pairs(&[(Types, "pnpm run check"), (Lint, "pnpm run lint"), (Tests, "pnpm test"), (Ui, "pnpm run test:e2e")])
        );
        assert_eq!(
            found(&[("package.json", r#"{"scripts": {"dev": "vite"}}"#), ("tsconfig.json", "{}"), ("playwright.config.ts", "")]),
            pairs(&[(Types, "npx --no-install tsc --noEmit"), (Ui, "npx --no-install playwright test --reporter=line")])
        );
        assert_eq!(found(&[("package.json", r#"{"scripts": {"test": "playwright test"}}"#)]), pairs(&[(Ui, "npm test")]));
        assert_eq!(found(&[("package.json", "{}"), ("test/sum.test.mjs", "")]), pairs(&[(Tests, "node --test")]));
        assert_eq!(found(&[("package.json", "{}"), ("cypress.config.js", "")]), pairs(&[(Ui, "npx --no-install cypress run")]));
        let python = if cfg!(windows) { "python" } else { "python3" };
        assert_eq!(
            found(&[("pyproject.toml", "[tool.ruff]\n[tool.mypy]\n[tool.pytest.ini_options]\n")]),
            vec![
                (Lint, "ruff check --output-format concise .".to_string()),
                (Types, "mypy .".to_string()),
                (Tests, format!("{python} -m pytest -q"))
            ]
        );
        assert_eq!(found(&[("test_shop.py", "import unittest\n")]), vec![(Tests, format!("{python} -m unittest"))]);
        let venv = if cfg!(windows) { ".venv/Scripts/python.exe" } else { ".venv/bin/python" };
        assert_eq!(found(&[("tests/test_shop.py", ""), (venv, "")]), vec![(Tests, format!("{venv} -m pytest -q"))]);
        let swift = if cfg!(target_os = "macos") { " --disable-sandbox" } else { "" };
        assert_eq!(
            found(&[("Package.swift", ""), ("Tests/AppTests/A.swift", "")]),
            vec![(Types, format!("swift build{swift}")), (Tests, format!("swift test{swift}"))]
        );
        assert_eq!(
            found(&[
                ("pubspec.yaml", "dependencies:\n  flutter:\n    sdk: flutter\n"),
                ("test/a_test.dart", ""),
                ("integration_test/app_test.dart", "")
            ]),
            pairs(&[(Types, "flutter analyze"), (Tests, "flutter test"), (Ui, "flutter test integration_test")])
        );
        assert_eq!(found(&[("Makefile", "test:\n\ttrue\ncheck:\n\ttrue\n")]), pairs(&[(Tests, "make test"), (Types, "make check")]));
        assert!(found(&[("README.md", "hello\n")]).is_empty());
        // An Xcode project is named, and not run: which scheme and device is not ours to guess.
        let xcode = detect(&project(&[("App.xcodeproj/project.pbxproj", "")]));
        assert_eq!(xcode.len(), 1);
        assert!(xcode[0].kind == Ui && xcode[0].missing.as_ref().unwrap().contains("xcodebuild test -scheme"));
    }

    #[test]
    fn nothing_is_installed_a_missing_runner_is_named() {
        // Packages the project needs and does not have: said, not fetched.
        let root = project(&[
            ("package.json", r#"{"scripts": {"test": "vitest"}, "devDependencies": {"vitest": "^3"}}"#),
            ("playwright.config.ts", ""),
        ]);
        let checks = detect(&root);
        if find_program("npm").is_some() {
            assert_eq!(checks[0].missing.as_deref(), Some("the packages of this project are not installed (run `npm install`)"));
        } else {
            assert_eq!(checks[0].missing.as_deref(), Some("npm is not installed on this computer"));
        }
        assert!(!has_fast(&root));
        std::fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
        let checks = detect(&root);
        if find_program("npm").is_some() {
            assert_eq!(checks[0].missing, None);
            assert_eq!(checks[1].missing.as_deref(), Some("Playwright is not installed in this project (run `npm install`)"));
        }
        assert_eq!(not_installed("no-such-program-rigbrew").unwrap(), "no-such-program-rigbrew is not installed on this computer");
        assert_eq!(wanted("").unwrap(), [Kind::Types, Kind::Lint, Kind::Tests]);
        assert_eq!(wanted("ui").unwrap(), [Kind::Ui]);
        assert_eq!(wanted("all").unwrap().len(), 4);
        assert!(wanted("everything").is_err());
        assert_eq!(missing_program("/usr/bin/python3: No module named pytest").unwrap(), "pytest is not installed for this Python");
        assert_eq!(missing_program("sh: ruff: command not found").unwrap(), "ruff is not installed on this computer");
        assert_eq!(missing_program("sh: 1: eslint: not found").unwrap(), "eslint is not installed on this computer");
        assert_eq!(missing_program("2 tests failed"), None);
    }

    #[test]
    fn the_summary_of_each_runner_is_counted() {
        let cargo = "test result: ok. 12 passed; 0 failed; 0 ignored\n\ntest result: FAILED. 2 passed; 2 failed; 0 ignored\n";
        assert_eq!(counts(cargo), Some((14, 2)));
        assert_eq!(counts("=========== 2 failed, 14 passed in 0.12s ===========\n"), Some((14, 2)));
        assert_eq!(counts("14 passed in 0.03s\n"), Some((14, 0)));
        assert_eq!(counts("1 failed, 3 passed, 1 error in 1.20s\n"), Some((3, 2)));
        assert_eq!(counts("Test Suites: 1 failed, 1 total\nTests:       2 failed, 14 passed, 16 total\n"), Some((14, 2)));
        assert_eq!(counts(" Test Files  1 failed | 3 passed (4)\n      Tests  2 failed | 14 passed (16)\n"), Some((14, 2)));
        assert_eq!(counts("      Tests  16 passed (16)\n"), Some((16, 0)));
        assert_eq!(counts("# tests 16\n# pass 14\n# fail 2\n"), Some((14, 2)));
        assert_eq!(counts("ℹ tests 3\nℹ pass 3\nℹ fail 0\n"), Some((3, 0)));
        assert_eq!(counts("Executed 16 tests, with 2 failures (0 unexpected) in 0.1 seconds\n"), Some((14, 2)));
        assert_eq!(counts("00:03 +14 -2: Some tests failed.\n"), Some((14, 2)));
        assert_eq!(counts("00:03 +16: All tests passed!\n"), Some((16, 0)));
        assert_eq!(counts("Ran 5 tests in 0.001s\n\nFAILED (failures=1, errors=1)\n"), Some((3, 2)));
        assert_eq!(counts("Ran 5 tests in 0.001s\n\nOK\n"), Some((5, 0)));
        assert_eq!(counts("  2 failed\n    [chromium] › a.spec.ts:3:1 › adds\n  14 passed (3.2s)\n"), Some((14, 2)));
        assert_eq!(counts("ok  \texample.com/x\t0.2s\n"), None);
        assert_eq!(counts(""), None);
    }

    #[test]
    fn failures_are_read_with_their_file_and_line() {
        let root = Path::new("/work/shop");
        let read = |output: &str| failures(output, root);
        // The Rust compiler, short and long.
        assert_eq!(
            read("src/main.rs:3:5: error[E0308]: mismatched types\nerror: could not compile `x`\n"),
            ["src/main.rs:3 error[E0308]: mismatched types"]
        );
        assert_eq!(
            read("error[E0425]: cannot find value `y` in this scope\n --> src/lib.rs:7:13\n  |\n7 |     let x = y;\n"),
            ["src/lib.rs:7 error[E0425]: cannot find value `y` in this scope"]
        );
        // A Rust test that panicked.
        let cargo = "---- tests::adds stdout ----\n\nthread 'tests::adds' (123) panicked at src/lib.rs:42:9:\nassertion `left == right` failed\n  left: 3\n right: 4\n\nfailures:\n    tests::adds\n\ntest result: FAILED. 1 passed; 1 failed;\nerror: test failed, to rerun pass `--lib`\n";
        assert_eq!(read(cargo), ["src/lib.rs:42 tests::adds: assertion `left == right` failed"]);
        // TypeScript, both ways it writes a place, and a path made relative to the folder.
        assert_eq!(
            read("src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.\n/work/shop/src/b.ts:9:1 - error TS2304: Cannot find name 'x'.\n"),
            ["src/a.ts:3 error TS2322: Type 'string' is not assignable to type 'number'.", "src/b.ts:9 error TS2304: Cannot find name 'x'."]
        );
        // svelte-check: the place on one line, the error on the next.
        assert_eq!(
            read("/work/shop/src/App.svelte:12:7\nError: 'count' is not defined. (js)\n"),
            ["src/App.svelte:12 Error: 'count' is not defined. (js)"]
        );
        // ESLint's own layout: the file, then the lines under it.
        assert_eq!(
            read("/work/shop/src/cart.js\n  4:7  error  'total' is assigned a value but never used  no-unused-vars\n  9:1  warning  Unexpected console statement  no-console\n\n✖ 2 problems\n"),
            ["src/cart.js:4 'total' is assigned a value but never used  no-unused-vars"]
        );
        // Linters that write one line per finding.
        assert_eq!(read("shop.py:3:1: F401 `os` imported but unused\nFound 1 error.\n"), ["shop.py:3 F401 `os` imported but unused"]);
        assert_eq!(
            read("shop.py:8: error: Incompatible return value type  [return-value]\n"),
            ["shop.py:8 error: Incompatible return value type  [return-value]"]
        );
        assert_eq!(
            read("# example.com/x\n./main.go:7:2: fmt.Printf format %d has arg s of wrong type string\n"),
            ["main.go:7 fmt.Printf format %d has arg s of wrong type string"]
        );
        // Test runners.
        assert_eq!(
            read("tests/test_shop.py:12: AssertionError\n=== short test summary info ===\nFAILED tests/test_shop.py::test_total - assert 3 == 4\n"),
            ["tests/test_shop.py:12 AssertionError", "tests/test_shop.py::test_total - assert 3 == 4"]
        );
        assert_eq!(
            read("--- FAIL: TestTotal (0.00s)\n    shop_test.go:12: got 3, want 4\nFAIL\n"),
            ["TestTotal (0.00s)", "shop_test.go:12 got 3, want 4"]
        );
        assert_eq!(
            read(" FAIL  src/sum.test.ts > adds two numbers\nAssertionError: expected 3 to be 4\n ❯ src/sum.test.ts:5:18\n"),
            ["src/sum.test.ts > adds two numbers"]
        );
        assert_eq!(read("  ● cart › adds an item\n\n    expect(received).toBe(expected)\n"), ["cart › adds an item"]);
        assert_eq!(read("not ok 2 - totals the cart\n  ---\n  duration_ms: 0.4\n"), ["totals the cart"]);
        // Node's runner names a failed test twice, the second time with its place: one line.
        let node = "✖ adds (2.67ms)\n✔ ok (0.29ms)\nℹ fail 1\n\n✖ failing tests:\n\ntest at sum.test.mjs:3:1\n✖ adds (2.67ms)\n  AssertionError [ERR_ASSERTION]: 3 == 4\n      at TestContext.<anonymous> (file:///work/shop/sum.test.mjs:3:27)\n";
        assert_eq!(read(node), ["sum.test.mjs:3 adds"]);
        assert_eq!(read("/work/shop/Tests/CartTests.swift:12: error: -[CartTests testTotal] : XCTAssertEqual failed: (\"3\") is not equal to (\"4\")\n").len(), 1);
        // Colours are not part of a path, the list is short, and a clean log has no failures.
        assert_eq!(read("\x1b[31msrc/a.ts:1:1 - error TS1: bad\x1b[0m\n"), ["src/a.ts:1 error TS1: bad"]);
        let many: String = (1..40).map(|i| format!("src/a.ts:{i}:1 - error TS1: bad\n")).collect();
        assert_eq!(read(&many).len(), MAX_FAILURES);
        assert!(read("Compiling x v0.1.0\nFinished in 3.2s\nrunning 4 tests\ntest a ... ok\n").is_empty());
        // Only warnings: they are shown when there is nothing else to show.
        assert_eq!(read("src/a.rs:1:1: warning: unused import\n"), ["src/a.rs:1 warning: unused import"]);
    }

    fn ran(ok: bool, output: &str) -> files::Ran {
        files::Ran { text: format!("exit code {} after 0.1 s\n{output}", if ok { 0 } else { 1 }), ok, output: output.to_string() }
    }

    #[test]
    fn the_answer_is_short_and_says_what_ran() {
        let root = Path::new("/work/shop");
        let check = |kind: Kind, command: &str| Check { kind, command: command.into(), missing: None };
        let types = judge(&check(Kind::Types, "cargo check"), &ran(true, "Finished\n"), root);
        let tests = judge(
            &check(Kind::Tests, "cargo test"),
            &ran(false, "thread 'tests::adds' panicked at src/lib.rs:42:9:\nleft != right\n\ntest result: FAILED. 14 passed; 2 failed;\n"),
            root,
        );
        let ui = Outcome {
            command: "npx --no-install playwright test --reporter=line".into(),
            state: State::NotRun("Playwright is not installed in this project (run `npm install`)".into()),
            counts: None,
            failures: vec![],
            tail: vec![],
        };
        let (text, ok) = report(&[types, tests, ui]);
        assert!(!ok);
        assert_eq!(
            text,
            "Checks: 15 passed, 2 failed, 1 not run\n\
             ok       cargo check\n\
             FAILED   cargo test: 14 passed, 2 failed\n  src/lib.rs:42 tests::adds: left != right\n\
             not run  npx --no-install playwright test --reporter=line: Playwright is not installed in this project (run `npm install`)\n\
             Fix what failed, then call check_project again."
        );
        // A check that failed without a failure that can be read: the end of its output.
        let odd = judge(&check(Kind::Lint, "npm run lint"), &ran(false, "one\n\ntwo\nsomething odd happened\n"), root);
        assert_eq!(odd.tail, ["one", "two", "something odd happened"]);
        let (text, ok) = report(&[odd]);
        assert!(!ok && text.starts_with("Checks: 0 passed, 1 failed\nFAILED   npm run lint\n  one\n"), "{text}");
        // Tests that did not even build count as one failure.
        let broken =
            judge(&check(Kind::Tests, "cargo test"), &ran(false, "error[E0425]: cannot find value `y`\n --> src/lib.rs:7:13\n"), root);
        assert!(report(&[broken]).0.starts_with("Checks: 0 passed, 1 failed\nFAILED   cargo test\n  src/lib.rs:7 error[E0425]"));
        // A program that is not there is "not run", not a failure of the project.
        let no_pytest = judge(&check(Kind::Tests, "python3 -m pytest -q"), &ran(false, "/usr/bin/python3: No module named pytest\n"), root);
        assert_eq!(no_pytest.state, State::NotRun("pytest is not installed for this Python".into()));
        let (text, ok) = report(&[no_pytest]);
        assert!(
            ok && text.starts_with("Checks: nothing ran\nnot run  python3 -m pytest -q") && text.contains("nothing is checked"),
            "{text}"
        );
        let all = judge(&check(Kind::Tests, "npm test"), &ran(true, "      Tests  16 passed (16)\n"), root);
        assert_eq!(
            report(&[all]),
            ("Checks: 16 passed, 0 failed\nok       npm test: 16 passed, 0 failed\nEverything that ran passed.".into(), true)
        );
        assert!(report(&[]).0.starts_with("Checks: none found."));
        let slow =
            files::Ran { text: "Stopped after 300 seconds without finishing.\n(no output)".into(), ok: false, output: String::new() };
        assert!(report(&[judge(&check(Kind::Tests, "npm test"), &slow, root)]).0.contains("FAILED   npm test: it did not finish in time"));
    }

    /// A real project with a real failing test, run through the real runner.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_small_project_is_checked_for_real() {
        if find_program("python3").is_none() {
            return;
        }
        let root = project(&[
            ("shop.py", "def total(prices):\n    return sum(prices) + 1\n"),
            (
                "test_shop.py",
                "import unittest\nfrom shop import total\n\nclass TestShop(unittest.TestCase):\n    def test_total(self):\n        self.assertEqual(total([1, 2]), 3)\n\n    def test_empty(self):\n        self.assertEqual(total([]), 1)\n\nif __name__ == '__main__':\n    unittest.main()\n",
            ),
        ]);
        let folder = Folder::open(root.to_str().unwrap()).unwrap();
        let checks = chosen(&root, wanted("fast").unwrap());
        assert_eq!(checks.len(), 1);
        let (text, ok) = report(&run(&folder, &checks, Net::default()).await);
        assert!(!ok, "{text}");
        assert!(text.starts_with("Checks: 1 passed, 1 failed\nFAILED   python3 -m unittest: 1 passed, 1 failed\n"), "{text}");
        assert!(text.contains("test_shop.py\", line 6") || text.contains("test_shop.py:6") || text.contains("test_total"), "{text}");
        std::fs::write(root.join("shop.py"), "def total(prices):\n    return sum(prices)\n").unwrap();
        std::fs::write(
            root.join("test_shop.py"),
            std::fs::read_to_string(root.join("test_shop.py")).unwrap().replace("total([]), 1", "total([]), 0"),
        )
        .unwrap();
        let (text, ok) = report(&run(&folder, &checks, Net::default()).await);
        assert!(ok && text.starts_with("Checks: 2 passed, 0 failed\nok       python3 -m unittest: 2 passed, 0 failed"), "{text}");
    }
}
