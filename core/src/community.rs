//! Community models: open chat models a person adds by pasting a link.
//!
//! They are not in the signed manifest and Rigbrew has not tested them. A link to a Hugging Face
//! repo, to a GGUF file in one, or to a GitHub repo or release is looked up, the GGUF files it
//! holds are listed, and the one the person picks becomes a chat package of its own. Those
//! packages are kept in `<data dir>/community.json`; their files live under
//! `<data dir>/community/<owner>__<repo>/`.
//!
//! Nothing here trusts what it reads: links are taken apart and checked piece by piece, only
//! `https://huggingface.co` and `https://github.com` are accepted, file names must be clean
//! relative paths ending in `.gguf`, and answers from the two sites are size-limited.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hardware::Backend;
use crate::manifest::{Access, Engine, FileRef, Modality, Package, Source, Variant};

/// Longest link or id that is looked at.
pub const MAX_INPUT: usize = 500;
/// Largest model file that is offered.
pub const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024 * 1024;
/// Largest answer read from a site's API.
const MAX_API_BYTES: usize = 16 * 1024 * 1024;
/// Largest README read from GitHub.
const MAX_README_BYTES: usize = 1024 * 1024;
/// Most GGUF files listed for one repo.
const MAX_FILES: usize = 200;
/// Most Hugging Face links reported from a README.
const MAX_LINKS: usize = 12;

const HF: &str = "https://huggingface.co";
const GH: &str = "https://github.com";
const GH_API: &str = "https://api.github.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Host {
    HuggingFace,
    GitHub,
}

impl Host {
    /// The site's name as people know it.
    pub fn label(self) -> &'static str {
        match self {
            Host::HuggingFace => "Hugging Face",
            Host::GitHub => "GitHub",
        }
    }
}

/// What a pasted link or id points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: Host,
    pub owner: String,
    pub repo: String,
    /// A Hugging Face revision or a GitHub release tag, when the link names one.
    pub rev: Option<String>,
    /// A file the link names, relative to the repo (Hugging Face) or a release asset (GitHub).
    pub file: Option<String>,
}

/// Why a link, an id or a file name was refused, in a sentence for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

fn refuse<T>(why: impl Into<String>) -> Result<T, Refused> {
    Err(Refused(why.into()))
}

/// An owner, repo, revision or tag: letters, digits, `.`, `_`, `-` (and `+` in tags), not
/// starting with a dot, so it can never be `.` or `..` or hide a path.
fn clean_name(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && !s.starts_with('.')
        && !s.contains("..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

/// Checks a file name from a link or a site's answer: a clean relative path ending in `.gguf`.
pub fn check_file(path: &str) -> Result<(), Refused> {
    if path.is_empty() || path.len() > 255 {
        return refuse("That file name is empty or too long.");
    }
    if !path.to_ascii_lowercase().ends_with(".gguf") {
        return refuse("Only GGUF files (ending in .gguf) can be added.");
    }
    if path.split('/').any(|part| !clean_name(part, 200)) {
        return refuse("That file name has characters Rigbrew does not accept.");
    }
    Ok(())
}

/// First path segments on huggingface.co that are not model owners.
const HF_RESERVED: &[&str] = &[
    "api",
    "datasets",
    "spaces",
    "models",
    "docs",
    "collections",
    "organizations",
    "settings",
    "join",
    "login",
    "blog",
    "papers",
    "posts",
    "tasks",
    "pricing",
    "enterprise",
    "chat",
    "learn",
];

/// Takes a pasted link or id apart. Accepts `owner/name`, `owner/name/file.gguf`, links to
/// `https://huggingface.co/owner/name` (also `/blob/`, `/resolve/` and `/tree/` links), and
/// links to `https://github.com/owner/name` (also to its releases, a tag or a release file).
pub fn parse(input: &str) -> Result<Target, Refused> {
    let input = input.trim();
    if input.is_empty() {
        return refuse("Paste a link to a model on Hugging Face or GitHub.");
    }
    if input.len() > MAX_INPUT || input.contains("..") || input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("That does not look like a link to a model.");
    }
    // Escapes and backslashes are rewritten when a link is read (`%2e%2e` becomes `..`): refuse
    // them as they are typed, everywhere before the query.
    if input.split(['?', '#']).next().is_some_and(|path| path.contains(['%', '\\'])) {
        return refuse("That link has characters Rigbrew does not accept.");
    }
    let lower = input.to_ascii_lowercase();
    let (host, path) = if lower.starts_with("https://") {
        let url = reqwest::Url::parse(input).map_err(|_| Refused("That link cannot be read.".into()))?;
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return refuse("Only plain links to huggingface.co and github.com are accepted.");
        }
        let host = match url.host_str() {
            Some("huggingface.co") => Host::HuggingFace,
            Some("github.com") => Host::GitHub,
            _ => return refuse("Only links to huggingface.co and github.com are accepted."),
        };
        (host, url.path().to_string())
    } else if lower.contains("://") {
        return refuse("Only https links to huggingface.co and github.com are accepted.");
    } else if let Some(rest) = lower.strip_prefix("huggingface.co/") {
        (Host::HuggingFace, input[input.len() - rest.len()..].to_string())
    } else if let Some(rest) = lower.strip_prefix("github.com/") {
        (Host::GitHub, input[input.len() - rest.len()..].to_string())
    } else {
        // A bare id is a Hugging Face one. A query or fragment has no place in it.
        if input.contains(['?', '#']) {
            return refuse("That does not look like a model id.");
        }
        (Host::HuggingFace, input.to_string())
    };
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let (Some(owner), Some(repo)) = (parts.first(), parts.get(1)) else {
        return refuse("The link has to name a model: an owner and a name, like bartowski/SmolLM2-360M-Instruct-GGUF.");
    };
    let repo = if host == Host::GitHub { repo.strip_suffix(".git").unwrap_or(repo) } else { repo };
    if host == Host::HuggingFace && HF_RESERVED.contains(&owner.to_ascii_lowercase().as_str()) {
        return refuse("That link is not a model page. Open the model on Hugging Face and copy its link.");
    }
    if !clean_name(owner, 96) || !clean_name(repo, 96) {
        return refuse("That owner or model name has characters Rigbrew does not accept.");
    }
    let mut target = Target { host, owner: owner.to_string(), repo: repo.to_string(), rev: None, file: None };
    let rest = &parts[2..];
    let rev = |r: &str| {
        if clean_name(r, 128) {
            Ok(Some(r.to_string()))
        } else {
            refuse("That version name has characters Rigbrew does not accept.")
        }
    };
    let file = |segments: &[&str]| -> Result<Option<String>, Refused> {
        if segments.is_empty() {
            return Ok(None);
        }
        let path = segments.join("/");
        check_file(&path)?;
        Ok(Some(path))
    };
    match host {
        Host::HuggingFace => match rest {
            [] => {}
            ["resolve" | "blob", r, name @ ..] => {
                target.rev = rev(r)?;
                target.file = file(name)?;
            }
            // A folder inside the repo is not a file; the whole repo is listed.
            ["tree", r, ..] => target.rev = rev(r)?,
            // `owner/name/file.gguf`, the short way to name a file.
            name if lower.ends_with(".gguf") => target.file = file(name)?,
            _ => {}
        },
        Host::GitHub => match rest {
            ["releases", "tag", t] => target.rev = rev(t)?,
            ["releases", "download", t, name] => {
                target.rev = rev(t)?;
                target.file = file(&[name])?;
            }
            _ => {}
        },
    }
    Ok(target)
}

