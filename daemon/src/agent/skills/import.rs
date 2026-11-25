//! Bringing a skill in from outside: from a folder on this computer, or from a folder of a
//! public repository on GitHub. Both end in the same check: a skill is a `SKILL.md` and a few
//! text files, 200 KB at most together. Anything else is refused as a whole, and the person
//! sees the name, the description, the licence and the size before anything is saved.
//!
//! From GitHub the list of files comes from one request to the API (the tree of the
//! repository) and each file from `raw.githubusercontent.com`, both over https and without an
//! account. Nothing of a skill is ever run: it is text a model reads.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::{inside_path, licence_of, parse, text_ending};

/// All files of one skill together.
pub const MAX_TOTAL_BYTES: u64 = 200 * 1024;
const MAX_SKILL_FILES: usize = 40;
/// Skills offered from one folder or repository.
const MAX_FOUND: usize = 40;
/// How deep below the given folder a skill is looked for.
const MAX_DEPTH: usize = 3;

/// One file of a skill: its path inside the skill folder and its content.
pub type File = (String, Vec<u8>);

/// A skill that could be brought in, as the person sees it before saving.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Found {
    pub name: String,
    pub description: String,
    /// What the skill says its licence is, or the first line of its licence file.
    pub licence: Option<String>,
    pub bytes: u64,
    /// Paths inside the skill folder, `SKILL.md` among them.
    pub files: Vec<String>,
    /// Why it cannot be brought in, when it cannot.
    pub problem: Option<String>,
    /// Where it is: the folder's name, or the path inside the repository.
    pub at: String,
}

fn kb(bytes: u64) -> u64 {
    bytes.div_ceil(1024)
}

/// What is wrong with the names and sizes of a skill's files, before any content is looked at.
pub fn check_listing(files: &[(String, u64)]) -> Result<(), String> {
    if !files.iter().any(|(path, _)| path == "SKILL.md") {
        return Err("There is no SKILL.md in it.".into());
    }
    if files.len() > MAX_SKILL_FILES {
        return Err(format!("It has {} files. A skill can have {MAX_SKILL_FILES} at most.", files.len()));
    }
    for (path, _) in files {
        if !inside_path(path) {
            return Err(format!("It holds a file with a path that is not allowed: {}.", path.chars().take(80).collect::<String>()));
        }
        if !text_ending(path) {
            return Err(format!("It holds {path}, which is not a text file. Only text can be brought in."));
        }
    }
    let total: u64 = files.iter().map(|(_, size)| size).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(format!("It is {} KB. A skill can be {} KB at most.", kb(total), kb(MAX_TOTAL_BYTES)));
    }
    Ok(())
}

/// Checks the files of one skill, content included, and says what it is. `at` is where it was found.
pub fn check(at: &str, files: &[File]) -> Found {
    let listing: Vec<(String, u64)> = files.iter().map(|(path, bytes)| (path.clone(), bytes.len() as u64)).collect();
    let mut found = Found {
        at: at.to_string(),
        bytes: listing.iter().map(|(_, size)| size).sum(),
        files: listing.iter().map(|(path, _)| path.clone()).collect(),
        ..Found::default()
    };
    found.files.sort();
    let folder_name = at.trim_end_matches('/').rsplit('/').next().unwrap_or(at).to_lowercase();
    let main = files.iter().find(|(path, _)| path == "SKILL.md").and_then(|(_, bytes)| std::str::from_utf8(bytes).ok());
    if let Some(skill) = main.and_then(|text| parse(text, &folder_name, "yours")) {
        found.name = skill.name;
        found.description = skill.description;
    }
    found.licence = main.and_then(licence_of).or_else(|| {
        let (_, bytes) = files
            .iter()
            .find(|(path, _)| matches!(path.to_ascii_uppercase().as_str(), "LICENSE" | "LICENCE" | "LICENSE.MD" | "LICENSE.TXT"))?;
        let text = std::str::from_utf8(bytes).ok()?;
        let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
        Some(line.chars().filter(|c| !c.is_control()).take(120).collect())
    });
    found.problem = check_listing(&listing).err().or_else(|| {
        for (path, bytes) in files {
            if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
                return Some(format!("It holds {path}, which is not a text file. Only text can be brought in."));
            }
        }
        if found.name.is_empty() {
            return Some("Its SKILL.md has no proper name and description at the top.".into());
        }
        None
    });
    found
}

