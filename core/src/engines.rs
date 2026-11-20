//! Installing engine programs from the manifest.
//!
//! An engine build is a `.tar.gz` listed in the signed manifest with its sha256. It is
//! downloaded with the same resumable, verified downloader as the models, unpacked into its own
//! folder under `<data dir>/engines/`, and the archive is deleted afterwards to save space.

use std::path::{Path, PathBuf};

use crate::download::{self, DownloadError, Progress};
use crate::hardware::Backend;
use crate::manifest::{Engine, EngineBuild, Manifest};

/// This machine's platform in the manifest's notation.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The build to use for `engine` here: one for this platform whose backends this machine has,
/// otherwise a CPU-only one.
pub fn pick<'a>(manifest: &'a Manifest, engine: Engine, backends: &[Backend]) -> Option<&'a EngineBuild> {
    let here = platform();
    let candidates = manifest.engines.iter().filter(|b| b.engine == engine && b.platform == here);
    candidates
        .clone()
        .find(|b| !b.backends.is_empty() && b.backends.iter().all(|x| backends.contains(x)))
        .or_else(|| candidates.clone().find(|b| b.backends.is_empty()))
}

pub fn install_dir(root: &Path, build: &EngineBuild) -> PathBuf {
    root.join("engines").join(format!("{:?}-{}-{}", build.engine, build.version, build.platform).to_lowercase())
}

/// Path of `binary` from `build` if it is installed.
pub fn installed(root: &Path, build: &EngineBuild, binary: &str) -> Option<PathBuf> {
    let path = install_dir(root, build).join(exe(binary));
    path.is_file().then_some(path)
}

#[derive(Debug)]
pub enum InstallError {
    Download(DownloadError),
    Unpack(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Download(e) => write!(f, "{e}"),
            InstallError::Unpack(why) => write!(f, "could not unpack the engine: {why}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Unpacks a `.tar.gz` into `dest`, refusing anything that is not a plain file or folder inside it.
fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    crate::bundle::unpack_tar(flate2::read::GzDecoder::new(file), dest)
}

/// Downloads, verifies and unpacks `build` unless it is already installed. Returns its folder.
pub async fn install(
    client: &reqwest::Client,
    manifest: &Manifest,
    build: &EngineBuild,
    root: &Path,
    on_progress: &mut (dyn FnMut(Progress<'_>) + Send),
) -> Result<PathBuf, InstallError> {
    let dir = install_dir(root, build);
    if build.binaries.iter().all(|b| dir.join(exe(b)).is_file()) {
        return Ok(dir);
    }
    let downloads = root.join("engines").join("_downloads");
    download::fetch_file(client, &manifest.base_url, &build.archive, &downloads, on_progress).await.map_err(InstallError::Download)?;
    let archive = downloads.join(&build.archive.path);
    let tmp = root.join("engines").join(format!(".unpack-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        unpack(&archive, &tmp)?;
        for b in &build.binaries {
            let path = tmp.join(exe(b));
            if !path.is_file() {
                return Err(format!("{} is not in the archive", exe(b)));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
        }
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &dir).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    std::fs::remove_file(&archive).ok();
    result.map_err(InstallError::Unpack)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileRef;

    fn build(engine: Engine, platform: &str, backends: Vec<Backend>) -> EngineBuild {
        EngineBuild {
            engine,
            version: "v1".into(),
            platform: platform.into(),
            backends,
            binaries: vec!["llama-server".into()],
            archive: FileRef { role: "archive".into(), path: "engines/x.tar.gz".into(), size: 1, sha256: "0".repeat(64) },
            license: "MIT".into(),
        }
    }

    fn manifest(engines: Vec<EngineBuild>) -> Manifest {
        Manifest { schema: 1, generated: "x".into(), base_url: "http://x/".into(), packages: vec![], engines }
    }

    #[test]
    fn picks_a_gpu_build_this_machine_can_use_then_falls_back_to_cpu() {
        let here = platform();
        let m = manifest(vec![
            build(Engine::Llama, "other-os", vec![Backend::Metal]),
            build(Engine::Llama, &here, vec![]),
            build(Engine::Llama, &here, vec![Backend::Cuda]),
        ]);
        let cuda = pick(&m, Engine::Llama, &[Backend::Cuda]).unwrap();
        assert_eq!(cuda.backends, vec![Backend::Cuda]);
        let cpu = pick(&m, Engine::Llama, &[Backend::Metal]).unwrap();
        assert!(cpu.backends.is_empty());
        assert!(pick(&m, Engine::Sdcpp, &[Backend::Cuda]).is_none());
    }

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpacks_plain_files_and_refuses_escapes_and_links() {
        let dir = std::env::temp_dir().join(format!("rigbrew-unpack-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.tar.gz");
        std::fs::write(&ok, tar_gz(&[("llama-server", b"#!/bin/sh\n"), ("LICENSE", b"MIT")])).unwrap();
        unpack(&ok, &dir.join("out")).unwrap();
        assert!(dir.join("out/llama-server").is_file());

        // An entry that tries to climb out of the folder (written raw: the builder refuses `..`).
        let mut header = tar::Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../escape.sh");
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        let mut raw = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        raw.append(&header, &b"x"[..]).unwrap();
        let bad = dir.join("bad.tar.gz");
        std::fs::write(&bad, raw.into_inner().unwrap().finish().unwrap()).unwrap();
        assert!(unpack(&bad, &dir.join("out2")).is_err());
        assert!(!dir.join("escape.sh").exists());

        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_cksum();
        let mut lb = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        lb.append_link(&mut link, "llama-server", "/bin/sh").unwrap();
        let linked = dir.join("link.tar.gz");
        std::fs::write(&linked, lb.into_inner().unwrap().finish().unwrap()).unwrap();
        assert!(unpack(&linked, &dir.join("out3")).is_err());

        std::fs::remove_dir_all(dir).ok();
    }
}
