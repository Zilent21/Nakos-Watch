# Nakos threat model

Nakos is a local, watch-only desktop app. It holds descriptors, extended public keys and addresses. It never holds a seed phrase, a private key or an extended private key, so it cannot move funds. This page states what is protected and what is not, based on the code. No independent security review of Nakos has been done.

## Vault: algorithm and parameters

| Item | Value | Where |
|---|---|---|
| KDF | Argon2id, version 1.3 | `src-tauri/src/crypto/mod.rs:129` |
| Cost | 19 MiB memory, 2 passes, 1 lane (OWASP minimum-tier Argon2id) | `crypto/mod.rs:146-148` |
| Salt | 16 random bytes per vault | `crypto/mod.rs:138` |
| Key | 32-byte KDF output, used directly as the AES-256-GCM key | `crypto/mod.rs:121` |
| Cipher | AES-256-GCM, fresh random 96-bit nonce per sealed value | `crypto/mod.rs:59-69` |
| Passphrase check | a known string sealed under the key (the "canary") | `crypto/mod.rs:160`, checked at `:174` |
| Stored | salt, the three cost numbers and the canary, in `db_meta.kdf_params` (plaintext) | `commands/backup.rs:103` |
| In memory | the derived key while unlocked; zeroed on drop; never written to disk | `crypto/mod.rs:51` |

The parameters are stored per vault, so a vault always opens with the settings it was created with. They were judged adequate, not weak, so no migration was made. A future increase for new vaults would not affect existing ones.

## Two cases

**1. Laptop stolen, operating-system disk encryption on, Nakos locked.** Without the OS login the attacker cannot read the database file. If they defeat OS protection they hold the database: they can read every plaintext column below, and can try passphrases offline against the Argon2id parameters above. Sealed values stay unreadable without the passphrase. Watch-only: no funds can be moved.

**2. Laptop stolen with the disk unencrypted, or the session or vault unlocked.** The attacker can read every plaintext column by opening the file, and while the vault is unlocked the app itself will show the sealed values. If the optional keychain helper is on, the passphrase is available to an unlocked OS user session. The risk is privacy: extended public keys reveal every address and balance of a wallet, and node credentials or notes may help an attacker. Funds cannot be moved from Nakos.

## Data classes

| Class | What | Where in code |
|---|---|---|
| **Plaintext at rest** in `nakos.db` | wallet names, descriptors, change descriptors, xpubs/ypubs/zpubs, master fingerprint, derivation path, `wallets.notes`; multisig cosigner xpubs and labels; addresses; coins, amounts, labels, cost-basis lots; transactions; balance snapshots; contact names, relationship and contact method; node host, port, RPC username, cookie file path; preferences; the review date | `migrations/0001_init.sql:12-291`, `0003`, `0004`, `0005`, `0006` |
| **Sealed at rest** (vault key) | node RPC password | `0001_init.sql:242-243`, `commands/node.rs:138` |
| | inheritance letter and per-wallet notes | `0001_init.sql:196-197`, `commands/inheritance.rs:84` |
| | recovery contact notes | `0001_init.sql:188-189`, `commands/inheritance.rs:144` |
| | review reminder note | `0004_inheritance_review.sql:6-7`, `commands/inheritance_packet.rs:936` |
| | explorer API token (`db_meta`) | `commands/explorer_cmd.rs:25-26` |
| **Never stored** | seed phrases, private keys, extended private keys, vault passphrase (except in the OS keychain if the user turns the helper on), macaroons (no Lightning) | add-wallet accepts public material only; `crypto/backup.rs` refuses any database containing xprv-style keys |
| **Copies on the same disk** | automatic local copies `backups/nakos-*.db` are plain copies of the database file, protected exactly like the live file; `nakos.db.pre-restore-bak` after a restore | `commands/backup.rs:627` (copies), `:320` (pre-restore file) |
| **Leaves the device** | see below | |

Plain copies under backups/nakos-*.db, written by the sqlite file copy in src-tauri/src/commands/backup.rs, are not NAKOSBK2. Node RPC password, inheritance notes, contact notes, review notes, and the explorer token remain ciphertext. Descriptors, xpubs, addresses, balances, coins, labels, cost basis, and contact names are readable to anyone who copies the file.

## What leaves the device, and only when you do it

| Export | Contains | Never contains |
|---|---|---|
| Family letter (simple) | plain-English steps, wallet names, last-known balances, your note if you opted in; the importable data block (xpubs, fingerprints, descriptors) only under "ADVANCED" at the bottom; no nodes | RPC passwords, RPC usernames, explorer token, vault passphrase, seeds, cost basis |
| Advanced packet | full wallet details, one enabled node per kind (host, port, cookie path), checklist, notes if you opted in, cost basis only if you opted in (JSON) | RPC passwords, RPC usernames, explorer token, vault passphrase, seeds |
| Encrypted vault backup | the whole database, sealed with the vault passphrase (see `BACKUP.md`) | anything in the clear |

Network: Nakos talks to the Electrum server and Bitcoin Core node you configure. The public explorer is off by default; with it off no explorer request is made. No telemetry, accounts or cloud.

## Logging

Secrets (RPC password, explorer token, passphrase) are not written to logs, toasts or window titles. The only `println!` calls are in tests and print public test-vector descriptors. Explorer error text never includes the request URL (which can carry a token).

## Known limits

- Plaintext columns above are readable to anyone with the database file.
- The 19 MiB Argon2id setting is the minimum tier; a weak passphrase is the weak point.
- The optional keychain helper trades protection for convenience; it is off by default and never used in development builds.
- A running, unlocked Nakos can be read by other software running as the same user.
