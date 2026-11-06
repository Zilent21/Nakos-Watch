//! Native file dialogs, opened by Rust and never by the webview.
//!
//! A command that reads or writes a user file used to take the path as an argument, picked in a
//! dialog the webview opened. Nothing could check that, so a compromised webview (an npm
//! dependency, say) could name any path: overwrite a user-writable file with backup or letter
//! bytes, or read one in. Now the commands open the dialog themselves and use only what the person
//! chose in it; no command takes a file path from the frontend (a test scans for that).
//!
//! On Android the dialog returns a `content://` URI, not a path, so the choice is kept as a
//! `FilePath` and read or written through the system (tauri-plugin-fs), never turned into a path.

use std::io::{Read, Write};
use super::durable;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};

/// "Save as" dialog. `Ok(None)` when the person cancelled.
pub(crate) async fn pick_save_file(app: &AppHandle, title: &str, default_name: &str, filter_name: &str, extensions: &[&str]) -> Result<Option<FilePath>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(title).set_file_name(default_name).add_filter(filter_name, extensions).save_file(move |picked| {
        let _ = tx.send(picked);
    });
    rx.await.map_err(|_| "the file dialog was closed".to_string())
}

/// "Open" dialog for one file. `Ok(None)` when the person cancelled.
pub(crate) async fn pick_open_file(app: &AppHandle, title: &str, filter_name: &str, extensions: &[&str]) -> Result<Option<FilePath>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(title).add_filter(filter_name, extensions).pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    rx.await.map_err(|_| "the file dialog was closed".to_string())
}

/// How a chosen file is named in messages: the path, or for a URI the file name in its decoded
/// last segment (`content://…/document/primary%3ADownload%2Fx.nakosbak` reads `x.nakosbak`).
/// Some providers end the URI in a bare document id (`…/document/16`); that reads "the file you
/// chose" rather than a number.
pub(crate) fn shown(file: &FilePath) -> String {
    match file {
        FilePath::Path(p) => p.to_string_lossy().into_owned(),
        FilePath::Url(u) => match u.to_file_path() {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(()) => {
                let last = percent_decode(u.path_segments().and_then(|mut s| s.next_back()).unwrap_or(""));
                let name = last.rsplit('/').next().unwrap_or("");
                if name.contains('.') { name.to_string() } else { "the file you chose".to_string() }
            }
        },
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read the chosen file; anything larger than `max` bytes is refused with `too_large`.
pub(crate) fn read_picked(app: &AppHandle, file: &FilePath, max: u64, too_large: &str) -> Result<Vec<u8>, String> {
    let name = shown(file);
    let reader = open(app, file, false).map_err(|e| format!("could not open {name}: {e}"))?;
    read_capped(reader, max, &name, too_large)
}

/// Write `bytes` to the chosen file, replacing what was there, and sync it to the disk before
/// reporting it saved.
pub(crate) fn write_picked(app: &AppHandle, file: &FilePath, bytes: &[u8]) -> Result<(), String> {
    write_all(open(app, file, true), bytes, &shown(file))
}

fn write_all(file: std::io::Result<std::fs::File>, bytes: &[u8], name: &str) -> Result<(), String> {
    let mut f = file.map_err(|e| format!("could not write {name}: {e}"))?;
    f.write_all(bytes).map_err(|e| format!("could not write {name}: {e}"))?;
    // A power cut right after "saved" must not leave an empty or partial backup.
    durable::sync_file(&f).map_err(|e| format!("could not save {name} to disk: {e}"))
}

/// `read_picked` for a plain path, without an app handle (tests).
#[cfg(test)]
pub(crate) fn read_path(path: &Path, max: u64, too_large: &str) -> Result<Vec<u8>, String> {
    let name = path.to_string_lossy();
    let f = open_path(path, false).map_err(|e| format!("could not open {name}: {e}"))?;
    read_capped(f, max, &name, too_large)
}

/// `write_picked` for a plain path, without an app handle (tests).
#[cfg(test)]
pub(crate) fn write_path(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_all(open_path(path, true), bytes, &path.to_string_lossy())
}

/// Reads at most `max` bytes; one byte more means the file is too large. A content URI has no
/// size to check first, so the cap is enforced while reading.
fn read_capped(reader: impl Read, max: u64, name: &str, too_large: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    reader.take(max + 1).read_to_end(&mut buf).map_err(|e| format!("could not read {name}: {e}"))?;
    if buf.len() as u64 > max {
        return Err(too_large.to_string());
    }
    Ok(buf)
}

fn open_path(path: &Path, write: bool) -> std::io::Result<std::fs::File> {
    if write {
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)
    } else {
        std::fs::File::open(path)
    }
}

fn open(app: &AppHandle, file: &FilePath, write: bool) -> std::io::Result<std::fs::File> {
    match file {
        FilePath::Path(p) => open_path(p, write),
        FilePath::Url(u) => {
            #[cfg(target_os = "android")]
            {
                // "wt": some providers keep old bytes past the new end with plain "w".
                android_fd::open(app, u.as_str(), if write { "wt" } else { "r" })
            }
            #[cfg(target_os = "ios")]
            {
                use tauri_plugin_fs::{FsExt, OpenOptions};
                let mut opts = OpenOptions::new();
                if write {
                    // "wt": some providers keep old bytes past the new end with plain "w".
                    opts.write(true).truncate(true);
                } else {
                    opts.read(true);
                }
                app.fs().open(FilePath::Url(u.clone()), opts)
            }
            // Desktop dialogs return paths, never URIs.
            #[cfg(not(mobile))]
            {
                let _ = (app, u, write);
                Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "a file URI is only used on phones"))
            }
        }
    }
}

