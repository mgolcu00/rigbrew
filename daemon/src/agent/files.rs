//! What a task may do on disk: everything stays inside the folder the person chose.
//!
//! Paths are resolved through the file system (`canonicalize`), so `..`, absolute paths and
//! symbolic links that lead out of the folder are all refused the same way. Everything that
//! goes back to the model is capped, and says so when it was cut.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Characters of one tool result the model gets to see.
pub const RESULT_CHARS: usize = 4000;
/// Largest file `read_file` and `edit_file` open.
pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
/// Largest file `search` looks into.
const MAX_SEARCH_BYTES: u64 = 1024 * 1024;
/// Largest content `write_file` accepts.
pub const MAX_WRITE_BYTES: usize = 400 * 1024;
/// Longest diff kept with a step.
const MAX_DIFF_CHARS: usize = 30_000;
const MAX_LIST_ENTRIES: usize = 300;
const MAX_SEARCH_HITS: usize = 50;
const MAX_SEARCH_FILES: usize = 5000;
/// Command output kept in memory while it runs.
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// Folders that are never listed or searched, whether an ignore file names them or not.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".svelte-kit", ".venv", "__pycache__", ".next", ".gradle"];

/// The first `max` characters, with a note when something was left out.
pub fn cap(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}\n[truncated: {max} of {total} characters shown]")
}

/// The start and the end of a long text (the end of a build or test log is where the answer is).
pub fn cap_ends(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let head_len = max / 3;
    let tail_len = max - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    format!("{head}\n[truncated: {} characters left out here]\n{tail}", total - max)
}

