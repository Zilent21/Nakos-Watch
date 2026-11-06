// Words that differ between desktop operating systems. `os` is Rust's `std::env::consts::OS`
// ("macos", "windows", "linux", ...), reported by `vault_status`. Kept free of React so the
// node tests can import it.

/** The command palette shortcut as the keyboard labels it. The palette opens on either key. */
export function paletteShortcut(os: string): string {
  return os === "macos" || os === "ios" ? "⌘K" : "Ctrl+K";
}

/** An example path for Bitcoin Core's cookie file, shown in the empty field. Nakos does not expand
 * `~` or `%VARIABLES%`, so the Browse button is the easy way to fill it. */
export function coreCookieExample(os: string): string {
  if (os === "windows") return "C:\\Users\\you\\AppData\\Local\\Bitcoin\\.cookie";
  if (os === "macos") return "~/Library/Application Support/Bitcoin/.cookie";
  return "~/.bitcoin/.cookie";
}

/** Where Bitcoin Core keeps its data folder (and the cookie file in it) on this OS. */
export function coreDataFolderHint(os: string): string {
  if (os === "windows") return "C:\\Users\\you\\AppData\\Local\\Bitcoin (older installs: AppData\\Roaming\\Bitcoin)";
  if (os === "macos") return "~/Library/Application Support/Bitcoin";
  return "~/.bitcoin";
}

/** What the OS does with a remembered passphrase, said next to the "Remember" choice. */
export function keychainNote(os: string): string {
  if (os === "windows") {
    return "Windows keeps it in Credential Manager for your Windows account and does not ask for a password. On a work PC with a roaming profile it can travel with your profile.";
  }
  if (os === "macos") return "macOS may ask for your login password.";
  return "Your desktop's keyring may ask you to unlock it.";
}