/// The `content://` file a dialog returns on Android, opened without tauri-plugin-fs's `open`.
/// That `open` (2.5.1) calls `unimplemented!()` when the provider hands back no file descriptor,
/// and a release build (`panic = "abort"`) then quits. This asks the same Kotlin `FsPlugin` for the
/// descriptor, registered again under its own name, and turns a missing one into an error.
#[cfg(target_os = "android")]
pub(crate) mod android_fd {
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{AppHandle, Manager, Wry};

    struct Fs(PluginHandle<Wry>);

    #[derive(serde::Serialize)]
    struct Payload<'a> {
        uri: &'a str,
        mode: &'a str,
    }

    #[derive(serde::Deserialize)]
    struct Descriptor {
        fd: Option<i32>,
    }

    pub(crate) fn init() -> TauriPlugin<Wry> {
        Builder::new("nakos-fd")
            .setup(|app, api| {
                app.manage(Fs(api.register_android_plugin("com.plugin.fs", "FsPlugin")?));
                Ok(())
            })
            .build()
    }

    pub(crate) fn open(app: &AppHandle, uri: &str, mode: &str) -> std::io::Result<std::fs::File> {
        let failed = |e: String| std::io::Error::new(std::io::ErrorKind::Other, format!("failed to open file: {e}"));
        let got: Descriptor = app.state::<Fs>().0.run_mobile_plugin("getFileDescriptor", Payload { uri, mode }).map_err(|e| failed(e.to_string()))?;
        let fd = got.fd.ok_or_else(|| failed("the app that holds this file did not hand it over".into()))?;
        // SAFETY: FsPlugin detaches the descriptor from its ParcelFileDescriptor, so this File is
        // its only owner.
        Ok(unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) })
    }
}

#[cfg(test)]
mod picked_file_tests {
    use super::*;