/// The folder a task works in.
#[derive(Clone, Debug)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    /// Opens an existing folder. The path is made canonical once, here.
    pub fn open(path: &str) -> Result<Folder, String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("Choose a folder first.".into());
        }
        let root = std::fs::canonicalize(path).map_err(|_| format!("There is no folder at {path}."))?;
        if !root.is_dir() {
            return Err(format!("{path} is a file, not a folder."));
        }
        if root.parent().is_none() {
            return Err("Choose a folder inside the disk, not the whole disk.".into());
        }
        Ok(Folder { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path as the model should name it: relative to the folder, with forward slashes.
    pub fn relative(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        let text = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        if text.is_empty() {
            ".".into()
        } else {
            text
        }
    }

    fn join(&self, path: &str) -> PathBuf {
        let path = path.trim();
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn outside(path: &str) -> String {
        format!("{path} is outside the folder. Use a path inside the folder, like src/main.rs.")
    }

    /// A file or folder that exists, inside the folder after every link is followed.
    pub fn existing(&self, path: &str) -> Result<PathBuf, String> {
        let wanted = if path.trim().is_empty() { "." } else { path };
        let real = std::fs::canonicalize(self.join(wanted)).map_err(|_| format!("There is no {wanted} in the folder."))?;
        if !real.starts_with(&self.root) {
            return Err(Self::outside(wanted));
        }
        Ok(real)
    }

    /// A file to write, which may not exist yet. The deepest part of the path that exists must
    /// be inside the folder after links are followed; what does not exist yet cannot contain `..`.
    pub fn target(&self, path: &str) -> Result<PathBuf, String> {
        if path.trim().is_empty() {
            return Err("A file path is needed.".into());
        }
        let joined = self.join(path);
        if joined.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!("{path} contains \"..\". Use a plain path inside the folder."));
        }
        // Walk up to the deepest part that exists. A link that points nowhere counts as existing,
        // so writing "through" it is refused below.
        let mut base = joined.clone();
        let mut rest: Vec<std::ffi::OsString> = Vec::new();
        while std::fs::symlink_metadata(&base).is_err() {
            let Some(name) = base.file_name().map(|n| n.to_os_string()) else {
                return Err(Self::outside(path));
            };
            rest.push(name);
            if !base.pop() {
                return Err(Self::outside(path));
            }
        }
        let real = std::fs::canonicalize(&base).map_err(|_| format!("{path} goes through a link that leads nowhere."))?;
        if !real.starts_with(&self.root) {
            return Err(Self::outside(path));
        }
        if rest.is_empty() && real.is_dir() {
            return Err(format!("{path} is a folder, not a file."));
        }
        if !rest.is_empty() && !real.is_dir() {
            return Err(format!("{path} goes through a file, not a folder."));
        }
        let mut out = real;
        for name in rest.into_iter().rev() {
            out.push(name);
        }
        let rel = out.strip_prefix(&self.root).unwrap_or(&out);
        if rel.components().next().is_some_and(|c| c.as_os_str() == ".git") {
            return Err("Files inside .git are not changed here.".into());
        }
        Ok(out)
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Whether this is a folder of build output or dependencies that is skipped even when no
/// ignore file names it.
fn heavy(entry: &ignore::DirEntry) -> bool {
    entry.depth() > 0 && entry.file_type().is_some_and(|t| t.is_dir()) && entry.file_name().to_str().is_some_and(|n| SKIP_DIRS.contains(&n))
}

/// Everything under `start`, folders before what is in them and names in order, the way git
/// sees the folder: what `.gitignore`, `.ignore` and git's own exclude files name is left out,
/// in a repository or not. Hidden files are kept (`.github`, `.env.example`); links are
/// reported and never followed. The folders in `SKIP_DIRS` are never entered; with `name_heavy`
/// they are still reported, so a listing can say they are there.
pub fn walk(start: &Path, depth: Option<usize>, name_heavy: bool) -> impl Iterator<Item = ignore::DirEntry> {
    let in_heavy = |e: &ignore::DirEntry| {
        e.depth() > 1 && e.path().parent().and_then(Path::file_name).and_then(|n| n.to_str()).is_some_and(|n| SKIP_DIRS.contains(&n))
    };
    ignore::WalkBuilder::new(start)
        .hidden(false)
        .require_git(false)
        .max_depth(depth)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |e| !in_heavy(e) && (name_heavy || !heavy(e)) && e.file_name() != ".DS_Store")
        .build()
        .flatten()
}

/// `list_files`: a tree of the folder, `depth` levels deep, without what git ignores.
pub fn list(folder: &Folder, path: &str, depth: usize) -> Result<String, String> {
    let start = folder.existing(path)?;
    if !start.is_dir() {
        return Err(format!("{path} is a file. Use read_file to read it."));
    }
    let depth = depth.clamp(1, 4);
    let mut lines = Vec::new();
    let mut more = false;
    for entry in walk(&start, Some(depth), true).filter(|e| e.depth() > 0) {
        if lines.len() >= MAX_LIST_ENTRIES {
            more = true;
            break;
        }
        let pad = "  ".repeat(entry.depth() - 1);
        let name = entry.file_name().to_string_lossy();
        let Some(kind) = entry.file_type() else { continue };
        lines.push(if heavy(&entry) {
            format!("{pad}{name}/ (not listed)")
        } else if kind.is_dir() {
            format!("{pad}{name}/")
        } else if kind.is_symlink() {
            format!("{pad}{name} (link)")
        } else {
            format!("{pad}{name} ({})", human_size(entry.metadata().map(|m| m.len()).unwrap_or(0)))
        });
    }
    if lines.is_empty() {
        return Ok(format!("{} is empty.", folder.relative(&start)));
    }
    let mut out = format!("{}/\n{}", folder.relative(&start), lines.join("\n"));
    if more {
        out.push_str(&format!("\n[truncated: only the first {MAX_LIST_ENTRIES} entries are shown; list a subfolder to see more]"));
    }
    Ok(cap(&out, RESULT_CHARS * 2))
}

pub fn read_text(path: &Path, shown: &str, limit: u64) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Cannot read {shown}: {e}"))?.len();
    if size > limit {
        return Err(format!("{shown} is {} and too large to open here. Use search to find the part you need.", human_size(size)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {shown}: {e}"))?;
    if looks_binary(&bytes) {
        return Err(format!("{shown} is not a text file."));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `read_file`: some lines of a text file, with where they are in the file.
pub fn read(folder: &Folder, path: &str, from_line: Option<usize>, lines: Option<usize>) -> Result<String, String> {
    let file = folder.existing(path)?;
    if file.is_dir() {
        return Err(format!("{path} is a folder. Use list_files to see what is in it."));
    }
    let text = read_text(&file, path, MAX_READ_BYTES)?;
    let all: Vec<&str> = text.lines().collect();
    let total = all.len();
    if total == 0 {
        return Ok(format!("{} is empty.", folder.relative(&file)));
    }
    let first = from_line.unwrap_or(1).max(1);
    if first > total {
        return Err(format!("{path} has only {total} lines."));
    }
    let want = lines.unwrap_or(200).clamp(1, 400);
    let mut out = String::new();
    let mut last = first - 1;
    let mut cut = false;
    for line in all.iter().skip(first - 1).take(want) {
        if !out.is_empty() && out.chars().count() + line.chars().count() > RESULT_CHARS {
            cut = true;
            break;
        }
        out.push_str(line);
        out.push('\n');
        last += 1;
    }
    let out = cap(&out, RESULT_CHARS);
    let mut head = format!("{} (lines {first} to {last} of {total})", folder.relative(&file));
    if cut {
        head.push_str(&format!(" [truncated: call read_file with from_line={} for more]", last + 1));
    } else if last < total {
        head.push_str(&format!(" [the file goes on: from_line={} reads more]", last + 1));
        if from_line.is_none() && total > 400 && super::outline::lang_of(&file).is_some() {
            head.push_str(" [outline shows what is on which line]");
        }
    }
    Ok(format!("{head}\n{out}"))
}

/// `search`: lines that contain `pattern`, as plain text or as a regular expression. Files git
/// ignores are not looked into, unless the path names one.
pub fn search(folder: &Folder, pattern: &str, path: Option<&str>) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("A pattern to look for is needed.".into());
    }
    let start = folder.existing(path.unwrap_or("."))?;
    // A pattern that is not a valid expression is still looked for as plain text.
    let re = regex::RegexBuilder::new(pattern).size_limit(1 << 20).build().ok();
    let hit = |line: &str| line.contains(pattern) || re.as_ref().is_some_and(|r| r.is_match(line));
    let mut hits: Vec<String> = Vec::new();
    let mut files = 0usize;
    let mut cut = false;
    for entry in walk(&start, None, false) {
        if hits.len() >= MAX_SEARCH_HITS || files >= MAX_SEARCH_FILES {
            cut = true;
            break;
        }
        // Links are never followed, so nothing outside the folder is read.
        if !entry.file_type().is_some_and(|t| t.is_file()) || entry.metadata().map_or(true, |m| m.len() > MAX_SEARCH_BYTES) {
            continue;
        }
        files += 1;
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        if looks_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            if hit(line) {
                if hits.len() >= MAX_SEARCH_HITS {
                    cut = true;
                    break;
                }
                let shown: String = line.trim().chars().take(200).collect();
                hits.push(format!("{}:{}: {shown}", folder.relative(entry.path()), n + 1));
            }
        }
    }
    if hits.is_empty() {
        return Ok(format!("Nothing in the folder matches {pattern}."));
    }
    let mut out = hits.join("\n");
    if cut {
        out.push_str(&format!("\n[truncated: only the first {MAX_SEARCH_HITS} matches are shown; narrow the pattern or the path]"));
    }
    Ok(cap(&out, RESULT_CHARS))
}

/// A change to one file, worked out before it is applied so the person can see it first.
#[derive(Debug)]
pub struct Change {
    path: PathBuf,
    /// Path relative to the folder, for messages.
    pub shown: String,
    old: Option<String>,
    new: String,
}

impl Change {
    /// The change as a unified diff.
    pub fn diff(&self) -> String {
        let old = self.old.as_deref().unwrap_or("");
        let from = if self.old.is_some() { format!("a/{}", self.shown) } else { "/dev/null".to_string() };
        let text = similar::TextDiff::from_lines(old, &self.new)
            .unified_diff()
            .context_radius(3)
            .header(&from, &format!("b/{}", self.shown))
            .to_string();
        cap(&text, MAX_DIFF_CHARS)
    }

    /// Writes the file. The path is resolved again, so a link put there in the meantime is refused.
    pub fn apply(&self, folder: &Folder) -> Result<String, String> {
        let path = folder.target(&self.path.to_string_lossy())?;
        if let Some(dir) = path.parent() {
            create_dirs(dir).map_err(|e| format!("Cannot create the folder for {}: {e}", self.shown))?;
        }
        let fresh = !path.exists();
        std::fs::write(&path, &self.new).map_err(|e| format!("Cannot write {}: {e}", self.shown))?;
        if fresh {
            open_up(&path, 0o644);
        }
        let lines = self.new.lines().count();
        Ok(if self.old.is_some() {
            format!("Changed {} ({lines} lines now).", self.shown)
        } else {
            format!("Created {} ({lines} lines).", self.shown)
        })
    }
}

/// The service keeps its own files private (umask 077); what a task writes in the person's
/// folder gets the usual permissions instead.
fn open_up(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).ok();
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

fn create_dirs(dir: &Path) -> std::io::Result<()> {
    let mut missing = Vec::new();
    let mut at = dir.to_path_buf();
    while !at.exists() {
        missing.push(at.clone());
        if !at.pop() {
            break;
        }
    }
    std::fs::create_dir_all(dir)?;
    for made in missing {
        open_up(&made, 0o755);
    }
    Ok(())
}

/// `write_file`: a new file, or the whole content of an existing one.
pub fn plan_write(folder: &Folder, path: &str, content: &str) -> Result<Change, String> {
    if content.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "That is {} of content; a file written here can be {} at most.",
            human_size(content.len() as u64),
            human_size(MAX_WRITE_BYTES as u64)
        ));
    }
    let target = folder.target(path)?;
    let shown = folder.relative(&target);
    let old = if target.exists() { Some(read_text(&target, &shown, MAX_READ_BYTES)?) } else { None };
    if old.as_deref() == Some(content) {
        return Err(format!("{shown} already has exactly this content."));
    }
    Ok(Change { path: target, shown, old, new: content.to_string() })
}

