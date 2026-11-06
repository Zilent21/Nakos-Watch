# Security policy

Nakos is a watch-only Bitcoin dashboard. It never asks for, stores, derives or signs with a seed
phrase or a private key. A bug that breaks that promise is the most serious kind we can have.

## Reporting a vulnerability

Email **Zilent@Tuta.io** with "Nakos security" in the subject. Tuta mail is end-to-end encrypted
between Tuta users. From another provider, ask in a first, short message for a way to send details
privately.

Please include:

- the Nakos version (Settings → About) and the platform (macOS, Windows, Linux, Android);
- what an attacker needs (local access, a malicious Electrum server, a crafted packet file, and so on);
- steps to reproduce, or a proof of concept.

**Never send a seed phrase, a vault passphrase or private keys**, not even "test" ones from a wallet
that holds funds. Nobody needs them to look into a report.

What to expect: an acknowledgement within 7 days, an assessment within 14, and a fixed release as
fast as the severity calls for. We will credit you in the release notes unless you'd rather we
didn't. Please give us a reasonable time to ship a fix before you publish.

## In scope

- Anything that makes Nakos store, show, log or send private key material or a seed.
- Weaknesses in the vault (Argon2id + AES-256-GCM), the encrypted backup, or how the passphrase or
  key is kept in memory or in the OS keychain.
- The auto-lock failing to lock, or leaving vault data readable after a lock.
- A crafted file (backup, inheritance packet or letter, CSV) that runs code, writes outside the
  folder chosen in the dialog, or corrupts the vault.
- A malicious Electrum server, Bitcoin Core node or block explorer that can do more than lie about
  balances: run code, read files, or reach the webview.
- A PSBT built by Nakos that pays somewhere the person did not choose.
- Supply-chain problems in this repository or in the release builds.

## Out of scope

- What a public Electrum server or explorer learns from the addresses you choose to send it (this is
  documented, and Nakos asks for consent first).
- Attacks that need an unlocked, already-compromised computer (malware with your user's rights can
  read any app's memory).
- The project's website, except where it serves the downloads or their checksums.

## Verifying a release

Every release publishes SHA-256 checksums next to the downloads at https://nakos.org/#verify. The
Android APK is always signed with the same certificate, and its fingerprint is printed there. Signed
macOS and Windows builds are on the way: see `docs/SIGNING.md` for where that stands.
