//! Pace: what keeps a small model from going round in circles, without ever ending its task
//! for it. Kept with the task and asked before and after every tool call.
//!
//! Three things live here:
//!
//! - **The web.** A search the task already made (the same words in any order, case or
//!   punctuation) is answered from memory, and so is a page it already opened. One request from
//!   the person gets `SEARCH_BUDGET` searches and `FETCH_BUDGET` pages; after that the tool
//!   answers that enough was searched, and it is no longer offered to the model at all. A few
//!   searches in a row carry a line that says to read a page or answer. Two searches in a row
//!   that the engine did not answer switch the search off for the request.
//! - **Runs of look-ups.** Reading, listing and searching again and again with nothing changed,
//!   run or answered in between.
//! - **The ladder.** The same call with the same result a third time, a long run of look-ups, or
//!   one tool failing again and again is a sign of being stuck. The first signs add a short
//!   nudge to the result, the next ones tell the model to take stock and change approach, and
//!   only the sixth with nothing new changed or run in between becomes one question for the
//!   person: the task waits for their answer, it is not ended. Tests that fail, a fix, and
//!   tests that fail in another way are work, not a sign.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Web searches one request from the person may send.
pub const SEARCH_BUDGET: usize = 6;
/// Pages one request from the person may open.
pub const FETCH_BUDGET: usize = 12;
/// Web searches in a row, with no other kind of step, before the result says to read a page.
const WEB_RUN: usize = 3;
/// Searches in a row that the engine did not answer before the search is switched off.
const SEARCH_FAILS: usize = 2;
/// Look-ups in a row, with nothing changed or run, that count as a sign of being stuck.
const LOOKUP_RUN: usize = 12;
/// How often the same call may give the same result before it is a sign of being stuck.
const SAME_OUTCOME: u8 = 3;
/// Signs of being stuck, with no progress between them, before the model is told to change
/// approach, and before the person is asked.
const RETHINK_AT: u8 = 3;
const ASK_AT: u8 = 6;
/// Failures of one tool in a row that count as a sign of being stuck.
const FAIL_RUN: usize = 3;
/// The same call this many times in a row is not run again.
const SAME_CALL: usize = 3;
/// Search results and pages kept to answer a repeat from memory.
const MAX_REMEMBERED: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Reads something in the folder or a skill; changes nothing.
    Lookup,
    /// A web search.
    Search,
    /// One web page.
    Page,
    /// Writes to a file.
    Change,
    /// A shell command.
    Command,
    /// The plan: neither a look-up nor progress in the folder.
    Plan,
    Other,
}

pub fn kind_of(tool: &str) -> Kind {
    match tool {
        "list_files" | "read_file" | "search" | "outline" | "use_skill" | "read_skill_file" | "find_tool" => Kind::Lookup,
        "web_search" => Kind::Search,
        "fetch_url" => Kind::Page,
        "edit_file" | "write_file" | "append_file" => Kind::Change,
        "run_command" => Kind::Command,
        "update_plan" => Kind::Plan,
        _ => Kind::Other,
    }
}

/// Words that do not make one search different from another.
const SMALL_WORDS: &[&str] = &["a", "an", "the", "of", "for", "in", "on", "to", "and", "or", "with", "how", "what", "is"];

/// What makes two searches the same search: the words, whatever their case, order or the
/// punctuation around them.
pub fn query_key(query: &str) -> String {
    let lower = query.to_lowercase();
    let mut words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| !SMALL_WORDS.contains(w)) {
        words.retain(|w| !SMALL_WORDS.contains(w));
    }
    words.sort_unstable();
    words.dedup();
    words.join(" ")
}

