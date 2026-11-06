//! "About Nakos": opens one of Nakos's own pages in the system browser.
//!
//! The webview can only pick which page (an enum), never a URL, so a compromised webview cannot use
//! this to launch an arbitrary URL or program. The links go to nakos.org paths that redirect (see
//! the site's .htaccess). The repository address can then move without an app update.

pub(crate) fn link_for(which: &str) -> Option<&'static str> {
    Some(match which {
        "source" => "https://nakos.org/source",
        "support" => "https://nakos.org/#support",
        "help_import" => "https://nakos.org/help/import",
        "security" => "https://nakos.org/security",
        _ => return None,
    })
}

#[tauri::command]
pub fn open_project_link(which: String) -> Result<(), String> {
    let url = link_for(&which).ok_or("unknown link")?;
    open_in_browser(url)
}

#[cfg(target_os = "macos")]
fn open_in_browser(url: &str) -> Result<(), String> {
    std::process::Command::new("/usr/bin/open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn open_in_browser(url: &str) -> Result<(), String> {
    // The URL is one of the constants above (https, no spaces or shell characters).
    std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "ios"))))]
fn open_in_browser(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn open_in_browser(url: &str) -> Result<(), String> {
    Err(format!("open {url} in your browser"))
}

#[cfg(test)]
mod tests {
    use super::link_for;

    #[test]
    fn only_known_pages_open_and_all_are_https_on_nakos_org() {
        for w in ["source", "support", "help_import", "security"] {
            let url = link_for(w).unwrap();
            assert!(url.starts_with("https://nakos.org/"), "{url}");
            assert!(!url.contains(|c: char| c.is_whitespace() || "&|;\"'`$<>".contains(c)), "{url}");
        }
        for w in ["", "https://evil.example", "file:///etc/passwd", "source "] {
            assert_eq!(link_for(w), None, "{w}");
        }
    }
}
