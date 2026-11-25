//! Recall: how a long task outlives the model's context window. The conversation is sent whole
//! while it fits. When it does not, the oldest part is taken out and what it held is kept as a
//! short running note: the files read and what was in them, the changes made, the commands run
//! and how they ended, what the person asked along the way. The note is written by the service
//! from the calls and their results, with no extra request to the model.
//!
//! The first request of the person is kept word for word and always sent first; the plan
//! travels in the system message (see `extra::context`), so neither is ever lost.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How a message starts that the service wrote for the model in the person's place (a hint
/// after a cut-off call, a reminder that the plan is not done). Such a message is not something
/// the person asked, and is not kept in the notes.
pub const FROM_SERVICE: &str = "[Rigbrew] ";

/// A message for the model from the service itself.
pub fn hint(text: &str) -> Value {
    json!({"role": "user", "content": format!("{FROM_SERVICE}{text}")})
}

const NOTES_HEAD: &str =
    "[Notes Rigbrew kept of the work so far. The older steps were left out to save space. Read a file again if you need its text.]";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    /// A later message of the person.
    Asked,
    /// An answer the model gave to an earlier message.
    Said,
    Read,
    /// A listing, a search in the folder, an outline.
    Looked,
    Web,
    Changed,
    Ran,
    Did,
}

/// Which notes go first when there are too many, most expendable first. What the person asked
/// is never dropped.
const EXPENDABLE: &[Kind] = &[Kind::Looked, Kind::Web, Kind::Read, Kind::Did];
/// Messages of the person that are always kept, however many notes there are.
const ASKED_KEPT: usize = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Note {
    kind: Kind,
    /// What the note is about (a path, a command, a query): a newer note about the same thing
    /// takes the place of the older one.
    key: String,
    text: String,
    #[serde(default)]
    times: usize,
}

/// What is kept of the part of a task's conversation that is no longer sent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    /// The first request of the person, word for word.
    first: String,
    notes: Vec<Note>,
    /// Notes that were dropped because there were too many.
    dropped: usize,
}

fn cut(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    format!("{}…", flat.chars().take(max.saturating_sub(1)).collect::<String>())
}

