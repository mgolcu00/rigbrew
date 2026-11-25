//! Engine manager: runs inference engines as child processes and talks to them over loopback.
//!
//! An engine is started for one model, bound to 127.0.0.1 on a free port and protected with a
//! random key that only this process knows. It is killed when the handle is dropped, so a
//! crashed or closed app never leaves a model sitting in memory.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::json;
use tokio::process::{Child, Command};

#[derive(Debug)]
pub enum EngineError {
    /// The engine binary is not installed.
    Missing(PathBuf),
    Spawn(std::io::Error),
    /// The process exited before it became ready.
    Died {
        code: Option<i32>,
        log: PathBuf,
    },
    Timeout {
        seconds: u64,
        log: PathBuf,
    },
    Http(reqwest::Error),
    Api {
        status: u16,
        body: String,
    },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Missing(p) => write!(f, "engine not installed: {}", p.display()),
            EngineError::Spawn(e) => write!(f, "could not start the engine: {e}"),
            EngineError::Died { code, log } => write!(f, "the engine exited while loading (code {code:?}); see {}", log.display()),
            EngineError::Timeout { seconds, log } => write!(f, "the engine was not ready after {seconds} s; see {}", log.display()),
            EngineError::Http(e) => write!(f, "engine request failed: {e}"),
            EngineError::Api { status, body } => write!(f, "engine answered HTTP {status}: {body}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        EngineError::Http(e)
    }
}

/// What to load into `llama-server`.
#[derive(Debug, Clone)]
pub struct LlamaConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    /// Vision projector; enables image input.
    pub mmproj: Option<PathBuf>,
    pub context: u32,
    /// Layers to place on the GPU; 999 means all, 0 means CPU only.
    pub gpu_layers: u32,
    /// Where the engine's own output is written.
    pub log: PathBuf,
}