/// `edit_file`: replaces one piece of text. `old` must be in the file exactly once.
pub fn plan_edit(folder: &Folder, path: &str, old: &str, new: &str) -> Result<Change, String> {
    let target = folder.existing(path)?;
    if target.is_dir() {
        return Err(format!("{path} is a folder, not a file."));
    }
    // The same rules as a write (nothing inside .git).
    let target = folder.target(&target.to_string_lossy())?;
    let shown = folder.relative(&target);
    if old.is_empty() {
        return Err("The text to replace is empty. Give the exact text that is in the file now.".into());
    }
    if old == new {
        return Err("The old and the new text are the same.".into());
    }
    let text = read_text(&target, &shown, MAX_READ_BYTES)?;
    let count = text.matches(old).count();
    // A file with Windows line ends, asked about with plain ones.
    let (text, old, new, count) = if count == 0 && text.contains("\r\n") && !old.contains('\r') {
        let (old, new) = (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"));
        let count = text.matches(&old).count();
        (text, old, new, count)
    } else {
        (text, old.to_string(), new.to_string(), count)
    };
    match count {
        0 => Err(format!("The text to replace is not in {shown}. Read the file again and copy the lines exactly as they are.")),
        1 => {
            let changed = text.replacen(&old, &new, 1);
            if changed.len() > MAX_WRITE_BYTES.max(text.len()) {
                return Err("The file would become too large.".into());
            }
            Ok(Change { path: target, shown, old: Some(text), new: changed })
        }
        n => Err(format!("The text to replace is in {shown} {n} times. Include more of the lines around it so it matches one place.")),
    }
}

/// What a command left behind.
pub struct Ran {
    /// What the model reads: how it ended, then the capped output.
    pub text: String,
    pub ok: bool,
}

/// Kills the command's whole process group when the task is stopped or the time is up.
struct Group(#[cfg_attr(not(unix), allow(dead_code))] Option<u32>);

impl Group {
    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.take() {
            // Negative pid: every process in the group the shell started.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn drain(mut pipe: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // Keep reading past the cap so the command never blocks on a full pipe.
                let room = MAX_OUTPUT_BYTES.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    kept
}

/// The macOS sandbox profile a command runs under: it may read anything the person can, write
/// only inside the folder and the temporary folders, and reach the network only when the task
/// was given the web. The last matching rule wins.
#[cfg(target_os = "macos")]
fn seatbelt_profile(folder: &Path, network: bool) -> String {
    // A path inside a profile string: backslash and quote are the only characters to escape.
    let quoted = |p: &Path| p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let mut profile = String::from("(version 1)\n(allow default)\n(deny file-write*)\n");
    profile.push_str(&format!("(allow file-write* (subpath \"{}\"))\n", quoted(folder)));
    for tmp in ["/private/tmp", "/private/var/folders", "/dev"] {
        profile.push_str(&format!("(allow file-write* (subpath \"{tmp}\"))\n"));
    }
    if !network {
        profile.push_str("(deny network*)\n(allow network* (local unix))\n");
    }
    profile
}

/// Whether commands run inside a sandbox here: on macOS, with the system's own tool.
#[cfg(target_os = "macos")]
fn sandboxed() -> bool {
    Path::new("/usr/bin/sandbox-exec").is_file()
}

/// A program the service itself starts in the folder (git, for undo), confined the way a
/// command of the task is: on macOS it can write only inside the folder and has no network.
/// What it creates gets the usual permissions, not the private ones of the service.
pub fn confined(folder: &Path, program: &str) -> std::process::Command {
    #[cfg(target_os = "macos")]
    let mut cmd = if sandboxed() {
        let mut c = std::process::Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(seatbelt_profile(folder, false)).arg(program);
        c
    } else {
        std::process::Command::new(program)
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = std::process::Command::new(program);
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::umask(0o022);
            Ok(())
        });
    }
    cmd.current_dir(folder);
    cmd
}

/// `run_command`: one shell command with the folder as its working directory. On macOS it runs
/// in a sandbox: it cannot write outside the folder, and has no network unless `network` is set.
pub async fn run(folder: &Folder, command: &str, timeout: Duration, network: bool) -> Ran {
    use std::process::Stdio;
    #[cfg(not(target_os = "macos"))]
    let _ = network;
    #[cfg(unix)]
    let mut cmd = {
        #[cfg(target_os = "macos")]
        let mut c = if sandboxed() {
            let mut c = tokio::process::Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(seatbelt_profile(folder.root(), network)).arg("sh");
            c
        } else {
            tokio::process::Command::new("sh")
        };
        #[cfg(not(target_os = "macos"))]
        let mut c = tokio::process::Command::new("sh");
        // Errors and output in one stream, in the order they were written.
        c.arg("-c").arg(format!("exec 2>&1\n{command}"));
        c.process_group(0);
        // The service's own files are private; what a command creates is not the service's.
        unsafe {
            c.pre_exec(|| {
                libc::umask(0o022);
                Ok(())
            });
        }
        c
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    cmd.current_dir(folder.root()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let started = std::time::Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Ran { text: format!("The command could not be started: {e}"), ok: false },
    };
    let mut group = Group(child.id());
    let out = tokio::spawn(drain(child.stdout.take().expect("piped")));
    let err = tokio::spawn(drain(child.stderr.take().expect("piped")));
    let status = tokio::time::timeout(timeout, child.wait()).await;
    let timed_out = status.is_err();
    if timed_out {
        child.start_kill().ok();
    }
    // Something the command left running in the background may still hold the pipes open.
    // When it does, or the time is up, everything the command started is stopped; the pipes
    // close and what was written until then is read.
    let collect = async {
        let mut bytes = out.await.unwrap_or_default();
        bytes.extend(err.await.unwrap_or_default());
        bytes
    };
    tokio::pin!(collect);
    let bytes = match tokio::time::timeout(Duration::from_secs(if timed_out { 0 } else { 2 }), &mut collect).await {
        Ok(bytes) => bytes,
        Err(_) => {
            group.kill();
            tokio::time::timeout(Duration::from_secs(3), &mut collect).await.unwrap_or_default()
        }
    };
    let output = String::from_utf8_lossy(&bytes).replace('\0', "");
    let output = output.trim_end();
    let shown = if output.is_empty() { "(no output)".to_string() } else { cap_ends(output, RESULT_CHARS) };
    match status {
        Err(_) => Ran { text: format!("Stopped after {} seconds without finishing.\n{shown}", timeout.as_secs()), ok: false },
        Ok(Err(e)) => Ran { text: format!("The command failed to run: {e}"), ok: false },
        Ok(Ok(status)) => {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "none (killed)".into());
            Ran { text: format!("exit code {code} after {:.1} s\n{shown}", started.elapsed().as_secs_f64()), ok: status.success() }
        }
    }
}

#[cfg(test)]
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rigbrew-agent-{name}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> (Folder, PathBuf, PathBuf) {
        let base = scratch("files");
        let inside = base.join("project");
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(inside.join("src")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(inside.join("src/main.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(outside.join("secret.txt"), "keep out\n").unwrap();
        (Folder::open(inside.to_str().unwrap()).unwrap(), inside, outside)
    }

    #[test]
    fn paths_inside_the_folder_resolve() {
        let (f, inside, _) = folder();
        let real = std::fs::canonicalize(&inside).unwrap();
        assert_eq!(f.existing("src/main.txt").unwrap(), real.join("src/main.txt"));
        assert_eq!(f.existing("src/../src/main.txt").unwrap(), real.join("src/main.txt"));
        assert_eq!(f.existing("").unwrap(), real);
        // An absolute path is fine when it is inside.
        assert_eq!(f.existing(real.join("src/main.txt").to_str().unwrap()).unwrap(), real.join("src/main.txt"));
        assert_eq!(f.target("src/new/deep.txt").unwrap(), real.join("src/new/deep.txt"));
        assert_eq!(f.relative(&real.join("src/main.txt")), "src/main.txt");
    }

    #[test]
    fn parent_and_absolute_paths_outside_are_refused() {
        let (f, _, outside) = folder();
        assert!(f.existing("../elsewhere/secret.txt").is_err());
        assert!(f.existing("src/../../elsewhere/secret.txt").is_err());
        assert!(f.existing(outside.join("secret.txt").to_str().unwrap()).is_err());
        assert!(f.existing("/etc/hosts").is_err());
        assert!(f.target("../elsewhere/new.txt").is_err());
        assert!(f.target("src/../../elsewhere/new.txt").is_err());
        assert!(f.target(outside.join("new.txt").to_str().unwrap()).is_err());
        assert!(f.target("/tmp/rigbrew-should-not-exist.txt").is_err());
        assert!(f.target("").is_err());
        assert!(read(&f, "../elsewhere/secret.txt", None, None).is_err());
        assert!(plan_write(&f, "../elsewhere/secret.txt", "x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_that_leave_the_folder_are_refused() {
        use std::os::unix::fs::symlink;
        let (f, inside, outside) = folder();
        symlink(outside.join("secret.txt"), inside.join("link.txt")).unwrap();
        symlink(&outside, inside.join("away")).unwrap();
        symlink(outside.join("not-there.txt"), inside.join("dangling.txt")).unwrap();
        symlink(inside.join("src/main.txt"), inside.join("near.txt")).unwrap();
        assert!(f.existing("link.txt").is_err());
        assert!(f.existing("away/secret.txt").is_err());
        assert!(read(&f, "link.txt", None, None).is_err());
        // Writing through a link, into a linked folder, or onto a link that points nowhere.
        assert!(f.target("link.txt").is_err());
        assert!(f.target("away/new.txt").is_err());
        assert!(f.target("away/sub/new.txt").is_err());
        assert!(f.target("dangling.txt").is_err());
        assert!(plan_write(&f, "dangling.txt", "x").is_err());
        assert!(plan_edit(&f, "link.txt", "keep", "let").is_err());
        assert!(!outside.join("not-there.txt").exists());
        // A link that stays inside is followed.
        assert!(read(&f, "near.txt", None, None).unwrap().contains("two"));
        // Search and list do not follow links out either.
        assert!(!search(&f, "keep out", None).unwrap().contains("secret"));
        assert!(list(&f, ".", 3).unwrap().contains("away (link)"));
    }

    #[test]
    fn git_internals_are_not_written() {
        let (f, inside, _) = folder();
        std::fs::create_dir_all(inside.join(".git")).unwrap();
        assert!(f.target(".git/config").is_err());
        assert!(f.target(".gitignore").is_ok());
    }

    #[test]
    fn write_creates_and_shows_a_diff() {
        let (f, inside, _) = folder();
        let change = plan_write(&f, "docs/notes.txt", "hello\n").unwrap();
        assert!(change.diff().contains("+hello"));
        assert!(change.diff().contains("/dev/null"));
        assert!(!inside.join("docs/notes.txt").exists(), "planning writes nothing");
        assert!(change.apply(&f).unwrap().starts_with("Created docs/notes.txt"));
        assert_eq!(std::fs::read_to_string(inside.join("docs/notes.txt")).unwrap(), "hello\n");
        assert!(plan_write(&f, "docs/notes.txt", "hello\n").is_err(), "same content");
        assert!(plan_write(&f, "big.txt", &"x".repeat(MAX_WRITE_BYTES + 1)).is_err());
        assert!(plan_write(&f, "src", "x").is_err(), "a folder");
    }

    #[test]
    fn edit_replaces_exactly_one_place() {
        let (f, inside, _) = folder();
        let change = plan_edit(&f, "src/main.txt", "two\n", "2\n").unwrap();
        let diff = change.diff();
        assert!(diff.contains("-two") && diff.contains("+2"), "{diff}");
        change.apply(&f).unwrap();
        assert_eq!(std::fs::read_to_string(inside.join("src/main.txt")).unwrap(), "one\n2\nthree\n");
        assert!(plan_edit(&f, "src/main.txt", "four", "4").unwrap_err().contains("is not in"));
        assert!(plan_edit(&f, "src/main.txt", "", "4").is_err());
        assert!(plan_edit(&f, "src/main.txt", "one", "one").is_err());
        assert!(plan_edit(&f, "src/missing.txt", "a", "b").is_err());
        std::fs::write(inside.join("twice.txt"), "a\nb\na\n").unwrap();
        assert!(plan_edit(&f, "twice.txt", "a", "c").unwrap_err().contains("2 times"));
        std::fs::write(inside.join("crlf.txt"), "a\r\nb\r\nc\r\n").unwrap();
        plan_edit(&f, "crlf.txt", "a\nb", "a\nB").unwrap().apply(&f).unwrap();
        assert_eq!(std::fs::read_to_string(inside.join("crlf.txt")).unwrap(), "a\r\nB\r\nc\r\n");
    }

    #[test]
    fn read_gives_a_range_and_says_when_there_is_more() {
        let (f, inside, _) = folder();
        let out = read(&f, "src/main.txt", Some(2), Some(1)).unwrap();
        assert!(out.starts_with("src/main.txt (lines 2 to 2 of 3)"), "{out}");
        assert!(out.contains("[the file goes on: from_line=3 reads more]"));
        assert!(out.ends_with("two\n"));
        assert!(read(&f, "src/main.txt", Some(9), None).is_err());
        assert!(read(&f, "src", None, None).is_err());
        let long: String = (0..2000).map(|i| format!("line number {i}\n")).collect();
        std::fs::write(inside.join("long.txt"), &long).unwrap();
        let out = read(&f, "long.txt", None, Some(400)).unwrap();
        assert!(out.chars().count() < RESULT_CHARS + 200);
        assert!(out.contains("[truncated: call read_file with from_line="));
        std::fs::write(inside.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        assert!(read(&f, "blob.bin", None, None).unwrap_err().contains("not a text file"));
    }

    #[test]
    fn search_finds_text_and_patterns_and_skips_what_it_should() {
        let (f, inside, _) = folder();
        std::fs::create_dir_all(inside.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(inside.join(".git")).unwrap();
        std::fs::create_dir_all(inside.join("target")).unwrap();
        std::fs::write(inside.join("node_modules/x/a.txt"), "three\n").unwrap();
        std::fs::write(inside.join(".git/a.txt"), "three\n").unwrap();
        std::fs::write(inside.join("target/a.txt"), "three\n").unwrap();
        std::fs::write(inside.join("blob.bin"), b"three\0").unwrap();
        assert_eq!(search(&f, "three", None).unwrap(), "src/main.txt:3: three");
        assert_eq!(search(&f, "^t(wo|en)$", None).unwrap(), "src/main.txt:2: two");
        // Not a valid expression: looked for as it is.
        std::fs::write(inside.join("code.txt"), "call(foo[\n").unwrap();
        assert_eq!(search(&f, "foo[", None).unwrap(), "code.txt:1: call(foo[");
        assert!(search(&f, "nowhere to be seen", None).unwrap().starts_with("Nothing"));
        assert!(search(&f, "one", Some("../elsewhere")).is_err());
        let many: String = (0..200).map(|i| format!("needle {i}\n")).collect();
        std::fs::write(inside.join("many.txt"), many).unwrap();
        let out = search(&f, "needle", None).unwrap();
        assert!(out.contains("[truncated"));
        assert_eq!(out.lines().filter(|l| l.starts_with("many.txt:")).count(), MAX_SEARCH_HITS);
    }

    #[test]
    fn what_git_ignores_is_not_listed_or_searched() {
        let (f, inside, _) = folder();
        // No repository here: the ignore file counts all the same.
        std::fs::write(inside.join(".gitignore"), "dist/\n*.log\n/local.txt\n!keep.log\n").unwrap();
        for dir in ["dist/assets", "src/dist", ".github/workflows", "docs"] {
            std::fs::create_dir_all(inside.join(dir)).unwrap();
        }
        for (path, text) in [
            ("dist/assets/app.txt", "needle in build output\n"),
            ("src/dist/deep.txt", "needle in nested build output\n"),
            ("debug.log", "needle in a log\n"),
            ("keep.log", "needle kept\n"),
            ("local.txt", "needle at the top\n"),
            ("docs/local.txt", "needle further down\n"),
            (".github/workflows/ci.txt", "needle in a hidden folder\n"),
            ("docs/.ignore", "draft.txt\n"),
            ("docs/draft.txt", "needle in a draft\n"),
        ] {
            std::fs::write(inside.join(path), text).unwrap();
        }
        let found = search(&f, "needle", None).unwrap();
        let files: Vec<&str> = found.lines().map(|l| l.split(':').next().unwrap()).collect();
        assert_eq!(files, [".github/workflows/ci.txt", "docs/local.txt", "keep.log"], "{found}");
        // The rules of the folder also hold when only a part of it is searched.
        assert!(search(&f, "needle", Some("src")).unwrap().starts_with("Nothing"));
        // A file named outright is looked into, ignored or not.
        assert_eq!(search(&f, "needle", Some("debug.log")).unwrap(), "debug.log:1: needle in a log");
        assert!(read(&f, "dist/assets/app.txt", None, None).unwrap().contains("needle"));
        let tree = list(&f, ".", 3).unwrap();
        assert!(tree.contains(".github/\n  workflows/\n    ci.txt"), "{tree}");
        assert!(tree.contains(".gitignore") && tree.contains("keep.log"), "{tree}");
        for gone in ["dist", "debug.log", "draft.txt", "deep.txt"] {
            assert!(!tree.contains(gone), "{gone} is listed: {tree}");
        }
        assert_eq!(tree.matches("local.txt").count(), 1, "{tree}");
        // An ignored folder can still be listed by name.
        assert!(list(&f, "dist", 2).unwrap().contains("app.txt"));
    }

    #[test]
    fn list_shows_a_tree_without_the_heavy_folders() {
        let (f, inside, _) = folder();
        std::fs::create_dir_all(inside.join("node_modules/x")).unwrap();
        let out = list(&f, ".", 2).unwrap();
        assert!(out.contains("src/\n  main.txt (14 B)"), "{out}");
        assert!(out.contains("node_modules/ (not listed)"));
        assert!(!list(&f, ".", 1).unwrap().contains("main.txt"));
        assert!(list(&f, "..", 1).is_err());
    }

    #[test]
    fn caps_say_what_was_left_out() {
        assert_eq!(cap("short", 10), "short");
        let capped = cap(&"a".repeat(50), 10);
        assert!(capped.starts_with("aaaaaaaaaa\n[truncated: 10 of 50"));
        let text = format!("{}{}", "a".repeat(100), "z".repeat(100));
        let ends = cap_ends(&text, 30);
        assert!(ends.starts_with("aaaaaaaaaa\n[truncated: 170 characters left out here]\n"));
        assert!(ends.ends_with(&"z".repeat(20)));
        // Characters, not bytes: nothing is cut in the middle of a letter.
        assert!(cap(&"ş".repeat(20), 5).starts_with("şşşşş\n"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn commands_run_in_the_folder_with_capped_output_and_a_time_limit() {
        let (f, inside, _) = folder();
        let ran = run(&f, "pwd; echo to-stderr 1>&2", Duration::from_secs(10), false).await;
        assert!(ran.ok);
        assert!(ran.text.contains(std::fs::canonicalize(&inside).unwrap().to_str().unwrap()), "{}", ran.text);
        assert!(ran.text.contains("to-stderr"));
        // On macOS the command cannot write outside the folder, and has no network unless given it.
        #[cfg(target_os = "macos")]
        {
            let outside = std::env::var("HOME").unwrap_or_default() + "/rigbrew-sandbox-probe";
            let ran =
                run(&f, &format!("echo inside > made-here.txt && cat made-here.txt; echo x > {outside}"), Duration::from_secs(10), false)
                    .await;
            assert!(ran.text.contains("inside"), "{}", ran.text);
            assert!(!std::path::Path::new(&outside).exists(), "a command wrote outside the folder");
            assert!(ran.text.contains("not permitted") || ran.text.contains("denied"), "{}", ran.text);
            let net = run(
                &f,
                "curl -s -m 5 -o /dev/null -w '%{http_code}' https://example.com || echo no-network",
                Duration::from_secs(15),
                false,
            )
            .await;
            assert!(net.text.contains("no-network"), "{}", net.text);
        }
        let ran = run(&f, "exit 3", Duration::from_secs(10), false).await;
        assert!(!ran.ok && ran.text.starts_with("exit code 3"), "{}", ran.text);
        let ran = run(&f, "i=0; while [ $i -lt 3000 ]; do echo line-$i; i=$((i+1)); done", Duration::from_secs(20), false).await;
        assert!(ran.text.contains("[truncated:") && ran.text.contains("line-2999") && ran.text.chars().count() < RESULT_CHARS + 200);
        let started = std::time::Instant::now();
        let ran = run(&f, "sleep 30", Duration::from_millis(300), false).await;
        assert!(!ran.ok && ran.text.starts_with("Stopped after"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
