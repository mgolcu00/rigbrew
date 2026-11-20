//! Saved conversations.
//!
//! The app owns the shape of a conversation; this store keeps it as an opaque JSON document
//! with a title and timestamps, in its own SQLite file next to the event queue. Nothing here
//! leaves the machine.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Largest conversation document accepted, in bytes.
pub const MAX_DATA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum ChatError {
    Db(rusqlite::Error),
    /// The id is not 1 to 64 characters of `A-Z a-z 0-9 _ -`.
    BadId,
    TooLarge(usize),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::Db(e) => write!(f, "chat store: {e}"),
            ChatError::BadId => write!(f, "a chat id is 1 to 64 letters, digits, '_' or '-'"),
            ChatError::TooLarge(n) => write!(f, "this chat is {n} bytes; the limit is {MAX_DATA_BYTES}"),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<rusqlite::Error> for ChatError {
    fn from(e: rusqlite::Error) -> Self {
        ChatError::Db(e)
    }
}

/// One line of the chat list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMeta {
    pub id: String,
    pub title: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// Size of the stored document in bytes.
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// The app's own JSON, returned as stored.
    pub data: serde_json::Value,
}

pub struct ChatStore {
    db: Connection,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl ChatStore {
    pub fn open(path: &Path) -> Result<ChatStore, ChatError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<ChatStore, ChatError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<ChatStore, ChatError> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS chats (
               id TEXT PRIMARY KEY,
               title TEXT NOT NULL,
               created_ms INTEGER NOT NULL,
               updated_ms INTEGER NOT NULL,
               data TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS chats_updated ON chats (updated_ms DESC);",
        )?;
        Ok(ChatStore { db })
    }

    /// All chats, most recently changed first.
    pub fn list(&self) -> Result<Vec<ChatMeta>, ChatError> {
        let mut stmt = self.db.prepare("SELECT id, title, created_ms, updated_ms, length(data) FROM chats ORDER BY updated_ms DESC, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(ChatMeta { id: r.get(0)?, title: r.get(1)?, created_ms: r.get(2)?, updated_ms: r.get(3)?, size: r.get::<_, i64>(4)? as u64 })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Chat>, ChatError> {
        if !valid_id(id) {
            return Err(ChatError::BadId);
        }
        let row = self
            .db
            .query_row("SELECT id, title, created_ms, updated_ms, data FROM chats WHERE id = ?1", params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?))
            })
            .optional()?;
        Ok(row.map(|(id, title, created_ms, updated_ms, data)| Chat {
            id,
            title,
            created_ms,
            updated_ms,
            data: serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
        }))
    }

    /// Creates or replaces a chat. The creation time of an existing chat is kept.
    pub fn put(&self, id: &str, title: &str, data: &serde_json::Value) -> Result<ChatMeta, ChatError> {
        if !valid_id(id) {
            return Err(ChatError::BadId);
        }
        let text = data.to_string();
        if text.len() > MAX_DATA_BYTES {
            return Err(ChatError::TooLarge(text.len()));
        }
        let title: String = title.trim().chars().take(200).collect();
        let title = if title.is_empty() { "Untitled chat".to_string() } else { title };
        let now = now_ms();
        self.db.execute(
            "INSERT INTO chats (id, title, created_ms, updated_ms, data) VALUES (?1, ?2, ?3, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, updated_ms = excluded.updated_ms, data = excluded.data",
            params![id, title, now, text],
        )?;
        let created_ms = self.db.query_row("SELECT created_ms FROM chats WHERE id = ?1", params![id], |r| r.get(0))?;
        Ok(ChatMeta { id: id.to_string(), title, created_ms, updated_ms: now, size: text.len() as u64 })
    }

    /// Returns whether a chat was removed.
    pub fn delete(&self, id: &str) -> Result<bool, ChatError> {
        if !valid_id(id) {
            return Err(ChatError::BadId);
        }
        Ok(self.db.execute("DELETE FROM chats WHERE id = ?1", params![id])? > 0)
    }

    /// Removes every chat; returns how many there were.
    pub fn delete_all(&self) -> Result<usize, ChatError> {
        Ok(self.db.execute("DELETE FROM chats", [])?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn put_get_list_and_delete() {
        let s = ChatStore::in_memory().unwrap();
        let a = s.put("a1", "  First  ", &json!({"messages": [{"role": "user", "content": "hi"}]})).unwrap();
        assert_eq!(a.title, "First");
        std::thread::sleep(std::time::Duration::from_millis(3));
        s.put("b2", "", &json!({"messages": []})).unwrap();
        let list = s.list().unwrap();
        assert_eq!(list.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b2", "a1"], "newest first");
        assert_eq!(list[0].title, "Untitled chat");
        assert_eq!(s.get("a1").unwrap().unwrap().data["messages"][0]["content"], "hi");
        assert!(s.get("zz").unwrap().is_none());
        assert!(s.delete("a1").unwrap());
        assert!(!s.delete("a1").unwrap());
        assert_eq!(s.delete_all().unwrap(), 1);
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn updating_keeps_the_creation_time_and_moves_the_chat_to_the_top() {
        let s = ChatStore::in_memory().unwrap();
        let first = s.put("a", "A", &json!(1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        s.put("b", "B", &json!(2)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let again = s.put("a", "A renamed", &json!(3)).unwrap();
        assert_eq!(again.created_ms, first.created_ms);
        assert!(again.updated_ms > first.updated_ms);
        let list = s.list().unwrap();
        assert_eq!(list[0].id, "a");
        assert_eq!(list[0].title, "A renamed");
    }

    #[test]
    fn rejects_bad_ids_and_oversized_chats() {
        let s = ChatStore::in_memory().unwrap();
        for bad in ["", "../x", "a b", "é", &"x".repeat(65)] {
            assert!(matches!(s.put(bad, "t", &json!({})), Err(ChatError::BadId)), "{bad:?}");
            assert!(matches!(s.get(bad), Err(ChatError::BadId)));
        }
        let big = json!("x".repeat(MAX_DATA_BYTES));
        assert!(matches!(s.put("ok", "t", &big), Err(ChatError::TooLarge(_))));
    }
}
