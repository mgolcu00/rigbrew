//! Downloader: fetch package files, resume interrupted downloads, verify every byte.
//!
//! A file is written to `<name>.part` and only renamed into place after its size and sha256
//! match the manifest. An interrupted download continues from the bytes already on disk
//! (HTTP Range); if the server ignores the range, the download starts over.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::manifest::{FileRef, Manifest, Package, Variant};

#[derive(Debug)]
pub enum DownloadError {
    Io(std::io::Error),
    Http(reqwest::Error),
    Status {
        url: String,
        status: u16,
    },
    /// The bytes we received are not the bytes the manifest promised.
    Corrupt {
        path: String,
        why: String,
    },
    NoSpace {
        needed_mb: u64,
        free_mb: u64,
    },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Io(e) => write!(f, "disk error: {e}"),
            DownloadError::Http(e) => write!(f, "network error: {e}"),
            DownloadError::Status { url, status } => write!(f, "{url} answered HTTP {status}"),
            DownloadError::Corrupt { path, why } => write!(f, "{path} failed verification ({why}); the partial file was removed"),
            DownloadError::NoSpace { needed_mb, free_mb } => write!(f, "not enough disk space: {needed_mb} MB needed, {free_mb} MB free"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        DownloadError::Io(e)
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        DownloadError::Http(e)
    }
}

/// Progress of one file: bytes on disk so far and the total.
#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    pub path: &'a str,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Already on disk and verified.
    Present,
    Downloaded,
    /// Continued from a partial file.
    Resumed,
}

/// HTTP client for registry downloads.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("rigbrew/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("http client")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Feeds an existing file into the hasher; returns its length.
async fn hash_file(path: &Path, hasher: &mut Sha256) -> std::io::Result<u64> {
    let mut f = File::open(path).await?;
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let read = f.read(&mut buf).await?;
        if read == 0 {
            return Ok(n);
        }
        hasher.update(&buf[..read]);
        n += read as u64;
    }
}

/// Downloads one manifest file into `data_dir`, resuming and verifying.
pub async fn fetch_file(
    client: &reqwest::Client,
    base_url: &str,
    file: &FileRef,
    data_dir: &Path,
    on_progress: &mut (dyn FnMut(Progress<'_>) + Send),
) -> Result<Outcome, DownloadError> {
    let dest = data_dir.join(&file.path);
    let part = PathBuf::from(format!("{}.part", dest.display()));
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).await?;
    }

    if let Ok(meta) = fs::metadata(&dest).await {
        if meta.len() == file.size {
            let mut h = Sha256::new();
            hash_file(&dest, &mut h).await?;
            if hex(&h.finalize()) == file.sha256.to_lowercase() {
                on_progress(Progress { path: &file.path, done: file.size, total: file.size });
                return Ok(Outcome::Present);
            }
        }
        fs::remove_file(&dest).await?;
    }

    let mut hasher = Sha256::new();
    let mut have = match fs::metadata(&part).await {
        Ok(meta) if meta.len() <= file.size => hash_file(&part, &mut hasher).await?,
        Ok(_) => {
            fs::remove_file(&part).await?;
            0
        }
        Err(_) => 0,
    };
    let resumed = have > 0;

    if have < file.size {
        let url = format!("{}/{}", base_url.trim_end_matches('/'), file.path);
        let mut req = client.get(&url);
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if have > 0 && status == reqwest::StatusCode::OK {
            // The server sent the whole file again: start over.
            have = 0;
            hasher = Sha256::new();
            fs::remove_file(&part).await.ok();
        } else if !(status == reqwest::StatusCode::OK || status == reqwest::StatusCode::PARTIAL_CONTENT) {
            return Err(DownloadError::Status { url, status: status.as_u16() });
        }
        let mut out = OpenOptions::new().create(true).append(true).open(&part).await?;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            have += chunk.len() as u64;
            if have > file.size {
                drop(out);
                fs::remove_file(&part).await.ok();
                return Err(DownloadError::Corrupt {
                    path: file.path.clone(),
                    why: "server sent more bytes than the manifest lists".into(),
                });
            }
            hasher.update(&chunk);
            out.write_all(&chunk).await?;
            on_progress(Progress { path: &file.path, done: have, total: file.size });
        }
        out.flush().await?;
        out.sync_all().await?;
    }

    if have != file.size {
        // Short read: keep the partial file so the next attempt resumes.
        return Err(DownloadError::Corrupt {
            path: file.path.clone(),
            why: format!("connection closed at {have} of {} bytes; run again to resume", file.size),
        });
    }
    if hex(&hasher.finalize()) != file.sha256.to_lowercase() {
        fs::remove_file(&part).await.ok();
        return Err(DownloadError::Corrupt { path: file.path.clone(), why: "sha256 does not match the manifest".into() });
    }
    fs::rename(&part, &dest).await?;
    Ok(if resumed { Outcome::Resumed } else { Outcome::Downloaded })
}

