//! The registry manifest: which packages exist, their variants and what each one needs.
//!
//! The manifest is the only thing the app trusts about a download. Every file is listed with
//! its size and sha256, and the manifest itself is signed (verification lands with the
//! downloader).

use serde::{Deserialize, Serialize};

use crate::hardware::Backend;

pub const SCHEMA: u32 = 1;

/// Key that signs Rigbrew's manifests (minisign, Ed25519). The secret half never leaves the
/// maintainer's vault; `registry/rigbrew-manifest.pub` holds the same key.
pub const PUBLIC_KEY: &str = "RWQOqLpvWC18PG/EdPwZLTUFV9L2GFDo/SBwy4yn6USftw02LowlaXv7";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    /// ISO 8601 date the manifest was generated.
    pub generated: String,
    /// Base URL that file paths are relative to.
    pub base_url: String,
    pub packages: Vec<Package>,
    /// Engine programs per platform. Older manifests have none.
    #[serde(default)]
    pub engines: Vec<EngineBuild>,
}

/// A packaged engine program for one platform: a `.tar.gz` archive with the programs at its
/// top level and their licence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineBuild {
    pub engine: Engine,
    /// Upstream version or commit.
    pub version: String,
    /// `<os>-<arch>` as Rust names them, for example `macos-aarch64`, `windows-x86_64`.
    pub platform: String,
    /// GPU backends the build uses; empty means CPU only.
    #[serde(default)]
    pub backends: Vec<Backend>,
    /// Program names inside the archive (without `.exe`).
    pub binaries: Vec<String>,
    pub archive: FileRef,
    pub license: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Chat,
    Image,
    Video,
    Voice,
    Listen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// llama.cpp `llama-server`.
    Llama,
    /// stable-diffusion.cpp `sd-server`.
    Sdcpp,
    /// sherpa-onnx, in process.
    Sherpa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Public,
    Account,
    Paid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    /// Stable id, for example "steep-4b".
    pub id: String,
    /// Display name, for example "Steep 4B".
    pub name: String,
    pub modality: Modality,
    pub version: u32,
    pub engine: Engine,
    pub access: Access,
    pub source: Source,
    /// The selector prefers the lightest variant at or above this quality (0 to 1).
    pub quality_floor: f32,
    pub variants: Vec<Variant>,
}

/// Where the weights came from. Shown in the app and required by the source licence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub url: String,
    pub license: String,
    /// What we changed, in one sentence.
    pub changes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    /// Precision tag, for example "q4", "i8", "f16".
    pub id: String,
    pub files: Vec<FileRef>,
    /// Peak memory on the accelerator at default settings.
    pub accel_mem_mb: u64,
    /// Peak memory on the CPU; absent when the variant is not usable without an accelerator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_mem_mb: Option<u64>,
    /// Backends this variant runs on. CPU is implied by `cpu_mem_mb`.
    pub backends: Vec<Backend>,
    /// Measured closeness to the full-precision reference, 0 to 1 (1 = identical).
    pub quality: f32,
    /// Raw measurements behind `quality` and speed, kept for display.
    #[serde(default)]
    pub metrics: serde_json::Value,
}

