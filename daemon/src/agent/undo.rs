//! Undo: the folder as it was before a request changed it, so the person can take the whole
//! request back.
//!
//! In a git repository (the folder has its own `.git`) the working tree is recorded as a commit
//! on a hidden ref, `refs/rigbrew/<task>/<request>`, built with a temporary index file. The
//! person's index, branch and stash are never touched. Taking a request back puts every tracked
//! and untracked file back to that commit and removes the files made since; what git ignores is
//! left alone. The state just before the undo is kept the same way, on `<request>-undone`.
//!
//! Anywhere else, each file is copied to `<data dir>/agent/undo/<task>/<request>/` before the
//! task's own write or edit touches it, and those copies are put back. What a command changed
//! there cannot be taken back, and the task says so.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::files::{self, Folder};
use super::{Step, Task};
use crate::{fail, Shared};

/// An untracked file larger than this is left out of a git snapshot, and never touched by undo.
const MAX_UNTRACKED_BYTES: u64 = 50 * 1024 * 1024;

pub const NOT_BY_COMMANDS: &str = "This folder is not a git repository, so what commands changed could not be taken back.";

/// A file as it was before the task changed it (outside git).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// Relative to the folder, with forward slashes.
    pub path: String,
    /// False for a file the task created: undo removes it.
    pub existed: bool,
}

/// The folder before the first change of one request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Which message of the person it belongs to: 1 for the first.
    pub request: usize,
    /// "git" or "copies".
    pub kind: String,
    /// git: the commit that holds the folder as it was.
    #[serde(default)]
    pub commit: Option<String>,
    /// git: large untracked files that were left out. Undo never touches them.
    #[serde(default)]
    pub skipped: Vec<String>,
    /// copies: the files saved so far, in the order they were first touched.
    #[serde(default)]
    pub files: Vec<Saved>,
    /// copies: a command ran in this request, and what it changed is not saved.
    #[serde(default)]
    pub commands: bool,
    /// The person took this request back.
    #[serde(default)]
    pub undone: bool,
    /// The request the task was at when that happened (0 when it did not).
    #[serde(default)]
    pub undone_at: usize,
}

impl Snapshot {
    /// Whether there is something to take back. Outside git, a request in which only commands
    /// ran has nothing saved.
    pub fn open(&self) -> bool {
        !self.undone && (self.kind == "git" || !self.files.is_empty())
    }
}

/// What an undo did.
#[derive(Debug, Default, PartialEq)]
pub struct Restored {
    /// Files put back to what they were.
    pub restored: usize,
    /// Files made since, now removed.
    pub removed: usize,
}

/// Which request of the task is being worked on: the number of messages the person sent.
pub fn request_of(task: &Task) -> usize {
    task.steps.iter().filter(|s| matches!(s, Step::User { .. })).count()
}

/// Whether the folder is the top of a git repository of its own. A folder inside a larger
/// repository, or a linked worktree, is handled with copies: its git files live outside it.
pub fn is_git(root: &Path) -> bool {
    root.join(".git").is_dir()
}

// --- git ---------------------------------------------------------------------------------------

/// Runs git in the folder, confined like a command of the task, and returns what it printed.
fn git(root: &Path, index: Option<&Path>, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut cmd = files::confined(root, "git");
    // No hooks, no file system monitor, no prompts: nothing of the repository's own runs here.
    cmd.args(["-c", "core.hooksPath=.git/rigbrew-no-hooks", "-c", "core.fsmonitor=false", "-c", "advice.addEmbeddedRepo=false"]);
    cmd.args(args);
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY"] {
        cmd.env_remove(name);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
    for who in ["AUTHOR", "COMMITTER"] {
        cmd.env(format!("GIT_{who}_NAME"), "Rigbrew").env(format!("GIT_{who}_EMAIL"), "undo@rigbrew.invalid");
    }
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("git could not be started: {e}"))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(bytes).ok();
    }
    let out = child.wait_with_output().map_err(|e| format!("git did not finish: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} did not work: {}", args.first().unwrap_or(&""), said.trim().chars().take(300).collect::<String>()));
    }
    Ok(out.stdout)
}

