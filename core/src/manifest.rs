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
    /// FFmpeg: joins, trims and encodes clips. No package runs on it; Studio uses it as a tool.
    Ffmpeg,
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
    /// More startup flags for the engine, for packages that need them (a sampler schedule, memory mapping).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engine_args: Vec<String>,
    /// How to ask this package for its work: steps, guidance, sizes, lengths. Shown to the app as it is.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub defaults: serde_json::Value,
    /// A notice the source licence wants shown wherever the package is used ("Powered by ...").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
    /// Licence terms the person has to agree to before the download starts (regions, permitted use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<Terms>,
}

/// One way a video package makes its clips, from a quick draft to the size the model was made
/// for. Listed in the package's `defaults.qualities`, fastest first; a manifest without them,
/// and an app from before them, see only `defaults.sizes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quality {
    /// "draft", "standard", "high".
    pub id: String,
    pub label: String,
    /// Width and height for each shape: "landscape", "portrait", "square".
    pub sizes: std::collections::BTreeMap<String, [u32; 2]>,
    /// Peak memory of the engine at this quality for a five-second clip, measured. A machine
    /// with less is not offered it.
    #[serde(default)]
    pub mem_mb: u64,
    /// The sampler schedule for this quality (ending at 0) when it differs from the package's own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sigmas: Vec<f32>,
}

impl Quality {
    /// "WxH" for a shape, if this quality has it.
    pub fn size(&self, shape: &str) -> Option<String> {
        self.sizes.get(shape).map(|[w, h]| format!("{w}x{h}"))
    }

    /// Whether `size` ("WxH") is one of this quality's sizes.
    pub fn has_size(&self, size: &str) -> bool {
        self.sizes.values().any(|[w, h]| format!("{w}x{h}") == size)
    }
}

/// The largest side a clip may have: the size the video model was made for, 1344 x 768.
pub const MAX_CLIP_SIDE: u32 = 1344;

fn usable_size([w, h]: [u32; 2]) -> bool {
    [w, h].iter().all(|v| (256..=MAX_CLIP_SIDE).contains(v) && v % 32 == 0)
}

