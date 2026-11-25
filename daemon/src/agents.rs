//! Agents the person makes: a saved way to use the chat model (a name, an instruction, the
//! abilities it may use, a first message to suggest). They live in `<data dir>/agents.json`, so
//! they outlive the app's own storage. Nothing here leaves the machine; the usage event for a
//! new agent carries its id and how many abilities it has, never its name or instruction.
//!
//! This file also serves the page a chat artifact is shown in (`artifact_frame`).

use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{fail, Shared};

pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_INSTRUCTION_CHARS: usize = 4000;
pub const MAX_STARTER_CHARS: usize = 300;
pub const MAX_ID_CHARS: usize = 40;
pub const MAX_AGENTS: usize = 200;

/// The abilities an agent can be given: the chat's own tools.
pub const TOOLS: &[&str] = &["generate_image", "read_aloud", "generate_video"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub instruction: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub starter: String,
    #[serde(default)]
    pub created_ms: i64,
    #[serde(default)]
    pub updated_ms: i64,
}

/// What `PUT /v1/agents/{id}` takes.
#[derive(Debug, Default, Deserialize)]
pub struct AgentBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    instruction: String,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    starter: String,
}

/// What `PUT /v1/agents` takes: the whole list.
#[derive(Debug, Deserialize)]
pub struct AgentsBody {
    agents: Vec<AgentEntry>,
}

#[derive(Debug, Deserialize)]
pub struct AgentEntry {
    id: String,
    #[serde(flatten)]
    body: AgentBody,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    v: u32,
    agents: Vec<Agent>,
}

pub fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Checks and tidies what was sent: the name and the suggestion are trimmed, an ability is
/// named once. `Err` says what is wrong in a sentence.
pub fn validate(id: &str, body: AgentBody) -> Result<Agent, String> {
    if !valid_id(id) {
        return Err(format!("an agent id is 1 to {MAX_ID_CHARS} lowercase letters, digits or '-'"));
    }
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err("an agent needs a name".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("the name is longer than {MAX_NAME_CHARS} characters"));
    }
    let instruction = body.instruction.trim().to_string();
    if instruction.is_empty() {
        return Err("an agent needs an instruction".into());
    }
    if instruction.chars().count() > MAX_INSTRUCTION_CHARS {
        return Err(format!("the instruction is longer than {MAX_INSTRUCTION_CHARS} characters"));
    }
    let starter = body.starter.trim().to_string();
    if starter.chars().count() > MAX_STARTER_CHARS {
        return Err(format!("the first message is longer than {MAX_STARTER_CHARS} characters"));
    }
    let mut tools: Vec<String> = Vec::new();
    for tool in body.tools {
        if !TOOLS.contains(&tool.as_str()) {
            return Err(format!("unknown ability: {}", tool.chars().take(40).collect::<String>()));
        }
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    Ok(Agent { id: id.to_string(), name, instruction, tools, starter, created_ms: 0, updated_ms: 0 })
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// One writer at a time: a save reads the file, changes the list and writes it back.
static LOCK: Mutex<()> = Mutex::new(());

fn file(root: &FsPath) -> PathBuf {
    root.join("agents.json")
}

/// The saved agents, in the order they were made. A missing file is an empty list; a file that
/// cannot be read as agents is an error, so a save never silently replaces it.
pub fn read(root: &FsPath) -> Result<Vec<Agent>, String> {
    match std::fs::read(file(root)) {
        Ok(bytes) => serde_json::from_slice::<Stored>(&bytes).map(|s| s.agents).map_err(|e| format!("agents.json cannot be read: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("agents.json cannot be read: {e}")),
    }
}

fn write(root: &FsPath, agents: &[Agent]) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(&json!({"v": 1, "agents": agents})).map_err(|e| e.to_string())?;
    // Written next to the target and renamed, so a reader never sees half a file.
    let tmp = root.join(".agents.json.tmp");
    std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file(root))).map_err(|e| format!("agents.json cannot be written: {e}"))
}

/// Saves one agent. Returns it as stored and whether it is new.
pub fn put(root: &FsPath, mut agent: Agent) -> Result<(Agent, bool), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut agents = read(root)?;
    let now = now_ms();
    agent.updated_ms = now;
    let created = match agents.iter_mut().find(|a| a.id == agent.id) {
        Some(known) => {
            agent.created_ms = known.created_ms;
            *known = agent.clone();
            false
        }
        None => {
            if agents.len() >= MAX_AGENTS {
                return Err(format!("there are already {MAX_AGENTS} agents"));
            }
            agent.created_ms = now;
            agents.push(agent.clone());
            true
        }
    };
    write(root, &agents)?;
    Ok((agent, created))
}

