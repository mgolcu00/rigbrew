//! The community catalog: open models Rigbrew suggests, kept in one signed file that grows
//! without an app release.
//!
//! The file is `community.json` next to the manifest, with a minisign signature in
//! `community.json.minisig` made with the manifest's key. A copy is kept in
//! `<data dir>/state/` only after its signature checked out, and it is checked again every time
//! it is read, so the list also shows offline.
//!
//! The file is read loosely: an entry that points anywhere but `https://huggingface.co` or
//! `https://github.com` is left out, kinds and statuses this build has not heard of are kept as
//! something to look at, and every string and list is cut to a size, so a bad file cannot hurt.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::community::{self, CommunityPackage, Host};
use crate::manifest::{unsigned_allowed, verify_signature, ManifestError};

/// Where the catalog is published. Its signature is at the same address with `.minisig` added.
pub const DEFAULT_URL: &str = "https://models.rigbrew.dev/community.json";
/// Largest catalog file that is read.
pub const MAX_BYTES: usize = 512 * 1024;
/// Largest signature file that is read.
pub const MAX_SIG_BYTES: usize = 4 * 1024;
/// Most models taken from one file.
pub const MAX_ENTRIES: usize = 500;
/// Most extra links of one model.
pub const MAX_MORE: usize = 4;

const MAX_URL: usize = community::MAX_INPUT;

/// A model Rigbrew can add now: the app looks its link up like a pasted one.
pub const ADD: &str = "add";
/// A model to read about: Rigbrew cannot run it yet.
pub const LISTED: &str = "listed";