fn first_line(text: &str) -> &str {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

fn last_line(text: &str) -> &str {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("")
}

/// What a piece of source defines, as a few names; for other text its first line.
fn gist(text: &str) -> String {
    static DEFINES: OnceLock<Regex> = OnceLock::new();
    let defines = DEFINES.get_or_init(|| {
        Regex::new(r"\b(?:fn|def|class|function|struct|enum|trait|interface|func|impl|type)\s+([A-Za-z_][A-Za-z0-9_]*)")
            .expect("static pattern")
    });
    let mut names: Vec<&str> = Vec::new();
    for found in defines.captures_iter(text) {
        let name = found.get(1).map_or("", |m| m.as_str());
        if !names.contains(&name) {
            names.push(name);
        }
        if names.len() == 8 {
            break;
        }
    }
    if names.is_empty() {
        cut(first_line(text), 90)
    } else {
        format!("defines {}", names.join(", "))
    }
}

/// One call and what it gave, as a line to keep. `None` for what needs no note (the plan is
/// sent on every turn anyway).
fn note_of(tool: &str, args: &Value, result: &str) -> Option<Note> {
    let arg = |key: &str| args[key].as_str().unwrap_or("").trim();
    let (head, body) = result.split_once('\n').unwrap_or((result, ""));
    let note = |kind, key: &str, text: String| Some(Note { kind, key: key.to_string(), text, times: 1 });
    match tool {
        "update_plan" | "finish" => None,
        "read_file" => {
            let path = arg("path");
            match head.find("(lines ").and_then(|at| head[at..].find(')').map(|end| &head[at..at + end + 1])) {
                Some(lines) => note(Kind::Read, path, format!("read {path} {lines}: {}", gist(body))),
                None => note(Kind::Read, path, format!("tried to read {path}: {}", cut(head, 120))),
            }
        }
        "list_files" => {
            let path = if arg("path").is_empty() { "." } else { arg("path") };
            let names: Vec<&str> = body
                .lines()
                .filter(|l| !l.starts_with(' ') && !l.starts_with('['))
                .map(|l| l.split(" (").next().unwrap_or(l))
                .take(10)
                .collect();
            let text = if names.is_empty() { cut(head, 120) } else { cut(&names.join(", "), 200) };
            note(Kind::Looked, &format!("list {path}"), format!("listed {path}: {text}"))
        }
        "search" => {
            let pattern = arg("pattern");
            let hits = result.lines().filter(|l| !l.starts_with('[') && l.contains(':')).count();
            let text = if hits == 0 || head.starts_with("Nothing") {
                "nothing found".to_string()
            } else {
                format!("{hits} lines, the first {}", cut(head, 100))
            };
            note(Kind::Looked, &format!("search {pattern}"), format!("searched for \"{}\": {text}", cut(pattern, 60)))
        }
        "outline" => note(Kind::Looked, &format!("outline {}", arg("path")), format!("outlined {}: {}", arg("path"), gist(result))),
        "web_search" => {
            let query = arg("query");
            // Results come as "1. Title", then the address, then a line of text.
            let lines: Vec<&str> = result.lines().map(str::trim).collect();
            let found: Vec<String> = lines
                .windows(2)
                .filter(|w| w[0].split_once(". ").is_some_and(|(n, _)| n.parse::<u8>().is_ok()) && w[1].starts_with("http"))
                .take(3)
                .map(|w| format!("{} ({})", cut(w[0].split_once(". ").map_or(w[0], |(_, t)| t), 60), w[1]))
                .collect();
            let text = if found.is_empty() { cut(head, 120) } else { found.join("; ") };
            note(
                Kind::Web,
                &format!("search {}", super::pace::query_key(query)),
                format!("searched the web for \"{}\": {text}", cut(query, 60)),
            )
        }
        "fetch_url" => {
            let url = arg("url");
            let text = if head.starts_with("http") { cut(first_line(body), 140) } else { cut(head, 140) };
            note(Kind::Web, &format!("page {}", super::pace::url_key(url)), format!("opened {url}: {text}"))
        }
        "edit_file" | "write_file" | "append_file" => {
            let path = arg("path");
            note(Kind::Changed, path, format!("{} {path}: {}", tool.trim_end_matches("_file"), cut(head, 120)))
        }
        "run_command" => {
            let command = arg("command");
            let (first, last) = (first_line(result), last_line(result));
            let ended = if first == last { cut(first, 160) } else { format!("{}; {}", cut(first, 60), cut(last, 140)) };
            note(Kind::Ran, command, format!("ran `{}`: {ended}", cut(command, 100)))
        }
        "use_skill" => note(
            Kind::Did,
            &format!("skill {}", arg("name")),
            format!("read the skill {} (call use_skill again for its text)", arg("name")),
        ),
        other => note(Kind::Did, &format!("{other} {args}"), format!("{other} {}: {}", cut(&args.to_string(), 80), cut(head, 100))),
    }
}

impl Memory {
    pub fn is_empty(&self) -> bool {
        self.first.is_empty() && self.notes.is_empty()
    }

    fn weight(&self) -> usize {
        self.notes.iter().map(|n| n.text.chars().count() + 4).sum()
    }

    fn keep(&mut self, note: Note) {
        let times =
            match self.notes.iter().position(|n| n.kind == note.kind && n.key == note.key && n.kind != Kind::Asked && n.kind != Kind::Said)
            {
                Some(old) => self.notes.remove(old).times + 1,
                None => 1,
            };
        self.notes.push(Note { times, ..note });
    }

    /// Drops the least needed notes, oldest first, until the rest is under `max` characters.
    fn trim(&mut self, max: usize) {
        let mut drop_oldest = |notes: &mut Vec<Note>, which: &dyn Fn(&Note) -> bool| match notes.iter().position(which) {
            Some(oldest) => {
                notes.remove(oldest);
                self.dropped += 1;
                true
            }
            None => false,
        };
        let weight = |notes: &[Note]| notes.iter().map(|n| n.text.chars().count() + 4).sum::<usize>();
        let mut notes = std::mem::take(&mut self.notes);
        // Look-ups first, then the oldest of what was changed, run or said, and only then the
        // oldest of what the person asked, of which the newest few always stay.
        while weight(&notes) > max && drop_oldest(&mut notes, &|n| EXPENDABLE.contains(&n.kind)) {}
        while weight(&notes) > max && drop_oldest(&mut notes, &|n| n.kind != Kind::Asked) {}
        while weight(&notes) > max && notes.len() > ASKED_KEPT && drop_oldest(&mut notes, &|_| true) {}
        self.notes = notes;
    }

    /// Takes messages that are no longer sent and keeps what they held.
    fn absorb(&mut self, gone: &[Value]) {
        for (i, message) in gone.iter().enumerate() {
            let content = message["content"].as_str().unwrap_or("").trim();
            if message["role"] == "user" {
                if content.starts_with(FROM_SERVICE) {
                    continue;
                }
                if self.is_empty() {
                    self.first = content.to_string();
                } else {
                    self.notes.push(Note {
                        kind: Kind::Asked,
                        key: String::new(),
                        text: format!("then the person asked: {}", cut(content, 600)),
                        times: 1,
                    });
                }
                continue;
            }
            if message["role"] != "assistant" {
                continue;
            }
            let Some(calls) = message["tool_calls"].as_array().filter(|c| !c.is_empty()) else {
                if !content.is_empty() {
                    self.notes.push(Note {
                        kind: Kind::Said,
                        key: String::new(),
                        text: format!("you answered: {}", cut(content, 240)),
                        times: 1,
                    });
                }
                continue;
            };
            for call in calls {
                let result = gone[i + 1..]
                    .iter()
                    .find(|m| m["role"] == "tool" && m["tool_call_id"] == call["id"])
                    .and_then(|m| m["content"].as_str())
                    .unwrap_or("");
                let args: Value =
                    call["function"]["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or_else(|| json!({}));
                if let Some(note) = note_of(call["function"]["name"].as_str().unwrap_or(""), &args, result) {
                    self.keep(note);
                }
            }
        }
    }

    /// The first request and the notes, as one text for the model.
    pub fn text(&self) -> String {
        let mut out = self.first.clone();
        if self.notes.is_empty() && self.dropped == 0 {
            return out;
        }
        out.push_str("\n\n");
        out.push_str(NOTES_HEAD);
        if self.dropped > 0 {
            out.push_str(&format!("\n- ({} older notes were dropped)", self.dropped));
        }
        for note in &self.notes {
            out.push_str("\n- ");
            out.push_str(&note.text);
            if note.times > 1 {
                out.push_str(&format!(" ({} times; this was the last)", note.times));
            }
        }
        out
    }
}

/// When the conversation is longer than `budget` characters: the oldest messages leave it and
/// become notes, until what is left is under three quarters of the budget: enough for the
/// model to still have the last two files it read in front of it. The newest exchange always
/// stays, and a call is never parted from its result. Says whether anything was taken out.
pub fn compact(messages: &mut Vec<Value>, memory: &mut Memory, budget: usize) -> bool {
    let total: usize = messages.iter().map(super::weight).sum::<usize>() + memory.weight();
    if total <= budget {
        return false;
    }
    // From the newest message back: the earliest place to cut so that what stays still fits.
    let (mut from, mut held, room) = (messages.len(), 0, budget / 4 * 3);
    for (i, message) in messages.iter().enumerate().rev() {
        held += super::weight(message);
        // A result belongs to the call before it: never cut between the two.
        if message["role"] != "tool" && (held <= room || from == messages.len()) {
            from = i;
        }
        if held > room && from != messages.len() {
            break;
        }
    }
    if from == 0 || from == messages.len() {
        return false;
    }
    let gone: Vec<Value> = messages.drain(..from).collect();
    memory.absorb(&gone);
    memory.trim(budget / 5);
    true
}

/// The conversation as it is sent: the first request with the notes, then the messages that
/// are still kept whole.
pub fn conversation(memory: &Memory, messages: &[Value]) -> Vec<Value> {
    if memory.is_empty() {
        return messages.to_vec();
    }
    let mut out = Vec::with_capacity(messages.len() + 1);
    let mut rest = messages;
    match messages.first() {
        // Two messages of the person in a row are sent as one.
        Some(next) if next["role"] == "user" => {
            let said = next["content"].as_str().unwrap_or("");
            let said =
                said.strip_prefix(FROM_SERVICE).map_or_else(|| format!("[The person's next message]\n{said}"), |hint| format!("[{hint}]"));
            out.push(json!({"role": "user", "content": format!("{}\n\n{said}", memory.text())}));
            rest = &messages[1..];
        }
        _ => out.push(json!({"role": "user", "content": memory.text()})),
    }
    out.extend_from_slice(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str, args: Value) -> Value {
        json!({"role": "assistant", "content": "", "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}]})
    }

    fn result(id: &str, content: &str) -> Value {
        json!({"role": "tool", "tool_call_id": id, "content": content})
    }

    fn user(text: &str) -> Value {
        json!({"role": "user", "content": text})
    }

    /// A long task: a request, then reads, changes and test runs, each with a result.
    fn long_task(rounds: usize) -> Vec<Value> {
        let mut messages = vec![user("Build a small calculator with tests.")];
        for n in 0..rounds {
            let filler = format!("    # line of file {n}\n").repeat(50);
            messages.push(call(&format!("r{n}"), "read_file", json!({"path": format!("src/mod{n}.py")})));
            messages.push(result(
                &format!("r{n}"),
                &format!("src/mod{n}.py (lines 1 to 40 of 40)\ndef add{n}(a, b):\n    return a + b\nclass Stack{n}:\n{filler}"),
            ));
            messages.push(call(&format!("w{n}"), "write_file", json!({"path": format!("tests/test_{n}.py"), "content": filler})));
            messages.push(result(&format!("w{n}"), &format!("Created tests/test_{n}.py (30 lines).\nCheck it: python3 -m pytest")));
            messages.push(call(&format!("c{n}"), "run_command", json!({"command": "python3 -m pytest"})));
            messages.push(result(&format!("c{n}"), &format!("exit code 0 after 0.4 s\n....\n{} passed in 0.1{n}s", n + 1)));
        }
        messages
    }

    #[test]
    fn a_conversation_that_fits_is_left_alone() {
        let mut messages = long_task(1);
        let before = messages.clone();
        let mut memory = Memory::default();
        assert!(!compact(&mut messages, &mut memory, 12_000));
        assert_eq!(messages, before);
        assert!(memory.is_empty());
        assert_eq!(conversation(&memory, &messages), before);
    }

    #[test]
    fn what_is_cut_becomes_notes_and_the_first_request_stays_word_for_word() {
        let mut messages = long_task(12);
        let whole: usize = messages.iter().map(super::super::weight).sum();
        assert!(whole > 24_000, "the task is much longer than the budget: {whole}");
        let mut memory = Memory::default();
        assert!(compact(&mut messages, &mut memory, 12_000));
        // What stays is the newest part, starts with a call, and every result has its call.
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages.last().unwrap()["tool_call_id"], "c11");
        let kept: usize = messages.iter().map(super::super::weight).sum();
        assert!(kept <= 9_000, "{kept}");
        let sent = conversation(&memory, &messages);
        assert_eq!(sent.len(), messages.len() + 1);
        let note = sent[0]["content"].as_str().unwrap();
        assert!(note.starts_with("Build a small calculator with tests.\n\n[Notes Rigbrew kept"), "{note}");
        // Files read and what they held, changes made, commands run and how they ended.
        assert!(note.contains("- read src/mod0.py (lines 1 to 40 of 40): defines add0, Stack0"), "{note}");
        assert!(note.contains("- write tests/test_0.py: Created tests/test_0.py (30 lines)."), "{note}");
        assert!(note.contains("- ran `python3 -m pytest`: exit code 0 after 0.4 s;"), "{note}");
        assert!(note.contains("passed in"));
        // The same command run many times is one note that says so.
        assert_eq!(note.matches("ran `python3 -m pytest`").count(), 1);
        assert!(note.contains("times; this was the last)"));
        assert!(note.chars().count() < 3_600, "the notes stay short: {}", note.chars().count());
        let total: usize = sent.iter().map(super::super::weight).sum();
        assert!(total <= 12_000, "{total}");
    }

    #[test]
    fn the_notes_roll_on_and_stay_small_over_a_very_long_task() {
        let all = long_task(120);
        let (mut messages, mut memory) = (vec![all[0].clone()], Memory::default());
        let mut cuts = 0;
        // The loop's order: a call and its result arrive, then the history is fitted.
        for pair in all[1..].chunks(2) {
            messages.extend_from_slice(pair);
            if compact(&mut messages, &mut memory, 12_000) {
                cuts += 1;
            }
            let sent = conversation(&memory, &messages);
            let total: usize = sent.iter().map(super::super::weight).sum();
            assert!(total <= 12_000 + 4_000, "what is sent stays near the budget: {total}");
            assert!(sent[0]["content"].as_str().unwrap().starts_with("Build a small calculator with tests."));
        }
        assert!(cuts > 20, "{cuts}");
        assert!(memory.weight() <= 2_400, "{}", memory.weight());
        assert!(memory.dropped > 0);
        let note = memory.text();
        // The newest changes are in the notes; the oldest reads made room.
        assert!(note.contains("older notes were dropped"));
        assert!(!note.contains("read src/mod0.py"));
        assert!(note.contains("tests/test_11"), "{note}");
        assert!(note.contains("ran `python3 -m pytest` (") || note.contains("ran `python3 -m pytest`:"));
    }

    #[test]
    fn later_messages_of_the_person_are_kept_and_service_hints_are_not() {
        let long = "x".repeat(2_000);
        let mut messages = vec![
            user("Why does the test fail?"),
            call("a", "search", json!({"pattern": "assert"})),
            result("a", &format!("tests/a.py:3: assert add(1, 2) == 4\ntests/b.py:9: assert True\n{long}")),
            json!({"role": "assistant", "content": "The test expects 4 where the sum is 3."}),
            user("Fix it then."),
            hint("Your last tool call was cut off."),
            call("b", "edit_file", json!({"path": "tests/a.py", "old": "4", "new": "3"})),
            result("b", "Changed tests/a.py (1 line).\nCheck it: python3 -m pytest"),
            call("c", "web_search", json!({"query": "pytest assert"})),
            result(
                "c",
                &format!("1. pytest docs\n   https://docs.pytest.org/\n   How to assert.\n2. Other\n   https://example.com/\n   {long}"),
            ),
            call("d", "list_files", json!({})),
            result("d", &format!("./\nsrc/\n{}tests/\nREADME.md (2 KB)", "  a.py (1 KB)\n".repeat(150))),
            user("And now the README."),
            call("e", "read_file", json!({"path": "README.md"})),
            result("e", &format!("README.md (lines 1 to 3 of 3)\n# Calc\n{long}")),
        ];
        let mut memory = Memory::default();
        assert!(compact(&mut messages, &mut memory, 5_000));
        assert_eq!(messages[0]["content"], "And now the README.");
        let sent = conversation(&memory, &messages);
        // The notes and the newest message of the person travel as one message.
        assert_eq!(sent.len(), messages.len());
        let note = sent[0]["content"].as_str().unwrap();
        assert!(note.starts_with("Why does the test fail?"));
        assert!(note.ends_with("[The person's next message]\nAnd now the README."), "{note}");
        let order = [
            "searched for \"assert\": 2 lines, the first tests/a.py:3",
            "you answered: The test expects 4",
            "then the person asked: Fix it then.",
            "edit tests/a.py: Changed tests/a.py (1 line).",
            "searched the web for \"pytest assert\": pytest docs (https://docs.pytest.org/); Other (https://example.com/)",
            "listed .: src/, tests/, README.md",
        ];
        let mut at = 0;
        for line in order {
            let found = note[at..].find(line).unwrap_or_else(|| panic!("{line} is missing or out of order in:\n{note}"));
            at += found;
        }
        assert!(!note.contains("cut off"), "a hint of the service is not something the person asked");
        assert_eq!(sent[1]["tool_calls"][0]["id"], "e");
    }

    #[test]
    fn one_exchange_too_large_for_the_budget_is_still_kept_whole() {
        let huge = "y".repeat(9_000);
        let mut messages = vec![
            user("go"),
            call("a", "read_file", json!({"path": "big.txt"})),
            result("a", &format!("big.txt (lines 1 to 9 of 9)\n{huge}")),
        ];
        let mut memory = Memory::default();
        assert!(compact(&mut messages, &mut memory, 4_000));
        assert_eq!(messages.len(), 2);
        assert_eq!(memory.first, "go");
        // Nothing more can go: the last call and its result stay together.
        assert!(!compact(&mut messages, &mut memory, 4_000));
        assert_eq!(conversation(&memory, &messages)[0]["content"], "go");
    }

    #[test]
    fn what_is_kept_reads_back() {
        let mut messages = long_task(12);
        let mut memory = Memory::default();
        compact(&mut messages, &mut memory, 12_000);
        let back: Memory = serde_json::from_str(&serde_json::to_string(&memory).unwrap()).unwrap();
        assert_eq!(back, memory);
        assert!(serde_json::from_str::<Memory>("{}").unwrap().is_empty());
    }
}