/// One GGUF file a repo offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundFile {
    pub file: String,
    pub size: u64,
    /// From the site, when it gives one. The download is checked against it.
    pub sha256: Option<String>,
}

/// What a lookup found.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub host: Host,
    pub owner: String,
    pub repo: String,
    /// The page a person can open.
    pub url: String,
    /// The exact revision (Hugging Face commit) or release tag the files belong to.
    pub rev: String,
    pub license: Option<String>,
    /// The owner asks people to sign in and agree before downloading; Rigbrew cannot do that.
    pub gated: bool,
    /// Single-file chat models, smallest first.
    pub files: Vec<FoundFile>,
    /// Vision parts (`mmproj` files): with one, the model also takes pictures.
    pub projectors: Vec<FoundFile>,
    /// GGUF files that were left out: parts of a split model, vision parts and other files that
    /// are not a chat model.
    pub skipped: usize,
    /// The file the link named, when it is one of `files`.
    pub wanted: Option<String>,
    /// Hugging Face models linked from a GitHub README, when the repo has no GGUF files itself.
    pub links: Vec<String>,
}

impl Found {
    /// Where `files` are downloaded from: `<base>/<file>`.
    pub fn base_url(&self) -> String {
        match self.host {
            Host::HuggingFace => format!("{HF}/{}/{}/resolve/{}", self.owner, self.repo, self.rev),
            Host::GitHub => format!("{GH}/{}/{}/releases/download/{}", self.owner, self.repo, self.rev),
        }
    }
}

/// Memory a GGUF chat model needs: its size on disk and about 15% for the context and buffers.
pub fn needs_mb(size: u64) -> u64 {
    (size + size / 100 * 15).div_ceil(1024 * 1024)
}

/// The largest file that fits `budget_mb`, if any does: bigger files of one model are closer to
/// the original.
pub fn pick(files: &[FoundFile], budget_mb: u64) -> Option<&FoundFile> {
    files.iter().filter(|f| needs_mb(f.size) <= budget_mb).max_by_key(|f| f.size)
}

/// Whether a GGUF file is one piece of a model split over several files (`-00001-of-00003.gguf`).
fn is_part(name: &str) -> bool {
    let stem = name.to_ascii_lowercase();
    let stem = stem.trim_end_matches(".gguf");
    let Some((head, total)) = stem.rsplit_once("-of-") else { return false };
    let digits = |s: &str| s.len() >= 3 && s.bytes().all(|b| b.is_ascii_digit());
    digits(total) && head.rsplit_once('-').is_some_and(|(_, n)| digits(n))
}

/// What a repo offers: the chat model files, the vision parts, and how many GGUF files were left out.
type Offered = (Vec<FoundFile>, Vec<FoundFile>, usize);