/// What makes two addresses the same page: no fragment, no slash at the end, http or https,
/// with or without "www.".
pub fn url_key(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let no_fragment = lower.split('#').next().unwrap_or("");
    let no_scheme = no_fragment.strip_prefix("https://").or_else(|| no_fragment.strip_prefix("http://")).unwrap_or(no_fragment);
    no_scheme.strip_prefix("www.").unwrap_or(no_scheme).trim_end_matches('/').to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Remembered {
    key: String,
    result: String,
}

/// An answer the service gives in place of running the tool.
#[derive(Debug, PartialEq)]
pub struct Held {
    pub text: String,
    /// The call asked for something it will not get (over the budget, the same call again):
    /// counted like a failure of that tool. A remembered result is not.
    pub refused: bool,
}

/// What a tool call led to, beyond its result.
#[derive(Debug, Default, PartialEq)]
pub struct Verdict {
    /// A line for the model, added to the end of the result.
    pub nudge: Option<String>,
    /// A question for the person: the request ends here and the task waits for their answer.
    pub ask: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sign {
    Same,
    Lookups,
    Fails,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pace {
    /// Web searches sent and pages opened for the current request.
    searches: usize,
    fetches: usize,
    /// Web searches in a row with no other kind of step between them.
    web_run: usize,
    /// Look-ups in a row with nothing changed or run between them.
    lookup_run: usize,
    last_call: String,
    same_calls: usize,
    failing: String,
    fail_run: usize,
    /// Searches in a row that came back as an error, and whether that switched the search off.
    search_fails: usize,
    search_off: bool,
    /// How often each look-up gave the same result since something last changed.
    looked: HashMap<String, u8>,
    /// How often each change or command gave the same result in the current request.
    done: HashMap<String, u8>,
    /// Signs of being stuck since the last progress.
    signs: u8,
    /// Search results and pages of this task, newest last.
    remembered: Vec<Remembered>,
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let cut: String = line.chars().take(max).collect();
    if cut.len() < line.len() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// A step in a few plain words, for the question to the person.
fn what(tool: &str, args: &Value) -> String {
    let arg = |key: &str| first_line(args[key].as_str().unwrap_or(""), 80);
    match tool {
        "run_command" => format!("the command `{}`", arg("command")),
        "edit_file" | "write_file" | "append_file" => format!("the change to {}", arg("path")),
        "read_file" => format!("reading {}", arg("path")),
        "search" => format!("the search for \"{}\"", arg("pattern")),
        "list_files" | "outline" => "looking through the folder".to_string(),
        "web_search" => "searching the web".to_string(),
        "fetch_url" => "opening a web page".to_string(),
        other => format!("the step {other}"),
    }
}

const RETHINK: &str = "[You are going round in circles. Change approach now: call update_plan with what is done and what is left, \
then take the next step in a different way. If nothing is left to do, call finish and say what you found and what you could not do.]";

impl Pace {
    /// A new message from the person: the budgets and the ladder start again. What was already
    /// searched stays remembered for the whole task.
    pub fn next_request(&mut self) {
        let remembered = std::mem::take(&mut self.remembered);
        *self = Pace { remembered, ..Default::default() };
    }

    /// Web searches sent for the current request.
    pub fn searches(&self) -> usize {
        self.searches
    }

    /// Whether a tool is closed for the rest of the request: it is then not offered to the
    /// model at all, which holds better than any answer it could be given.
    pub fn closed(&self, tool: &str) -> bool {
        match tool {
            "web_search" => self.search_off || self.searches >= SEARCH_BUDGET,
            "fetch_url" => self.fetches >= FETCH_BUDGET,
            _ => false,
        }
    }

    /// What the model is told, at the end of the system message, about a tool that is closed.
    pub fn note(&self, web: bool) -> String {
        let mut out = String::new();
        if !web {
            return out;
        }
        if self.search_off {
            out.push_str("\n\nweb_search gives no results right now and is switched off for this request. Open a page you know with fetch_url (an official site or its documentation), or answer and say what you could not check.");
        } else if self.searches >= SEARCH_BUDGET {
            out.push_str(&format!("\n\nweb_search is used up for this request ({SEARCH_BUDGET} searches). Work with what you found: open one of the pages with fetch_url, or answer."));
        }
        if self.fetches >= FETCH_BUDGET {
            out.push_str("\n\nfetch_url is used up for this request. Work with what you have read and answer.");
        }
        out
    }

    fn recall(&self, key: &str) -> Option<&str> {
        self.remembered.iter().find(|r| r.key == key).map(|r| r.result.as_str())
    }

    fn remember(&mut self, key: String, result: &str) {
        self.remembered.retain(|r| r.key != key);
        self.remembered.push(Remembered { key, result: result.to_string() });
        if self.remembered.len() > MAX_REMEMBERED {
            self.remembered.remove(0);
        }
    }

    /// The key a web call is remembered under, if it is one.
    fn web_key(tool: &str, args: &Value) -> Option<String> {
        match tool {
            "web_search" => args["query"].as_str().map(|q| format!("s {}", query_key(q))),
            "fetch_url" => args["url"].as_str().map(|u| format!("p {}", url_key(u))),
            _ => None,
        }
    }

    /// Asked before a tool runs. `Some` is the answer to give instead of running it.
    pub fn before(&mut self, tool: &str, args: &Value) -> Option<Held> {
        let call = format!("{tool} {args}");
        if self.last_call == call {
            self.same_calls += 1;
        } else {
            self.last_call = call;
            self.same_calls = 1;
        }
        if self.same_calls >= SAME_CALL {
            return Some(Held {
                text: "You made this exact call three times in a row. It gives the same result. Do something else, or call finish.".into(),
                refused: true,
            });
        }
        let key = Self::web_key(tool, args)?;
        if key.len() <= 2 {
            return None;
        }
        if let Some(result) = self.recall(&key) {
            let said = if tool == "web_search" {
                "You already searched for this (the same words). Nothing new was fetched; this is the result from before."
            } else {
                "You already opened this page. It was not fetched again; this is what it said."
            };
            return Some(Held { text: format!("{said}\n\n{result}"), refused: false });
        }
        if tool == "web_search" && self.search_off {
            return Some(Held { text: super::web::NO_ANSWER.into(), refused: true });
        }
        let (used, budget, enough) = if tool == "web_search" {
            (
                &mut self.searches,
                SEARCH_BUDGET,
                "Enough has been searched for this request: no more searches will run. Work with the results you have: \
open one of the pages found with fetch_url, or answer.",
            )
        } else {
            (
                &mut self.fetches,
                FETCH_BUDGET,
                "Enough pages were opened for this request: no more will be fetched. Work with what you have read and answer.",
            )
        };
        if *used >= budget {
            return Some(Held { text: enough.into(), refused: true });
        }
        *used += 1;
        None
    }

    /// Told what a tool call gave. `status` is the step's: "done", "failed" or "denied".
    pub fn after(&mut self, tool: &str, args: &Value, result: &str, status: &str, held: Option<bool>) -> Verdict {
        let kind = kind_of(tool);
        let ok = status == "done" && held != Some(true);
        let failed = status == "failed" || held == Some(true);
        let web_key = Self::web_key(tool, args);
        if ok && held.is_none() {
            if let Some(key) = web_key.clone() {
                self.remember(key, result);
            }
        }
        match kind {
            Kind::Search => {
                self.web_run += 1;
                self.lookup_run += 1;
            }
            Kind::Lookup | Kind::Page => {
                self.web_run = 0;
                self.lookup_run += 1;
            }
            Kind::Plan => {}
            Kind::Change | Kind::Command | Kind::Other => self.web_run = 0,
        }
        let mut sign = None;
        // The same call with the same result: a remembered search counts under its words, so
        // "rust book" and "Book, Rust!" are one, and a command counts without the time it took.
        let lookup = matches!(kind, Kind::Lookup | Kind::Search | Kind::Page);
        let stable = match (kind, result.split_once('\n')) {
            (Kind::Command, Some((first, rest))) => format!("{}\n{rest}", first.split(" after ").next().unwrap_or(first)),
            _ => result.to_string(),
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (tool, web_key.unwrap_or_else(|| args.to_string()), stable).hash(&mut hasher);
        let seen = if lookup { &mut self.looked } else { &mut self.done }.entry(format!("{:x}", hasher.finish())).or_insert(0);
        *seen = seen.saturating_add(1);
        let fresh = *seen == 1;
        if *seen >= SAME_OUTCOME {
            sign = Some(Sign::Same);
        }
        // A search the engine did not answer, twice in a row: no point in a third.
        if kind == Kind::Search && held.is_none() {
            self.search_fails = if status == "failed" { self.search_fails + 1 } else { 0 };
            self.search_off |= self.search_fails >= SEARCH_FAILS;
        }
        if failed {
            if self.failing == tool {
                self.fail_run += 1;
            } else {
                self.failing = tool.to_string();
                self.fail_run = 1;
            }
            if self.fail_run.is_multiple_of(FAIL_RUN) {
                sign = Some(Sign::Fails);
            }
        } else if ok && self.failing == tool {
            self.fail_run = 0;
        }
        // Something new changed or ran: that is progress, and the ladder starts again. Reading a
        // file once more after a change is not a repeat, and tests that fail after a fix are
        // not the same failure.
        if ok && fresh && matches!(kind, Kind::Change | Kind::Command | Kind::Other) {
            self.lookup_run = 0;
            self.signs = 0;
            self.fail_run = 0;
            self.looked.clear();
        }
        if lookup && self.lookup_run > 0 && self.lookup_run.is_multiple_of(LOOKUP_RUN) {
            sign = sign.or(Some(Sign::Lookups));
        }
        let Some(sign) = sign else {
            let nudge = (kind == Kind::Search && held != Some(true) && self.web_run >= WEB_RUN).then(|| {
                format!(
                    "[That is {} web searches in a row. Open one of the pages found with fetch_url, or answer with what you have. {} of {SEARCH_BUDGET} searches are left for this request.]",
                    self.web_run,
                    SEARCH_BUDGET.saturating_sub(self.searches)
                )
            });
            return Verdict { nudge, ask: None };
        };
        self.signs = self.signs.saturating_add(1);
        match self.signs {
            n if n < RETHINK_AT => Verdict {
                nudge: Some(match sign {
                    Sign::Same => "[You did exactly this before and got the same result. Doing it again will not help. Take a different step.]".to_string(),
                    Sign::Lookups => format!(
                        "[{} look-ups in a row and nothing was changed, run or answered. You know enough to act: make the change, run the check, or call finish with what you found.]",
                        self.lookup_run
                    ),
                    Sign::Fails => format!("[{tool} did not work {} times in a row. Read the error above and do it differently, or use another tool.]", self.fail_run),
                }),
                ask: None,
            },
            n if n < ASK_AT => Verdict { nudge: Some(RETHINK.into()), ask: None },
            _ => Verdict {
                nudge: None,
                ask: Some(match sign {
                    Sign::Same => format!(
                        "I keep repeating {} and getting the same result. How should I go on? Tell me what to try, or say \"go on\" and I will keep trying.",
                        what(tool, args)
                    ),
                    Sign::Lookups => "I have been looking things up for a long time without getting further. What should I do next? Tell me where to look or what to change, or say \"go on\" and I will keep trying.".to_string(),
                    Sign::Fails => format!(
                        "I cannot get past {}: {}. How should I go on? Tell me what to try, or say \"go on\" and I will keep trying.",
                        what(tool, args),
                        first_line(result, 160)
                    ),
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One call as the loop makes it: asked before, told after. Gives (what the model reads, the question).
    fn step(pace: &mut Pace, tool: &str, args: Value, result: &str, status: &str) -> (String, Option<String>) {
        let held = pace.before(tool, &args);
        let (text, status, refused) = match &held {
            Some(h) => (h.text.clone(), if h.refused { "failed" } else { "done" }, Some(h.refused)),
            None => (result.to_string(), status, None),
        };
        let verdict = pace.after(tool, &args, &text, status, refused);
        (verdict.nudge.map_or(text.clone(), |n| format!("{text}\n\n{n}")), verdict.ask)
    }

    fn search(pace: &mut Pace, query: &str) -> String {
        step(pace, "web_search", json!({"query": query}), &format!("1. A page about {query}\n   https://example.com/\n   Text."), "done").0
    }

    #[test]
    fn searches_that_differ_only_in_case_order_or_punctuation_are_one() {
        assert_eq!(query_key("Rust async book"), "async book rust");
        assert_eq!(query_key("  book: ASYNC, rust!  "), "async book rust");
        assert_eq!(query_key("the rust async book"), query_key("Async Rust Book"));
        assert_eq!(query_key("rust rust book"), "book rust");
        assert_ne!(query_key("rust async book"), query_key("rust sync book"));
        // A search of small words only is still a search.
        assert_eq!(query_key("What is the of"), "is of the what");
        assert_eq!(query_key("?!"), "");
        assert_eq!(url_key("https://www.Example.com/Docs/#install"), "example.com/docs");
        assert_eq!(url_key("http://example.com/docs"), url_key("https://example.com/docs/"));
        assert_ne!(url_key("https://example.com/docs?page=2"), url_key("https://example.com/docs"));
    }

    #[test]
    fn a_search_made_before_is_answered_from_memory_and_costs_nothing() {
        let mut pace = Pace::default();
        let first = search(&mut pace, "svelte runes");
        assert!(first.starts_with("1. A page about svelte runes"));
        assert_eq!(pace.searches(), 1);
        let again = search(&mut pace, "Runes, Svelte!");
        assert!(again.starts_with("You already searched for this"), "{again}");
        assert!(again.contains("1. A page about svelte runes"));
        assert_eq!(pace.searches(), 1, "a remembered search does not use the budget");
        // It is still remembered after the person's next message.
        pace.next_request();
        assert!(search(&mut pace, "svelte runes").starts_with("You already searched"));
        assert_eq!(pace.searches(), 0);
        // A page opened before is not fetched again either.
        let page = json!({"url": "https://example.com/a"});
        assert!(pace.before("fetch_url", &page).is_none());
        pace.after("fetch_url", &page, "https://example.com/a\n\nHello", "done", None);
        let held = pace.before("fetch_url", &json!({"url": "https://www.example.com/a/#top"})).unwrap();
        assert!(!held.refused && held.text.contains("Hello") && held.text.starts_with("You already opened this page"));
        // A search that failed is not remembered: it may work the next time.
        let q = json!({"query": "flaky"});
        assert!(pace.before("web_search", &q).is_none());
        pace.after("web_search", &q, "The search took too long to answer.", "failed", None);
        assert!(pace.before("web_search", &json!({"query": "Flaky"})).is_none());
    }

    #[test]
    fn a_request_has_a_budget_of_searches_and_the_next_message_renews_it() {
        let mut pace = Pace::default();
        for i in 0..SEARCH_BUDGET {
            let out = search(&mut pace, &format!("topic number{i}"));
            assert!(out.starts_with("1. A page"), "search {i} runs: {out}");
        }
        assert_eq!(pace.searches(), SEARCH_BUDGET);
        let over = pace.before("web_search", &json!({"query": "one more thing"})).unwrap();
        assert!(over.refused && over.text.starts_with("Enough has been searched"), "{}", over.text);
        assert!(over.text.contains("fetch_url"));
        assert_eq!(pace.searches(), SEARCH_BUDGET, "a refused search is not sent");
        // Pages have their own, larger budget.
        for i in 0..FETCH_BUDGET {
            assert!(pace.before("fetch_url", &json!({"url": format!("https://example.com/{i}")})).is_none());
        }
        assert!(pace.before("fetch_url", &json!({"url": "https://example.com/more"})).unwrap().refused);
        pace.next_request();
        assert_eq!(pace.searches(), 0);
        assert!(pace.before("web_search", &json!({"query": "one more thing"})).is_none());
        assert!(pace.before("fetch_url", &json!({"url": "https://example.com/more"})).is_none());
    }

    #[test]
    fn a_tool_that_is_used_up_is_closed_and_the_model_is_told_once() {
        let mut pace = Pace::default();
        assert!(!pace.closed("web_search") && !pace.closed("fetch_url") && pace.note(true).is_empty());
        for i in 0..SEARCH_BUDGET {
            search(&mut pace, &format!("subject{i}"));
        }
        assert!(pace.closed("web_search") && !pace.closed("fetch_url") && !pace.closed("read_file"));
        assert!(pace.note(true).contains("web_search is used up for this request (6 searches)"), "{}", pace.note(true));
        assert!(pace.note(false).is_empty(), "nothing to say in a task without the web");
        pace.next_request();
        assert!(!pace.closed("web_search") && pace.note(true).is_empty());
    }

    #[test]
    fn two_searches_the_engine_did_not_answer_switch_the_search_off_for_the_request() {
        let mut pace = Pace::default();
        let fail = |pace: &mut Pace, query: &str| step(pace, "web_search", json!({"query": query}), super::super::web::NO_ANSWER, "failed");
        fail(&mut pace, "first words");
        assert!(!pace.closed("web_search"));
        // One that works in between starts the count again.
        search(&mut pace, "works");
        fail(&mut pace, "second words");
        assert!(!pace.closed("web_search"));
        fail(&mut pace, "third words");
        assert!(pace.closed("web_search"));
        assert_eq!(pace.searches(), 4);
        assert!(pace.note(true).contains("switched off for this request") && pace.note(true).contains("fetch_url"));
        // Asked for all the same, it is not sent.
        let held = pace.before("web_search", &json!({"query": "fourth words"})).unwrap();
        assert!(held.refused && held.text.contains("fetch_url"));
        assert_eq!(pace.searches(), 4);
        // Pages can still be opened, and the next message of the person tries the search again.
        assert!(pace.before("fetch_url", &json!({"url": "https://example.com/"})).is_none());
        pace.next_request();
        assert!(!pace.closed("web_search") && pace.before("web_search", &json!({"query": "fourth words"})).is_none());
    }

    #[test]
    fn a_few_searches_in_a_row_say_to_read_a_page() {
        let mut pace = Pace::default();
        assert!(!search(&mut pace, "alpha").contains('['));
        assert!(!search(&mut pace, "beta").contains('['));
        let third = search(&mut pace, "gamma");
        assert!(third.contains("3 web searches in a row") && third.contains("3 of 6 searches are left"), "{third}");
        // Another kind of step ends the run.
        step(&mut pace, "fetch_url", json!({"url": "https://example.com/"}), "https://example.com/\n\nText", "done");
        assert!(!search(&mut pace, "delta").contains('['));
    }

    #[test]
    fn a_long_run_of_look_ups_is_nudged_then_told_to_change_approach_then_asks_the_person() {
        let mut pace = Pace::default();
        let read = |pace: &mut Pace, n: usize| {
            step(
                pace,
                "read_file",
                json!({"path": format!("src/f{n}.rs")}),
                &format!("src/f{n}.rs (lines 1 to 2 of 2)\nfn f{n}() {{}}"),
                "done",
            )
        };
        for n in 1..LOOKUP_RUN {
            let (text, ask) = read(&mut pace, n);
            assert!(!text.contains('[') && ask.is_none(), "look-up {n} is quiet");
        }
        let (text, ask) = read(&mut pace, LOOKUP_RUN);
        assert!(text.contains("12 look-ups in a row") && ask.is_none(), "{text}");
        // Every twelve more are another sign: nudges first, then the change of approach, and
        // the question only at the sixth.
        for sign in 2..=ASK_AT as usize {
            for n in (sign - 1) * LOOKUP_RUN + 1..sign * LOOKUP_RUN {
                let (text, ask) = read(&mut pace, n);
                assert!(!text.contains('[') && ask.is_none());
            }
            let (text, ask) = read(&mut pace, sign * LOOKUP_RUN);
            if sign < RETHINK_AT as usize {
                assert!(text.contains("look-ups in a row") && ask.is_none(), "{sign}: {text}");
            } else if sign < ASK_AT as usize {
                assert!(text.contains("Change approach now") && text.contains("update_plan") && ask.is_none(), "{sign}: {text}");
            } else {
                assert!(ask.unwrap().contains("What should I do next?"));
            }
        }
    }

    #[test]
    fn a_change_or_a_command_is_progress_and_the_ladder_starts_again() {
        let mut pace = Pace::default();
        for n in 0..LOOKUP_RUN {
            step(&mut pace, "search", json!({"pattern": format!("p{n}")}), "Nothing in the folder matches.", "done");
        }
        assert_eq!(pace.signs, 1);
        // The plan is neither a look-up nor progress.
        step(&mut pace, "update_plan", json!({"steps": ["a"]}), "Plan saved", "done");
        assert_eq!((pace.signs, pace.lookup_run), (1, LOOKUP_RUN));
        step(&mut pace, "edit_file", json!({"path": "a.rs", "old": "x", "new": "y"}), "Changed a.rs", "done");
        assert_eq!((pace.signs, pace.lookup_run), (0, 0));
        for n in 0..LOOKUP_RUN - 1 {
            assert!(!step(&mut pace, "read_file", json!({"path": format!("{n}")}), "text", "done").0.contains('['));
        }
        // A change that did not go in is not progress.
        step(&mut pace, "edit_file", json!({"path": "a.rs", "old": "gone", "new": "y"}), "That text is not in a.rs.", "failed");
        assert_eq!(pace.lookup_run, LOOKUP_RUN - 1);
    }

    #[test]
    fn the_same_call_with_the_same_result_climbs_the_ladder() {
        let mut pace = Pace::default();
        let round = |pace: &mut Pace| {
            let a = step(pace, "read_file", json!({"path": "a.rs"}), "a.rs (lines 1 to 1 of 1)\nfn a() {}", "done");
            // The time a command took is not part of what it gave.
            let took = pace.done.len() + pace.looked.len() + usize::from(pace.signs);
            let b =
                step(pace, "run_command", json!({"command": "make test"}), &format!("exit code 2 after 0.{took} s\nerror: nope"), "failed");
            (a, b)
        };
        let (a, b) = round(&mut pace);
        assert!(!a.0.contains('[') && !b.0.contains('['));
        round(&mut pace);
        // The third time round: the read is the first sign, the command the second. Both are nudges.
        let (a, b) = round(&mut pace);
        assert!(a.0.contains("got the same result"), "{}", a.0);
        assert!(b.0.contains("run_command did not work 3 times in a row"), "{}", b.0);
        // The fourth: signs three and four say to change approach.
        let (a, b) = round(&mut pace);
        assert!(a.0.contains("Change approach now") && a.1.is_none(), "{}", a.0);
        assert!(b.0.contains("Change approach now") && b.1.is_none(), "{}", b.0);
        // The fifth: one more of those, and the sixth sign is the question.
        let (a, b) = round(&mut pace);
        assert!(a.0.contains("Change approach now") && a.1.is_none());
        let question = b.1.expect("the sixth sign is a question for the person");
        assert!(question.contains("the command `make test`") && question.contains("go on"), "{question}");
        // Their answer starts the ladder again.
        pace.next_request();
        let (a, b) = round(&mut pace);
        assert!(!a.0.contains('[') && !b.0.contains('[') && a.1.is_none());
    }

    #[test]
    fn the_same_call_three_times_in_a_row_is_not_run_again() {
        let mut pace = Pace::default();
        let args = json!({"path": "."});
        assert!(pace.before("list_files", &args).is_none());
        assert!(pace.before("list_files", &args).is_none());
        let held = pace.before("list_files", &args).unwrap();
        assert!(held.refused && held.text.contains("three times in a row"));
        assert!(pace.before("list_files", &json!({"path": "src"})).is_none());
        assert!(pace.before("list_files", &args).is_none());
    }

    #[test]
    fn a_tool_that_keeps_failing_is_a_sign_and_a_no_from_the_person_is_not() {
        let mut pace = Pace::default();
        let fail = |pace: &mut Pace, n: usize| {
            step(
                pace,
                "edit_file",
                json!({"path": "a.rs", "old": format!("try {n}"), "new": "y"}),
                &format!("\"try {n}\" is not in a.rs."),
                "failed",
            )
        };
        assert!(!fail(&mut pace, 1).0.contains('['));
        assert!(!fail(&mut pace, 2).0.contains('['));
        let third = fail(&mut pace, 3).0;
        assert!(third.contains("edit_file did not work 3 times in a row"), "{third}");
        for n in 4..=17 {
            let (text, ask) = fail(&mut pace, n);
            assert!(ask.is_none());
            match n {
                6 => assert!(text.contains("did not work 6 times in a row"), "{text}"),
                9 | 12 | 15 => assert!(text.contains("Change approach now"), "{text}"),
                _ => assert!(!text.contains('['), "{n}: {text}"),
            }
        }
        let question = fail(&mut pace, 18).1.unwrap();
        assert!(question.contains("the change to a.rs") && question.contains("\"try 18\" is not in a.rs."), "{question}");

        // Tests that fail, a fix, tests that fail in another way: that is work, however long it goes on.
        let mut pace = Pace::default();
        for n in 0..30 {
            let (ran, ask) =
                step(&mut pace, "run_command", json!({"command": "make test"}), &format!("exit code 1 after 0.1 s\n{n} failed"), "failed");
            assert!(!ran.contains('[') && ask.is_none(), "{n}: {ran}");
            // Reading the same two files again after each fix is not a repeat either.
            for file in ["a.rs", "a_test.rs"] {
                let (read, ask) = step(&mut pace, "read_file", json!({"path": file}), "the same text as before", "done");
                assert!(!read.contains('[') && ask.is_none(), "{n}: {read}");
            }
            step(
                &mut pace,
                "edit_file",
                json!({"path": "a.rs", "old": format!("v{n}"), "new": format!("v{}", n + 1)}),
                "Changed a.rs",
                "done",
            );
        }
        // The same edit back and forth is not progress: the ladder is climbed.
        let mut pace = Pace::default();
        let mut asked = false;
        for n in 0..40 {
            let (old, new) = if n % 2 == 0 { ("x", "y") } else { ("y", "x") };
            step(&mut pace, "edit_file", json!({"path": "a.rs", "old": old, "new": new}), "Changed a.rs", "done");
            asked |=
                step(&mut pace, "run_command", json!({"command": "make test"}), &format!("exit code 1 after 0.{n} s\n1 failed"), "failed")
                    .1
                    .is_some();
        }
        assert!(asked, "going back and forth ends in a question");

        // Searches refused for the budget count as that tool not working.
        let mut pace = Pace::default();
        for i in 0..SEARCH_BUDGET {
            search(&mut pace, &format!("thing{i}"));
            step(&mut pace, "run_command", json!({"command": format!("echo {i}")}), "exit code 0", "done");
        }
        let over = |pace: &mut Pace, n: usize| step(pace, "web_search", json!({"query": format!("extra{n}")}), "", "done");
        assert!(over(&mut pace, 1).0.starts_with("Enough has been searched"));
        over(&mut pace, 2);
        assert!(over(&mut pace, 3).0.contains("web_search did not work 3 times in a row"));

        // What the person refused is theirs to decide, not a failure.
        let mut pace = Pace::default();
        for n in 0..6 {
            let (text, ask) =
                step(&mut pace, "run_command", json!({"command": format!("rm -rf {n}")}), "The person did not allow this.", "denied");
            assert!(!text.contains('[') && ask.is_none());
        }
    }

    #[test]
    fn what_is_kept_reads_back() {
        let mut pace = Pace::default();
        search(&mut pace, "kept");
        let back: Pace = serde_json::from_str(&serde_json::to_string(&pace).unwrap()).unwrap();
        assert_eq!(back.searches(), 1);
        assert!(back.recall("s kept").is_some());
        let empty: Pace = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.searches(), 0);
        // Only so many results are kept.
        for i in 0..40 {
            pace.remember(format!("s {i}"), "r");
        }
        assert_eq!(pace.remembered.len(), MAX_REMEMBERED);
    }
}
