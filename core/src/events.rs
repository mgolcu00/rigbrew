//! Local event store.
//!
//! Every analytics event is written to a SQLite queue on this machine first, online or not.
//! A sync pass later sends pending events in batches and marks them sent, so nothing is lost
//! while the machine is offline.
//!
//! Two layers:
//! - **Basic usage data** is always recorded: what was opened, clicked, installed, loaded, how
//!   fast, what failed. It never holds what people type or what models answer: without consent
//!   every payload passes through [`strip_content`] before it is stored.
//! - **Content** (prompt and answer text, picture prompts) is stored only while consent stands.
//!   Withdrawing consent strips content from everything not yet sent; basic data continues.
//!
//! The sender decides per batch what may leave the machine (see `daemon/src/cloud.rs`).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug)]
pub enum EventError {
    Db(rusqlite::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventError::Db(e) => write!(f, "event store: {e}"),
            EventError::Json(e) => write!(f, "event payload: {e}"),
        }
    }
}

impl std::error::Error for EventError {}

impl From<rusqlite::Error> for EventError {
    fn from(e: rusqlite::Error) -> Self {
        EventError::Db(e)
    }
}

impl From<serde_json::Error> for EventError {
    fn from(e: serde_json::Error) -> Self {
        EventError::Json(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    /// Version of the consent text that was accepted.
    pub version: String,
    /// Unix time in milliseconds.
    pub accepted_at_ms: i64,
}

/// One queued event, as it will be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// UUIDv7: unique and ordered by creation time, so the server can deduplicate retries.
    pub id: String,
    /// When it happened on this machine (Unix ms), not when it was sent.
    pub ts_ms: i64,
    pub kind: String,
    /// Random id of this installation.
    pub install_id: String,
    /// One run of the app.
    pub session_id: String,
    /// Account id when signed in.
    pub user_id: Option<String>,
    pub payload: serde_json::Value,
    /// True when the payload holds content (text people typed or models wrote).
    #[serde(default)]
    pub content: bool,
}

/// Payload keys that hold what people typed or what a model wrote.
const CONTENT_KEYS: &[&str] = &["messages", "prompt", "text", "content", "tool_calls", "answer", "input", "output", "query"];

/// Whether a payload carries content.
pub fn has_content(payload: &serde_json::Value) -> bool {
    payload.as_object().is_some_and(|o| o.keys().any(|k| CONTENT_KEYS.contains(&k.as_str())))
}

fn chars(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::String(s) => s.chars().count(),
        serde_json::Value::Array(a) => a.iter().map(chars).sum(),
        serde_json::Value::Object(o) => o.get("content").or_else(|| o.get("text")).map(chars).unwrap_or(0),
        _ => 0,
    }
}

fn basic_value(v: &serde_json::Value, depth: usize) -> Option<serde_json::Value> {
    use serde_json::Value;
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(v.clone()),
        // Short labels (ids, screen names, error classes) stay; long strings may be content.
        Value::String(s) => (s.chars().count() <= 120).then(|| v.clone()),
        Value::Array(a) if depth < 2 && a.len() <= 20 => Some(Value::Array(a.iter().filter_map(|x| basic_value(x, depth + 1)).collect())),
        Value::Object(o) if depth < 2 => Some(Value::Object(
            o.iter()
                .filter(|(k, _)| !CONTENT_KEYS.contains(&k.as_str()))
                .filter_map(|(k, x)| basic_value(x, depth + 1).map(|x| (k.clone(), x)))
                .collect(),
        )),
        _ => None,
    }
}

/// The basic-usage form of a payload: content removed, sizes kept as numbers, long strings
/// dropped, at most two levels deep. Errors are kept but shortened.
pub fn strip_content(payload: &serde_json::Value) -> serde_json::Value {
    use serde_json::{json, Value};
    let Some(obj) = payload.as_object() else { return json!({}) };
    let mut out = match basic_value(payload, 0) {
        Some(Value::Object(o)) => o,
        _ => serde_json::Map::new(),
    };
    if let Some(m) = obj.get("messages").and_then(Value::as_array) {
        out.insert("message_count".into(), json!(m.len()));
        let last_user = m.iter().rev().find(|x| x["role"] == "user").map(chars).unwrap_or(0);
        out.insert("user_chars".into(), json!(last_user));
    }
    for key in ["prompt", "text", "answer"] {
        if let Some(v) = obj.get(key) {
            out.insert(format!("{key}_chars"), json!(chars(v)));
        }
    }
    if let Some(calls) = obj.get("tool_calls").and_then(Value::as_array) {
        let names: Vec<Value> = calls
            .iter()
            .filter_map(|c| c["name"].as_str().or(c["function"]["name"].as_str()))
            .map(|n| json!(n.chars().take(60).collect::<String>()))
            .collect();
        out.insert("tool_names".into(), Value::Array(names));
    }
    if let Some(e) = obj.get("error").and_then(Value::as_str) {
        out.insert("error".into(), json!(e.chars().take(300).collect::<String>()));
    }
    Value::Object(out)
}