/// Sorts `(name, size, sha256)` candidates into the files offered, the vision parts, and a count
/// of those left out.
fn offer(candidates: Vec<(String, u64, Option<String>)>) -> Offered {
    let (mut files, mut projectors) = (Vec::new(), Vec::new());
    let mut skipped = 0;
    for (file, size, sha256) in candidates {
        if check_file(&file).is_err() {
            continue;
        }
        let base = file.rsplit('/').next().unwrap_or(&file).to_ascii_lowercase();
        let sha256 = sha256.map(|s| s.to_ascii_lowercase()).filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()));
        if base.contains("mmproj") && !is_part(&file) && size > 0 && size <= MAX_FILE_BYTES {
            skipped += 1;
            projectors.push(FoundFile { file, size, sha256 });
            continue;
        }
        // An importance matrix is a file for making quantized models, not a model.
        if is_part(&file) || base.contains("imatrix") || size == 0 || size > MAX_FILE_BYTES {
            skipped += 1;
            continue;
        }
        files.push(FoundFile { file, size, sha256 });
    }
    files.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.file.cmp(&b.file)));
    files.truncate(MAX_FILES);
    projectors.truncate(MAX_FILES);
    (files, projectors, skipped)
}

/// How well a vision part's number format keeps a picture, best first: 8 and 16 bits see as well
/// as 32 at a fraction of the size, fewer bits blur what the model sees.
fn projector_rank(name: &str) -> u8 {
    let n = name.to_ascii_lowercase();
    if n.contains("q8") {
        0
    } else if n.contains("f16") {
        1
    } else if n.contains("f32") {
        2
    } else {
        3
    }
}

/// The letters and digits of a file name that say which model it is: without the folder, the
/// ending, `mmproj` and the number format.
fn family(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    let base = base.trim_end_matches(".gguf").replace("mmproj", "");
    base.split(['-', '_', '.'])
        .filter(|part| {
            let quant = part.starts_with('q') && part[1..].chars().next().is_some_and(|c| c.is_ascii_digit());
            !(quant || matches!(*part, "f16" | "bf16" | "f32" | "k" | "m" | "s" | "l" | "xl" | "0" | "1"))
        })
        .collect()
}

/// The vision part that goes with a model file of the repo, when it has one: the one in the same
/// folder whose name shares the most with the model's, in the best format (see `projector_rank`),
/// the smaller of two equal ones.
pub fn projector_for<'a>(projectors: &'a [FoundFile], model: &str) -> Option<&'a FoundFile> {
    let folder = |p: &str| p.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
    let wanted = family(model);
    let shared = |p: &FoundFile| family(&p.file).chars().zip(wanted.chars()).take_while(|(a, b)| a == b).count();
    projectors.iter().max_by_key(|p| {
        (folder(&p.file) == folder(model), shared(p), std::cmp::Reverse(projector_rank(&p.file)), std::cmp::Reverse(p.size))
    })
}

/// A licence name out of a Hugging Face model's card, or its tags.
fn hf_license(model: &Value) -> Option<String> {
    let card = &model["cardData"];
    let named = match &card["license"] {
        Value::String(s) => Some(s.clone()),
        Value::Array(list) => list.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    let named = match named.as_deref() {
        Some("other") | None => card["license_name"].as_str().map(str::to_string).or(named),
        _ => named,
    };
    named
        .or_else(|| model["tags"].as_array()?.iter().filter_map(Value::as_str).find_map(|t| t.strip_prefix("license:")).map(str::to_string))
        .map(|s| s.chars().filter(|c| !c.is_control()).take(60).collect::<String>())
        .filter(|s| !s.trim().is_empty())
}

/// Reads the answer of `https://huggingface.co/api/models/<id>?blobs=true`.
pub fn read_hf_model(target: &Target, model: &Value) -> Result<Found, Refused> {
    let rev =
        model["sha"].as_str().filter(|s| clean_name(s, 128)).map(str::to_string).or_else(|| target.rev.clone()).unwrap_or("main".into());
    let candidates = model["siblings"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let name = s["rfilename"].as_str()?;
                    let size = s["lfs"]["size"].as_u64().or_else(|| s["size"].as_u64()).unwrap_or(0);
                    Some((name.to_string(), size, s["lfs"]["sha256"].as_str().map(str::to_string)))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (files, projectors, skipped) = offer(candidates);
    let wanted = target.file.clone().filter(|w| files.iter().any(|f| f.file == *w));
    Ok(Found {
        host: Host::HuggingFace,
        owner: target.owner.clone(),
        repo: target.repo.clone(),
        url: format!("{HF}/{}/{}", target.owner, target.repo),
        rev,
        license: hf_license(model),
        gated: !matches!(model["gated"], Value::Null | Value::Bool(false)),
        files,
        projectors,
        skipped,
        wanted,
        links: Vec::new(),
    })
}

/// Reads a GitHub release (`/repos/<owner>/<repo>/releases/...`): its tag and GGUF assets.
pub fn read_github_release(release: &Value) -> Option<(String, Offered)> {
    let tag = release["tag_name"].as_str().filter(|t| clean_name(t, 128))?.to_string();
    let candidates = release["assets"]
        .as_array()?
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            let sha = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_string);
            Some((name.to_string(), a["size"].as_u64().unwrap_or(0), sha))
        })
        .collect();
    Some((tag, offer(candidates)))
}