    /// The commands are thin glue around the helpers above and need a running app, and the fs
    /// plugin is registered only on phones, so the host checks the wiring in the source.
    #[test]
    fn the_commands_read_and_write_only_the_picked_file_with_their_caps() {
        let body = |src: &'static str, sig: &str| -> &'static str {
            let start = src.find(sig).unwrap_or_else(|| panic!("{sig}"));
            &src[start..start + src[start..].find("\n}\n").unwrap()]
        };
        let backup = include_str!("backup.rs");
        let packet = include_str!("inheritance_packet.rs");
        assert!(body(backup, "pub async fn export_encrypted_backup(").contains("file_dialog::write_picked(&app, &dest, bytes)"));
        assert!(body(backup, "pub async fn pick_backup_to_restore(").contains("state.grant_restore_file(file)"));
        assert!(body(backup, "pub fn restore_encrypted_backup(").contains("file_dialog::read_picked(&app, &chosen, MAX_RESTORE_BYTES, TOO_LARGE_FOR_A_BACKUP)"));
        assert!(body(packet, "pub async fn import_inheritance_packet(").contains("file_dialog::read_picked(&app, &file, MAX_PACKET_BYTES, TOO_LARGE_FOR_A_PACKET)"));
        assert!(body(packet, "pub async fn export_inheritance_packet(").contains("file_dialog::write_picked(&app, &file, bytes)"));
        let lib = include_str!("../lib.rs");
        assert!(lib.contains("#[cfg(mobile)]\n    let builder = builder.plugin(tauri_plugin_fs::init());"));
        assert!(lib.contains("#[cfg(target_os = \"android\")]\n    let builder = builder.plugin(commands::file_dialog::android_fd::init());"));
        let open_fn = body(include_str!("file_dialog.rs"), "fn open(app: &AppHandle");
        assert!(open_fn.contains("opts.write(true).truncate(true);") && open_fn.contains("opts.read(true);"));
        // Android goes around tauri-plugin-fs's `open`, whose missing-descriptor branch aborts.
        assert!(open_fn.contains(r#"android_fd::open(app, u.as_str(), if write { "wt" } else { "r" })"#));
        assert!(!open_fn.contains("#[cfg(mobile)]"));
    }

    #[test]
    fn a_content_uri_is_shown_by_its_decoded_last_segment() {
        let u = |s: &str| FilePath::Url(tauri::Url::parse(s).unwrap());
        assert_eq!(shown(&u("content://com.android.externalstorage.documents/document/primary%3ADownload%2Fnakos-vault-backup.nakosbak")), "nakos-vault-backup.nakosbak");
        assert_eq!(shown(&u("content://com.android.externalstorage.documents/document/primary%3Aletter.md")), "primary:letter.md");
        // What the emulator's Downloads picker returned on 2026-10-14.
        assert_eq!(shown(&u("content://com.android.providers.downloads.documents/document/16")), "the file you chose");
        assert_eq!(shown(&u("content://com.android.providers.downloads.documents/document/msf%3A1000")), "the file you chose");
        assert_eq!(shown(&u("content://x.provider/")), "the file you chose");
        // A file URL becomes a path only with a drive letter on Windows.
        #[cfg(not(windows))]
        assert_eq!(shown(&u("file:///tmp/a%20b.nakosbak")), "/tmp/a b.nakosbak");
        #[cfg(windows)]
        assert_eq!(shown(&u("file:///C:/tmp/a%20b.nakosbak")), r"C:\tmp\a b.nakosbak");
        assert_eq!(shown(&FilePath::Path("/tmp/x.md".into())), "/tmp/x.md");
        // A stray or short escape is kept as written.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }

    #[test]
    fn reads_are_capped_while_reading() {
        assert_eq!(read_capped(&b"abcd"[..], 4, "f", "big").unwrap(), b"abcd");
        assert_eq!(read_capped(&b"abcde"[..], 4, "f", "big").unwrap_err(), "big");
        assert_eq!(read_capped(&b""[..], 0, "f", "big").unwrap(), b"");
    }

    #[test]
    fn a_path_is_written_whole_and_read_back() {
        let dir = crate::test_dir::TestDir::new("picked");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        write_path(&p, b"a longer first version").unwrap();
        // A shorter second write replaces the file: the old tail must not survive.
        write_path(&p, b"short").unwrap();
        assert_eq!(read_path(&p, 1024, "big").unwrap(), b"short");
        assert_eq!(read_path(&p, 4, "big").unwrap_err(), "big");
        assert!(read_path(&dir.join("missing"), 10, "big").unwrap_err().starts_with("could not open"));
    }
}
