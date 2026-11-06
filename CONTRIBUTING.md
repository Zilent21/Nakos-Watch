# Contributing to Nakos

Thanks for helping. Nakos is small and opinionated. Please read the three rules before you open a
pull request.

## The three rules

1. **Watch-only, always.** No code that accepts, stores, derives, displays or signs with a seed
   phrase or a private key. Imports go through `refuse_private_key_material`, and new export formats
   must too. A PR that adds signing will be closed, however useful it is.
2. **Nothing phones home.** No analytics, telemetry, crash reporting, update pings or remote fonts.
   Network calls go only to servers the person configured: their node, their Electrum server, or an
   explorer they turned on.
3. **Secrets stay sealed.** Anything sensitive (RPC passwords, tokens, notes, the letter) is
   encrypted with the vault key. Plaintext columns are listed in `docs/THREAT_MODEL.md`. Keep that
   list honest when you add one.

## Getting set up

See "Building from source" in `README.md`. Before you push:

```bash
cd src-tauri && cargo test          # backend, about 15 s
npx tsc -b && npm run test:ui       # frontend types + unit tests
scripts/secret-scan.sh              # nothing private in the tree or the history
```

## Style

- Small commits with a clear subject (`area: what changed`), for example
  `autolock: lock when a phone comes back after the grace period`.
- Tests for every behaviour change. Where a behaviour needs a running app, the code base uses
  source-scan tests (`include_str!`) that pin the important call order. Follow that pattern.
- UI copy is plain English, short sentences, no hype. Say what Nakos does and doesn't do.
- No new dependency without a reason in the PR description. Every crate and npm package is attack
  surface for an app that holds a vault.

## Reporting bugs

Open an issue with the version (Settings → About), the platform, and the steps. **Never paste a
seed, a vault passphrase, a private key, or a full descriptor of a wallet with funds** in an issue.
For security problems, follow `SECURITY.md` instead of opening an issue.

## License

By contributing you agree that your work is released under the MIT license in `LICENSE`.