/// Replaces the whole list. Agents that were there keep the time they were made.
pub fn put_all(root: &FsPath, mut wanted: Vec<Agent>) -> Result<Vec<Agent>, String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let known = read(root)?;
    let now = now_ms();
    for agent in &mut wanted {
        agent.updated_ms = now;
        agent.created_ms = known.iter().find(|k| k.id == agent.id).map_or(now, |k| k.created_ms);
    }
    write(root, &wanted)?;
    Ok(wanted)
}

pub fn remove(root: &FsPath, id: &str) -> Result<bool, String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut agents = read(root)?;
    let before = agents.len();
    agents.retain(|a| a.id != id);
    if agents.len() == before {
        return Ok(false);
    }
    write(root, &agents)?;
    Ok(true)
}

pub fn remove_all(root: &FsPath) -> Result<usize, String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let agents = read(root)?;
    if !agents.is_empty() {
        write(root, &[])?;
    }
    Ok(agents.len())
}

fn broken(message: String) -> Response {
    fail(StatusCode::INTERNAL_SERVER_ERROR, message)
}

pub async fn list(State(app): State<Shared>) -> Response {
    match read(&app.root) {
        Ok(agents) => Json(json!({"agents": agents})).into_response(),
        Err(e) => broken(e),
    }
}

pub async fn get(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    if !valid_id(&id) {
        return fail(StatusCode::BAD_REQUEST, "bad agent id");
    }
    match read(&app.root) {
        Ok(agents) => match agents.into_iter().find(|a| a.id == id) {
            Some(agent) => Json(json!(agent)).into_response(),
            None => fail(StatusCode::NOT_FOUND, "no agent with that id"),
        },
        Err(e) => broken(e),
    }
}

