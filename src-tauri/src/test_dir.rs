//! Scratch folders for tests that are removed when the test ends, also when it fails.
//!
//! Tests used to make `temp_dir()/nakos-…` folders and delete them on their last line, if at all:
//! a failing test, or Windows refusing to delete a database that a pool still had open, left them
//! behind (thousands had piled up on the Mac and the Windows VM). Make the `TestDir` first in a
//! test, so it is dropped last, after the pools and connections that use it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long `drop` keeps trying: on Windows a pool's background threads can hold a database file
/// for a moment after the pool itself is dropped, and the folder cannot be removed until they let go.
const REMOVE_FOR: Duration = Duration::from_secs(2);

pub(crate) struct TestDir(PathBuf);

impl TestDir {
    /// A new, empty folder `temp_dir()/nakos-test-<tag>-<pid>-<uuid>`.
    pub(crate) fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nakos-test-{tag}-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    /// Best effort: cleanup never fails a test.
    fn drop(&mut self) {
        let deadline = Instant::now() + REMOVE_FOR;
        while std::fs::remove_dir_all(&self.0).is_err() && self.0.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TestDir;

    #[test]
    fn the_folder_is_there_while_in_use_and_gone_afterwards() {
        let dir = TestDir::new("self");
        let path = dir.path().to_path_buf();
        assert!(path.is_dir());
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("nakos-test-self-"));
        std::fs::create_dir_all(dir.join("a").join("b")).unwrap();
        std::fs::write(dir.join("a").join("b").join("f"), b"x").unwrap();
        drop(dir);
        assert!(!path.exists(), "removed with everything in it");
    }

    #[test]
    fn a_failing_test_still_cleans_up() {
        let made = std::sync::Mutex::new(None);
        let result = std::panic::catch_unwind(|| {
            let dir = TestDir::new("panics");
            *made.lock().unwrap() = Some(dir.path().to_path_buf());
            panic!("an assertion fails");
        });
        assert!(result.is_err());
        let path = made.lock().unwrap().clone().unwrap();
        assert!(!path.exists(), "removed while the panic unwound");
    }

    /// SQLite opens its files so that Windows will not delete them while a connection is open; the
    /// folder goes once the connection closes. (Rust's own `File` would not show this: it lets
    /// Windows delete a file that is still open.)
    #[test]
    fn a_database_held_open_for_a_moment_does_not_keep_the_folder() {
        let dir = TestDir::new("held");
        let path = dir.path().to_path_buf();
        let conn = rusqlite::Connection::open(dir.join("nakos.db")).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE t (x); INSERT INTO t VALUES (1);").unwrap();
        let holder = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(conn);
        });
        drop(dir);
        holder.join().unwrap();
        assert!(!path.exists());
    }

    /// A folder that cannot be removed (here a read-only subfolder) is retried for a while, then
    /// left: cleanup must neither hang a test run nor fail a test.
    #[cfg(unix)]
    #[test]
    fn it_keeps_trying_for_a_while_then_gives_up() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TestDir::new("stuck");
        let path = dir.path().to_path_buf();
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("f"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let start = std::time::Instant::now();
        drop(dir);
        let took = start.elapsed();
        if !path.exists() {
            // Running as root (a build container): permissions don't stop root, so nothing was stuck.
            eprintln!("skipped: the read-only folder was removed at once (running as root?)");
            return;
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(&path).unwrap();
        assert!(took >= std::time::Duration::from_millis(1800), "it retried: {took:?}");
        assert!(took < std::time::Duration::from_secs(4), "then it gave up: {took:?}");
    }

    #[test]
    fn a_folder_that_is_already_gone_is_fine() {
        let dir = TestDir::new("gone");
        std::fs::remove_dir_all(dir.path()).unwrap();
        drop(dir);
    }
}
