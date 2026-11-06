# Nakos

**See every coin. Hold no keys.** Nakos is a free, open-source, watch-only Bitcoin dashboard for macOS,
Windows, Linux and Android. It reads public keys only (descriptors, xpubs, addresses), syncs from your own
Electrum server or a public one, and keeps its records in an encrypted vault on your device. No account, no
Nakos server, no telemetry, and no license key: every feature is free for everyone.

Downloads and checksums: <https://nakos.org>. License: [MIT](LICENSE). Security reports: [SECURITY.md](SECURITY.md).
Contributing: [CONTRIBUTING.md](CONTRIBUTING.md).

Built with [Tauri 2](https://tauri.app), a Rust backend, and a React 19 + TypeScript frontend.

## Security model

- **Watch-only, always.** Nakos never accepts, derives, stores, or asks for a private key or seed phrase. Every
  wallet is added via a public descriptor, an xpub/ypub/zpub, a single address, or a set of cosigner xpubs for
  multisig. It cannot move funds.
- **100% local.** All wallet data lives in a single SQLite database in your OS's application data directory
  (`nakos.db`). There is no account, no server, no cloud sync, and no telemetry of any kind.
- **Encrypted at rest.** Sensitive fields — RPC credentials, inheritance instructions, recovery-contact notes —
  are encrypted with AES-256-GCM using a key derived from your vault passphrase via Argon2id. The passphrase
  itself is never stored; only a KDF salt/params and a small "canary" ciphertext used to verify unlock attempts.
  You may optionally cache the passphrase in your OS keychain ("remember on this device").
- **Network usage is opt-in and narrow.** With no node configured, Nakos works entirely from previously-synced
  local data. If you connect a Bitcoin Core node (JSON-RPC) or an Electrum server, Nakos uses it to sync UTXOs,
  addresses, and transactions, and to read node/network status. If neither is connected, fee estimates can fall
  back to a public explorer (mempool.space by default), but only if you turn the explorer on in Settings; it is
  off by default and no request is made until then. With it on, Nakos also queries the explorer for one address
  when you enter that address in Settings. The explorer is never used to sync wallet balances or transaction
  history.
- **PSBTs, not signatures.** The UTXO Manager and Spending Planner generate unsigned PSBTs (with witness UTXO
  data populated) for consolidations and spends. You sign them with your own hardware wallet or signer — Nakos
  never has signing capability.

## Tech stack

- **Framework:** Tauri 2 (Rust backend, system webview frontend)
- **Frontend:** React 19, TypeScript, Tailwind CSS, Radix-based shadcn-style components, Zustand, TanStack Query
- **Database:** SQLite via `rusqlite` + `r2d2`, migrated with `rusqlite_migration`
- **Bitcoin:** `bitcoin`, `bdk_wallet` / `bdk_electrum` (descriptor parsing, UTXO/tx sync), `bitcoincore-rpc`,
  `electrum-client`
- **Crypto:** `aes-gcm`, `argon2`, `keyring` (OS keychain)

## Project layout

```
src/                     React frontend
  bindings/               TypeScript types + typed invoke() wrappers matching Rust commands
  components/             Shared UI (shadcn-style primitives, layout, command palette)
  modules/                One folder per product module (portfolio, security, utxo-manager, ...)
  stores/                 Zustand stores (vault/session, preferences)
src-tauri/                Rust backend
  src/db/                 SQLite pool, migrations, row <-> struct models
  src/crypto/             Argon2id key derivation + AES-256-GCM helpers
  src/bitcoin/            Descriptor normalization, BDK sync, PSBT construction
  src/rpc/                Bitcoin Core RPC, Electrum, public fee-estimate fallback
  src/scoring/            Security / Privacy / Health scoring engines (deterministic, no fake numbers)
  src/commands/           Tauri commands (the IPC surface consumed by src/bindings)
  migrations/             Numbered SQL migrations; 0001_init.sql is the base schema
```

## Building from source

Everything below builds the same app that is published on nakos.org. Release builds also need signing
for macOS and Windows; see [docs/SIGNING.md](docs/SIGNING.md). Packaging details per platform are in
[docs/PACKAGING.md](docs/PACKAGING.md).

### Prerequisites (all platforms)

- Node.js 20+ and npm
- Rust stable via [rustup](https://rustup.rs) (the minimum is in `src-tauri/Cargo.toml`, `rust-version`)

### macOS (Apple Silicon)

```bash
xcode-select --install
npm ci
npm run tauri build          # → src-tauri/target/release/bundle/{macos,dmg}/
```

### Windows 11 (x64)

Install the Visual Studio 2022 Build Tools ("Desktop development with C++"), and WebView2 if it's
missing (Windows 11 has it). Then, in PowerShell:

```powershell
npm ci
npm run tauri build -- --bundles nsis   # → src-tauri\target\release\bundle\nsis\
```

### Linux (x86_64)

Ubuntu 22.04 or newer, Debian 12, Fedora 38+ (WebKitGTK 4.1). On Debian/Ubuntu:

```bash
sudo apt install build-essential curl file pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev rpm patchelf
npm ci
npm run tauri build -- --bundles appimage,deb,rpm   # → src-tauri/target/release/bundle/
```

Fedora: `sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel dbus-devel rpm-build patchelf`
and the C toolchain (`sudo dnf group install c-development`).

For the published bundles, use `scripts/build-linux.sh`, natively on Ubuntu 22.04 or in
`scripts/linux.Dockerfile`. Building on the oldest supported glibc is what makes the AppImage run on
newer distributions. "Remember on this computer" uses the Secret Service (GNOME Keyring or KWallet).
Without one running, it is unavailable and Nakos asks for the passphrase each time.

### Android (arm64)

See the Android section of [docs/PACKAGING.md](docs/PACKAGING.md) (JDK 17, Android SDK 36, NDK, then
`npm run tauri android build -- --apk --target aarch64`). The release APK is signed by the maintainer;
a build you make yourself is signed with your own key and won't install over the published one.

### Develop

```bash
npm ci
npm run tauri dev                 # app with hot reload
cd src-tauri && cargo test        # backend tests
npx tsc -b && npm run test:ui     # frontend types + unit tests
```

### Check before you publish

```bash
scripts/secret-scan.sh            # nothing private in the tree or the git history
```

## First-run guide

1. **Create a vault passphrase.** On first launch, Nakos asks you to set a passphrase (min. 8 characters). This
   encrypts sensitive local fields only — it is unrelated to your Bitcoin keys, which Nakos never touches.
2. **Add a wallet.** From Portfolio, click *Add wallet* and paste a descriptor, xpub/ypub/zpub, or a single
   address — or configure a multisig vault with your cosigners' xpubs and a threshold. Set a custody type and
   risk level so Portfolio, Security Center, and the Health Score can classify it correctly.
3. **(Optional) Connect a node.** Under Node Operations, add a local Bitcoin Core RPC or Electrum connection to
   sync live UTXOs/transactions and see real node/mempool status. Without one, Nakos still works fully offline
   from whatever was last synced or manually entered.
4. **Fill in your real setup under Security Center.** Firmware status, passphrase usage, geographic backup
   redundancy, backup-restore testing, and exchange 2FA all feed the Security Score and BTC Health Score directly
   — the score simply reflects what you report.
5. **Set an exchange rate** under Settings if you want fiat values — Nakos never fetches balances from a price
   API, only an optional fee-estimate fallback.
6. **Set up Inheritance & Recovery.** Add recovery contacts, write an encrypted instruction package, and run
   (then mark) a real recovery drill — an untested plan is flagged everywhere in the app until you do.

## Data ownership

Your entire database is one file. Back it up manually or automatically (Settings → Backups), or export an
encrypted `.nakosbak` snapshot for cold storage. Deleting the app removes nothing from your Bitcoin — Nakos is a
window onto chain state you already control, not a wallet.
