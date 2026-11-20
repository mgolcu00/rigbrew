//! Speech to text with the Sift family: sherpa-onnx's `sherpa-onnx-offline` reading an unpacked
//! Whisper bundle, one run per request. Like the voice, it runs on the processor and keeps
//! nothing in memory between requests.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

use crate::engine::EngineError;

/// The program name inside the sherpa engine build.
pub const PROGRAM: &str = "sherpa-onnx-offline";

/// Longest recording one request may carry, in bytes of 16-bit mono WAV at 16 kHz: ten minutes.
pub const MAX_WAV_BYTES: usize = 10 * 60 * 16_000 * 2 + 1024;

#[derive(Debug, Clone)]
pub struct ListenModel {
    pub binary: PathBuf,
    /// The unpacked bundle: `encoder.onnx`, `decoder.onnx`, `tokens.txt`.
    pub dir: PathBuf,
    pub threads: usize,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    /// The language the model heard, when it says.
    pub language: Option<String>,
    pub audio_seconds: f32,
    pub seconds: f32,
}

/// The first file in `dir` whose name contains `part` and ends with `ext`.
fn find(dir: &Path, part: &str, ext: &str) -> Option<PathBuf> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    names.sort();
    names
        .into_iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(part) && n.ends_with(ext) && !n.starts_with("._")))
}

/// Writes down what is said in `wav` (16-bit PCM WAV). `language` is a Whisper code such as
/// "en" or "tr"; `None` lets the model tell.
pub async fn transcribe(model: &ListenModel, wav: &Path, language: Option<&str>) -> Result<Transcript, EngineError> {
    if !model.binary.is_file() {
        return Err(EngineError::Missing(model.binary.clone()));
    }
    let dir = &model.dir;
    let (Some(encoder), Some(decoder), Some(tokens)) =
        (find(dir, "encoder", ".onnx"), find(dir, "decoder", ".onnx"), find(dir, "tokens", ".txt"))
    else {
        return Err(EngineError::Api { status: 500, body: format!("the listening model in {} is incomplete", dir.display()) });
    };
    let audio_seconds = std::fs::read(wav).ok().and_then(|b| crate::voice::wav_seconds(&b)).unwrap_or(0.0);
    let started = Instant::now();
    let mut cmd = Command::new(&model.binary);
    cmd.arg(format!("--whisper-encoder={}", encoder.display()))
        .arg(format!("--whisper-decoder={}", decoder.display()))
        .arg(format!("--tokens={}", tokens.display()))
        .arg("--whisper-task=transcribe")
        .arg(format!("--num-threads={}", model.threads.max(1)));
    if let Some(lang) = language.filter(|l| !l.is_empty() && l.len() <= 8 && l.bytes().all(|b| b.is_ascii_lowercase())) {
        cmd.arg(format!("--whisper-language={lang}"));
    }
    let run = cmd.arg("--").arg(wav).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).output();
    let output = tokio::time::timeout(Duration::from_secs(600), run)
        .await
        .map_err(|_| EngineError::Api { status: 504, body: "listening took longer than 10 minutes".into() })?
        .map_err(EngineError::Spawn)?;
    if !output.status.success() {
        let log = String::from_utf8_lossy(&output.stderr);
        let tail: String = log.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(EngineError::Api { status: 500, body: format!("the listening engine failed: {tail}") });
    }
    // One JSON object per input file on stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = stdout.lines().rev().find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok()).unwrap_or_default();
    let text = result["text"].as_str().unwrap_or("").trim().to_string();
    let language =
        result["lang"].as_str().map(|l| l.trim_matches(|c| c == '<' || c == '|' || c == '>').to_string()).filter(|l| !l.is_empty());
    Ok(Transcript { text, language, audio_seconds, seconds: started.elapsed().as_secs_f32() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_files_are_found_by_their_role() {
        let dir = std::env::temp_dir().join(format!("rigbrew-listen-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["turbo-encoder.int8.onnx", "turbo-decoder.int8.onnx", "turbo-tokens.txt", "._turbo-encoder.int8.onnx"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        assert_eq!(find(&dir, "encoder", ".onnx").unwrap().file_name().unwrap(), "turbo-encoder.int8.onnx");
        assert_eq!(find(&dir, "tokens", ".txt").unwrap().file_name().unwrap(), "turbo-tokens.txt");
        assert!(find(&dir, "joiner", ".onnx").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_missing_listening_engine_is_reported() {
        let m = ListenModel { binary: PathBuf::from("/nonexistent/sherpa-onnx-offline"), dir: PathBuf::from("."), threads: 1 };
        assert!(matches!(transcribe(&m, Path::new("x.wav"), None).await, Err(EngineError::Missing(_))));
    }
}