impl Variant {
    pub fn disk_mb(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum::<u64>().div_ceil(1024 * 1024)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    /// What the file is: "model", "mmproj", "dit", "te", "vae", "voices", "data".
    pub role: String,
    /// Path relative to `Manifest::base_url`.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug)]
pub enum ManifestError {
    Json(serde_json::Error),
    UnsupportedSchema(u32),
    Invalid(String),
    /// The signature is missing, malformed, made with another key, or does not match the bytes.
    BadSignature(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Json(e) => write!(f, "manifest is not valid JSON: {e}"),
            ManifestError::UnsupportedSchema(v) => {
                write!(f, "manifest schema {v} is newer than this build supports ({SCHEMA}); update Rigbrew")
            }
            ManifestError::Invalid(why) => write!(f, "manifest is invalid: {why}"),
            ManifestError::BadSignature(why) => write!(f, "manifest signature check failed: {why}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Whether this build accepts a manifest without a signature: development builds do, and any
/// build does when `RIGBREW_ALLOW_UNSIGNED=1` is set by the person running it.
pub fn unsigned_allowed() -> bool {
    cfg!(debug_assertions) || std::env::var("RIGBREW_ALLOW_UNSIGNED").as_deref() == Ok("1")
}

/// Reads a manifest file and its `<file>.minisig`. A bad signature is always refused; a missing
/// one only when [`unsigned_allowed`] says so (and then `signed` in the result is false).
pub fn load_file(path: &std::path::Path) -> Result<(Manifest, bool), ManifestError> {
    let json = std::fs::read_to_string(path).map_err(|e| ManifestError::Invalid(format!("cannot read {}: {e}", path.display())))?;
    let sig_path = std::path::PathBuf::from(format!("{}.minisig", path.display()));
    match std::fs::read_to_string(&sig_path) {
        Ok(sig) => Ok((Manifest::parse_signed(&json, &sig)?, true)),
        Err(_) if unsigned_allowed() => Ok((Manifest::parse(&json)?, false)),
        Err(_) => Err(ManifestError::BadSignature(format!("{} is missing", sig_path.display()))),
    }
}

/// Checks a minisign signature of `bytes` against a base64 public key.
pub fn verify_signature(bytes: &[u8], signature: &str, public_key: &str) -> Result<(), ManifestError> {
    let bad = |e: minisign_verify::Error| ManifestError::BadSignature(e.to_string());
    let key = minisign_verify::PublicKey::from_base64(public_key).map_err(bad)?;
    let sig = minisign_verify::Signature::decode(signature).map_err(bad)?;
    key.verify(bytes, &sig, false).map_err(bad)
}

impl Manifest {
    /// Parses a manifest only if `signature` (a minisign `.minisig` file) is a valid signature of
    /// exactly these bytes by Rigbrew's key.
    pub fn parse_signed(json: &str, signature: &str) -> Result<Manifest, ManifestError> {
        verify_signature(json.as_bytes(), signature, PUBLIC_KEY)?;
        Manifest::parse(json)
    }

    pub fn parse(json: &str) -> Result<Manifest, ManifestError> {
        let m: Manifest = serde_json::from_str(json).map_err(ManifestError::Json)?;
        if m.schema > SCHEMA {
            return Err(ManifestError::UnsupportedSchema(m.schema));
        }
        m.validate()?;
        Ok(m)
    }

    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.id == id)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        let bad = |why: String| Err(ManifestError::Invalid(why));
        for p in &self.packages {
            if p.variants.is_empty() {
                return bad(format!("{} has no variants", p.id));
            }
            for v in &p.variants {
                if v.files.is_empty() {
                    return bad(format!("{}/{} has no files", p.id, v.id));
                }
                if !(0.0..=1.0).contains(&v.quality) {
                    return bad(format!("{}/{} quality {} is outside 0..1", p.id, v.id, v.quality));
                }
                for f in &v.files {
                    let hex = f.sha256.len() == 64 && f.sha256.bytes().all(|b| b.is_ascii_hexdigit());
                    if !hex {
                        return bad(format!("{}/{} file {} has no valid sha256", p.id, v.id, f.path));
                    }
                    // Paths are joined to the base URL and to the local data dir.
                    if f.path.starts_with('/') || f.path.split('/').any(|part| part == ".." || part.is_empty()) {
                        return bad(format!("{}/{} file path {:?} is not a clean relative path", p.id, v.id, f.path));
                    }
                }
            }
        }
        for e in &self.engines {
            check_file(&e.archive).map_err(|why| ManifestError::Invalid(format!("engine {:?} {}: {why}", e.engine, e.version)))?;
            let clean = |name: &String| !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if e.binaries.is_empty()
                || !e.binaries.iter().all(clean)
                || !clean(&e.platform.replace('-', "_"))
                || !clean(&e.version.replace('.', "_"))
            {
                return bad(format!("engine {:?} {} has an unsafe name", e.engine, e.version));
            }
        }
        Ok(())
    }
}

fn check_file(f: &FileRef) -> Result<(), String> {
    if !(f.sha256.len() == 64 && f.sha256.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(format!("file {} has no valid sha256", f.path));
    }
    if f.path.starts_with('/') || f.path.split('/').any(|part| part == ".." || part.is_empty()) {
        return Err(format!("file path {:?} is not a clean relative path", f.path));
    }
    Ok(())
}