/// Writes a checked skill into `<data dir>/skills/<name>/`. Refuses when the person already has
/// one by that name.
pub fn store(data_dir: &Path, found: &Found, files: &[File]) -> Result<(), String> {
    if let Some(problem) = &found.problem {
        return Err(problem.clone());
    }
    if !super::valid_name(&found.name) {
        return Err("Its SKILL.md has no proper name and description at the top.".into());
    }
    if super::own(data_dir).iter().any(|s| s.name == found.name) {
        return Err(format!("You already have a skill named {}. Delete yours first, or rename it.", found.name));
    }
    let dir = super::own_dir(data_dir).join(&found.name);
    if dir.exists() {
        return Err(format!("There is already a folder named {} in your skills folder.", found.name));
    }
    let wrote = || -> std::io::Result<()> {
        for (path, bytes) in files {
            // Checked above, and checked again where it is written.
            if !inside_path(path) {
                continue;
            }
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)?;
        }
        Ok(())
    };
    wrote().map_err(|e| {
        std::fs::remove_dir_all(&dir).ok();
        format!("The skill could not be saved: {e}")
    })
}

// --- from a folder --------------------------------------------------------------------------------

/// The files of one skill folder on disk. Hidden files and links are left out; a folder that
/// is too large is given up on before it is read.
fn read_skill_folder(dir: &Path) -> Result<Vec<File>, String> {
    fn walk(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, std::path::PathBuf, u64)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') || out.len() > MAX_SKILL_FILES {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if depth < 4 {
                    walk(base, &path, depth + 1, out);
                }
                continue;
            }
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path, entry.metadata().map(|m| m.len()).unwrap_or(0)));
        }
    }
    let mut listed = Vec::new();
    walk(dir, dir, 1, &mut listed);
    check_listing(&listed.iter().map(|(rel, _, size)| (rel.clone(), *size)).collect::<Vec<_>>())?;
    listed
        .into_iter()
        .map(|(rel, path, _)| match std::fs::read(&path) {
            Ok(bytes) => Ok((rel, bytes)),
            Err(e) => Err(format!("{rel} could not be read: {e}")),
        })
        .collect()
}

/// The skill folders in or under `dir`: the folder itself when it holds a `SKILL.md`, otherwise
/// the folders below it that do.
fn skill_folders(dir: &Path) -> Vec<std::path::PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        if out.len() >= MAX_FOUND {
            return;
        }
        if dir.join("SKILL.md").is_file() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth > MAX_DEPTH {
            return;
        }
        let mut below: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|k| k.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .filter(|p| !p.file_name().is_some_and(|n| n == "node_modules" || n == "target"))
            .collect();
        below.sort();
        for path in below {
            walk(&path, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(dir, 1, &mut out);
    out
}

/// The skills in a folder on this computer, each with its files when it can be brought in.
pub fn from_folder(location: &str) -> Result<Vec<(Found, Vec<File>)>, String> {
    let dir = Path::new(location.trim());
    if !dir.is_absolute() || !dir.is_dir() {
        return Err("Choose a folder that holds a skill, or a folder of skills.".into());
    }
    let folders = skill_folders(dir);
    if folders.is_empty() {
        return Err("No SKILL.md was found in that folder or in the folders inside it.".into());
    }
    Ok(folders
        .into_iter()
        .map(|folder| {
            let at = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match read_skill_folder(&folder) {
                Ok(files) => (check(&at, &files), files),
                Err(problem) => {
                    // Too large or not text: still shown, by what its SKILL.md says, with the reason.
                    let main: Vec<File> =
                        std::fs::read(folder.join("SKILL.md")).map(|b| vec![("SKILL.md".to_string(), b)]).unwrap_or_default();
                    let mut found = check(&at, &main);
                    found.problem = Some(problem);
                    (found, Vec::new())
                }
            }
        })
        .collect())
}

// --- from GitHub ----------------------------------------------------------------------------------

/// A folder of a repository on GitHub.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub owner: String,
    pub repo: String,
    /// Branch, tag or commit; the repository's own default when the address names none.
    pub reference: String,
    /// The folder inside the repository, without slashes around it; empty for the top.
    pub path: String,
}