fn line(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Paths from output that ends each one with a zero byte.
fn paths(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

/// A temporary index next to the real one, removed when it goes out of scope.
struct TempIndex(PathBuf);

impl TempIndex {
    /// Starts as a copy of the person's index, so only files that changed are read again.
    fn like_the_real_one(root: &Path) -> TempIndex {
        let path = root.join(".git").join(format!("rigbrew-index-{}", uuid::Uuid::new_v4().simple()));
        std::fs::copy(root.join(".git/index"), &path).ok();
        TempIndex(path)
    }

    fn empty(root: &Path) -> TempIndex {
        TempIndex(root.join(".git").join(format!("rigbrew-index-{}", uuid::Uuid::new_v4().simple())))
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
        std::fs::remove_file(self.0.with_extension("lock")).ok();
    }
}

/// The working tree as a git tree: tracked and untracked files, without what git ignores and
/// without the paths in `leave_out`. Returns the tree and every path that was left out.
fn git_tree(root: &Path, leave_out: &[String]) -> Result<(String, Vec<String>), String> {
    let mut skipped: Vec<String> = leave_out.to_vec();
    for path in paths(&git(root, None, &["ls-files", "--others", "--exclude-standard", "-z"], None)?) {
        let large = std::fs::symlink_metadata(root.join(&path)).is_ok_and(|m| m.is_file() && m.len() > MAX_UNTRACKED_BYTES);
        if large && !skipped.contains(&path) {
            skipped.push(path);
        }
    }
    let index = TempIndex::like_the_real_one(root);
    let excluded: Vec<String> = skipped.iter().map(|p| format!(":(exclude,literal){p}")).collect();
    let mut args = vec!["add", "-A", "--", "."];
    args.extend(excluded.iter().map(String::as_str));
    git(root, Some(&index.0), &args, None)?;
    let tree = line(git(root, Some(&index.0), &["write-tree"], None)?);
    Ok((tree, skipped))
}

/// Keeps a tree as a commit on a ref of its own.
fn git_keep(root: &Path, tree: &str, name: &str, what: &str) -> Result<String, String> {
    let head = git(root, None, &["rev-parse", "-q", "--verify", "HEAD^{commit}"], None).map(line).unwrap_or_default();
    let mut args = vec!["commit-tree", tree, "-m", what];
    if !head.is_empty() {
        args.extend(["-p", head.as_str()]);
    }
    let commit = line(git(root, None, &args, None)?);
    git(root, None, &["update-ref", &format!("refs/rigbrew/{name}"), &commit], None)?;
    Ok(commit)
}

/// Records the folder as it is now. Returns the commit and the large files left out of it.
pub fn git_snapshot(root: &Path, task: &str, request: usize) -> Result<(String, Vec<String>), String> {
    let (tree, skipped) = git_tree(root, &[])?;
    let commit = git_keep(root, &tree, &format!("{task}/{request}"), &format!("Rigbrew: the folder before request {request}"))?;
    Ok((commit, skipped))
}

/// Removes a file and the folders that are empty once it is gone, up to the folder itself.
fn remove_with_empty_parents(root: &Path, relative: &str) -> bool {
    let path = root.join(relative);
    if std::fs::remove_file(&path).is_err() {
        return false;
    }
    let mut dir = path.parent();
    while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    true
}

/// Puts the folder back to a snapshot: files that changed or went missing are written again,
/// files made since are removed. The index and the branch stay as they are.
pub fn git_restore(root: &Path, task: &str, snapshot: &Snapshot) -> Result<Restored, String> {
    let commit = snapshot.commit.as_deref().ok_or("This snapshot has no commit.")?;
    git(root, None, &["cat-file", "-e", &format!("{commit}^{{commit}}")], None)
        .map_err(|_| "The saved state of the folder is gone from the repository.".to_string())?;
    let (now, _) = git_tree(root, &snapshot.skipped)?;
    git_keep(
        root,
        &now,
        &format!("{task}/{}-undone", snapshot.request),
        &format!("Rigbrew: the folder before request {} was taken back", snapshot.request),
    )?;
    let diff = |filter: &str| {
        git(root, None, &["diff", "--name-only", "--no-renames", "-z", &format!("--diff-filter={filter}"), commit, &now], None)
            .map(|o| paths(&o))
    };
    let (made, changed) = (diff("A")?, diff("DMT")?);
    let mut done = Restored::default();
    for path in &made {
        if remove_with_empty_parents(root, path) {
            done.removed += 1;
        }
    }
    if !changed.is_empty() {
        let index = TempIndex::empty(root);
        git(root, Some(&index.0), &["read-tree", commit], None)?;
        let list: Vec<u8> = changed.iter().flat_map(|p| p.bytes().chain(std::iter::once(0))).collect();
        git(root, Some(&index.0), &["checkout-index", "-f", "-z", "--stdin"], Some(&list))?;
        done.restored = changed.len();
    }
    Ok(done)
}

/// Forgets the hidden refs of a task.
fn git_forget(root: &Path, task: &str) {
    let Ok(out) = git(root, None, &["for-each-ref", "--format=%(refname)", &format!("refs/rigbrew/{task}/")], None) else { return };
    for name in String::from_utf8_lossy(&out).lines().filter(|l| l.starts_with("refs/rigbrew/")) {
        git(root, None, &["update-ref", "-d", name], None).ok();
    }
}

// --- copies ------------------------------------------------------------------------------------

/// Where the copies of one request are kept.
pub fn store(agent_dir: &Path, task: &str, request: usize) -> PathBuf {
    agent_dir.join("undo").join(task).join(request.to_string())
}

/// Keeps a file as it is now, once: the first state of a request is the one to go back to.
pub fn copy_save(store: &Path, root: &Path, saved: &mut Vec<Saved>, relative: &str) -> Result<(), String> {
    if saved.iter().any(|s| s.path == relative) {
        return Ok(());
    }
    let from = root.join(relative);
    let existed = std::fs::symlink_metadata(&from).is_ok();
    if existed {
        std::fs::create_dir_all(store).map_err(|e| format!("The copy for undo could not be kept: {e}"))?;
        std::fs::copy(&from, store.join(saved.len().to_string())).map_err(|e| format!("The copy for undo could not be kept: {e}"))?;
    }
    saved.push(Saved { path: relative.to_string(), existed });
    Ok(())
}

/// Puts the saved files back and removes the ones the task created.
pub fn copy_restore(store: &Path, root: &Path, saved: &[Saved]) -> Result<Restored, String> {
    let mut done = Restored::default();
    for (i, file) in saved.iter().enumerate() {
        let to = root.join(&file.path);
        if file.existed {
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            std::fs::copy(store.join(i.to_string()), &to).map_err(|e| format!("{} could not be put back: {e}", file.path))?;
            done.restored += 1;
        } else if remove_with_empty_parents(root, &file.path) {
            done.removed += 1;
        }
    }
    Ok(done)
}

// --- the task ----------------------------------------------------------------------------------

/// Called right before a change is applied (`path`) or an allowed command runs (no path). The
/// first call of a request records the folder; a failure here never stops the work, it only
/// means that request cannot be taken back.
pub async fn before_change(app: &Shared, id: &str, folder: &Folder, path: Option<&str>) {
    let Some(task) = app.agent.get(id) else { return };
    let request = request_of(&task);
    let (root, agent_dir) = (folder.root().to_path_buf(), app.agent.dir.clone());
    let (task_id, path) = (id.to_string(), path.map(str::to_string));
    let known = task.extra.undo.iter().find(|s| s.request == request).cloned();
    let fresh = known.is_none();
    let made = tokio::task::spawn_blocking(move || {
        let mut snapshot = known.unwrap_or_else(|| {
            let git = if is_git(&root) { git_snapshot(&root, &task_id, request).ok() } else { None };
            let (kind, commit, skipped) = match git {
                Some((commit, skipped)) => ("git", Some(commit), skipped),
                None => ("copies", None, Vec::new()),
            };
            Snapshot { request, kind: kind.into(), commit, skipped, files: Vec::new(), commands: false, undone: false, undone_at: 0 }
        });
        let before = (snapshot.files.len(), snapshot.commands);
        if snapshot.kind == "copies" {
            match &path {
                Some(path) => {
                    copy_save(&store(&agent_dir, &task_id, request), &root, &mut snapshot.files, path).ok();
                }
                None => snapshot.commands = true,
            }
        }
        (fresh || before != (snapshot.files.len(), snapshot.commands)).then_some(snapshot)
    })
    .await
    .ok()
    .flatten();
    if let Some(snapshot) = made {
        app.agent.change(id, |t| {
            t.extra.undo.retain(|s| s.request != snapshot.request);
            t.extra.undo.push(snapshot);
        });
    }
}

/// What the model is told during the request that follows an undo.
pub fn context(task: &Task) -> Option<&'static str> {
    let request = request_of(task);
    task.extra.undo.iter().find(|s| s.undone && s.undone_at + 1 == request).map(|_| {
        "The person took back the changes of the last request. The files are as they were before it. Read a file again before you change it."
    })
}

/// The sentence the person reads after an undo.
fn said(done: &Restored, snapshot: &Snapshot) -> String {
    let count = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    let mut text = match (done.restored, done.removed) {
        (0, 0) => "Nothing had changed since this request began.".to_string(),
        (r, 0) => format!("Took this request back: {} put back.", count(r, "file", "files")),
        (0, m) => format!("Took this request back: {} removed.", count(m, "new file", "new files")),
        (r, m) => format!("Took this request back: {} put back, {} removed.", count(r, "file", "files"), count(m, "new file", "new files")),
    };
    if snapshot.kind == "copies" && snapshot.commands {
        text.push(' ');
        text.push_str(NOT_BY_COMMANDS);
    }
    text
}

/// `POST /v1/agent/tasks/{id}/undo`: takes back the newest request that was not taken back yet.
pub async fn task_undo(State(app): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    let Some(task) = app.agent.get(&id) else { return fail(StatusCode::NOT_FOUND, "no such task") };
    if task.active() {
        return fail(StatusCode::CONFLICT, "This task is still working. Wait for it, or stop it first.");
    }
    let Some(snapshot) = task.extra.undo.iter().rev().find(|s| s.open()).cloned() else {
        return fail(StatusCode::CONFLICT, "There is nothing to take back in this task.");
    };
    let folder = match Folder::open(&task.folder) {
        Ok(f) => f,
        Err(e) => return fail(StatusCode::CONFLICT, e),
    };
    let (root, agent_dir, task_id, snap) = (folder.root().to_path_buf(), app.agent.dir.clone(), id.clone(), snapshot.clone());
    let outcome = tokio::task::spawn_blocking(move || match snap.kind.as_str() {
        "git" => git_restore(&root, &task_id, &snap),
        _ => copy_restore(&store(&agent_dir, &task_id, snap.request), &root, &snap.files),
    })
    .await
    .unwrap_or_else(|_| Err("That did not work.".into()));
    let done = match outcome {
        Ok(done) => done,
        Err(why) => return fail(StatusCode::CONFLICT, format!("The request could not be taken back. {why}")),
    };
    let text = said(&done, &snapshot);
    app.record("agent_undo", json!({"kind": snapshot.kind, "restored": done.restored, "removed": done.removed}));
    // Checked again and changed in one step: the task may have been given new work meanwhile.
    app.agent.change(&id, |t| {
        let at = request_of(t);
        if let Some(s) = t.extra.undo.iter_mut().find(|s| s.request == snapshot.request) {
            s.undone = true;
            s.undone_at = at;
        }
        t.steps.push(Step::Note { text: text.clone() });
    });
    match app.agent.get(&id) {
        Some(task) => Json(json!({"task": task.view(), "restored": done.restored, "removed": done.removed})).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such task"),
    }
}

/// A task is removed: its copies and hidden refs go with it.
pub fn forget(agent_dir: &Path, task: &Task) {
    std::fs::remove_dir_all(agent_dir.join("undo").join(&task.id)).ok();
    if task.extra.undo.iter().any(|s| s.kind == "git") {
        let (root, id) = (PathBuf::from(&task.folder), task.id.clone());
        std::thread::spawn(move || {
            if is_git(&root) {
                git_forget(&root, &id);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let base = files::scratch(name);
        let root = base.join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::canonicalize(root).unwrap()
    }

    fn sh(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Person",
                "-c",
                "user.email=person@example.com",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn read(root: &Path, path: &str) -> String {
        std::fs::read_to_string(root.join(path)).unwrap()
    }

    fn write(root: &Path, path: &str, text: &str) {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), text).unwrap();
    }

    fn snapshot_of(root: &Path, task: &str, request: usize) -> Snapshot {
        let (commit, skipped) = git_snapshot(root, task, request).unwrap();
        Snapshot { request, kind: "git".into(), commit: Some(commit), skipped, files: vec![], commands: false, undone: false, undone_at: 0 }
    }

    #[test]
    fn a_git_repository_goes_back_to_the_snapshot_and_nothing_else_moves() {
        let root = project("undo-git");
        sh(&root, &["init", "-q"]);
        write(&root, ".gitignore", "build/\n*.log\n");
        write(&root, "src/lib.txt", "one\n");
        write(&root, "src/gone.txt", "will be deleted by the task\n");
        write(&root, "staged.txt", "first\n");
        sh(&root, &["add", "-A"]);
        sh(&root, &["commit", "-q", "-m", "start"]);
        // What the person did before the task: an edit that is not committed, a staged change,
        // a new file git does not know yet, and files git ignores.
        write(&root, "src/lib.txt", "one\nthe person's own line\n");
        write(&root, "staged.txt", "second\n");
        sh(&root, &["add", "staged.txt"]);
        write(&root, "notes.txt", "mine\n");
        write(&root, "build/out.bin", "built before\n");
        write(&root, "debug.log", "before\n");
        let (head, status, staged) =
            (sh(&root, &["rev-parse", "HEAD"]), sh(&root, &["status", "--porcelain"]), sh(&root, &["diff", "--cached"]));

        let snap = snapshot_of(&root, "t1", 1);
        assert_eq!(sh(&root, &["rev-parse", "refs/rigbrew/t1/1"]).trim(), snap.commit.as_deref().unwrap());
        assert_eq!(sh(&root, &["status", "--porcelain"]), status, "taking a snapshot changes nothing the person sees");
        assert_eq!(sh(&root, &["rev-parse", "HEAD"]), head);

        // The task: edits, creates (also in a new folder), deletes, and a build writes ignored files.
        write(&root, "src/lib.txt", "changed by the task\n");
        write(&root, "notes.txt", "overwritten by the task\n");
        write(&root, "src/new/made.txt", "made by the task\n");
        write(&root, "made-too.txt", "made by the task\n");
        std::fs::remove_file(root.join("src/gone.txt")).unwrap();
        write(&root, "build/out.bin", "built by the task\n");
        write(&root, "debug.log", "after\n");

        let done = git_restore(&root, "t1", &snap).unwrap();
        assert_eq!(done, Restored { restored: 3, removed: 2 });
        assert_eq!(read(&root, "src/lib.txt"), "one\nthe person's own line\n", "the person's edit from before the task is kept");
        assert_eq!(read(&root, "notes.txt"), "mine\n");
        assert_eq!(read(&root, "src/gone.txt"), "will be deleted by the task\n");
        assert!(!root.join("src/new/made.txt").exists(), "a file the task created is removed");
        assert!(!root.join("src/new").exists(), "and the folder that held only it");
        assert!(!root.join("made-too.txt").exists());
        assert_eq!(read(&root, "build/out.bin"), "built by the task\n", "what git ignores is not touched");
        assert_eq!(read(&root, "debug.log"), "after\n");
        // The index, the branch and the stash are where the person left them.
        assert_eq!(sh(&root, &["rev-parse", "HEAD"]), head);
        assert_eq!(sh(&root, &["diff", "--cached"]), staged);
        assert_eq!(sh(&root, &["status", "--porcelain"]), status);
        assert_eq!(sh(&root, &["stash", "list"]), "");
        assert_eq!(sh(&root, &["branch", "--list"]).trim(), "* main");
        // The state before the undo is kept too.
        assert!(sh(&root, &["show", "refs/rigbrew/t1/1-undone:made-too.txt"]).contains("made by the task"));
        // Nothing of the temporary index is left behind.
        let left: Vec<_> = std::fs::read_dir(root.join(".git"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("rigbrew-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        // A second undo of the same snapshot finds nothing to do.
        assert_eq!(git_restore(&root, "t1", &snap).unwrap(), Restored::default());
        git_forget(&root, "t1");
        assert_eq!(sh(&root, &["for-each-ref", "refs/rigbrew/"]), "");
    }

    #[test]
    fn a_repository_without_a_commit_can_be_taken_back_too() {
        let root = project("undo-git-empty");
        sh(&root, &["init", "-q"]);
        write(&root, "a.txt", "a\n");
        let snap = snapshot_of(&root, "t2", 1);
        write(&root, "a.txt", "b\n");
        write(&root, "b.txt", "b\n");
        assert_eq!(git_restore(&root, "t2", &snap).unwrap(), Restored { restored: 1, removed: 1 });
        assert_eq!(read(&root, "a.txt"), "a\n");
        assert!(!root.join("b.txt").exists());
    }

    #[test]
    fn a_folder_that_is_not_its_own_repository_uses_copies() {
        let root = project("undo-kind");
        assert!(!is_git(&root));
        // A linked worktree and a submodule have a `.git` file, not a folder.
        write(&root, ".git", "gitdir: /somewhere/else\n");
        assert!(!is_git(&root));
        std::fs::remove_file(root.join(".git")).unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        assert!(is_git(&root));
    }

    #[test]
    fn a_plain_folder_gets_its_files_back_from_copies() {
        let root = project("undo-copies");
        let keep = files::scratch("undo-store").join("agent");
        let store = store(&keep, "t3", 1);
        write(&root, "src/lib.txt", "as the person left it\n");
        write(&root, "other.txt", "never touched\n");
        let mut saved = Vec::new();
        // Before each write: the file as it is. A second change of the same file keeps the first copy.
        copy_save(&store, &root, &mut saved, "src/lib.txt").unwrap();
        write(&root, "src/lib.txt", "first change\n");
        copy_save(&store, &root, &mut saved, "src/lib.txt").unwrap();
        write(&root, "src/lib.txt", "second change\n");
        copy_save(&store, &root, &mut saved, "docs/new/made.txt").unwrap();
        write(&root, "docs/new/made.txt", "made by the task\n");
        assert_eq!(
            saved,
            [Saved { path: "src/lib.txt".into(), existed: true }, Saved { path: "docs/new/made.txt".into(), existed: false }]
        );
        write(&root, "other.txt", "changed by a command\n");

        assert_eq!(copy_restore(&store, &root, &saved).unwrap(), Restored { restored: 1, removed: 1 });
        assert_eq!(read(&root, "src/lib.txt"), "as the person left it\n", "the state from before the task comes back");
        assert!(!root.join("docs/new/made.txt").exists(), "a file the task created is removed");
        assert!(!root.join("docs").exists());
        assert_eq!(read(&root, "other.txt"), "changed by a command\n", "what a command changed stays");
    }

    #[test]
    fn the_person_is_told_what_came_back() {
        let mut snap = Snapshot {
            request: 1,
            kind: "git".into(),
            commit: None,
            skipped: vec![],
            files: vec![],
            commands: false,
            undone: false,
            undone_at: 0,
        };
        assert_eq!(said(&Restored { restored: 2, removed: 1 }, &snap), "Took this request back: 2 files put back, 1 new file removed.");
        assert_eq!(said(&Restored { restored: 1, removed: 0 }, &snap), "Took this request back: 1 file put back.");
        assert_eq!(said(&Restored::default(), &snap), "Nothing had changed since this request began.");
        snap.kind = "copies".into();
        snap.commands = true;
        assert!(said(&Restored { restored: 0, removed: 3 }, &snap).ends_with(NOT_BY_COMMANDS));
        assert!(!snap.open(), "outside git, a request in which only commands ran has nothing to take back");
        snap.files.push(Saved { path: "a.txt".into(), existed: true });
        assert!(snap.open());
        snap.undone = true;
        assert!(!snap.open());
    }
}