impl Package {
    /// The qualities a video package offers, fastest first. One that lists none has one: its own sizes.
    /// Entries this build cannot use (no id, no usable size) are left out, so a newer manifest still reads.
    pub fn qualities(&self) -> Vec<Quality> {
        let listed: Vec<Quality> = self.defaults["qualities"]
            .as_array()
            .map(|list| list.iter().filter_map(|q| serde_json::from_value::<Quality>(q.clone()).ok()).collect())
            .unwrap_or_default();
        let mut out: Vec<Quality> = listed
            .into_iter()
            .filter(|q| !q.id.is_empty())
            .map(|mut q| {
                q.sizes.retain(|_, s| usable_size(*s));
                q.sigmas.retain(|s| s.is_finite() && *s >= 0.0);
                if q.sigmas.len() < 2 {
                    q.sigmas.clear();
                }
                q
            })
            .filter(|q| !q.sizes.is_empty())
            .collect();
        let mut seen = std::collections::HashSet::new();
        out.retain(|q| seen.insert(q.id.clone()));
        if out.is_empty() {
            let sizes: std::collections::BTreeMap<String, [u32; 2]> = self.defaults["sizes"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(shape, v)| Some((shape.clone(), [v[0].as_u64()? as u32, v[1].as_u64()? as u32])))
                        .filter(|(_, s)| usable_size(*s))
                        .collect()
                })
                .unwrap_or_default();
            if !sizes.is_empty() {
                let mem_mb = self.variants.iter().map(|v| v.accel_mem_mb).min().unwrap_or(0);
                out.push(Quality { id: "draft".into(), label: "Draft".into(), sizes, mem_mb, sigmas: Vec::new() });
            }
        }
        out
    }

    /// The quality whose sizes include `size` ("WxH").
    pub fn quality_of(&self, size: &str) -> Option<Quality> {
        self.qualities().into_iter().find(|q| q.has_size(size))
    }

    /// The languages a voice package reads, by their English names (`defaults.languages`). A voice
    /// from before the list reads English and Chinese, as the first one did.
    pub fn languages(&self) -> Vec<String> {
        let listed: Vec<String> = self.defaults["languages"]
            .as_array()
            .map(|l| l.iter().filter_map(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        if !listed.is_empty() || self.modality != Modality::Voice {
            return listed;
        }
        vec!["English".into(), "Chinese".into()]
    }

    /// Whether this voice package reads `language` (an English name, any case).
    pub fn reads(&self, language: &str) -> bool {
        self.languages().iter().any(|l| l.eq_ignore_ascii_case(language.trim()))
    }
}

/// What a licence asks of the person, said before the download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Terms {
    /// One or two plain sentences: what is not allowed and where.
    pub summary: String,
    /// The licence text.
    pub url: String,
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
    /// What the file is: "model", "mmproj", "dit", "te", "vae", "tae", "connector", "audio_vae", "voices", "data".
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

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "ee251661dffdebfa4a8c2555d2967b642f0efe9f2e350830a6e5c691935e23c3";

    fn manifest(id: &str, modality: &str, defaults: &str) -> String {
        format!(
            r#"{{"schema":1,"generated":"2026-11-26","base_url":"https://models.rigbrew.dev/","packages":[{{"id":"{id}","name":"Pour 14B",
               "modality":"{modality}","version":1,"engine":"sdcpp","access":"account","quality_floor":0.9,
               "source":{{"name":"s","url":"https://x.y","license":"l","changes":"c"}},"defaults":{defaults},
               "variants":[{{"id":"q4","accel_mem_mb":19070,"backends":["metal"],"quality":0.9,
                             "files":[{{"role":"dit","path":"{id}/v1/dit.gguf","size":1,"sha256":"{SHA}"}}]}}]}}]}}"#
        )
    }

    #[test]
    fn a_package_from_before_qualities_has_one_made_of_its_sizes() {
        // The defaults as the 2026-11-26 manifest has them.
        let old = r#"{"steps":6,"fixed_steps":true,"cfg":1.0,"fps":24,"frame_step":8,"frames":121,"max_frames":241,"audio":true,
                      "sizes":{"landscape":[640,384],"portrait":[384,640],"square":[512,512]},
                      "lengths":[{"frames":49,"label":"2 s"},{"frames":121,"label":"5 s"}]}"#;
        let m = Manifest::parse(&manifest("pour-14b", "video", old)).unwrap();
        let q = m.packages[0].qualities();
        assert_eq!(q.len(), 1);
        assert_eq!((q[0].id.as_str(), q[0].label.as_str(), q[0].mem_mb), ("draft", "Draft", 19070));
        assert_eq!(q[0].size("portrait").as_deref(), Some("384x640"));
        assert!(q[0].has_size("512x512") && !q[0].has_size("512x896"));
        assert!(q[0].sigmas.is_empty());
        assert_eq!(m.packages[0].quality_of("640x384").map(|q| q.id), Some("draft".into()));
    }

    #[test]
    fn qualities_are_read_from_the_defaults_and_what_cannot_be_used_is_left_out() {
        let new = r#"{"steps":6,"sizes":{"landscape":[640,384]},"qualities":[
            {"id":"draft","label":"Draft","sizes":{"landscape":[640,384],"portrait":[384,640]},"mem_mb":19000},
            {"id":"standard","label":"Standard","sizes":{"landscape":[896,512],"portrait":[512,896],"odd":[500,500]},"mem_mb":21500,
             "a_field_from_later":true},
            {"id":"high","label":"High","sizes":{"landscape":[1344,768]},"mem_mb":27000,"sigmas":[1.0,0.99,0.5,0.0]},
            {"id":"standard","label":"Again","sizes":{"landscape":[896,512]}},
            {"id":"","label":"Nameless","sizes":{"landscape":[640,384]}},
            {"id":"huge","label":"Huge","sizes":{"landscape":[2048,1152]}},
            {"label":"Broken"}]}"#;
        let m = Manifest::parse(&manifest("pour-14b", "video", new)).unwrap();
        let q = m.packages[0].qualities();
        let ids: Vec<&str> = q.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["draft", "standard", "high"]);
        assert_eq!(q[1].sizes.len(), 2, "a size that is not a multiple of 32 is left out");
        assert_eq!(q[1].label, "Standard");
        assert_eq!(q[2].size("landscape").as_deref(), Some("1344x768"));
        assert_eq!(q[2].sigmas, [1.0, 0.99, 0.5, 0.0]);
        assert_eq!(m.packages[0].quality_of("512x896").map(|q| q.id), Some("standard".into()));
        assert_eq!(m.packages[0].quality_of("700x400"), None);
        // What is read is written back as it was, so the app is shown the manifest as it is.
        let again: Manifest = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(again.packages[0].qualities(), q);
    }

    #[test]
    fn a_voice_says_which_languages_it_reads() {
        let old = Manifest::parse(&manifest("whistle-82m", "voice", r#"{"speakers":53}"#)).unwrap();
        assert_eq!(old.packages[0].languages(), ["English", "Chinese"]);
        assert!(old.packages[0].reads("english ") && !old.packages[0].reads("Turkish"));
        let new = Manifest::parse(&manifest("chime-99m", "voice", r#"{"languages":["English","Turkish",""," German "]}"#)).unwrap();
        assert_eq!(new.packages[0].languages(), ["English", "Turkish", "German"]);
        assert!(new.packages[0].reads("turkish") && !new.packages[0].reads("Chinese"));
        // Other kinds of package read nothing aloud.
        let video = Manifest::parse(&manifest("pour-14b", "video", "{}")).unwrap();
        assert!(video.packages[0].languages().is_empty());
    }
}