const NOT_GITHUB: &str =
    "That is not the address of a folder on GitHub. It looks like https://github.com/owner/repository/tree/main/skills/name.";

/// Reads the address of a repository, of a folder in it, or of a `SKILL.md` in it.
pub fn parse_github(raw: &str) -> Result<Repo, String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| NOT_GITHUB.to_string())?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("github.com" | "www.github.com")) {
        return Err(NOT_GITHUB.into());
    }
    let parts: Vec<String> = url.path_segments().map(|p| p.filter(|s| !s.is_empty()).map(percent_decode).collect()).unwrap_or_default();
    let name = |s: &str| {
        !s.is_empty() && s.len() <= 100 && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    if parts.len() < 2 || !name(&parts[0]) || !name(&parts[1]) {
        return Err(NOT_GITHUB.into());
    }
    let (owner, repo) = (parts[0].clone(), parts[1].trim_end_matches(".git").to_string());
    let (reference, mut path) = match parts.get(2).map(String::as_str) {
        None => ("HEAD".to_string(), Vec::new()),
        Some("tree" | "blob") if parts.len() >= 4 => (parts[3].clone(), parts[4..].to_vec()),
        _ => return Err(NOT_GITHUB.into()),
    };
    if path.last().is_some_and(|last| last == "SKILL.md") {
        path.pop();
    }
    let reference_ok = !reference.is_empty()
        && reference.len() <= 200
        && !reference.starts_with('.')
        && !reference.contains("..")
        && reference.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !reference_ok || path.iter().any(|p| p.starts_with('.') || p.contains(['/', '\\']) || p.chars().any(|c| c.is_control())) {
        return Err(NOT_GITHUB.into());
    }
    Ok(Repo { owner, repo, reference, path: path.join("/") })
}

fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(path: &str) -> String {
    path.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// One skill folder of a repository, as its tree lists it: where it is and its files with sizes.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// Path of the skill folder inside the repository; empty for the top.
    pub at: String,
    /// Paths inside the skill folder, with sizes.
    pub files: Vec<(String, u64)>,
    /// A link or a module in it: something that is not a plain file.
    pub odd: Option<String>,
}

/// Finds the skill folders at or below `base` in the tree of a repository (`git/trees?recursive=1`).
pub fn plan(tree: &[Value], base: &str) -> Vec<Listed> {
    let prefix = if base.is_empty() { String::new() } else { format!("{base}/") };
    let below = |path: &str| path.strip_prefix(&prefix).map(str::to_string);
    let mut roots: Vec<String> = tree
        .iter()
        .filter(|e| e["type"] == "blob")
        .filter_map(|e| below(e["path"].as_str()?))
        .filter_map(|rel| if rel == "SKILL.md" { Some(String::new()) } else { rel.strip_suffix("/SKILL.md").map(str::to_string) })
        .filter(|root| root.split('/').filter(|p| !p.is_empty()).count() <= MAX_DEPTH && !root.split('/').any(|p| p.starts_with('.')))
        .collect();
    roots.sort();
    // The given folder is a skill itself: that one, and nothing inside it.
    if roots.first().is_some_and(String::is_empty) {
        roots.truncate(1);
    }
    roots.truncate(MAX_FOUND);
    roots
        .iter()
        .map(|root| {
            let inner = if root.is_empty() { prefix.clone() } else { format!("{prefix}{root}/") };
            let mut listed = Listed { at: format!("{prefix}{root}").trim_end_matches('/').to_string(), files: Vec::new(), odd: None };
            for entry in tree {
                let Some(rel) = entry["path"].as_str().and_then(|p| p.strip_prefix(&inner)) else { continue };
                // Hidden files are not part of a skill.
                if rel.split('/').any(|p| p.starts_with('.')) {
                    continue;
                }
                match (entry["type"].as_str(), entry["mode"].as_str()) {
                    (Some("tree"), _) => {}
                    (Some("blob"), Some(mode)) if mode != "120000" => {
                        listed.files.push((rel.to_string(), entry["size"].as_u64().unwrap_or(0)))
                    }
                    _ => listed.odd = Some(rel.to_string()),
                }
            }
            listed.files.sort();
            listed
        })
        .collect()
}