/// Hugging Face models a text links to, as `owner/name`, in the order they appear.
pub fn hf_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in text.match_indices("huggingface.co/") {
        let rest = &text[at + "huggingface.co/".len()..];
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))).unwrap_or(rest.len());
        let mut parts = rest[..end].split('/');
        let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else { continue };
        let repo = repo.trim_end_matches('.');
        if !clean_name(owner, 96) || !clean_name(repo, 96) || HF_RESERVED.contains(&owner.to_ascii_lowercase().as_str()) {
            continue;
        }
        let id = format!("{owner}/{repo}");
        if !out.contains(&id) {
            out.push(id);
        }
        if out.len() == MAX_LINKS {
            break;
        }
    }
    out
}

/// Why a lookup did not work, in a sentence for the person.
#[derive(Debug)]
pub enum LookupError {
    /// The link or id is not one Rigbrew accepts.
    Refused(Refused),
    /// The site has no such repo, or it is private.
    NotFound(String),
    /// The site could not be reached or answered something unexpected.
    Site(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::Refused(r) => r.fmt(f),
            LookupError::NotFound(why) | LookupError::Site(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for LookupError {}

impl From<Refused> for LookupError {
    fn from(r: Refused) -> Self {
        LookupError::Refused(r)
    }
}

/// Fetches `url` and returns its body, refusing anything longer than `cap`.
async fn get(client: &reqwest::Client, url: &str, accept: &str, site: &str, cap: usize) -> Result<Vec<u8>, LookupError> {
    let unreachable = |e: reqwest::Error| LookupError::Site(format!("{site} could not be reached: {e}"));
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(unreachable)?;
    match resp.status().as_u16() {
        200 => {}
        401 | 404 => {
            return Err(LookupError::NotFound(format!(
                "{site} has no public model at that link. It may be private or the name may be wrong."
            )))
        }
        403 | 429 => {
            return Err(LookupError::Site(format!(
                "{site} is not answering more questions from this computer right now. Try again in a while."
            )))
        }
        other => return Err(LookupError::Site(format!("{site} answered HTTP {other}."))),
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(unreachable)?;
        if body.len() + chunk.len() > cap {
            return Err(LookupError::Site(format!("{site} sent more than Rigbrew reads for one model.")));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn get_json(client: &reqwest::Client, url: &str, site: &str) -> Result<Value, LookupError> {
    let body = get(client, url, "application/json", site, MAX_API_BYTES).await?;
    serde_json::from_slice(&body).map_err(|_| LookupError::Site(format!("{site} sent an answer Rigbrew cannot read.")))
}

/// Asks the site what a target holds.
pub async fn lookup(client: &reqwest::Client, target: &Target) -> Result<Found, LookupError> {
    match target.host {
        Host::HuggingFace => {
            let rev = target.rev.as_ref().map(|r| format!("/revision/{r}")).unwrap_or_default();
            let url = format!("{HF}/api/models/{}/{}{rev}?blobs=true", target.owner, target.repo);
            Ok(read_hf_model(target, &get_json(client, &url, "Hugging Face").await?)?)
        }
        Host::GitHub => lookup_github(client, target).await,
    }
}

async fn lookup_github(client: &reqwest::Client, target: &Target) -> Result<Found, LookupError> {
    let repo_api = format!("{GH_API}/repos/{}/{}", target.owner, target.repo);
    let repo = get_json(client, &repo_api, "GitHub").await?;
    let license = repo["license"]["spdx_id"]
        .as_str()
        .filter(|s| *s != "NOASSERTION")
        .or_else(|| repo["license"]["name"].as_str())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(60).collect::<String>());
    let release_url = match &target.rev {
        Some(tag) => format!("{repo_api}/releases/tags/{tag}"),
        None => format!("{repo_api}/releases/latest"),
    };
    let release = match get_json(client, &release_url, "GitHub").await {
        Ok(v) => read_github_release(&v),
        // A repo without releases is still a repo: its README may point somewhere.
        Err(LookupError::NotFound(_)) => None,
        Err(e) => return Err(e),
    };
    let (rev, (files, projectors, skipped)) = release.unwrap_or_default();
    let mut found = Found {
        host: Host::GitHub,
        owner: target.owner.clone(),
        repo: target.repo.clone(),
        url: format!("{GH}/{}/{}", target.owner, target.repo),
        rev,
        license,
        gated: false,
        wanted: target.file.clone().filter(|w| files.iter().any(|f| f.file == *w)),
        files,
        projectors,
        skipped,
        links: Vec::new(),
    };
    if found.files.is_empty() {
        let readme = get(client, &format!("{repo_api}/readme"), "application/vnd.github.raw+json", "GitHub", MAX_README_BYTES).await;
        if let Ok(text) = readme {
            found.links = hf_links(&String::from_utf8_lossy(&text));
        }
    }
    Ok(found)
}

/// A community model as it is kept on disk: the package the rest of Rigbrew works with, and
/// where its files come from and go.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityPackage {
    pub package: Package,
    pub host: Host,
    pub owner: String,
    pub repo: String,
    /// Files are downloaded from `<base_url>/<path>`.
    pub base_url: String,
    /// Folder under `<data dir>/community/`.
    pub dir: String,
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The package id a file of a repo gets: `community-<owner>-<repo>-<file>`, in a-z, 0-9 and `-`.
pub fn package_id(owner: &str, repo: &str, file: &str) -> String {
    let stem = file.rsplit('/').next().unwrap_or(file);
    let stem = &stem[..stem.len().saturating_sub(".gguf".len())];
    let mut id = format!("community-{}-{}-{}", slug(owner), slug(repo), slug(stem));
    id.truncate(160);
    id.trim_end_matches('-').to_string()
}

/// Makes the package for one file of a lookup. It is a chat model for the llama.cpp engine with
/// a single variant, and the repo's vision part for it when there is one (see `projector_for`):
/// with that, the model takes pictures.
pub fn package(found: &Found, file: &FoundFile) -> CommunityPackage {
    let stem = file.file.rsplit('/').next().unwrap_or(&file.file);
    let name = stem[..stem.len().saturating_sub(".gguf".len())].to_string();
    let projector = projector_for(&found.projectors, &file.file);
    let mem = needs_mb(file.size + projector.map_or(0, |p| p.size));
    let mut files =
        vec![FileRef { role: "model".into(), path: file.file.clone(), size: file.size, sha256: file.sha256.clone().unwrap_or_default() }];
    if let Some(p) = projector {
        files.push(FileRef { role: "mmproj".into(), path: p.file.clone(), size: p.size, sha256: p.sha256.clone().unwrap_or_default() });
    }
    CommunityPackage {
        package: Package {
            id: package_id(&found.owner, &found.repo, &file.file),
            name,
            modality: Modality::Chat,
            version: 1,
            engine: Engine::Llama,
            access: Access::Public,
            source: Source {
                name: format!("{}/{}", found.owner, found.repo),
                url: found.url.clone(),
                license: found.license.clone().unwrap_or_else(|| "no licence stated".into()),
                changes: String::new(),
            },
            quality_floor: 0.0,
            variants: vec![Variant {
                id: "gguf".into(),
                files,
                accel_mem_mb: mem,
                cpu_mem_mb: Some(mem),
                backends: vec![Backend::Metal, Backend::Cuda, Backend::Vulkan],
                quality: 0.0,
                metrics: serde_json::json!({"quality_measured": false}),
            }],
            engine_args: Vec::new(),
            defaults: Value::Null,
            credit: None,
            terms: None,
        },
        host: found.host,
        owner: found.owner.clone(),
        repo: found.repo.clone(),
        base_url: found.base_url(),
        dir: format!("{}__{}", found.owner, found.repo),
    }
}

impl CommunityPackage {
    /// Whether an entry read from disk is one this module could have written: the same checks
    /// as for a pasted link, so an edited `community.json` cannot point outside its folder or
    /// at another site.
    fn sound(&self) -> bool {
        let base_ok = match self.host {
            Host::HuggingFace => self.base_url.starts_with(&format!("{HF}/{}/{}/resolve/", self.owner, self.repo)),
            Host::GitHub => self.base_url.starts_with(&format!("{GH}/{}/{}/releases/download/", self.owner, self.repo)),
        };
        let p = &self.package;
        base_ok
            && clean_name(&self.owner, 96)
            && clean_name(&self.repo, 96)
            && self.base_url.rsplit('/').next().is_some_and(|rev| clean_name(rev, 128))
            && self.dir == format!("{}__{}", self.owner, self.repo)
            && p.id.starts_with("community-")
            && p.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && p.modality == Modality::Chat
            && p.engine == Engine::Llama
            && p.variants.len() == 1
            && p.variants[0].files.first().is_some_and(|f| f.role == "model")
            && p.variants[0].files.iter().skip(1).all(|f| f.role == "mmproj")
            && p.variants[0].files.len() <= 2
            && p.variants[0].files.iter().all(|f| {
                check_file(&f.path).is_ok()
                    && (f.sha256.is_empty() || (f.sha256.len() == 64 && f.sha256.bytes().all(|b| b.is_ascii_hexdigit())))
            })
    }
}

/// The community models of this install, kept in `<data dir>/community.json`.
#[derive(Debug, Default)]
pub struct Store {
    path: PathBuf,
    list: Vec<CommunityPackage>,
}

#[derive(Serialize, Deserialize, Default)]
struct OnDisk {
    packages: Vec<CommunityPackage>,
}

impl Store {
    /// Reads the list. A missing or unreadable file is an empty list; entries that do not pass
    /// the checks are left out.
    pub fn load(root: &Path) -> Store {
        let path = root.join("community.json");
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<OnDisk>(&t).ok())
            .map(|d| d.packages.into_iter().filter(CommunityPackage::sound).collect())
            .unwrap_or_default();
        Store { path, list }
    }

    pub fn list(&self) -> &[CommunityPackage] {
        &self.list
    }

    pub fn get(&self, id: &str) -> Option<&CommunityPackage> {
        self.list.iter().find(|c| c.package.id == id)
    }

    /// Adds a model, in place of one with the same id.
    pub fn put(&mut self, entry: CommunityPackage) -> std::io::Result<()> {
        self.list.retain(|c| c.package.id != entry.package.id);
        self.list.push(entry);
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> std::io::Result<Option<CommunityPackage>> {
        let Some(pos) = self.list.iter().position(|c| c.package.id == id) else { return Ok(None) };
        let gone = self.list.remove(pos);
        self.save()?;
        Ok(Some(gone))
    }

    fn save(&self) -> std::io::Result<()> {
        // Write to a temporary file and rename, so a crash never leaves a half-written list.
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&OnDisk { packages: self.list.clone() }).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// The folder a community model's files live in.
pub fn files_dir(root: &Path, entry: &CommunityPackage) -> PathBuf {
    root.join("community").join(&entry.dir)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn hf(owner: &str, repo: &str, rev: Option<&str>, file: Option<&str>) -> Target {
        Target { host: Host::HuggingFace, owner: owner.into(), repo: repo.into(), rev: rev.map(Into::into), file: file.map(Into::into) }
    }

    #[test]
    fn hugging_face_links_and_ids() {
        let repo = hf("bartowski", "SmolLM2-360M-Instruct-GGUF", None, None);
        for input in [
            "bartowski/SmolLM2-360M-Instruct-GGUF",
            "  https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF  ",
            "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/",
            "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF?library=llama-cpp",
            "huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF",
            "HTTPS://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF",
        ] {
            assert_eq!(parse(input).unwrap(), repo, "{input}");
        }
        assert_eq!(parse("https://huggingface.co/a/b/tree/main").unwrap(), hf("a", "b", Some("main"), None));
        let file = "SmolLM2-360M-Instruct-Q4_K_M.gguf";
        for input in [
            format!("https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/{file}"),
            format!("https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/{file}?download=true"),
            format!("https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/blob/main/{file}"),
        ] {
            assert_eq!(parse(&input).unwrap(), hf("bartowski", "SmolLM2-360M-Instruct-GGUF", Some("main"), Some(file)), "{input}");
        }
        assert_eq!(parse(&format!("a/b/{file}")).unwrap(), hf("a", "b", None, Some(file)));
        assert_eq!(parse("a/b/resolve/v1.0/sub/m.gguf").unwrap(), hf("a", "b", Some("v1.0"), Some("sub/m.gguf")));
    }

    #[test]
    fn github_links() {
        let gh = |rev: Option<&str>, file: Option<&str>| Target {
            host: Host::GitHub,
            owner: "someone".into(),
            repo: "tiny".into(),
            rev: rev.map(Into::into),
            file: file.map(Into::into),
        };
        assert_eq!(parse("https://github.com/someone/tiny").unwrap(), gh(None, None));
        assert_eq!(parse("https://github.com/someone/tiny.git").unwrap(), gh(None, None));
        assert_eq!(parse("github.com/someone/tiny/releases").unwrap(), gh(None, None));
        assert_eq!(parse("https://github.com/someone/tiny/tree/main/docs").unwrap(), gh(None, None));
        assert_eq!(parse("https://github.com/someone/tiny/releases/tag/v1.2").unwrap(), gh(Some("v1.2"), None));
        assert_eq!(parse("https://github.com/someone/tiny/releases/download/v1.2/m.gguf").unwrap(), gh(Some("v1.2"), Some("m.gguf")));
    }

    #[test]
    fn refuses_other_hosts_and_unsafe_names() {
        for input in [
            "",
            "   ",
            "http://huggingface.co/a/b",
            "ftp://huggingface.co/a/b",
            "file:///etc/passwd",
            "https://example.com/a/b",
            "https://huggingface.co.evil.example/a/b",
            "https://evil.example/huggingface.co/a/b",
            "https://user:pw@huggingface.co/a/b",
            "https://huggingface.co@evil.example/a/b",
            "https://huggingface.co:8443/a/b",
            "https://api.github.com/repos/a/b",
            "https://raw.githubusercontent.com/a/b/main/m.gguf",
            "https://huggingface.co/",
            "https://huggingface.co/justone",
            "https://huggingface.co/datasets/a/b",
            "https://huggingface.co/spaces/a/b",
            "https://huggingface.co/api/models/a",
            "a",
            "a/b c",
            "a/b\nc",
            "a/..",
            "../b",
            "a/.hidden",
            "a/b?x=1",
            "https://huggingface.co/a/b/resolve/main/../../etc/passwd.gguf",
            "https://huggingface.co/a/b/resolve/main/%2e%2e/x.gguf",
            "https://huggingface.co/a/b/resolve/main/model.bin",
            "https://huggingface.co/a/b/resolve/main/model.safetensors",
            "https://huggingface.co/a/b/resolve/ma%20in/model.gguf",
            "https://huggingface.co\\@evil.example/a/b",
            "https://huggingface.co/a\\b/c",
            "https://github.com/a/b/releases/download/v1/model.zip",
            "https://github.com/a/b/releases/download/v1/.gguf",
            "a/b/.gguf",
            "a/b/x/../y.gguf",
        ] {
            assert!(parse(input).is_err(), "{input:?} should be refused");
        }
        assert!(parse(&format!("a/{}", "b".repeat(MAX_INPUT))).is_err());
    }

    #[test]
    fn file_names() {
        for good in ["m.gguf", "Model-Q4_K_M.GGUF", "sub/dir/m.gguf", "a+b.gguf"] {
            assert!(check_file(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "m.bin",
            "/m.gguf",
            "a//m.gguf",
            "../m.gguf",
            "a/../m.gguf",
            "a\\m.gguf",
            ".m.gguf",
            "m gguf.gguf",
            "a/./m.gguf",
            "m.gguf/",
            "C:m.gguf",
        ] {
            assert!(check_file(bad).is_err(), "{bad}");
        }
        assert!(check_file(&format!("{}.gguf", "a".repeat(300))).is_err());
    }

    fn file(name: &str, mb: u64) -> FoundFile {
        FoundFile { file: name.into(), size: mb * 1024 * 1024, sha256: None }
    }

    #[test]
    fn picks_the_largest_file_that_fits() {
        let files = [file("q2.gguf", 1000), file("q4.gguf", 2000), file("q8.gguf", 4000), file("f16.gguf", 8000)];
        // 15% on top of the file: 4000 MB needs 4600 MB.
        assert_eq!(needs_mb(4000 * 1024 * 1024), 4600);
        assert_eq!(pick(&files, 4600).unwrap().file, "q8.gguf");
        assert_eq!(pick(&files, 4599).unwrap().file, "q4.gguf");
        assert_eq!(pick(&files, 100_000).unwrap().file, "f16.gguf");
        assert_eq!(pick(&files, 1150).unwrap().file, "q2.gguf");
        assert!(pick(&files, 1149).is_none());
        assert!(pick(&files, 0).is_none());
        assert!(pick(&[], 10_000).is_none());
    }

    #[test]
    fn reads_a_hugging_face_model() {
        let sha = "a".repeat(64);
        let model = json!({
            "sha": "0123abcd",
            "gated": false,
            "cardData": {"license": "apache-2.0"},
            "siblings": [
                {"rfilename": "README.md", "size": 10},
                {"rfilename": "big-Q8_0.gguf", "size": 3000, "lfs": {"sha256": sha, "size": 3000}},
                {"rfilename": "small-Q4_K_M.gguf", "size": 1000, "lfs": {"sha256": "NOTHEX", "size": 1000}},
                {"rfilename": "mmproj-model-f16.gguf", "size": 500},
                {"rfilename": "huge-00001-of-00002.gguf", "size": 500},
                {"rfilename": "huge-00002-of-00002.gguf", "size": 500},
                {"rfilename": "../escape.gguf", "size": 500},
                {"rfilename": "empty.gguf", "size": 0},
            ],
        });
        let found = read_hf_model(&hf("o", "r", Some("main"), Some("small-Q4_K_M.gguf")), &model).unwrap();
        assert_eq!(found.files.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), ["small-Q4_K_M.gguf", "big-Q8_0.gguf"]);
        assert_eq!(found.files[0].sha256, None);
        assert_eq!(found.files[1].sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(found.skipped, 4);
        assert_eq!(found.projectors.iter().map(|f| f.file.as_str()).collect::<Vec<_>>(), ["mmproj-model-f16.gguf"]);
        assert_eq!(found.license.as_deref(), Some("apache-2.0"));
        assert_eq!(found.wanted.as_deref(), Some("small-Q4_K_M.gguf"));
        assert!(!found.gated);
        assert_eq!(found.base_url(), "https://huggingface.co/o/r/resolve/0123abcd");

        let other = json!({"gated": "auto", "cardData": {"license": "other", "license_name": "example-licence"}, "siblings": []});
        let found = read_hf_model(&hf("o", "r", None, Some("missing.gguf")), &other).unwrap();
        assert!(found.gated && found.files.is_empty() && found.wanted.is_none());
        assert_eq!(found.license.as_deref(), Some("example-licence"));
        assert_eq!(found.rev, "main");
        let tagged = json!({"tags": ["gguf", "license:mit"], "siblings": []});
        assert_eq!(read_hf_model(&hf("o", "r", None, None), &tagged).unwrap().license.as_deref(), Some("mit"));
    }

    #[test]
    fn reads_a_github_release() {
        let sha = "b".repeat(64);
        let release = json!({"tag_name": "v1.0", "assets": [
            {"name": "model.gguf", "size": 2000, "digest": format!("sha256:{sha}")},
            {"name": "source.zip", "size": 10},
            {"name": "other.gguf", "size": 1000, "digest": null},
        ]});
        let (tag, (files, projectors, skipped)) = read_github_release(&release).unwrap();
        assert_eq!((tag.as_str(), skipped, projectors.len()), ("v1.0", 0, 0));
        assert_eq!(
            files,
            [
                FoundFile { file: "other.gguf".into(), size: 1000, sha256: None },
                FoundFile { file: "model.gguf".into(), size: 2000, sha256: Some(sha) }
            ]
        );
        assert!(read_github_release(&json!({"tag_name": "../x", "assets": []})).is_none());
    }

    #[test]
    fn a_model_that_sees_comes_with_its_vision_part() {
        let siblings = json!([
            {"rfilename": "Inkling-2-Mini-16GB-64K.gguf", "size": 15_000, "lfs": {"sha256": "d".repeat(64), "size": 15_000}},
            {"rfilename": "Inkling-2-Mini-16GB-128K.gguf", "size": 13_000},
            {"rfilename": "mmproj-Inkling-2-Mini-F16.gguf", "size": 640},
            {"rfilename": "mmproj-Inkling-2-Mini-Q8_0.gguf", "size": 345, "lfs": {"sha256": "e".repeat(64), "size": 345}},
            {"rfilename": "imatrix/Inkling-2-Mini.imatrix.gguf", "size": 90},
        ]);
        let found =
            read_hf_model(&hf("drofn", "Inkling-2-Mini-16GB-GGUF", None, None), &json!({"sha": "abc", "siblings": siblings})).unwrap();
        assert_eq!(found.files.len(), 2, "the vision parts and the importance matrix are not chat models");
        assert_eq!(found.skipped, 3);
        let entry = package(&found, &found.files[1]);
        let files = &entry.package.variants[0].files;
        assert_eq!(
            files.iter().map(|f| (f.role.as_str(), f.path.as_str())).collect::<Vec<_>>(),
            [("model", "Inkling-2-Mini-16GB-64K.gguf"), ("mmproj", "mmproj-Inkling-2-Mini-Q8_0.gguf")]
        );
        assert_eq!(files[1].sha256, "e".repeat(64));
        assert_eq!(entry.package.variants[0].accel_mem_mb, needs_mb(15_345));
        assert!(entry.sound());
        // An entry with a third file, or a second model, is not one this module writes.
        let mut odd = entry.clone();
        odd.package.variants[0].files.push(files[1].clone());
        assert!(!odd.sound());
        odd.package.variants[0].files.truncate(2);
        odd.package.variants[0].files[1].role = "model".into();
        assert!(!odd.sound());

        // Of two models in one repo, each gets its own; in one folder over another; 8 bits over 16.
        let p = |name: &str, size: u64| FoundFile { file: name.into(), size, sha256: None };
        let parts = [
            p("mmproj-gemma-3-4b-f16.gguf", 800),
            p("mmproj-gemma-3-12b-F16.gguf", 850),
            p("mmproj-gemma-3-12b-Q8_0.gguf", 500),
            p("mmproj-gemma-3-12b-F32.gguf", 1700),
        ];
        assert_eq!(projector_for(&parts, "gemma-3-12b-it-Q4_K_M.gguf").unwrap().file, "mmproj-gemma-3-12b-Q8_0.gguf");
        assert_eq!(projector_for(&parts, "gemma-3-4b-it-Q4_K_M.gguf").unwrap().file, "mmproj-gemma-3-4b-f16.gguf");
        let foldered = [p("a/mmproj-f16.gguf", 10), p("b/mmproj-f16.gguf", 10)];
        assert_eq!(projector_for(&foldered, "b/model-Q4_K_M.gguf").unwrap().file, "b/mmproj-f16.gguf");
        assert!(projector_for(&[], "model.gguf").is_none());
        // A model without one stays as it was.
        let plain = read_hf_model(&hf("o", "r", None, None), &json!({"siblings": [{"rfilename": "m.gguf", "size": 5}]})).unwrap();
        assert_eq!(package(&plain, &plain.files[0]).package.variants[0].files.len(), 1);
    }

    #[test]
    fn finds_hugging_face_links_in_a_readme() {
        let readme = "Weights: [GGUF](https://huggingface.co/someone/tiny-GGUF/tree/main), also\n\
                      <a href=\"https://huggingface.co/other/model.v2\">here</a> and https://huggingface.co/someone/tiny-GGUF.\n\
                      Data: https://huggingface.co/datasets/someone/data and https://huggingface.co/docs/hub";
        assert_eq!(hf_links(readme), ["someone/tiny-GGUF", "other/model.v2"]);
        assert!(hf_links("nothing here, not even huggingface.co/").is_empty());
    }

    #[test]
    fn package_and_store() {
        let found = read_hf_model(
            &hf("bartowski", "SmolLM2-360M-Instruct-GGUF", None, None),
            &json!({"sha": "abc123", "cardData": {"license": "apache-2.0"}, "siblings": [{"rfilename": "SmolLM2-360M-Instruct-Q4_K_M.gguf", "size": 271_000_000, "lfs": {"sha256": "c".repeat(64)}}]}),
        )
        .unwrap();
        let entry = package(&found, &found.files[0]);
        assert_eq!(entry.package.id, "community-bartowski-smollm2-360m-instruct-gguf-smollm2-360m-instruct-q4-k-m");
        assert_eq!(entry.package.name, "SmolLM2-360M-Instruct-Q4_K_M");
        assert_eq!(entry.dir, "bartowski__SmolLM2-360M-Instruct-GGUF");
        assert_eq!(entry.base_url, "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/abc123");
        assert_eq!(entry.package.source.license, "apache-2.0");
        assert!(entry.sound());

        let root = std::env::temp_dir().join(format!("rigbrew-community-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let mut store = Store::load(&root);
        assert!(store.list().is_empty());
        store.put(entry.clone()).unwrap();
        store.put(entry.clone()).unwrap();
        assert_eq!(Store::load(&root).list().len(), 1);
        assert_eq!(files_dir(&root, &entry), root.join("community").join("bartowski__SmolLM2-360M-Instruct-GGUF"));

        // An entry edited to leave its folder or to download from elsewhere is dropped on load.
        let mut text = std::fs::read_to_string(root.join("community.json")).unwrap();
        text = text.replace("\"bartowski__SmolLM2-360M-Instruct-GGUF\"", "\"../../elsewhere\"");
        std::fs::write(root.join("community.json"), &text).unwrap();
        assert!(Store::load(&root).list().is_empty());
        let mut moved = entry.clone();
        moved.base_url = "https://evil.example/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/abc123".into();
        assert!(!moved.sound());
        let mut escaped = entry.clone();
        escaped.package.variants[0].files[0].path = "../../outside.gguf".into();
        assert!(!escaped.sound());

        let mut store = Store::load(&root);
        store.put(entry.clone()).unwrap();
        assert!(store.remove(&entry.package.id).unwrap().is_some());
        assert!(store.remove(&entry.package.id).unwrap().is_none());
        assert!(Store::load(&root).list().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
