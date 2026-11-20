//! Speech from text with the Whistle family: sherpa-onnx's `sherpa-onnx-offline-tts` reading an
//! unpacked Kokoro bundle, one run per request. The program loads the model in about a second
//! and runs on the processor, so nothing stays in memory between requests.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

use crate::engine::EngineError;

/// The program name inside the sherpa engine build.
pub const PROGRAM: &str = "sherpa-onnx-offline-tts";

/// Longest text one request may read.
pub const MAX_CHARS: usize = 5000;

#[derive(Debug, Clone)]
pub struct VoiceModel {
    pub binary: PathBuf,
    /// The unpacked bundle: `model.onnx`, `voices.bin`, `tokens.txt`, `lexicon-*.txt`.
    pub dir: PathBuf,
    pub threads: usize,
}

#[derive(Debug, Clone)]
pub struct Speech {
    pub text: String,
    /// Speaker number in `voices.bin`.
    pub speaker: u32,
    /// 1.0 is normal; 0.5 to 2.0.
    pub speed: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SpeechStats {
    pub audio_seconds: f32,
    pub seconds: f32,
}

/// Text as the voice should read it: no Markdown marks, links or code, one line.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || t.is_empty() {
            continue;
        }
        let t = strip_links(t.trim_start_matches(['#', '>', '-', '*', '+', ' ']));
        let mut word = String::new();
        for w in t.split_whitespace() {
            if w.starts_with("http://") || w.starts_with("https://") {
                continue;
            }
            word.push_str(w);
            word.push(' ');
        }
        let cleaned: String = word.chars().filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '|' | '<' | '>' | '[' | ']')).collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        out.push_str(cleaned);
        // A line without closing punctuation still ends a sentence when read aloud.
        if !cleaned.ends_with(['.', '!', '?', ':', ';', ',']) {
            out.push('.');
        }
        out.push(' ');
    }
    out.chars().filter(|c| !c.is_control()).take(MAX_CHARS).collect::<String>().trim().to_string()
}

/// `[text](address)` becomes `[text]`; the brackets are dropped later.
fn strip_links(line: &str) -> String {
    let mut s = line.to_string();
    while let Some(i) = s.find("](") {
        match s[i..].find(')') {
            Some(j) => s.replace_range(i + 1..i + j + 1, ""),
            None => break,
        }
    }
    s
}

/// Reads `speech` into a WAV file at `out`.
pub async fn speak(model: &VoiceModel, speech: &Speech, out: &Path) -> Result<SpeechStats, EngineError> {
    if !model.binary.is_file() {
        return Err(EngineError::Missing(model.binary.clone()));
    }
    let dir = &model.dir;
    let lexicons: Vec<String> = ["lexicon-us-en.txt", "lexicon-zh.txt"]
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string())
        .collect();
    let speed = speech.speed.clamp(0.5, 2.0);
    let started = Instant::now();
    let run = Command::new(&model.binary)
        .arg(format!("--kokoro-model={}", dir.join("model.onnx").display()))
        .arg(format!("--kokoro-voices={}", dir.join("voices.bin").display()))
        .arg(format!("--kokoro-tokens={}", dir.join("tokens.txt").display()))
        .arg(format!("--kokoro-lexicon={}", lexicons.join(",")))
        .arg(format!("--kokoro-length-scale={}", 1.0 / speed))
        .arg(format!("--num-threads={}", model.threads.max(1)))
        .arg(format!("--sid={}", speech.speaker))
        .arg(format!("--output-filename={}", out.display()))
        // Everything after "--" is the text, even if it starts with a dash.
        .arg("--")
        .arg(&speech.text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(600), run)
        .await
        .map_err(|_| EngineError::Api { status: 504, body: "reading aloud took longer than 10 minutes".into() })?
        .map_err(EngineError::Spawn)?;
    if !output.status.success() || !out.is_file() {
        let log = String::from_utf8_lossy(&output.stderr);
        let tail: String = log.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(EngineError::Api { status: 500, body: format!("the voice engine failed: {tail}") });
    }
    let audio_seconds = std::fs::read(out).ok().and_then(|b| wav_seconds(&b)).unwrap_or(0.0);
    Ok(SpeechStats { audio_seconds, seconds: started.elapsed().as_secs_f32() })
}

/// Length of a PCM WAV file in seconds, from its `fmt ` and `data` chunks.
pub fn wav_seconds(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut pos, mut byte_rate, mut data) = (12usize, 0u32, None);
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 12 <= bytes.len() {
            byte_rate = u32::from_le_bytes(bytes[body + 8..body + 12].try_into().ok()?);
        } else if id == b"data" {
            data = Some(size.min(bytes.len().saturating_sub(body)));
        }
        pos = body + size + (size & 1);
    }
    (byte_rate > 0).then(|| data.map(|d| d as f32 / byte_rate as f32)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_read_as_sentences() {
        let md = "## Why it floats\n\nA **paper boat** floats because of [buoyancy](https://x.y).\n\n```rust\nfn main() {}\n```\n- It is light\n- Air is trapped inside";
        assert_eq!(plain(md), "Why it floats. A paper boat floats because of buoyancy. It is light. Air is trapped inside.");
    }

    #[test]
    fn wav_length_comes_from_the_header() {
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&0u32.to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&1u16.to_le_bytes()); // mono
        w.extend_from_slice(&24_000u32.to_le_bytes());
        w.extend_from_slice(&48_000u32.to_le_bytes()); // bytes per second
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&96_000u32.to_le_bytes());
        w.extend(std::iter::repeat_n(0u8, 96_000));
        assert_eq!(wav_seconds(&w), Some(2.0));
        assert_eq!(wav_seconds(b"not a wav"), None);
    }

    #[tokio::test]
    async fn a_missing_voice_engine_is_reported() {
        let m = VoiceModel { binary: PathBuf::from("/nonexistent/sherpa"), dir: PathBuf::from("."), threads: 1 };
        let s = Speech { text: "hi".into(), speaker: 0, speed: 1.0 };
        assert!(matches!(speak(&m, &s, Path::new("/tmp/x.wav")).await, Err(EngineError::Missing(_))));
    }
}
