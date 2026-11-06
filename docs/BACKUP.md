# Encrypted vault backup

This is a backup of your whole vault, for you. It is not the family letter and not the advanced packet; those are watch-only exports meant for other people.

## What it is

One file (`.nakosbak`) made from Settings, "Export encrypted backup". It contains the whole database, sealed with AES-256-GCM under the vault key. There is no separate backup key: the key is derived from your vault passphrase. The KDF settings are written into the file header, so the file can be opened on a fresh install with only the passphrase.

## Format (version 2)

```
"NAKOSBK2"         8 bytes   magic
header_len         4 bytes   big-endian
header             JSON      { format, created_at, app_version, kdf { alg, version, salt_b64, mem_cost_kib, time_cost, parallelism } }
nonce             12 bytes   random
ciphertext + tag   rest      AES-256-GCM of the raw SQLite file
```

The magic, length and header are bound as associated data, so changing any of them makes the file fail to open. A restore caps the KDF cost it will accept from a file. RPC passwords and the explorer token are already sealed inside the database under the same key, so they are never in the clear, even with the outer layer removed. Wallet data is plaintext inside the sealed blob, as in the live database.

## Restore

Settings, "Restore from encrypted backup" (or "Restore from an encrypted backup" on the first-run screen). Enter the passphrase the backup was made with. Nakos opens the file, checks on a scratch copy that the vault inside matches the header and that the passphrase unlocks it, and only then swaps it in. The current database is kept as `nakos.db.pre-restore-bak`, and stale write-ahead files are removed. The backup's passphrase becomes the vault passphrase.

Refused: family letters and packets, files with a wrong passphrase or any edit, and any database containing an extended private key.

Older files (`NAKOSBK1`) carry no KDF settings and can only be restored into the vault they came from, while it is unlocked.

## Not covered

Keep the file and the passphrase in different places. Nakos does not back up your signing devices or seeds.