/// Downloads every file of a variant. Checks free disk space first.
pub async fn pull(
    client: &reqwest::Client,
    manifest: &Manifest,
    package: &Package,
    variant: &Variant,
    data_dir: &Path,
    free_mb: u64,
    on_progress: &mut (dyn FnMut(Progress<'_>) + Send),
) -> Result<Vec<(String, Outcome)>, DownloadError> {
    let _ = package;
    let mut missing = 0u64;
    for f in &variant.files {
        let on_disk = fs::metadata(data_dir.join(&f.path)).await.map(|m| m.len()).unwrap_or(0);
        let partial = fs::metadata(format!("{}.part", data_dir.join(&f.path).display())).await.map(|m| m.len()).unwrap_or(0);
        missing += f.size.saturating_sub(on_disk.max(partial));
    }
    let needed_mb = missing.div_ceil(1024 * 1024);
    // Keep 1 GB free for the system.
    if needed_mb + 1024 > free_mb && missing > 0 {
        return Err(DownloadError::NoSpace { needed_mb, free_mb });
    }
    let mut results = Vec::new();
    for f in &variant.files {
        let outcome = fetch_file(client, &manifest.base_url, f, data_dir, on_progress).await?;
        results.push((f.path.clone(), outcome));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// How the test server misbehaves.
    #[derive(Clone, Copy)]
    enum Mode {
        Good,
        /// Close the connection after this many body bytes on the first request.
        CutFirst(usize),
        IgnoreRange,
    }

    /// Minimal HTTP/1.1 server with Range support. Returns its base URL and a request counter.
    async fn serve(body: Vec<u8>, mode: Mode) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let n = counter.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let read = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..read]).to_lowercase();
                    let start = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.trim_end_matches('-').trim().parse::<usize>().ok())
                        .filter(|_| !matches!(mode, Mode::IgnoreRange));
                    let (status, slice) = match start {
                        Some(s) => ("206 Partial Content", &body[s..]),
                        None => ("200 OK", &body[..]),
                    };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len());
                    sock.write_all(head.as_bytes()).await.ok();
                    match mode {
                        Mode::CutFirst(k) if n == 0 => {
                            sock.write_all(&slice[..k]).await.ok();
                        }
                        _ => {
                            sock.write_all(slice).await.ok();
                        }
                    }
                    sock.shutdown().await.ok();
                });
            }
        });
        (format!("http://{addr}"), hits)
    }

    fn sample() -> (Vec<u8>, FileRef) {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let sha = hex(&Sha256::digest(&body));
        let file = FileRef { role: "model".into(), path: "pkg/v1/model.gguf".into(), size: body.len() as u64, sha256: sha };
        (body, file)
    }

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("rigbrew-dl-{}", uuid::Uuid::now_v7()))
    }

    #[tokio::test]
    async fn downloads_verifies_and_skips_when_present() {
        let (body, file) = sample();
        let (base, hits) = serve(body.clone(), Mode::Good).await;
        let dir = tmp();
        let client = reqwest::Client::new();
        let mut last = 0;
        let out = fetch_file(&client, &base, &file, &dir, &mut |p| last = p.done).await.unwrap();
        assert_eq!(out, Outcome::Downloaded);
        assert_eq!(last, file.size);
        assert_eq!(std::fs::read(dir.join(&file.path)).unwrap(), body);
        assert!(!dir.join("pkg/v1/model.gguf.part").exists());

        let out = fetch_file(&client, &base, &file, &dir, &mut |_| {}).await.unwrap();
        assert_eq!(out, Outcome::Present);
        assert_eq!(hits.load(Ordering::SeqCst), 1, "a verified file is not requested again");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn resumes_after_the_connection_drops() {
        let (body, file) = sample();
        let (base, _) = serve(body.clone(), Mode::CutFirst(100_000)).await;
        let dir = tmp();
        let client = reqwest::Client::new();
        let first = fetch_file(&client, &base, &file, &dir, &mut |_| {}).await;
        assert!(first.is_err(), "the cut connection is reported");
        let part = dir.join("pkg/v1/model.gguf.part");
        let kept = std::fs::metadata(&part).unwrap().len();
        assert!(kept > 0 && kept < file.size, "partial bytes are kept: {kept}");

        let out = fetch_file(&client, &base, &file, &dir, &mut |_| {}).await.unwrap();
        assert_eq!(out, Outcome::Resumed);
        assert_eq!(std::fs::read(dir.join(&file.path)).unwrap(), body);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn restarts_when_the_server_ignores_range() {
        let (body, file) = sample();
        let (base, _) = serve(body.clone(), Mode::IgnoreRange).await;
        let dir = tmp();
        std::fs::create_dir_all(dir.join("pkg/v1")).unwrap();
        std::fs::write(dir.join("pkg/v1/model.gguf.part"), &body[..50_000]).unwrap();
        let out = fetch_file(&reqwest::Client::new(), &base, &file, &dir, &mut |_| {}).await.unwrap();
        assert_eq!(out, Outcome::Resumed);
        assert_eq!(std::fs::read(dir.join(&file.path)).unwrap(), body);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn rejects_bytes_that_do_not_match_the_manifest() {
        let (mut body, file) = sample();
        body[1234] ^= 0xff;
        let (base, _) = serve(body, Mode::Good).await;
        let dir = tmp();
        let err = fetch_file(&reqwest::Client::new(), &base, &file, &dir, &mut |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Corrupt { .. }), "{err}");
        assert!(!dir.join(&file.path).exists());
        assert!(!dir.join("pkg/v1/model.gguf.part").exists(), "a corrupt partial file is removed");
        std::fs::remove_dir_all(dir).ok();
    }
}