/// An extra link of a model ("Original", "Paper").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// One model of the catalog, after the checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// `chat`, `image`, `video`, `speech`, `listen`, `music`, `tool`, or whatever the file says.
    pub kind: String,
    pub by: String,
    pub url: String,
    pub license: String,
    pub about: String,
    /// [`ADD`] or [`LISTED`].
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub more: Vec<Link>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    /// The date the file says it was last changed.
    pub updated: String,
    pub models: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    TooBig(usize),
    Invalid(String),
    /// The signature is missing, malformed, made with another key, or does not match the bytes.
    BadSignature(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::TooBig(n) => write!(f, "the community catalog is {n} bytes, more than the {MAX_BYTES} allowed"),
            CatalogError::Invalid(why) => write!(f, "the community catalog cannot be read: {why}"),
            CatalogError::BadSignature(why) => write!(f, "the community catalog's signature check failed: {why}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// A string of the file, without control characters and cut to `max` characters.
fn text(entry: &Value, key: &str, max: usize) -> String {
    let s = entry.get(key).and_then(Value::as_str).unwrap_or("");
    let clean: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    clean.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

/// A link of the file as it is handed on: a plain `https` link to huggingface.co or github.com,
/// with no port and no sign-in in it. Anything else is `None`.
pub fn link(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() > MAX_URL || raw.contains('\\') || raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    if !(raw.starts_with("https://huggingface.co/") || raw.starts_with("https://github.com/")) {
        return None;
    }
    let url = reqwest::Url::parse(raw).ok()?;
    let plain = url.scheme() == "https"
        && matches!(url.host_str(), Some("huggingface.co" | "github.com"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none();
    plain.then(|| url.to_string())
}

/// The repo a link names, when it is one the lookup accepts.
fn repo_of(url: &str) -> Option<(Host, String, String)> {
    community::parse(url).ok().map(|t| (t.host, t.owner, t.repo))
}

fn entry(raw: &Value) -> Option<Entry> {
    let id = text(raw, "id", 80);
    let name = text(raw, "name", 80);
    let url = link(raw.get("url")?.as_str()?)?;
    if id.is_empty() || name.is_empty() {
        return None;
    }
    let kind = text(raw, "kind", 20).to_ascii_lowercase();
    // Only a chat model whose link the lookup accepts can be added; the rest is to read about.
    let status = if text(raw, "status", 20) == ADD && kind == "chat" && repo_of(&url).is_some() { ADD } else { LISTED };
    let note = Some(text(raw, "note", 120)).filter(|n| !n.is_empty());
    let more = raw
        .get("more")
        .and_then(Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|l| {
                    let label = text(l, "label", 40);
                    let url = link(l.get("url")?.as_str()?)?;
                    (!label.is_empty()).then_some(Link { label, url })
                })
                .take(MAX_MORE)
                .collect()
        })
        .unwrap_or_default();
    Some(Entry {
        id,
        name,
        kind,
        by: text(raw, "by", 80),
        url,
        license: text(raw, "license", 60),
        about: text(raw, "about", 200),
        status: status.to_string(),
        note,
        more,
    })
}

/// Reads a catalog. Entries that do not pass the checks are left out, never an error; the file
/// as a whole is refused only when it is too big, not JSON, or has no list of models.
pub fn parse(json: &str) -> Result<Catalog, CatalogError> {
    if json.len() > MAX_BYTES {
        return Err(CatalogError::TooBig(json.len()));
    }
    let file: Value = serde_json::from_str(json).map_err(|e| CatalogError::Invalid(e.to_string()))?;
    let Some(list) = file.get("models").and_then(Value::as_array) else {
        return Err(CatalogError::Invalid("it has no list of models".into()));
    };
    let mut models: Vec<Entry> = Vec::new();
    for raw in list {
        if models.len() >= MAX_ENTRIES {
            break;
        }
        // The id names a row in the app: the first entry with an id keeps it.
        if let Some(e) = entry(raw).filter(|e| models.iter().all(|m| m.id != e.id)) {
            models.push(e);
        }
    }
    Ok(Catalog { updated: text(&file, "updated", 32), models })
}

/// Reads a catalog only if `signature` (a minisign `.minisig` file) is a valid signature of
/// exactly these bytes by `public_key`.
pub fn parse_signed(json: &str, signature: &str, public_key: &str) -> Result<Catalog, CatalogError> {
    if json.len() > MAX_BYTES {
        return Err(CatalogError::TooBig(json.len()));
    }
    verify_signature(json.as_bytes(), signature, public_key).map_err(|e| match e {
        ManifestError::BadSignature(why) => CatalogError::BadSignature(why),
        other => CatalogError::BadSignature(other.to_string()),
    })?;
    parse(json)
}

/// Reads a catalog from a file with its `<file>.minisig`, for development. A bad signature is
/// always refused; a missing one only when [`unsigned_allowed`] says so.
pub fn load_file(path: &Path, public_key: &str) -> Result<Catalog, CatalogError> {
    let json = std::fs::read_to_string(path).map_err(|e| CatalogError::Invalid(format!("cannot read {}: {e}", path.display())))?;
    match std::fs::read_to_string(format!("{}.minisig", path.display())) {
        Ok(sig) => parse_signed(&json, &sig, public_key),
        Err(_) if unsigned_allowed() => parse(&json),
        Err(_) => Err(CatalogError::BadSignature(format!("{}.minisig is missing", path.display()))),
    }
}

/// Where the verified copy is kept: `<data dir>/state/community-catalog.json`, with its signature
/// next to it.
pub fn kept_path(root: &Path) -> PathBuf {
    root.join("state").join("community-catalog.json")
}

fn sig_path(root: &Path) -> PathBuf {
    root.join("state").join("community-catalog.json.minisig")
}

fn read_capped(path: &Path, max: usize) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > max as u64 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// The kept copy, if there is one and its signature still checks out.
pub fn kept(root: &Path, public_key: &str) -> Option<Catalog> {
    let json = read_capped(&kept_path(root), MAX_BYTES)?;
    let sig = read_capped(&sig_path(root), MAX_SIG_BYTES)?;
    parse_signed(&json, &sig, public_key).ok()
}

fn keep(root: &Path, json: &str, sig: &str) -> std::io::Result<()> {
    let (file, sig_file) = (kept_path(root), sig_path(root));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // The signature goes first, so a crash in between leaves a pair that is refused, not a new
    // file with an old signature that happens to match nothing.
    for (path, body) in [(&sig_file, sig), (&file, json)] {
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// What the service answers with after trying to fetch the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settled {
    pub catalog: Catalog,
    /// The catalog is the kept copy (or nothing), because the fetch or its signature failed.
    pub stale: bool,
    /// Why the fetch did not count, for the service's log.
    pub problem: Option<String>,
}

/// Decides what to show. `fetched` is the file and its signature as they came from the network,
/// or why they did not come. A file whose signature checks out is kept and shown; otherwise the
/// kept copy is shown as stale, and with nothing kept the list is empty.
pub fn settle(root: &Path, fetched: Result<(String, String), String>, public_key: &str) -> Settled {
    let problem = match fetched {
        Ok((json, sig)) => match parse_signed(&json, &sig, public_key) {
            Ok(catalog) => {
                let problem = keep(root, &json, &sig).err().map(|e| format!("could not keep a copy: {e}"));
                return Settled { catalog, stale: false, problem };
            }
            Err(e) => e.to_string(),
        },
        Err(e) => e,
    };
    Settled { catalog: kept(root, public_key).unwrap_or_default(), stale: true, problem: Some(problem) }
}

/// Whether a model of the catalog is already among the person's community models: the same
/// repo on the same site, in any file of it.
pub fn added(entry: &Entry, have: &[CommunityPackage]) -> bool {
    if entry.status != ADD {
        return false;
    }
    let Some((host, owner, repo)) = repo_of(&entry.url) else { return false };
    have.iter().any(|c| c.host == host && c.owner.eq_ignore_ascii_case(&owner) && c.repo.eq_ignore_ascii_case(&repo))
}
