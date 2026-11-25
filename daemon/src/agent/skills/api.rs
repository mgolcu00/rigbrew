//! The API behind the app's Skills sheet, and what the chat needs to use a skill.
//!
//!   GET    /v1/skills?folder=             every skill: where it is from, whether it is on, where it is offered
//!   POST   /v1/skills                     {name, description, body, copy_of?}: a new skill of the person's own
//!   GET    /v1/skills/{name}?folder=      one skill with its instructions and its other files
//!   PUT    /v1/skills/{name}              {enabled} switches it; {name, description, body} changes it
//!   DELETE /v1/skills/{name}              deletes the person's own skill (a built-in one comes back)
//!   POST   /v1/skills/{name}/use          what `use_skill` answers, for the chat
//!   GET    /v1/skills/{name}/file?path=   one of its other files, for the chat's `read_skill_file`
//!   POST   /v1/skills/import              {from: "folder" | "github", location, save?: [names]}

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{import, Skill};
use crate::{fail, Shared};

#[derive(Deserialize, Default)]
pub struct FolderQuery {
    folder: Option<String>,
}

fn folder_of(q: &FolderQuery) -> Option<std::path::PathBuf> {
    q.folder
        .as_deref()
        .filter(|f| !f.trim().is_empty())
        .and_then(|f| super::super::files::Folder::open(f).ok())
        .map(|f| f.root().to_path_buf())
}

fn row(skill: &Skill) -> Value {
    json!({
        "name": skill.name, "description": skill.description, "source": skill.source, "enabled": skill.enabled,
        "where": skill.scope.label(),
        // The person's own skill that stands in for one that comes with Rigbrew.
        "replaces_built_in": skill.source != "built-in" && super::is_built_in(&skill.name),
    })
}

fn find(app: &Shared, name: &str, folder: Option<&std::path::Path>) -> Option<Skill> {
    super::load_all(&app.root, folder).into_iter().find(|s| s.name == name)
}

fn missing() -> Response {
    fail(StatusCode::NOT_FOUND, "There is no skill with that name.")
}

pub async fn list(State(app): State<Shared>, Query(q): Query<FolderQuery>) -> Json<Value> {
    let all = super::load_all(&app.root, folder_of(&q).as_deref());
    Json(json!({"skills": all.iter().map(row).collect::<Vec<_>>(), "dir": super::own_dir(&app.root)}))
}

pub async fn get(State(app): State<Shared>, Path(name): Path<String>, Query(q): Query<FolderQuery>) -> Response {
    let Some(skill) = find(&app, &name, folder_of(&q).as_deref()) else { return missing() };
    // The whole instructions, as they are in the file: the form edits these.
    let file = skill.dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("SKILL.md")).ok());
    let text = file.as_deref().or_else(|| super::built_in_text(&skill.name)).unwrap_or("");
    let files: Vec<Value> = skill
        .dir
        .as_deref()
        .map(super::other_files)
        .unwrap_or_default()
        .into_iter()
        .map(|(path, bytes)| json!({"path": path, "bytes": bytes}))
        .collect();
    let mut out = row(&skill);
    out["body"] = json!(super::full_body(text));
    out["licence"] = json!(super::licence_of(text));
    out["files"] = json!(files);
    Json(out).into_response()
}

#[derive(Deserialize)]
pub struct NewSkill {
    name: String,
    description: String,
    body: String,
    /// The skill this is a copy of: its other files come along.
    #[serde(default)]
    copy_of: Option<String>,
}

pub async fn create(State(app): State<Shared>, Json(body): Json<NewSkill>) -> Response {
    let source = body.copy_of.as_deref().and_then(|name| find(&app, name, None));
    if let Err(e) = super::save(&app.root, None, &body.name, &body.description, &body.body) {
        return fail(StatusCode::BAD_REQUEST, e);
    }
    let Some(made) = find(&app, body.name.trim(), None) else { return fail(StatusCode::INTERNAL_SERVER_ERROR, "The skill was not saved.") };
    if let (Some(from), Some(to)) = (source.and_then(|s| s.dir), made.dir.as_deref()) {
        super::copy_files(&from, to);
    }
    app.record("skill_created", json!({"copy": body.copy_of.is_some(), "characters": body.body.chars().count()}));
    Json(row(&made)).into_response()
}

