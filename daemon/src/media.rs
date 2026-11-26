//! Files the service made, played by the webview's own `<video>` from an address:
//!
//!   GET    /v1/media?token=               the file the token is for, with `Range` (206) so a long film streams
//!
//! A `<video src>` cannot send the bearer token, and a 120 MB film should not be read into memory
//! before it starts. So the app asks for a token for one file (`POST /v1/studio/projects/{id}/media`:
//! the exported film, or a shot's clip), and the address with it plays that file only. A token is
//! random, good for one file, and goes after 20 minutes without use (12 hours at most); the
//! address answers only when it is asked for by a loopback name, so a page that rebinds a name
//! of its own to 127.0.0.1 gets nothing. Only files the service resolved itself get a token.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::Query;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;

use crate::fail;

/// A token goes after this long without a request.
const IDLE: Duration = Duration::from_secs(20 * 60);
/// And after this long in any case.
const LONGEST: Duration = Duration::from_secs(12 * 3600);
/// Tokens kept at once; the oldest goes first.
const MOST: usize = 64;

struct Grant {
    token: String,
    path: PathBuf,
    kind: &'static str,
    made: Instant,
    used: Instant,
}

impl Grant {
    fn live(&self, now: Instant) -> bool {
        now.duration_since(self.used) < IDLE && now.duration_since(self.made) < LONGEST
    }
}

static GRANTS: Mutex<Vec<Grant>> = Mutex::new(Vec::new());

/// A token for one file the service made, and the address that plays it.
pub fn grant(path: PathBuf, kind: &'static str) -> String {
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let now = Instant::now();
    let mut all = GRANTS.lock().unwrap();
    all.retain(|g| g.live(now));
    if all.len() >= MOST {
        all.remove(0);
    }
    all.push(Grant { token: token.clone(), path, kind, made: now, used: now });
    format!("/v1/media?token={token}")
}

/// The file a token is for, while the token lives. Using it keeps it alive.
fn redeem(token: &str) -> Option<(PathBuf, &'static str)> {
    let now = Instant::now();
    let mut all = GRANTS.lock().unwrap();
    all.retain(|g| g.live(now));
    let g = all.iter_mut().find(|g| g.token == token)?;
    g.used = now;
    Some((g.path.clone(), g.kind))
}

/// Whether a `Host` header names this machine by a loopback name.
fn loopback(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(name, _)| name),
    };
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// The bytes a `Range` header asks for, of a file of `len` bytes: `Ok(None)` for the whole file
/// (no header, another unit, several ranges, or one that cannot be read), `Ok(Some((first, last)))`
/// for one part, `Err(())` for a part outside the file.
pub fn range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Ok(None) };
    let Some((from, to)) = spec.split_once('-').filter(|_| !spec.contains(',')) else { return Ok(None) };
    let (from, to) = (from.trim(), to.trim());
    let number = |s: &str| s.parse::<u64>().ok();
    if from.is_empty() {
        // The last `n` bytes.
        return match number(to) {
            Some(n) if n > 0 && len > 0 => Ok(Some((len - n.min(len), len - 1))),
            Some(_) => Err(()),
            None => Ok(None),
        };
    }
    let Some(first) = number(from) else { return Ok(None) };
    let last = match (to.is_empty(), number(to)) {
        (true, _) => None,
        (false, Some(last)) if last >= first => Some(last),
        (false, _) => return Ok(None),
    };
    if first >= len {
        return Err(());
    }
    Ok(Some((first, last.map_or(len - 1, |last| last.min(len - 1)))))
}

#[derive(Deserialize)]
pub struct MediaQuery {
    #[serde(default)]
    token: String,
}

