# Packaging (document only)

Nothing here is run by the repository's tooling. So far only the unsigned path has been used: 0.1.2 was built ad hoc signed, passed `SMOKE.md` step 8 (backup and restore into an empty vault) on 2026-10-14, and is published at https://nakos.org. 0.2.0 (2026-10-15) adds an Android APK, signed by the owner with their own key (see Android below). Developer ID signing and notarization have not been done. A Windows installer is built and tested on the local branch `windows` but not published or signed (see Windows below). For any future release, do not package before step 8 has passed on that build.

**Do not notarize or sign from the machine that holds your live vault** until you have restored an encrypted backup into a separate, empty vault and confirmed the wallets and sealed fields come back.

## macOS

1. Enrol in the Apple Developer Program and create a **Developer ID Application** certificate.
2. Build a release bundle on a clean build machine (`npm run tauri build`). Release builds use the OS keychain only if the user turns the helper on in Settings; development builds never touch it.
3. Sign with the hardened runtime. No special entitlements are needed for the default, keychain-off behaviour. Add none unless a concrete failure shows one is required.
4. Notarize with `xcrun notarytool submit <file> --wait`, using a stored credentials profile.
5. Staple the ticket with `xcrun stapler staple <app-or-dmg>`.
6. Verify with `spctl -a -vv <app>` and `codesign --verify --deep --strict <app>`.

Without a Developer ID, `tauri.conf.json` signs the app ad hoc (`"signingIdentity": "-"`), and Gatekeeper warns the first time a downloaded copy is opened. That is expected:

- **macOS 15 and later:** the Control-click, Open bypass no longer works. macOS says "Apple could not verify "Nakos" is free of malware…" with a single Done button; the user clicks Done, then System Settings, Privacy & Security, Open Anyway, and confirms with their password or Touch ID. On a standard (non-admin) account, macOS asks for an administrator's name and password instead.
- **macOS 11 to 14:** the user Control-clicks the app in Applications, chooses Open, then Open again.

Tested on 2026-10-13 with a DMG downloaded from an email on macOS 26 (Apple Silicon): the macOS 15+ route above, including the administrator prompt on a standard account, and the app then opened to "Create your vault" on an account with no vault. The macOS 11 to 14 route has not been tried on those versions.

**Distribute the DMG only.** A `.zip` of the `.app` sent by email arrived as an archive without Unix permissions, so the app lost its executable flag and macOS refused to start it ("The application "Nakos" can't be opened"). A disk image keeps the permissions. If a zip is ever needed, make it on a Mac with `ditto -c -k --keepParent Nakos.app Nakos.zip`.

**Serve the DMG as a download.** Hostinger served it as `text/plain` until an `.htaccess` set `AddType application/x-apple-diskimage .dmg` and `Content-Disposition: attachment`; the page's links also carry the HTML `download` attribute.

## Android

Direct download only (no Play Store). Tested route, 2026-10-14 and 15, on this Mac:

1. Toolchain (on the external disk): JDK 17 (`brew install openjdk@17`), Android SDK with platform 36, build-tools 36.1.0, NDK 29.0.14206865, platform-tools; set `JAVA_HOME`, `ANDROID_HOME`, `NDK_HOME` and the PATH (a small env script you `source` is handy). The Android project is `src-tauri/gen/android` (tracked in git); `src-tauri/tauri.android.conf.json` sets `org.nakos.app` and takes the version from `package.json` (versionCode from the semver: 0.2.0 is 2000).
2. Build the unsigned release APK: `npm run tauri -- android build --apk --target aarch64` (arm64 only). Output: `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`.
3. Align it into the release folder: `zipalign -P 16 -f 4 <unsigned> <release>/nakos-<version>-arm64-unsigned.apk`.
4. Before signing, check it on the emulator with a copy signed by the SDK's debug key (`~/.android/debug.keystore`, the public `android` password). The emulator image is `userdebug`, so DevTools attach to a release build there and `node scripts/android-layout-check.mjs --units --tabs --keyboard` runs; that copy is never distributed.
5. The owner signs, in their own terminal, so the password never appears anywhere else: `apksigner sign --ks <path to the release keystore> --ks-key-alias nakos --out <release>/Nakos_<version>_arm64.apk <release>/nakos-<version>-arm64-unsigned.apk` (needs JDK 17 on the PATH). The key is RSA 4096, `CN=Nakos`, outside the repository (`*.jks` is git-ignored). **Back it up offline with its password; without it no update can be installed over existing copies.**
6. Verify: `apksigner verify --verbose --print-certs` must show v2 and v3 verified and certificate SHA-256 `e9cf53a5c9b92f133a673d08716fe34078e5676d91f43bef3770aae91e66e3fc` (the same for every release); compare every entry with the tested unsigned APK (only `META-INF/MANIFEST.MF`, `NAKOS.SF` and `NAKOS.RSA` may be added); write `SHA256SUMS`. The `apksigner` warnings about `META-INF/*.version` and license files "not protected by signature" concern the old v1 scheme only and are expected. Do not publish the `.idsig` file `apksigner` writes next to the APK.