async fn github_get(net: &reqwest::Client, url: &str, accept: &str, max: u64) -> Result<Vec<u8>, String> {
    use futures_util::StreamExt;
    let answer = net
        .get(url)
        .header("Accept", accept)
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "GitHub could not be reached. Check the internet connection.".to_string())?;
    match answer.status().as_u16() {
        200 => {}
        404 => return Err("GitHub has nothing at that address. A private repository cannot be read.".into()),
        403 | 429 => return Err("GitHub answers no more requests from this computer for now. Try again in an hour.".into()),
        code => return Err(format!("GitHub answered {code}.")),
    }
    let mut out = Vec::new();
    let mut stream = answer.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "The answer from GitHub was cut off.".to_string())?;
        out.extend_from_slice(&chunk);
        if out.len() as u64 > max {
            return Err("The answer from GitHub is larger than it said.".into());
        }
    }
    Ok(out)
}

/// The skills in a folder of a public repository on GitHub. With `only`, the files of the
/// skills named there are fetched too, and checked; without, only each `SKILL.md`.
pub async fn from_github(net: &reqwest::Client, location: &str, only: Option<&[String]>) -> Result<Vec<(Found, Vec<File>)>, String> {
    let repo = parse_github(location)?;
    let api = format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
        percent_encode(&repo.owner),
        percent_encode(&repo.repo),
        percent_encode(&repo.reference)
    );
    let raw = github_get(net, &api, "application/vnd.github+json", 8 * 1024 * 1024).await?;
    let tree: Value = serde_json::from_slice(&raw).map_err(|_| "The answer from GitHub could not be read.".to_string())?;
    let listed = plan(tree["tree"].as_array().map(Vec::as_slice).unwrap_or_default(), &repo.path);
    if listed.is_empty() {
        return Err("No SKILL.md was found at that address or in the folders inside it.".into());
    }
    // The commit the tree belongs to: every file is read from exactly that state.
    let commit = tree["sha"].as_str().filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())).unwrap_or(&repo.reference);
    let file_url = |skill: &Listed, path: &str| {
        let whole = if skill.at.is_empty() { path.to_string() } else { format!("{}/{path}", skill.at) };
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            percent_encode(&repo.owner),
            percent_encode(&repo.repo),
            percent_encode(commit),
            percent_encode(&whole)
        )
    };
    let mut out = Vec::new();
    for skill in &listed {
        let problem = match &skill.odd {
            Some(odd) => Some(format!("It holds {odd}, which is a link, not a file.")),
            None => check_listing(&skill.files).err(),
        };
        let main_size = skill.files.iter().find(|(p, _)| p == "SKILL.md").map_or(0, |(_, s)| *s);
        let main = match github_get(net, &file_url(skill, "SKILL.md"), "*/*", main_size.clamp(1, MAX_TOTAL_BYTES) + 1024).await {
            Ok(bytes) => bytes,
            Err(e) => {
                out.push((Found { at: skill.at.clone(), problem: Some(e), ..Found::default() }, Vec::new()));
                continue;
            }
        };
        let mut found = check(&skill.at, &[("SKILL.md".to_string(), main.clone())]);
        found.bytes = skill.files.iter().map(|(_, size)| size).sum();
        found.files = skill.files.iter().map(|(path, _)| path.clone()).collect();
        if found.licence.is_none() {
            found.licence = skill
                .files
                .iter()
                .map(|(p, _)| p)
                .find(|p| p.to_ascii_uppercase().starts_with("LICEN"))
                .map(|p| format!("See the file {p}"));
        }
        found.problem = problem.or(found.problem);
        let wanted = only.is_some_and(|names| names.contains(&found.name)) && found.problem.is_none();
        if !wanted {
            out.push((found, Vec::new()));
            continue;
        }
        let mut files: Vec<File> = vec![("SKILL.md".to_string(), main)];
        let mut failed = None;
        for (path, size) in skill.files.iter().filter(|(p, _)| p != "SKILL.md") {
            match github_get(net, &file_url(skill, path), "*/*", size + 1024).await {
                Ok(bytes) => files.push((path.clone(), bytes)),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        // What was listed is what is believed until the files are here; then they are checked themselves.
        let mut checked = check(&skill.at, &files);
        checked.licence = checked.licence.or(found.licence);
        if let Some(e) = failed {
            checked.problem = Some(e);
        }
        out.push((checked, files));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SKILL: &str =
        "---\nname: tidy-imports\ndescription: Sort the imports. Use when asked to tidy them.\nlicense: MIT\n---\n\n# Steps\n\nDo it.\n";

    fn file(path: &str, text: &str) -> File {
        (path.to_string(), text.as_bytes().to_vec())
    }

    #[test]
    fn a_good_skill_says_what_it_is() {
        let found = check(
            "skills/tidy-imports",
            &[file("SKILL.md", SKILL), file("reference/order.md", "std first"), file("LICENSE", "MIT License\n\nCopyright")],
        );
        assert_eq!(found.problem, None);
        assert_eq!((found.name.as_str(), found.licence.as_deref()), ("tidy-imports", Some("MIT")));
        assert!(found.description.starts_with("Sort the imports."));
        assert_eq!(found.files, ["LICENSE", "SKILL.md", "reference/order.md"]);
        assert_eq!(found.bytes as usize, SKILL.len() + 9 + 22);
        // Without a licence line the licence file's first line is shown.
        let plain = SKILL.replace("license: MIT\n", "");
        assert_eq!(
            check("x", &[file("SKILL.md", &plain), file("LICENSE", "\nApache License\nVersion 2.0")]).licence.as_deref(),
            Some("Apache License")
        );
        assert_eq!(check("x", &[file("SKILL.md", &plain)]).licence, None);
        // A skill without a name takes the name of its folder.
        let unnamed = "---\ndescription: Does a thing when asked.\n---\nBody";
        assert_eq!(check("some/My-Skill", &[file("SKILL.md", unnamed)]).name, "my-skill");
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        let hidden = format!(".{}/hooks/x.sh", "git");
        for bad in ["../evil.md", "a/../../evil.md", "/etc/passwd.md", "a\\b.md", "C:/x.md", hidden.as_str(), "a//b.md"] {
            let found = check("x", &[file("SKILL.md", SKILL), file(bad, "text")]);
            assert!(found.problem.as_deref().is_some_and(|p| p.contains("not allowed")), "{bad}: {:?}", found.problem);
        }
    }

    #[test]
    fn anything_that_is_not_text_is_refused() {
        let by_name = check("x", &[file("SKILL.md", SKILL), file("logo.png", "not really a picture")]);
        assert!(by_name.problem.unwrap().contains("logo.png, which is not a text file"));
        let by_content = check("x", &[file("SKILL.md", SKILL), ("notes.md".to_string(), b"text\0more".to_vec())]);
        assert!(by_content.problem.unwrap().contains("notes.md, which is not a text file"));
        let not_utf8 = check("x", &[file("SKILL.md", SKILL), ("notes.md".to_string(), vec![0xff, 0xfe, 0x41])]);
        assert!(not_utf8.problem.is_some());
        assert!(check("x", &[file("run", "#!/bin/sh"), file("SKILL.md", SKILL)]).problem.is_some(), "a file without an ending is not text");
    }

    #[test]
    fn size_and_shape_are_checked() {
        let big = "x".repeat(MAX_TOTAL_BYTES as usize);
        assert!(check("x", &[file("SKILL.md", SKILL), file("big.md", &big)]).problem.unwrap().contains("200 KB at most"));
        let just_fits = "x".repeat(MAX_TOTAL_BYTES as usize - SKILL.len());
        assert_eq!(check("x", &[file("SKILL.md", SKILL), file("fits.md", &just_fits)]).problem, None);
        assert!(check("x", &[file("README.md", "hello")]).problem.unwrap().contains("no SKILL.md"));
        assert!(check("x", &[file("SKILL.md", "# no front matter")]).problem.unwrap().contains("no proper name"));
        let many: Vec<File> = (0..41).map(|i| file(&format!("f{i}.md"), "x")).chain([file("SKILL.md", SKILL)]).collect();
        assert!(check("x", &many).problem.unwrap().contains("40 at most"));
        assert!(
            check_listing(&[("SKILL.md".into(), 10), ("a.md".into(), MAX_TOTAL_BYTES)]).is_err(),
            "sizes as listed count before anything is fetched"
        );
    }

    #[test]
    fn a_checked_skill_is_written_once() {
        let data = super::super::super::files::scratch("skills-import");
        let files = [file("SKILL.md", SKILL), file("reference/order.md", "std first")];
        let found = check("tidy-imports", &files);
        store(&data, &found, &files).unwrap();
        assert_eq!(std::fs::read_to_string(data.join("skills/tidy-imports/reference/order.md")).unwrap(), "std first");
        assert!(super::super::load(&data, None).iter().any(|s| s.name == "tidy-imports" && s.source == "yours"));
        assert!(store(&data, &found, &files).unwrap_err().contains("already have"));
        let bad = check("x", &[file("SKILL.md", SKILL), file("logo.png", "x")]);
        assert!(store(&data, &bad, &[]).is_err());
        assert!(!data.join("skills/x").exists());
    }

    #[test]
    fn a_folder_or_a_folder_of_folders_is_read() {
        let base = super::super::super::files::scratch("skills-folder");
        let one = base.join("pack/tidy-imports");
        std::fs::create_dir_all(one.join("reference")).unwrap();
        std::fs::write(one.join("SKILL.md"), SKILL).unwrap();
        std::fs::write(one.join("reference/order.md"), "std first").unwrap();
        std::fs::write(one.join(".DS_Store"), b"\0\0").unwrap();
        let two = base.join("pack/nested/with-picture");
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(two.join("SKILL.md"), SKILL.replace("tidy-imports", "with-picture")).unwrap();
        std::fs::write(two.join("logo.png"), b"\x89PNG").unwrap();
        std::fs::create_dir_all(base.join("pack/not-a-skill")).unwrap();

        let single = from_folder(one.to_str().unwrap()).unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!((single[0].0.name.as_str(), single[0].0.problem.clone(), single[0].1.len()), ("tidy-imports", None, 2));
        assert_eq!(single[0].0.files, ["SKILL.md", "reference/order.md"], "hidden files are left out");

        let pack = from_folder(base.join("pack").to_str().unwrap()).unwrap();
        let names: Vec<&str> = pack.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(names, ["with-picture", "tidy-imports"]);
        assert!(pack[0].0.problem.as_deref().unwrap().contains("logo.png"), "the reason is shown with the skill");
        assert!(pack[0].1.is_empty());

        assert!(from_folder(base.join("pack/not-a-skill").to_str().unwrap()).is_err());
        assert!(from_folder("relative/path").is_err());
        assert!(from_folder(base.join("missing").to_str().unwrap()).is_err());
    }

    #[test]
    fn github_addresses() {
        let repo = |owner: &str, repo: &str, reference: &str, path: &str| Repo {
            owner: owner.into(),
            repo: repo.into(),
            reference: reference.into(),
            path: path.into(),
        };
        assert_eq!(parse_github("https://github.com/acme/skills").unwrap(), repo("acme", "skills", "HEAD", ""));
        assert_eq!(parse_github(" https://github.com/acme/skills/ ").unwrap(), repo("acme", "skills", "HEAD", ""));
        assert_eq!(parse_github("https://github.com/acme/skills.git").unwrap(), repo("acme", "skills", "HEAD", ""));
        assert_eq!(parse_github("https://github.com/acme/skills/tree/main").unwrap(), repo("acme", "skills", "main", ""));
        assert_eq!(
            parse_github("https://github.com/acme/skills/tree/v1.2/skills/tidy").unwrap(),
            repo("acme", "skills", "v1.2", "skills/tidy")
        );
        assert_eq!(
            parse_github("https://github.com/acme/skills/blob/main/skills/tidy/SKILL.md").unwrap(),
            repo("acme", "skills", "main", "skills/tidy")
        );
        assert_eq!(parse_github("https://github.com/acme/skills/tree/main/my%20skills").unwrap().path, "my skills");
        for bad in [
            "http://github.com/acme/skills",
            "https://gitlab.com/acme/skills",
            "https://github.com.evil.example/acme/skills",
            "https://github.com/acme",
            "https://github.com/acme/skills/issues/3",
            "https://github.com/acme/skills/tree/main/../../other",
            "https://github.com/acme/skills/tree/main/a%2Fb",
            "https://github.com/ac me/skills",
            "not an address",
        ] {
            assert!(parse_github(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_tree_of_a_repository_is_searched_for_skills() {
        let blob = |path: &str, size: u64| json!({"path": path, "type": "blob", "mode": "100644", "size": size});
        let hidden = format!("skills/tidy/.{}/x", "github");
        let tree = vec![
            blob("README.md", 100),
            json!({"path": "skills", "type": "tree", "mode": "040000"}),
            blob("skills/tidy/SKILL.md", 300),
            blob("skills/tidy/reference/order.md", 50),
            blob(&hidden, 5),
            blob("skills/report/SKILL.md", 400),
            json!({"path": "skills/report/link.md", "type": "blob", "mode": "120000", "size": 9}),
            blob("skills/deep/a/b/c/d/SKILL.md", 10),
            blob("other/SKILL.md", 20),
        ];
        let all = plan(&tree, "");
        let at: Vec<&str> = all.iter().map(|l| l.at.as_str()).collect();
        assert_eq!(at, ["other", "skills/report", "skills/tidy"], "too deep is not looked at");
        assert_eq!(
            all[2].files,
            [("SKILL.md".to_string(), 300), ("reference/order.md".to_string(), 50)],
            "hidden files are not part of it"
        );
        assert_eq!(all[1].odd.as_deref(), Some("link.md"), "a link is not a file");
        let under = plan(&tree, "skills");
        assert_eq!(under.iter().map(|l| l.at.as_str()).collect::<Vec<_>>(), ["skills/report", "skills/tidy"]);
        let one = plan(&tree, "skills/tidy");
        assert_eq!((one.len(), one[0].at.as_str(), one[0].files.len()), (1, "skills/tidy", 2));
        assert!(plan(&tree, "skill").is_empty(), "a folder name is matched whole");
        // A repository that is one skill at its top.
        let top = vec![blob("SKILL.md", 10), blob("extra/SKILL.md", 10), blob("notes.md", 3)];
        let found = plan(&top, "");
        assert_eq!((found.len(), found[0].at.as_str(), found[0].files.len()), (1, "", 3));
    }
}