pub async fn get(headers: HeaderMap, Query(q): Query<MediaQuery>) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    if !loopback(host) {
        return fail(StatusCode::FORBIDDEN, "this address answers on this machine only");
    }
    let Some((path, kind)) = redeem(&q.token) else {
        return fail(StatusCode::NOT_FOUND, "this address is not valid any more: open the film again");
    };
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return fail(StatusCode::NOT_FOUND, "the file is not there any more");
    };
    let Ok(len) = file.metadata().await.map(|m| m.len()) else {
        return fail(StatusCode::NOT_FOUND, "the file is not there any more");
    };
    let answer = Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    let wanted = range(headers.get(header::RANGE).and_then(|h| h.to_str().ok()), len);
    let (answer, first, count) = match wanted {
        Ok(None) => (answer.status(StatusCode::OK), 0, len),
        Ok(Some((first, last))) => (
            answer.status(StatusCode::PARTIAL_CONTENT).header(header::CONTENT_RANGE, format!("bytes {first}-{last}/{len}")),
            first,
            last - first + 1,
        ),
        Err(()) => {
            return answer
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .unwrap()
        }
    };
    if first > 0 {
        use tokio::io::AsyncSeekExt;
        if file.seek(std::io::SeekFrom::Start(first)).await.is_err() {
            return fail(StatusCode::INTERNAL_SERVER_ERROR, "the file could not be read");
        }
    }
    answer.header(header::CONTENT_LENGTH, count).body(Body::from_stream(chunks(file, count))).unwrap()
}

/// `count` bytes of a file from where it stands, a piece at a time.
fn chunks(file: tokio::fs::File, count: u64) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> {
    futures_util::stream::unfold((file, count), |(mut file, left)| async move {
        use tokio::io::AsyncReadExt;
        if left == 0 {
            return None;
        }
        let mut piece = vec![0u8; left.min(256 * 1024) as usize];
        match file.read(&mut piece).await {
            Ok(0) => None,
            Ok(n) => {
                piece.truncate(n);
                Some((Ok(Bytes::from(piece)), (file, left - n as u64)))
            }
            Err(e) => Some((Err(e), (file, 0))),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_read_as_the_bytes_it_asks_for() {
        assert_eq!(range(None, 100), Ok(None));
        assert_eq!(range(Some("bytes=0-1"), 100), Ok(Some((0, 1))));
        assert_eq!(range(Some("bytes=10-"), 100), Ok(Some((10, 99))));
        assert_eq!(range(Some("bytes=90-500"), 100), Ok(Some((90, 99))), "the end is held to the file");
        assert_eq!(range(Some("bytes=-30"), 100), Ok(Some((70, 99))));
        assert_eq!(range(Some("bytes=-300"), 100), Ok(Some((0, 99))));
        assert_eq!(range(Some("bytes=100-"), 100), Err(()), "past the end");
        assert_eq!(range(Some("bytes=-0"), 100), Err(()));
        assert_eq!(range(Some("bytes=0-"), 0), Err(()), "an empty file has no bytes to give");
        // Several ranges, another unit or nonsense: the whole file.
        assert_eq!(range(Some("bytes=0-1, 5-9"), 100), Ok(None));
        assert_eq!(range(Some("items=0-1"), 100), Ok(None));
        assert_eq!(range(Some("bytes=a-b"), 100), Ok(None));
        assert_eq!(range(Some("bytes=9-3"), 100), Ok(None));
    }

    #[test]
    fn only_a_loopback_name_is_answered() {
        for host in ["127.0.0.1:5555", "localhost:5555", "LOCALHOST", "[::1]:5555", "127.0.0.1"] {
            assert!(loopback(host), "{host}");
        }
        for host in ["", "evil.example:5555", "127.0.0.1.evil.example:5555", "10.0.0.2:5555", "[fe80::1]:5555"] {
            assert!(!loopback(host), "{host}");
        }
    }

    #[test]
    fn a_token_is_for_one_file_and_goes() {
        let url = grant(PathBuf::from("/f/Film.mp4"), "video/mp4");
        let token = url.strip_prefix("/v1/media?token=").unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(redeem(token), Some((PathBuf::from("/f/Film.mp4"), "video/mp4")));
        assert_eq!(redeem("0123"), None);
        // Unused for too long, or too old: gone.
        let all = GRANTS.lock().unwrap();
        let g = all.iter().find(|g| g.token == token).unwrap();
        assert!(g.live(Instant::now()));
        assert!(!g.live(Instant::now() + IDLE));
        assert!(!g.live(g.made + LONGEST));
    }
}