Install for testing: `adb -s <device serial> install -r <release>/Nakos_<version>_arm64.apk` (with the emulator also attached, always pass `-s`). A newer versionCode with the same key installs over the old copy and keeps its data.

**Publishing the APK.** Serve it as a download, like the DMG: `AddType application/vnd.android.package-archive .apk` and `Content-Disposition: attachment` in `.htaccess`, with `SHA256SUMS` and the certificate fingerprint on the page. Not done yet. People install it by allowing their browser or Files app to install unknown apps. Since 2026-09-30, Android blocks the normal install of apps from developers not registered with Google on certified devices in Brazil, Indonesia, Singapore and Thailand (globally from 2027); there users need adb or Android's "advanced flow" (a one-time 24-hour wait in Developer options). Registering as a developer (separate from the Play Store) is not done.

## Windows

Not published. Built on the local branch `windows`, x64 only, unsigned, as a per-user NSIS installer. Tested only in a Windows 11 on ARM virtual machine (VMware Fusion on this Mac), where the x64 build runs under Windows' emulation; not tested on x64 hardware or on Windows 10.

### Build on this Mac (cross-compile)

1. Toolchain, once: `rustup target add x86_64-pc-windows-msvc`, `brew install llvm lld makensis nasm`, `cargo install --locked cargo-xwin`. The first build downloads Microsoft's C runtime and Windows SDK through cargo-xwin, which accepts Microsoft's licence for them; keep its cache on the external disk.
2. Environment: put Homebrew's `llvm` and `lld` on the PATH (they are keg-only), point `XWIN_CACHE_DIR` at a disk with room, and set a UTF-8 locale (`LC_ALL=en_US.UTF-8`). Under the C locale makensis aborts with `std::bad_alloc` at "Generating language tables".
3. `npm run tauri build -- --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis`. Output: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Nakos_<version>_x64-setup.exe`. The bundler warns that cross-compiling is experimental and that signing was skipped; both are expected.

`src-tauri/tauri.windows.conf.json` holds the Windows-only settings (a test pins them): no tray icon, per-user install, publisher "Nakos", the round Nakos icon on the installer and uninstaller, and Nakos's own English installer messages (`src-tauri/windows/English.nsh`; the uninstaller's box reads "Also delete my Nakos vault (cannot be undone)"). The icon is rendered by `scripts/windows-icon.swift`; `icon.ico` is made from it with `tauri icon … -o <scratch>`, copying back only `icon.ico`.

### What the installer and the app do

- Installs for the current user, without administrator rights, to `%LOCALAPPDATA%\Nakos`, with a Start menu shortcut and an uninstall entry under the user's registry (HKCU).
- WebView2: Windows 11 includes it. If it is missing (some Windows 10 PCs), the installer downloads Microsoft's bootstrapper, so it needs internet then.
- The vault lives in `%LOCALAPPDATA%\com.nakos.app` (Local, not Roaming AppData, so a company network's roaming profile does not copy it); the webview's cache is in the same folder.
- The "Remember" keychain helper stores the passphrase in Credential Manager as a generic credential `master-key.com.nakos.app`. The keyring crate saves it with enterprise persistence: on a work PC with a roaming profile it can travel with the profile (Settings says so). Windows shows no prompt. Turning the helper off and Erase all data remove it.
- The window is fitted to the screen at start-up, so a smaller or scaled screen does not hide Settings behind the taskbar.

### Uninstall

Settings, Apps, Installed apps, Nakos, Uninstall. Windows first shows its own "This app and its related info will be uninstalled" prompt (it removes nothing by itself), then the Nakos uninstaller. Its box **"Also delete my Nakos vault (cannot be undone)" is unticked by default**: a plain uninstall removes the app, its shortcuts and its uninstall entry and keeps the vault. Ticked, it deletes `%APPDATA%\com.nakos.app` and `%LOCALAPPDATA%\com.nakos.app`. The uninstaller does not remove a remembered passphrase from Credential Manager; Erase all data before uninstalling does.

### SmartScreen (unsigned)

Tested on 2026-10-24 with the installer downloaded in Edge (Windows marked it as coming from the internet, `ZoneId=3`, as a download from the site would be):

1. Edge's download list: "Nakos_<version>_x64-setup.exe isn't commonly downloaded. Make sure you trust … before you open it." Keep it from the download's ⋯ menu.
2. Opening it: "Windows protected your PC. Microsoft Defender SmartScreen prevented an unrecognized app from starting." Click **More info**; it shows the file name and **Publisher: Unknown publisher**. Click **Run anyway**.

Other browsers (Chrome, Brave, Opera, Firefox) may show their own warning first; the person keeps the file. The SmartScreen screen comes from Windows and appears whichever browser downloaded the file. Not tested in those browsers. Offer the `.exe` itself, not a zip: some unzip tools drop the "from the internet" mark, and the person then never sees the warning that names the file as unsigned. Microsoft Defender raised no detection in the test VM across the installs.

### Signing (scripts ready, certificate not bought)

`scripts/sign-windows.ps1` signs and verifies the installer once a certificate exists; see `docs/SIGNING.md`. An Authenticode certificate would put a publisher name in place of "Unknown publisher" and let SmartScreen reputation build for that publisher across downloads; the warning does not necessarily disappear at once. Options: buy an OV or EV code-signing certificate (usually kept on a hardware token or a cloud signing service), or use Microsoft's Azure Trusted Signing (an Azure account and identity validation). Tauri signs on Windows with `bundle.windows.certificateThumbprint`, or anywhere with `bundle.windows.signCommand` (for example `osslsigncode` on this Mac). Nothing here has been set up; it needs the owner's account and go-ahead.

### Serving and checking

Serve it as a download like the DMG and APK: `AddType application/vnd.microsoft.portable-executable .exe`, and add `exe` to the `Content-Disposition: attachment` rule in `.htaccess`; put its line in `SHA256SUMS`. Not done yet.

To test, a Windows 11 virtual machine with throwaway data and the public BIP-32 test-vector xpub is enough. `cargo test` passes there; Credential Manager accepts credentials only from a logged-in Windows session, so its test is ignored in a plain run and run in that session with `--ignored`. After reinstalling over an older build with a different icon, Windows may keep showing the old icon on the taskbar from its icon cache; a fresh install shows the current one.

## Linux

New in 0.5.0. Three bundles from one build, all x86_64:

- `Nakos_<version>_x86_64.AppImage`: portable, runs on most distributions with glibc 2.35+ (Ubuntu 22.04, Debian 12, Fedora 36 and newer). It bundles WebKitGTK, so no other packages are needed. `chmod +x` and run it.
- `Nakos_<version>_amd64.deb`: Debian, Ubuntu, Mint, Pop!_OS. `sudo apt install ./Nakos_<version>_amd64.deb` pulls in WebKitGTK 4.1.
- `Nakos-<version>-1.x86_64.rpm`: Fedora, openSUSE. `sudo dnf install ./Nakos-<version>-1.x86_64.rpm`.

Build with `scripts/build-linux.sh` on Ubuntu 22.04, or in `scripts/linux.Dockerfile`. On this Mac that is colima with Rosetta and `--platform linux/amd64`: give the VM 5 GB (`colima start --memory 5`; with 2 GB rustc is killed), point `src-tauri/target/release/bundle` at a folder inside the container with a symlink (linuxdeploy fails with "Permission denied" on the shared folder; don't use a Docker volume for the target, it lives on the Mac's internal disk and filled it), and clear the AppImage magic bytes in the cached `linuxdeploy-plugin-appimage` once (Rosetta refuses it with "Exec format error"). The Dockerfile's header has the exact commands. A full build takes about 25 minutes there, so a real x86_64 host or VM is faster. 0.5.0 was built this way on 2026-11-05. The vault lives in `~/.local/share/com.nakos.app`. The keychain helper uses the Secret Service over D-Bus (GNOME Keyring, KWallet). With no Secret Service running, turning it on fails with a clear error and nothing is stored.

None of the Linux bundles are signed. Publish their lines in `SHA256SUMS`. Signing the .deb/.rpm with a GPG key and publishing an apt/dnf repository is possible later and isn't planned for 0.5.0.

Partly tested: `cargo test --release` passes in the build container, and the AppImage starts on Ubuntu 24.04 under Xvfb (in a container, with the desktop libraries it expects from the host installed). It has not been run on a real Linux desktop. Before publishing, run the AppImage on Ubuntu 22.04 and Fedora, create a vault, add the BIP-32 test-vector xpub, lock and unlock, and export and restore an encrypted backup.

## Before any release

Bump the version in `src-tauri/Cargo.toml` (and `package.json`; a test fails if they differ), update `CHANGELOG.md`, run `cargo test`, `tsc -b`, `cargo audit` and `npm audit`, and tag in git yourself.