pub async fn put_one(State(app): State<Shared>, Path(id): Path<String>, Json(body): Json<AgentBody>) -> Response {
    let agent = match validate(&id, body) {
        Ok(agent) => agent,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    match put(&app.root, agent) {
        Ok((agent, created)) => {
            if created {
                // The id and a count: never the name or the instruction.
                app.record("agent_created", json!({"agent": agent.id, "abilities": agent.tools.len()}));
            }
            Json(json!(agent)).into_response()
        }
        Err(e) if e.starts_with("there are already") => fail(StatusCode::CONFLICT, e),
        Err(e) => broken(e),
    }
}

pub async fn put_list(State(app): State<Shared>, Json(body): Json<AgentsBody>) -> Response {
    if body.agents.len() > MAX_AGENTS {
        return fail(StatusCode::BAD_REQUEST, format!("at most {MAX_AGENTS} agents"));
    }
    let mut wanted: Vec<Agent> = Vec::with_capacity(body.agents.len());
    for entry in body.agents {
        match validate(&entry.id, entry.body) {
            Ok(agent) if wanted.iter().any(|a| a.id == agent.id) => {
                return fail(StatusCode::BAD_REQUEST, format!("the id {} is there twice", agent.id));
            }
            Ok(agent) => wanted.push(agent),
            Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        }
    }
    match put_all(&app.root, wanted) {
        Ok(agents) => Json(json!({"agents": agents})).into_response(),
        Err(e) => broken(e),
    }
}

pub async fn delete_one(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    if !valid_id(&id) {
        return fail(StatusCode::BAD_REQUEST, "bad agent id");
    }
    match remove(&app.root, &id) {
        Ok(true) => Json(json!({"deleted": id})).into_response(),
        Ok(false) => fail(StatusCode::NOT_FOUND, "no agent with that id"),
        Err(e) => broken(e),
    }
}

pub async fn delete_all(State(app): State<Shared>) -> Response {
    match remove_all(&app.root) {
        Ok(n) => Json(json!({"deleted": n})).into_response(),
        Err(e) => broken(e),
    }
}

/// What a page shown as an artifact may do: its own styles and scripts, pictures it carries
/// itself, and nothing from anywhere else. `sandbox` gives it an origin of its own even when the
/// address is opened directly.
pub const FRAME_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data: blob:; \
                             form-action 'none'; base-uri 'none'; sandbox allow-scripts";

/// The empty page a chat artifact is written into. The app's own window forbids inline scripts
/// and every frame made from its text inherits that, so a page the model wrote is shown in this
/// one instead: it says when it is ready, the app posts the text, and it is written here under
/// `FRAME_CSP`. It holds no data and needs no token.
const FRAME_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>Artifact</title><script>\
addEventListener('message', function (event) {\
 if (event.source !== parent || typeof event.data !== 'string') return;\
 document.open(); document.write(event.data); document.close();\
});\
parent.postMessage('rigbrew:frame-ready', '*');\
</script>";

pub async fn artifact_frame() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_SECURITY_POLICY, FRAME_CSP)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(FRAME_PAGE))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(name: &str, instruction: &str, tools: &[&str], starter: &str) -> AgentBody {
        AgentBody {
            name: name.into(),
            instruction: instruction.into(),
            tools: tools.iter().map(|t| t.to_string()).collect(),
            starter: starter.into(),
        }
    }

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rigbrew-agents-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ids() {
        assert!(valid_id("editor"));
        assert!(valid_id("a-1"));
        assert!(valid_id(&"a".repeat(40)));
        assert!(!valid_id(""));
        assert!(!valid_id(&"a".repeat(41)));
        assert!(!valid_id("Editor"));
        assert!(!valid_id("a_b"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("é"));
    }

    #[test]
    fn validation() {
        let ok = validate("tutor", body("  Tutor ", " Explain step by step. ", &["read_aloud", "read_aloud"], " Teach me ")).unwrap();
        assert_eq!(ok.name, "Tutor");
        assert_eq!(ok.instruction, "Explain step by step.");
        assert_eq!(ok.tools, vec!["read_aloud"]);
        assert_eq!(ok.starter, "Teach me");

        assert!(validate("Bad Id", body("a", "b", &[], "")).is_err());
        assert!(validate("a", body("   ", "b", &[], "")).is_err());
        assert!(validate("a", body("a", "  ", &[], "")).is_err());
        assert!(validate("a", body("a", "b", &["run_shell"], "")).is_err());
        // Lengths are counted in characters, not bytes.
        assert!(validate("a", body(&"ü".repeat(60), "b", &[], "")).is_ok());
        assert!(validate("a", body(&"ü".repeat(61), "b", &[], "")).is_err());
        assert!(validate("a", body("a", &"ü".repeat(4000), &[], "")).is_ok());
        assert!(validate("a", body("a", &"x".repeat(4001), &[], "")).is_err());
        assert!(validate("a", body("a", "b", &[], &"x".repeat(301))).is_err());
    }

    #[test]
    fn store() {
        let root = temp();
        assert!(read(&root).unwrap().is_empty());
        let (first, created) = put(&root, validate("one", body("One", "Do one thing.", &["generate_image"], "")).unwrap()).unwrap();
        assert!(created);
        assert!(first.created_ms > 0);
        put(&root, validate("two", body("Two", "Do two things.", &[], "Hello")).unwrap()).unwrap();

        // Saving again changes it in place and keeps when it was made.
        let (again, created) = put(&root, validate("one", body("One more", "Do it better.", &[], "")).unwrap()).unwrap();
        assert!(!created);
        assert_eq!(again.created_ms, first.created_ms);
        let all = read(&root).unwrap();
        assert_eq!(all.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["One more", "Two"]);
        assert_eq!(all[1].starter, "Hello");

        assert!(remove(&root, "one").unwrap());
        assert!(!remove(&root, "one").unwrap());
        assert_eq!(read(&root).unwrap().len(), 1);

        let kept = put_all(
            &root,
            vec![validate("two", body("Two", "x", &[], "")).unwrap(), validate("three", body("Three", "y", &[], "")).unwrap()],
        )
        .unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(remove_all(&root).unwrap(), 2);
        assert!(read(&root).unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_broken_file_is_not_replaced() {
        let root = temp();
        std::fs::write(file(&root), "not json").unwrap();
        assert!(read(&root).is_err());
        assert!(put(&root, validate("one", body("One", "x", &[], "")).unwrap()).is_err());
        assert_eq!(std::fs::read_to_string(file(&root)).unwrap(), "not json");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_frame_allows_nothing_from_outside() {
        assert!(FRAME_CSP.starts_with("default-src 'none'"));
        assert!(FRAME_CSP.contains("sandbox allow-scripts"));
        assert!(!FRAME_CSP.contains("http") && !FRAME_CSP.contains("connect-src") && !FRAME_CSP.contains("allow-same-origin"));
    }
}
