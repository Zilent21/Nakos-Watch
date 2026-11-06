//! Writes that have to survive a power cut.
//!
//! `std::fs::write` and `copy` return once the bytes are in the OS cache, and a `rename` is only
//! kept once the folder that holds it is synced. On 2026-10-14 a hard power-off of the Android
//! emulator shortly after a restore left the old, empty database in place. The restore swap and
//! the files the person saves go through these helpers (a test checks the call sites).

use std::io::Write;
use std::path::Path;

/// Flush a file's bytes to the disk. Only a regular file is synced: some Android document
/// providers hand out a pipe or socket for the chosen file, which has nothing on this device to
/// flush and refuses the call.
pub(crate) fn sync_file(f: &std::fs::File) -> std::io::Result<()> {
    if f.metadata()?.file_type().is_file() {
        f.sync_all()
    } else {
        Ok(())
    }
}

/// `std::fs::write`, synced to the disk before it returns.
pub(crate) fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    sync_file(&f).map_err(|e| e.to_string())
}

/// `std::fs::copy`, with the copy synced to the disk before it returns.
pub(crate) fn copy_synced(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map_err(|e| e.to_string())?;
    // Opened for writing (without truncating): Windows only flushes a handle that can write.
    let f = std::fs::OpenOptions::new().write(true).open(to).map_err(|e| e.to_string())?;
    sync_file(&f).map_err(|e| e.to_string())
}

/// Keep the files created, renamed or removed in `dir` across a power cut. Unix only (Android
/// included); Windows has no way to sync a folder from std, and NTFS journals renames itself.
pub(crate) fn sync_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::File::open(dir).and_then(|d| d.sync_all()).map_err(|e| format!("could not save the changes in {}: {e}", dir.display()))?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

#[cfg(test)]
mod durable_tests {
    use super::*;

    /// A power cut can't be staged in a test, so the host checks each helper really syncs.
    #[test]
    fn every_helper_syncs() {
        let src = include_str!("durable.rs");
        let body = |sig: &str| -> &'static str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig}"));
            &src[start..start + src[start..].find("\n}\n").unwrap()]
        };
        assert!(body("pub(crate) fn sync_file(").contains("f.sync_all()"));
        assert!(body("pub(crate) fn write_synced(").contains("sync_file(&f)"));
        assert!(body("pub(crate) fn copy_synced(").contains("sync_file(&f)"));
        let dir = body("pub(crate) fn sync_dir(");
        assert!(dir.contains("#[cfg(unix)]\n    std::fs::File::open(dir).and_then(|d| d.sync_all())"), "{dir}");
    }

    #[test]
    fn the_helpers_write_copy_and_sync_what_they_are_given() {
        let dir = crate::test_dir::TestDir::new("durable");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        write_synced(&a, b"a longer first version").unwrap();
        write_synced(&a, b"short").unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"short", "a second write replaces the file");
        let b = dir.join("b");
        std::fs::write(&b, b"an older, longer copy").unwrap();
        copy_synced(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"short");
        sync_dir(&dir).unwrap();
        assert!(sync_dir(&dir.join("missing")).is_err() || cfg!(not(unix)));
        assert!(write_synced(&dir.join("missing").join("x"), b"x").is_err());
        // Writable, as every caller's is: Windows refuses to flush a read-only handle.
        sync_file(&std::fs::OpenOptions::new().write(true).open(&a).unwrap()).unwrap();
    }

    /// What a pipe-backed document provider hands out: nothing to sync, and not an error.
    #[cfg(unix)]
    #[test]
    fn a_pipe_is_not_synced() {
        let (_r, w) = std::io::pipe().unwrap();
        let f = std::fs::File::from(std::os::fd::OwnedFd::from(w));
        assert!(f.sync_all().is_err(), "premise: the OS refuses to sync a pipe");
        sync_file(&f).unwrap();
    }
}
