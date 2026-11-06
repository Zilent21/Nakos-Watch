//! Several unit tests read the Rust sources (include_str!, read_to_string) and match "\n}\n". Git
//! for Windows checks text out with CRLF by default, which would fail them for no real reason.
//! `.gitattributes` pins LF, and this test says so directly if a checkout ignored it.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn text_files_are_checked_out_with_lf() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let attributes = std::fs::read_to_string(root.join("../.gitattributes")).expect(".gitattributes at the repository root");
    assert!(attributes.lines().any(|l| l.trim() == "* text=auto eol=lf"), "{attributes}");

    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    assert!(files.len() > 30, "the scan did not find the sources ({})", files.len());
    let crlf: Vec<_> = files.iter().filter(|p| std::fs::read_to_string(p).unwrap().contains("\r\n")).collect();
    assert!(crlf.is_empty(), "checked out with CRLF (re-clone with the .gitattributes in place): {crlf:?}");
}