/// One day of activity: (days since the Unix epoch, prompts, images, tokens out, events).
pub type DayActivity = (i64, u64, u64, u64, u64);

/// What was done on this machine in a period.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub chat_messages: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub images: u64,
    pub seconds_generating: f64,
}

pub struct EventStore {
    db: Connection,
    install_id: String,
    session_id: String,
    user_id: Option<String>,
    fresh_install: bool,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl EventStore {
    /// Opens (or creates) the store and starts a new session.
    pub fn open(path: &Path) -> Result<EventStore, EventError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<EventStore, EventError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<EventStore, EventError> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events (
               id TEXT PRIMARY KEY,
               ts_ms INTEGER NOT NULL,
               kind TEXT NOT NULL,
               session_id TEXT NOT NULL,
               user_id TEXT,
               payload TEXT NOT NULL,
               sent_at_ms INTEGER
             );
             CREATE INDEX IF NOT EXISTS events_pending ON events (id) WHERE sent_at_ms IS NULL;",
        )?;
        let has_flag: bool =
            db.query_row("SELECT COUNT(*) FROM pragma_table_info('events') WHERE name = 'content'", [], |r| r.get::<_, i64>(0))? > 0;
        if !has_flag {
            db.execute_batch(
                "ALTER TABLE events ADD COLUMN content INTEGER NOT NULL DEFAULT 0;
                 UPDATE events SET content = 1 WHERE kind IN ('prompt', 'response');",
            )?;
        }
        let (install_id, fresh_install) =
            match db.query_row("SELECT value FROM meta WHERE key = 'install_id'", [], |r| r.get::<_, String>(0)).optional()? {
                Some(id) => (id, false),
                None => {
                    let id = Uuid::now_v7().to_string();
                    db.execute("INSERT INTO meta (key, value) VALUES ('install_id', ?1)", params![id])?;
                    (id, true)
                }
            };
        Ok(EventStore { db, install_id, session_id: Uuid::now_v7().to_string(), user_id: None, fresh_install })
    }

    pub fn install_id(&self) -> &str {
        &self.install_id
    }

    /// True on the first start of this installation (the install id was just created).
    pub fn fresh_install(&self) -> bool {
        self.fresh_install
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Set on sign-in, cleared on sign-out. Events recorded in between carry the account id.
    pub fn set_user(&mut self, user_id: Option<String>) {
        self.user_id = user_id;
    }

    pub fn consent(&self) -> Result<Option<Consent>, EventError> {
        let raw = self.db.query_row("SELECT value FROM meta WHERE key = 'consent'", [], |r| r.get::<_, String>(0)).optional()?;
        Ok(raw.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub fn accept_consent(&self, version: &str) -> Result<Consent, EventError> {
        let consent = Consent { version: version.to_string(), accepted_at_ms: now_ms() };
        self.db.execute(
            "INSERT INTO meta (key, value) VALUES ('consent', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![serde_json::to_string(&consent)?],
        )?;
        Ok(consent)
    }

    /// Stops collecting content and strips it from every event that has not left the machine.
    /// Basic usage data keeps being recorded.
    pub fn withdraw_consent(&self) -> Result<(), EventError> {
        self.db.execute("DELETE FROM meta WHERE key = 'consent'", [])?;
        let mut stmt = self.db.prepare("SELECT id, payload FROM events WHERE sent_at_ms IS NULL AND content = 1")?;
        let rows: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (id, payload) in rows {
            let stripped = strip_content(&serde_json::from_str(&payload).unwrap_or_default());
            self.db.execute("UPDATE events SET payload = ?1, content = 0 WHERE id = ?2", params![stripped.to_string(), id])?;
        }
        // The link to the account is part of what consent covered.
        self.db.execute("UPDATE events SET user_id = NULL WHERE sent_at_ms IS NULL", [])?;
        Ok(())
    }

    /// Queues an event. Without consent the payload is reduced to basic usage data first, and
    /// no account id is attached. Always returns `true`.
    pub fn record(&self, kind: &str, payload: serde_json::Value) -> Result<bool, EventError> {
        let consented = self.consent()?.is_some();
        let (payload, content, user) = if consented {
            let content = has_content(&payload);
            (payload, content, self.user_id.clone())
        } else {
            (strip_content(&payload), false, None)
        };
        self.db.execute(
            "INSERT INTO events (id, ts_ms, kind, session_id, user_id, payload, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![Uuid::now_v7().to_string(), now_ms(), kind, self.session_id, user, serde_json::to_string(&payload)?, content],
        )?;
        Ok(true)
    }

    /// Oldest unsent events, up to `limit`.
    pub fn pending(&self, limit: usize) -> Result<Vec<Event>, EventError> {
        let mut stmt = self.db.prepare(
            "SELECT id, ts_ms, kind, session_id, user_id, payload, content FROM events WHERE sent_at_ms IS NULL ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, ts_ms, kind, session_id, user_id, payload, content) = row?;
            out.push(Event {
                id,
                ts_ms,
                kind,
                install_id: self.install_id.clone(),
                session_id,
                user_id,
                payload: serde_json::from_str(&payload)?,
                content,
            });
        }
        Ok(out)
    }

    /// Newest unsent events first, up to `limit`: what a "what is being collected" screen shows.
    pub fn pending_newest(&self, limit: usize) -> Result<Vec<Event>, EventError> {
        let mut stmt = self.db.prepare("SELECT id FROM events WHERE sent_at_ms IS NULL ORDER BY id DESC LIMIT ?1")?;
        let ids: Vec<String> = stmt.query_map(params![limit as i64], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut all = self.pending(usize::MAX >> 1)?;
        all.retain(|e| ids.contains(&e.id));
        all.reverse();
        Ok(all)
    }

    pub fn pending_count(&self) -> Result<u64, EventError> {
        Ok(self.db.query_row("SELECT COUNT(*) FROM events WHERE sent_at_ms IS NULL", [], |r| r.get::<_, i64>(0))? as u64)
    }

    /// Marks a delivered batch. Call only after the server has acknowledged it.
    pub fn mark_sent(&self, ids: &[String]) -> Result<(), EventError> {
        let now = now_ms();
        let mut stmt = self.db.prepare("UPDATE events SET sent_at_ms = ?1 WHERE id = ?2")?;
        for id in ids {
            stmt.execute(params![now, id])?;
        }
        Ok(())
    }

    /// Totals of what was done on this machine since `since_ms`, from the local log (sent or not).
    pub fn usage_since(&self, since_ms: i64) -> Result<Usage, EventError> {
        Ok(self.db.query_row(
            "SELECT
               COALESCE(SUM(kind = 'prompt' AND json_extract(payload, '$.modality') = 'chat'), 0),
               COALESCE(SUM(CASE WHEN kind = 'response' THEN COALESCE(json_extract(payload, '$.tokens_in'), 0) ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN kind = 'response' THEN COALESCE(json_extract(payload, '$.tokens_out'), 0) ELSE 0 END), 0),
               COALESCE(SUM(kind = 'image_generated'), 0),
               COALESCE(SUM(CASE WHEN kind IN ('response', 'image_generated') THEN COALESCE(json_extract(payload, '$.seconds'), 0) ELSE 0 END), 0.0)
             FROM events WHERE ts_ms >= ?1",
            params![since_ms],
            |r| {
                Ok(Usage {
                    chat_messages: r.get::<_, i64>(0)? as u64,
                    tokens_in: r.get::<_, i64>(1)? as u64,
                    tokens_out: r.get::<_, i64>(2)? as u64,
                    images: r.get::<_, i64>(3)? as u64,
                    seconds_generating: r.get::<_, f64>(4)?,
                })
            },
        )?)
    }

    /// Activity per day (days since the Unix epoch, UTC) since `since_ms`: (day, prompts, images, tokens out, events).
    pub fn daily_activity(&self, since_ms: i64) -> Result<Vec<DayActivity>, EventError> {
        let mut stmt = self.db.prepare(
            "SELECT ts_ms / 86400000,
                    SUM(kind = 'prompt'),
                    SUM(kind = 'image_generated'),
                    SUM(CASE WHEN kind = 'response' THEN COALESCE(json_extract(payload, '$.tokens_out'), 0) ELSE 0 END),
                    COUNT(*)
             FROM events WHERE ts_ms >= ?1 GROUP BY 1 ORDER BY 1",
        )?;
        let rows = stmt.query_map(params![since_ms], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, i64>(3)? as u64,
                r.get::<_, i64>(4)? as u64,
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How often each package was used since `since_ms`: (package, prompts), most used first.
    pub fn package_use(&self, since_ms: i64) -> Result<Vec<(String, u64)>, EventError> {
        let mut stmt = self.db.prepare(
            "SELECT json_extract(payload, '$.package'), COUNT(*) FROM events
             WHERE kind = 'prompt' AND ts_ms >= ?1 AND json_extract(payload, '$.package') IS NOT NULL
             GROUP BY 1 ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(params![since_ms], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The newest events of the given kinds, sent or not, newest first.
    pub fn recent(&self, kinds: &[&str], limit: usize) -> Result<Vec<Event>, EventError> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; kinds.len()].join(", ");
        let sql =
            format!("SELECT id, ts_ms, kind, session_id, user_id, payload FROM events WHERE kind IN ({marks}) ORDER BY id DESC LIMIT ?");
        let mut stmt = self.db.prepare(&sql)?;
        let mut args: Vec<&dyn rusqlite::ToSql> = kinds.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
        let limit = limit as i64;
        args.push(&limit);
        let rows = stmt.query_map(args.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, ts_ms, kind, session_id, user_id, payload) = row?;
            let payload: serde_json::Value = serde_json::from_str(&payload)?;
            let content = has_content(&payload);
            out.push(Event { id, ts_ms, kind, install_id: self.install_id.clone(), session_id, user_id, payload, content });
        }
        Ok(out)
    }

    /// Time of the first event on this machine, if any.
    pub fn first_event_ms(&self) -> Result<Option<i64>, EventError> {
        Ok(self.db.query_row("SELECT MIN(ts_ms) FROM events", [], |r| r.get::<_, Option<i64>>(0))?)
    }

    /// Deletes sent events older than `keep_days`; returns how many were removed.
    pub fn prune_sent(&self, keep_days: u32) -> Result<usize, EventError> {
        let cutoff = now_ms() - i64::from(keep_days) * 86_400_000;
        Ok(self.db.execute("DELETE FROM events WHERE sent_at_ms IS NOT NULL AND sent_at_ms < ?1", params![cutoff])?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn before_consent_only_basic_usage_is_recorded() {
        let store = EventStore::in_memory().unwrap();
        assert!(store.record("app_opened", json!({"screen": "welcome"})).unwrap());
        store.record("prompt", json!({"package": "steep-4b", "messages": [{"role": "user", "content": "my secret plan"}]})).unwrap();
        let pending = store.pending(10).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].payload["screen"], "welcome");
        let p = &pending[1].payload;
        assert!(!pending[1].content);
        assert!(p.get("messages").is_none());
        assert_eq!(p["package"], "steep-4b");
        assert_eq!(p["message_count"], 1);
        assert_eq!(p["user_chars"], 14);
        assert!(!serde_json::to_string(p).unwrap().contains("secret"));
    }

    #[test]
    fn stripping_keeps_numbers_and_short_labels_and_drops_text() {
        let long = "x".repeat(500);
        let p = json!({
            "package": "crema-6b", "seconds": 11.4, "ok": true,
            "prompt": "a cat", "text": "answer text", "tool_calls": [{"name": "generate_image", "arguments": "{\"prompt\":\"a cat\"}"}],
            "note": long, "nested": {"query": "secret", "width": 768, "deeper": {"a": {"b": 1}}},
            "error": "e".repeat(1000),
        });
        let s = strip_content(&p);
        assert_eq!(s["package"], "crema-6b");
        assert_eq!(s["seconds"], 11.4);
        assert_eq!(s["prompt_chars"], 5);
        assert_eq!(s["text_chars"], 11);
        assert_eq!(s["tool_names"], json!(["generate_image"]));
        assert!(s.get("note").is_none());
        assert!(s["nested"].get("query").is_none());
        assert_eq!(s["nested"]["width"], 768);
        assert_eq!(s["error"].as_str().unwrap().len(), 300);
        let text = s.to_string();
        assert!(!text.contains("a cat") && !text.contains("answer text") && !text.contains("secret"));
        assert!(!has_content(&s));
    }

    #[test]
    fn events_queue_in_order_and_leave_the_queue_when_sent() {
        let mut store = EventStore::in_memory().unwrap();
        store.accept_consent("2026-11-20").unwrap();
        assert!(store.record("app_opened", json!({"version": "0.1.0"})).unwrap());
        store.set_user(Some("u_1".into()));
        store.record("prompt", json!({"package": "steep-4b", "text": "hello"})).unwrap();
        store.set_user(None);
        store.record("signed_out", json!({})).unwrap();

        let pending = store.pending(10).unwrap();
        assert_eq!(pending.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["app_opened", "prompt", "signed_out"]);
        assert_eq!(pending[0].user_id, None);
        assert_eq!(pending[1].user_id.as_deref(), Some("u_1"));
        assert_eq!(pending[1].payload["text"], "hello");
        assert!(pending.iter().all(|e| e.session_id == store.session_id() && e.install_id == store.install_id()));

        store.mark_sent(&[pending[0].id.clone(), pending[1].id.clone()]).unwrap();
        let left = store.pending(10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].kind, "signed_out");
    }

    #[test]
    fn usage_adds_up_chat_and_image_work() {
        let store = EventStore::in_memory().unwrap();
        store.accept_consent("v1").unwrap();
        store.record("prompt", json!({"modality": "chat"})).unwrap();
        store.record("response", json!({"tokens_in": 12, "tokens_out": 40, "seconds": 1.5})).unwrap();
        store.record("prompt", json!({"modality": "image"})).unwrap();
        store.record("image_generated", json!({"seconds": 10.0})).unwrap();
        let u = store.usage_since(0).unwrap();
        assert_eq!(u, Usage { chat_messages: 1, tokens_in: 12, tokens_out: 40, images: 1, seconds_generating: 11.5 });
        assert_eq!(store.usage_since(i64::MAX).unwrap(), Usage::default());
    }

    #[test]
    fn recent_returns_the_newest_events_of_the_asked_kinds_sent_or_not() {
        let store = EventStore::in_memory().unwrap();
        store.accept_consent("v1").unwrap();
        for (k, i) in [("prompt", 1), ("screen_viewed", 2), ("prompt", 3), ("package_removed", 4)] {
            store.record(k, json!({"i": i})).unwrap();
        }
        let ids: Vec<String> = store.pending(10).unwrap().iter().map(|e| e.id.clone()).collect();
        store.mark_sent(&ids[..2]).unwrap();
        let got = store.recent(&["prompt", "package_removed"], 10).unwrap();
        assert_eq!(got.iter().map(|e| e.payload["i"].as_i64().unwrap()).collect::<Vec<_>>(), [4, 3, 1]);
        assert_eq!(store.recent(&["prompt"], 1).unwrap().len(), 1);
        assert!(store.recent(&[], 10).unwrap().is_empty());
    }

    #[test]
    fn batches_respect_the_limit() {
        let store = EventStore::in_memory().unwrap();
        store.accept_consent("v1").unwrap();
        for i in 0..5 {
            store.record("tick", json!({"i": i})).unwrap();
        }
        let batch = store.pending(2).unwrap();
        assert_eq!(batch.iter().map(|e| e.payload["i"].as_i64().unwrap()).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn withdrawing_consent_strips_content_but_keeps_basic_usage() {
        let mut store = EventStore::in_memory().unwrap();
        store.accept_consent("v1").unwrap();
        store.set_user(Some("u_1".into()));
        store.record("prompt", json!({"modality": "chat", "messages": [{"role": "user", "content": "private words"}]})).unwrap();
        store.record("screen_viewed", json!({"screen": "models"})).unwrap();
        assert!(store.pending(10).unwrap()[0].content);
        store.withdraw_consent().unwrap();
        assert_eq!(store.consent().unwrap(), None);
        let pending = store.pending(10).unwrap();
        assert_eq!(pending.len(), 2, "basic data stays queued");
        assert!(pending.iter().all(|e| !e.content && e.user_id.is_none()));
        assert!(!serde_json::to_string(&pending).unwrap().contains("private words"));
        store.record("app_opened", json!({})).unwrap();
        assert_eq!(store.pending_count().unwrap(), 3, "and keeps being recorded");
    }

    #[test]
    fn install_id_survives_reopen_and_sessions_differ() {
        let dir = std::env::temp_dir().join(format!("rigbrew-test-{}", Uuid::now_v7()));
        let path = dir.join("events.db");
        let (install, session) = {
            let s = EventStore::open(&path).unwrap();
            assert!(s.fresh_install());
            s.accept_consent("v1").unwrap();
            s.record("a", json!({})).unwrap();
            (s.install_id().to_string(), s.session_id().to_string())
        };
        let s = EventStore::open(&path).unwrap();
        assert!(!s.fresh_install());
        assert_eq!(s.install_id(), install);
        assert_ne!(s.session_id(), session);
        assert_eq!(s.pending_count().unwrap(), 1);
        assert_eq!(s.consent().unwrap().unwrap().version, "v1");
        std::fs::remove_dir_all(dir).ok();
    }
}