/// A running `llama-server`.
pub struct LlamaServer {
    child: Child,
    port: u16,
    key: String,
    client: reqwest::Client,
    pub load_seconds: f32,
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

impl LlamaServer {
    /// Starts the engine and waits until the model is loaded.
    pub async fn start(cfg: &LlamaConfig, ready_timeout: Duration) -> Result<LlamaServer, EngineError> {
        if !cfg.binary.is_file() {
            return Err(EngineError::Missing(cfg.binary.clone()));
        }
        let port = free_port().map_err(EngineError::Spawn)?;
        let key = uuid::Uuid::new_v4().simple().to_string();
        if let Some(dir) = cfg.log.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let log = std::fs::File::create(&cfg.log).map_err(EngineError::Spawn)?;
        let log_err = log.try_clone().map_err(EngineError::Spawn)?;

        let mut cmd = Command::new(&cfg.binary);
        cmd.arg("-m")
            .arg(&cfg.model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--api-key", &key])
            .args(["-c", &cfg.context.to_string()])
            .args(["-ngl", &cfg.gpu_layers.to_string()])
            .arg("--no-webui")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(true);
        if let Some(mmproj) = &cfg.mmproj {
            cmd.arg("--mmproj").arg(mmproj);
        }
        let mut child = cmd.spawn().map_err(EngineError::Spawn)?;

        let client = reqwest::Client::builder().no_proxy().build().expect("http client");
        let started = Instant::now();
        let url = format!("http://127.0.0.1:{port}/health");
        loop {
            if let Some(status) = child.try_wait().map_err(EngineError::Spawn)? {
                return Err(EngineError::Died { code: status.code(), log: cfg.log.clone() });
            }
            if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(2)).send().await {
                if resp.status().is_success() {
                    break;
                }
            }
            if started.elapsed() > ready_timeout {
                child.kill().await.ok();
                return Err(EngineError::Timeout { seconds: ready_timeout.as_secs(), log: cfg.log.clone() });
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Ok(LlamaServer { child, port, key, client, load_seconds: started.elapsed().as_secs_f32() })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Key the engine expects as a bearer token. Never leaves this machine.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Sends a chat request and calls `on_text` for every piece of the answer as it arrives.
    /// Returns generation statistics reported by the engine.
    pub async fn chat(
        &self,
        messages: &serde_json::Value,
        max_tokens: u32,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatStats, EngineError> {
        let body = json!({
            "messages": messages,
            "max_tokens": max_tokens,
            "stream": true,
            "timings_per_token": false,
            "chat_template_kwargs": {"enable_thinking": false},
        });
        let resp = self
            .client
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", self.port))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(EngineError::Api { status: resp.status().as_u16(), body: resp.text().await.unwrap_or_default() });
        }
        let mut stats = ChatStats::default();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buf.find('\n') {
                let line: String = buf.drain(..=end).collect();
                let Some(data) = line.trim().strip_prefix("data: ") else { continue };
                if data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                    if !text.is_empty() {
                        on_text(text);
                    }
                }
                if let Some(t) = v.get("timings") {
                    stats.prompt_tokens = t["prompt_n"].as_u64().unwrap_or(0);
                    stats.generated_tokens = t["predicted_n"].as_u64().unwrap_or(0);
                    stats.tokens_per_second = t["predicted_per_second"].as_f64().unwrap_or(0.0) as f32;
                }
            }
        }
        Ok(stats)
    }

    /// Stops the engine and frees its memory.
    pub async fn stop(mut self) {
        self.child.kill().await.ok();
    }
}

/// One video request (`sd-server`'s `vid_gen`).
#[derive(Debug, Clone)]
pub struct VideoRequest {
    pub prompt: String,
    pub negative: String,
    pub width: u32,
    pub height: u32,
    /// 4k+1 frames.
    pub frames: u32,
    pub fps: u32,
    pub steps: u32,
    /// Negative means random.
    pub seed: i64,
    pub cfg: f32,
    /// None leaves the schedule to the engine's own default for the model.
    pub flow_shift: Option<f32>,
    /// Decode in spatial tiles: less memory.
    pub tiling: bool,
    /// A picture the clip starts from (JPEG or PNG, base64), for packages that can continue from one.
    pub init_image: Option<String>,
}

/// Progress as the engine reports it: a stage (`conditioning`, `sampling`, `vae_decode`, ...)
/// and, where it counts, steps done of a total.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct VideoProgress {
    pub stage: String,
    pub pass: u32,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub elapsed_ms: u64,
}

#[derive(Debug)]
pub enum Job {
    Running(VideoProgress),
    Done { b64: String, mime: String, progress: VideoProgress },
    Failed(String),
    Cancelled,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ChatStats {
    pub prompt_tokens: u64,
    pub generated_tokens: u64,
    pub tokens_per_second: f32,
}

/// Where an engine binary lives: `$RIGBREW_ENGINE_DIR/<name>` if set, else `<data_dir>/engines/<name>`.
pub fn engine_path(data_dir: &Path, name: &str) -> PathBuf {
    let name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    match std::env::var_os("RIGBREW_ENGINE_DIR") {
        Some(dir) => PathBuf::from(dir).join(name),
        None => data_dir.join("engines").join(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_engine_is_reported_before_anything_starts() {
        let cfg = LlamaConfig {
            binary: PathBuf::from("/nonexistent/llama-server"),
            model: PathBuf::from("m.gguf"),
            mmproj: None,
            context: 4096,
            gpu_layers: 0,
            log: std::env::temp_dir().join("rigbrew-engine-test.log"),
        };
        assert!(matches!(LlamaServer::start(&cfg, Duration::from_secs(1)).await, Err(EngineError::Missing(_))));
    }

    // Uses /usr/bin/false as an engine that exits at once.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_engine_that_exits_while_loading_is_reported_with_its_log() {
        // `false` exits immediately with code 1, like an engine that cannot read its model.
        let cfg = LlamaConfig {
            binary: PathBuf::from("/usr/bin/false"),
            model: PathBuf::from("m.gguf"),
            mmproj: None,
            context: 4096,
            gpu_layers: 0,
            log: std::env::temp_dir().join(format!("rigbrew-engine-{}.log", uuid::Uuid::now_v7())),
        };
        match LlamaServer::start(&cfg, Duration::from_secs(5)).await {
            Err(EngineError::Died { code, .. }) => assert_eq!(code, Some(1)),
            other => panic!("expected Died, got {:?}", other.err().map(|e| e.to_string())),
        }
        std::fs::remove_file(&cfg.log).ok();
    }
}

/// What to load into `sd-server` (stable-diffusion.cpp).
#[derive(Debug, Clone)]
pub struct SdConfig {
    pub binary: PathBuf,
    /// Diffusion model (transformer).
    pub dit: PathBuf,
    /// Text encoder.
    pub text_encoder: PathBuf,
    /// How `sd-server` is told about the text encoder: `--llm` for Crema, `--t5xxl` for Pour.
    pub text_encoder_flag: &'static str,
    /// The full decoder. A video package decodes with `tae` instead.
    pub vae: Option<PathBuf>,
    /// A tiny decoder (TAEHV): seconds instead of many minutes for a video clip.
    pub tae: Option<PathBuf>,
    /// Joins the text encoder to the diffusion model, for packages that ship one.
    pub connector: Option<PathBuf>,
    /// Decodes the sound track, for video packages that make sound.
    pub audio_vae: Option<PathBuf>,
    /// Lets the text encoder see a picture, so a clip can start from one.
    pub vision: Option<PathBuf>,
    /// More startup flags (`--vae-tiling`, ...).
    pub extra_args: Vec<String>,
    pub log: PathBuf,
}

/// One image request.
#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    /// Negative means random.
    pub seed: i64,
}

/// A running `sd-server`.
///
/// `sd-server` has no access key of its own; it is bound to loopback on a random port that only
/// this process knows. Other local programs could find it by scanning ports.
pub struct SdServer {
    child: Child,
    port: u16,
    client: reqwest::Client,
    pub load_seconds: f32,
}

impl SdServer {
    pub async fn start(cfg: &SdConfig, ready_timeout: Duration) -> Result<SdServer, EngineError> {
        if !cfg.binary.is_file() {
            return Err(EngineError::Missing(cfg.binary.clone()));
        }
        let port = free_port().map_err(EngineError::Spawn)?;
        if let Some(dir) = cfg.log.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let log = std::fs::File::create(&cfg.log).map_err(EngineError::Spawn)?;
        let log_err = log.try_clone().map_err(EngineError::Spawn)?;
        let mut command = Command::new(&cfg.binary);
        command
            .args(["--listen-ip", "127.0.0.1", "--listen-port", &port.to_string()])
            .arg("--diffusion-model")
            .arg(&cfg.dit)
            .arg(cfg.text_encoder_flag)
            .arg(&cfg.text_encoder);
        if let Some(vae) = &cfg.vae {
            command.arg("--vae").arg(vae);
        }
        if let Some(tae) = &cfg.tae {
            command.arg("--tae").arg(tae);
        }
        if let Some(connector) = &cfg.connector {
            command.arg("--embeddings-connectors").arg(connector);
        }
        if let Some(audio_vae) = &cfg.audio_vae {
            command.arg("--audio-vae").arg(audio_vae);
        }
        if let Some(vision) = &cfg.vision {
            command.arg("--llm_vision").arg(vision);
        }
        let mut child = command
            .args(["--cfg-scale", "1.0"])
            .args(&cfg.extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(true)
            .spawn()
            .map_err(EngineError::Spawn)?;

        let client = reqwest::Client::builder().no_proxy().build().expect("http client");
        let started = Instant::now();
        let url = format!("http://127.0.0.1:{port}/v1/models");
        loop {
            if let Some(status) = child.try_wait().map_err(EngineError::Spawn)? {
                return Err(EngineError::Died { code: status.code(), log: cfg.log.clone() });
            }
            if let Ok(resp) = client.get(&url).timeout(Duration::from_secs(2)).send().await {
                if resp.status().is_success() {
                    break;
                }
            }
            if started.elapsed() > ready_timeout {
                child.kill().await.ok();
                return Err(EngineError::Timeout { seconds: ready_timeout.as_secs(), log: cfg.log.clone() });
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Ok(SdServer { child, port, client, load_seconds: started.elapsed().as_secs_f32() })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Generates one image and returns it as base64-encoded PNG, plus the seconds it took.
    pub async fn generate(&self, req: &ImageRequest) -> Result<(String, f32), EngineError> {
        let body = json!({
            "prompt": req.prompt,
            "width": req.width,
            "height": req.height,
            "steps": req.steps,
            "cfg_scale": 1.0,
            "seed": req.seed,
            "batch_size": 1,
        });
        let started = Instant::now();
        let resp = self.client.post(format!("http://127.0.0.1:{}/sdapi/v1/txt2img", self.port)).json(&body).send().await?;
        if !resp.status().is_success() {
            return Err(EngineError::Api { status: resp.status().as_u16(), body: resp.text().await.unwrap_or_default() });
        }
        let v: serde_json::Value = resp.json().await?;
        match v["images"][0].as_str() {
            Some(b64) => Ok((b64.to_string(), started.elapsed().as_secs_f32())),
            None => Err(EngineError::Api { status: 200, body: "the engine returned no image".into() }),
        }
    }

    /// Starts an image job in the engine's queue and returns its id. Poll it with [`SdServer::job`];
    /// unlike [`SdServer::generate`] it reports its steps and can be cancelled.
    pub async fn submit_image(&self, req: &ImageRequest) -> Result<String, EngineError> {
        let body = json!({
            "prompt": req.prompt,
            "width": req.width,
            "height": req.height,
            "seed": req.seed,
            "batch_count": 1,
            "sample_params": {"sample_steps": req.steps, "guidance": {"txt_cfg": 1.0}},
            "output_format": "png",
        });
        self.submit("img_gen", &body).await
    }

    async fn submit(&self, mode: &str, body: &serde_json::Value) -> Result<String, EngineError> {
        let resp = self.client.post(format!("http://127.0.0.1:{}/sdcpp/v1/{mode}", self.port)).json(body).send().await?;
        if !resp.status().is_success() {
            return Err(EngineError::Api { status: resp.status().as_u16(), body: resp.text().await.unwrap_or_default() });
        }
        let v: serde_json::Value = resp.json().await?;
        v["id"].as_str().map(str::to_string).ok_or(EngineError::Api { status: 202, body: "the engine returned no job id".into() })
    }

    /// Starts a video job in the engine's queue and returns its id. Poll it with [`SdServer::job`].
    pub async fn submit_video(&self, req: &VideoRequest) -> Result<String, EngineError> {
        let mut body = json!({
            "prompt": req.prompt,
            "negative_prompt": req.negative,
            "width": req.width,
            "height": req.height,
            "seed": req.seed,
            "video_frames": req.frames,
            "fps": req.fps,
            "sample_params": {
                "sample_method": "euler",
                "sample_steps": req.steps,
                "guidance": {"txt_cfg": req.cfg},
            },
            "vae_tiling_params": {"enabled": req.tiling},
            "output_format": "webm",
        });
        if let Some(shift) = req.flow_shift {
            body["sample_params"]["flow_shift"] = json!(shift);
        }
        if let Some(image) = &req.init_image {
            body["init_image"] = json!(image);
        }
        self.submit("vid_gen", &body).await
    }

    /// Where an image or video job is. A finished one carries its result, base64-encoded.
    pub async fn job(&self, id: &str) -> Result<Job, EngineError> {
        let resp = self.client.get(format!("http://127.0.0.1:{}/sdcpp/v1/jobs/{id}", self.port)).send().await?;
        if !resp.status().is_success() {
            return Err(EngineError::Api { status: resp.status().as_u16(), body: resp.text().await.unwrap_or_default() });
        }
        let v: serde_json::Value = resp.json().await?;
        let p = &v["progress"];
        let progress = VideoProgress {
            stage: p["stage"].as_str().unwrap_or("").to_string(),
            pass: p["pass"].as_u64().unwrap_or(0) as u32,
            completed: p["completed"].as_u64(),
            total: p["total"].as_u64(),
            elapsed_ms: p["elapsed_ms"].as_u64().unwrap_or(0),
        };
        Ok(match v["status"].as_str().unwrap_or("") {
            "completed" => {
                let r = &v["result"];
                // A video job has one media file; an image job a list of images.
                let (b64, mime) = match r["b64_json"].as_str() {
                    Some(b) => (b, r["mime_type"].as_str().unwrap_or("video/webm")),
                    None => (r["images"][0]["b64_json"].as_str().unwrap_or(""), "image/png"),
                };
                if b64.is_empty() {
                    return Err(EngineError::Api { status: 200, body: "the engine finished without a result".into() });
                }
                Job::Done { b64: b64.to_string(), mime: mime.to_string(), progress }
            }
            "failed" => Job::Failed(v["error"]["message"].as_str().or(v["error"].as_str()).unwrap_or("the engine failed").to_string()),
            "cancelled" => Job::Cancelled,
            _ => Job::Running(progress),
        })
    }

    /// Asks the engine to stop a job; it stops at the next step.
    pub async fn cancel_job(&self, id: &str) {
        self.client.post(format!("http://127.0.0.1:{}/sdcpp/v1/jobs/{id}/cancel", self.port)).send().await.ok();
    }

    pub async fn stop(mut self) {
        self.child.kill().await.ok();
    }
}
