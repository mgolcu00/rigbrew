//! Bundles: a package file that is a whole folder packed as a plain tar (a voice model with its
//! lexicons, for example). It is downloaded and verified like any other file, then unpacked next
//! to where it was saved and the archive is deleted, so its space is used once.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::manifest::FileRef;

/// Written last into an unpacked bundle; a folder without it is incomplete.
const DONE: &str = ".unpacked";

pub fn is_bundle(f: &FileRef) -> bool {
    f.role == "bundle"
}

/// The folder a bundle unpacks into: its path without `.tar`.
pub fn dir(root: &Path, f: &FileRef) -> PathBuf {
    root.join(&f.path).with_extension("")
}

pub fn unpacked(root: &Path, f: &FileRef) -> bool {
    dir(root, f).join(DONE).is_file()
}

/// Whether a package file is on disk in the form it is used in.
pub fn present(root: &Path, f: &FileRef) -> bool {
    if is_bundle(f) {
        unpacked(root, f)
    } else {
        root.join(&f.path).is_file()
    }
}

/// Unpacks a downloaded, verified bundle and deletes the archive. Does nothing if it is unpacked.
pub fn unpack(root: &Path, f: &FileRef) -> Result<PathBuf, String> {
    let dest = dir(root, f);
    if unpacked(root, f) {
        return Ok(dest);
    }
    let archive = root.join(&f.path);
    let parent = dest.parent().ok_or("a bundle needs a folder")?;
    let tmp = parent.join(format!(".unpack-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let file = std::fs::File::open(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        unpack_tar(file, &tmp)?;
        std::fs::write(tmp.join(DONE), b"").map_err(|e| e.to_string())?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        std::fs::remove_dir_all(&tmp).ok();
    }
    result?;
    std::fs::remove_file(&archive).ok();
    Ok(dest)
}

/// Removes an unpacked bundle and its archive, if either is there. Returns whether anything was.
pub fn remove(root: &Path, f: &FileRef) -> bool {
    let a = std::fs::remove_dir_all(dir(root, f)).is_ok();
    let b = std::fs::remove_file(root.join(&f.path)).is_ok();
    a || b
}

/// Unpacks a tar stream into `dest`, refusing anything that is not a plain file or folder inside
/// it. macOS AppleDouble files (`._name`) are skipped.
pub(crate) fn unpack_tar<R: Read>(reader: R, dest: &Path) -> Result<(), String> {
    let mut tar = tar::Archive::new(reader);
    tar.set_preserve_permissions(false);
    tar.set_unpack_xattrs(false);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(format!("unexpected {kind:?} entry in the archive"));
        }
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path.is_absolute() || path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err(format!("unsafe path {} in the archive", path.display()));
        }
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("._")) {
            continue;
        }
        // unpack_in refuses to write outside `dest` as a second line of defence.
        if !entry.unpack_in(dest).map_err(|e| e.to_string())? {
            return Err(format!("refused to unpack {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap()
    }

    fn file(path: &str) -> FileRef {
        FileRef { role: "bundle".into(), path: path.into(), size: 0, sha256: String::new() }
    }

    #[test]
    fn a_bundle_unpacks_next_to_itself_and_the_archive_goes() {
        let root = std::env::temp_dir().join(format!("rigbrew-bundle-{}", uuid::Uuid::new_v4().simple()));
        let f = file("voice/v1/voice-v1.f32.tar");
        std::fs::create_dir_all(root.join("voice/v1")).unwrap();
        std::fs::write(root.join(&f.path), tar_with(&[("./model.onnx", b"m"), ("./dict/a.txt", b"a"), ("./._model.onnx", b"junk")]))
            .unwrap();
        assert!(!present(&root, &f));
        let d = unpack(&root, &f).unwrap();
        assert_eq!(d, root.join("voice/v1/voice-v1.f32"));
        assert_eq!(std::fs::read(d.join("model.onnx")).unwrap(), b"m");
        assert!(d.join("dict/a.txt").is_file());
        assert!(!d.join("._model.onnx").exists());
        assert!(!root.join(&f.path).exists());
        assert!(present(&root, &f));
        assert!(remove(&root, &f));
        assert!(!present(&root, &f));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_bundle_that_climbs_out_is_refused_and_leaves_nothing() {
        let root = std::env::temp_dir().join(format!("rigbrew-bundle-{}", uuid::Uuid::new_v4().simple()));
        let f = file("v/v.tar");
        std::fs::create_dir_all(root.join("v")).unwrap();
        // tar::Builder refuses "..", so write the name into the header by hand.
        let mut h = tar::Header::new_gnu();
        h.as_gnu_mut().unwrap().name[..9].copy_from_slice(b"../evil.t");
        h.set_size(1);
        h.set_mode(0o644);
        h.set_cksum();
        let mut b = tar::Builder::new(Vec::new());
        b.append(&h, &b"x"[..]).unwrap();
        std::fs::write(root.join(&f.path), b.into_inner().unwrap()).unwrap();
        assert!(unpack(&root, &f).unwrap_err().contains("unsafe path"));
        assert!(!present(&root, &f));
        assert!(!root.join("evil.t").exists());
        assert_eq!(std::fs::read_dir(root.join("v")).unwrap().count(), 1, "only the archive is left");
        std::fs::remove_dir_all(&root).ok();
    }
}