#[derive(Deserialize)]
pub struct ChangedSkill {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

pub async fn put(State(app): State<Shared>, Path(name): Path<String>, Json(body): Json<ChangedSkill>) -> Response {
    let Some(skill) = find(&app, &name, None) else { return missing() };
    let mut now = skill.name.clone();
    if let (Some(description), Some(text)) = (&body.description, &body.body) {
        let wanted = body.name.as_deref().unwrap_or(&skill.name).trim().to_string();
        // A built-in skill keeps its name: the person's version stands in for it.
        if skill.source == "built-in" && wanted != skill.name {
            return fail(
                StatusCode::BAD_REQUEST,
                "A skill that comes with Rigbrew keeps its name. Duplicate it to make one with another name.",
            );
        }
        if let Err(e) = super::save(&app.root, Some(&skill.name), &wanted, description, text) {
            return fail(StatusCode::BAD_REQUEST, e);
        }
        now = wanted;
    }
    if let Some(enabled) = body.enabled {
        if let Err(e) = super::set_enabled(&app.root, &now, enabled) {
            return fail(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
    }
    match find(&app, &now, None) {
        Some(skill) => Json(row(&skill)).into_response(),
        None => missing(),
    }
}

pub async fn delete(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    match super::delete(&app.root, &name) {
        Ok(true) => Json(json!({"deleted": name})).into_response(),
        Ok(false) if super::is_built_in(&name) => {
            fail(StatusCode::CONFLICT, "A skill that comes with Rigbrew cannot be deleted. Switch it off instead.")
        }
        Ok(false) => missing(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// `use_skill` for the chat: only a skill that is switched on and not only for Code.
pub async fn use_skill(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let all = super::load_all(&app.root, None);
    let usable: Vec<&Skill> = all.iter().filter(|s| s.enabled && s.scope != super::Scope::Code).collect();
    match usable.iter().find(|s| s.name == name.trim()) {
        Some(skill) => Json(json!({"ok": true, "text": super::used(&app, skill, "chat")})).into_response(),
        None => Json(json!({
            "ok": false,
            "text": format!("There is no skill named {}. The skills are: {}.", name.trim().chars().take(64).collect::<String>(),
                usable.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")),
        }))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
}

pub async fn file(State(app): State<Shared>, Path(name): Path<String>, Query(q): Query<FileQuery>) -> Response {
    let Some(skill) = find(&app, name.trim(), None).filter(|s| s.enabled) else {
        return Json(json!({"ok": false, "text": "There is no skill with that name."})).into_response();
    };
    match super::read_file(&skill, &q.path) {
        Ok(text) => Json(json!({"ok": true, "text": text})).into_response(),
        Err(why) => Json(json!({"ok": false, "text": why})).into_response(),
    }
}

#[derive(Deserialize)]
pub struct Import {
    /// "folder" or "github".
    from: String,
    location: String,
    /// The names to save. Without it the skills are only looked at.
    #[serde(default)]
    save: Option<Vec<String>>,
}

pub async fn import(State(app): State<Shared>, Json(body): Json<Import>) -> Response {
    let found = match body.from.as_str() {
        "folder" => {
            let location = body.location.clone();
            tokio::task::spawn_blocking(move || import::from_folder(&location)).await.unwrap_or_else(|_| Err("That did not work.".into()))
        }
        "github" => import::from_github(&app.net, &body.location, body.save.as_deref()).await,
        _ => Err("A skill comes from a folder or from GitHub.".into()),
    };
    let found = match found {
        Ok(found) => found,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    let mine: Vec<String> = super::own(&app.root).into_iter().map(|s| s.name).collect();
    let mut saved: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for (mut skill, files) in found {
        let wanted = body.save.as_ref().is_some_and(|names| names.contains(&skill.name));
        if wanted && skill.problem.is_none() {
            match import::store(&app.root, &skill, &files) {
                Ok(()) => saved.push(skill.name.clone()),
                Err(e) => skill.problem = Some(e),
            }
        } else if skill.problem.is_none() && mine.contains(&skill.name) {
            skill.problem = Some(format!("You already have a skill named {}. Delete yours first, or rename it.", skill.name));
        }
        let mut row = json!(skill);
        row["replaces_built_in"] = json!(super::is_built_in(&skill.name));
        rows.push(row);
    }
    if !saved.is_empty() {
        // Where from and how many: never the address, the folder or the names.
        app.record("skill_imported", json!({"source": body.from, "count": saved.len()}));
    }
    Json(json!({"found": rows, "saved": saved})).into_response()
}
