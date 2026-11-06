# Code signing

Where it stands in 0.5.0:

| Platform | Signed? | What's ready | What's missing |
|---|---|---|---|
| macOS | Ad hoc only (`signingIdentity: "-"`) | Hardened runtime on, `scripts/sign-macos.sh` (codesign, notarytool, stapler, spctl) | Apple Developer Program membership, Developer ID Application certificate, App Store Connect API key |
| Windows | No | `scripts/sign-windows.ps1` (signtool, RFC 3161 timestamp, verify) | A code-signing certificate (OV/EV or Azure Trusted Signing) |
| Android | Yes, by the maintainer's release key | Unchanged | — |
| Linux | No (checksums only) | — | Optional: GPG-signed .deb/.rpm repository later |

Until macOS and Windows are signed, the site and the app say so plainly, and the install steps show
how to get past Gatekeeper and SmartScreen. The SHA-256 checksums prove a download is intact, not who
made it.

## macOS: Developer ID + notarization

One-time setup:

1. Join the Apple Developer Program (individual is fine; the name on the certificate is what users see).
2. In Xcode → Settings → Accounts (or developer.apple.com → Certificates), create a **Developer ID
   Application** certificate. It goes into the login keychain. Back up the private key (`.p12`) offline.
3. App Store Connect → Users and Access → Integrations → **Team Keys**: create an API key with the
   Developer role. Download `AuthKey_XXXX.p8` **once** and keep it outside the repository
   (`~/.appstoreconnect/`). `.gitignore` already blocks `*.p8`, `*.p12` and `*.cer`.

Every release:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
export APPLE_API_KEY_ID=<key id>
export APPLE_API_ISSUER=<issuer uuid>
export APPLE_API_KEY_PATH=$HOME/.appstoreconnect/AuthKey_<key id>.p8
scripts/sign-macos.sh
```

The script builds if needed, signs the `.app` with the hardened runtime and a secure timestamp,
builds and signs the DMG, submits it to Apple's notary service and waits, staples the ticket, and
checks the result with `spctl`. The last line is the DMG's SHA-256 for `SHA256SUMS`.

No entitlements are needed. Add one only when a concrete failure shows it is required.

Then, in `tauri.conf.json`, change `signingIdentity` from `"-"` to the identity (or leave it and pass
`APPLE_SIGNING_IDENTITY` at build time). Update the site's install steps: a notarized DMG opens with
a single "downloaded from the internet" confirmation, with no trip to Privacy & Security.

Caveat for "Remember on this computer": the macOS keychain ties the saved item to the app's code
signature. The first signed release counts as a new app to the keychain, so people who turned
Remember on get one prompt (or have to turn it on again). Mention this in the release notes.

## Windows: Authenticode

Choose one:

- **Azure Trusted Signing** (cheapest, no hardware): an Azure subscription plus identity validation.
  Sign with the `Azure.CodeSigning.Dlib` signtool plugin, and set `NAKOS_SIGN_COMMAND` to its signtool
  command line with `{file}` where the path goes.
- **OV or EV certificate** from a CA: it ships on a hardware token or through the CA's cloud HSM. Put
  the certificate in the user store and set `NAKOS_CERT_THUMBPRINT`.

Every release, on the Windows build machine:

```powershell
npm run tauri build -- --bundles nsis
$env:NAKOS_CERT_THUMBPRINT = "<thumbprint>"     # or NAKOS_SIGN_COMMAND
powershell -ExecutionPolicy Bypass -File scripts\sign-windows.ps1
```

The script signs with SHA-256 and a DigiCert RFC 3161 timestamp (so the signature outlives the
certificate), verifies it with `signtool verify /pa`, and prints the SHA-256.

Tauri can sign during the build instead: `bundle.windows.certificateThumbprint` or
`bundle.windows.signCommand` in `tauri.windows.conf.json`. That also signs `nakos.exe` inside the
installer, which the script alone does not. Once a certificate exists, prefer the config route, and
keep the script for verifying.

SmartScreen reputation builds per certificate over downloads. An OV certificate may still show the
warning for the first weeks. EV reputation starts higher.

## What to update when signing lands

- `website/support-src/build.py`: the "not signed yet" strings (search for `SIGN_`).
- The Install section: drop the Gatekeeper/SmartScreen bypass steps for the signed platform.
- `docs/PACKAGING.md` and this file's table.
- The release notes: say which platforms are signed, and the macOS keychain caveat above.
