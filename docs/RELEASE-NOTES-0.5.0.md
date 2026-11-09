# Nakos 0.5.0

**Free. Optional support. No account. Checked offline.**

## Nakos is now free and open source

Every feature is free for everyone, with as many wallets as you like. There's no license key and no
limit. The source code is public under the MIT license, so you can read it, build it yourself, and
check that it does what this page says.

If Nakos is useful to you, you can support development with a one-time payment over Lightning or
on-chain (21,000 sats suggested) at nakos.org/#support. It's optional and unlocks nothing.

## New

- **Linux.** A portable AppImage, plus .deb and .rpm packages. New and not yet tried on a Linux desktop:
  tell us how it goes.
- **Auto-lock.** Nakos locks itself after 10 minutes without use (5 on a phone). Change it under
  Settings → Vault, from 1 minute to 1 hour, or turn it off. On Android it also locks 30 seconds
  after you leave the app. The timer keeps running while Nakos is minimised.
- **"Remember on this computer" lasts only while Nakos is unlocked.** Any lock, by you or by the
  timer, removes the passphrase from your keychain. Tick Remember again the next time you unlock if
  you want it.
- **A clearer family letter**, in sections: what this is, what to do, where things are kept, what
  never to share, and what the letter does *not* contain. Choose a template: simple family,
  single-sig cold storage, or technical multisig.
- **Printable PDFs** of the family letter (with blanks to fill in by hand) and the recovery packet,
  which now gives every wallet its type, where its public keys are, the hardware model, and the next
  steps.
- **A guide for adding your first wallet**: where to find the descriptor or xpub in Sparrow,
  Electrum and hardware wallets, why a descriptor is better, and how to be sure you're not pasting
  something secret. Nakos now also refuses a pasted seed phrase or a single private key, not just an
  xprv.
- **About Nakos** in Settings, with links to the source code and to report a security issue.

## Still the same

Nakos is watch-only. It never asks for, stores or signs with a seed phrase or a private key, and
nothing in this release changes that. Your vault, your backups and your node settings carry over as
they are.

## Known limits

- **Not code-signed yet on macOS and Windows.** You'll still see the Gatekeeper or SmartScreen
  warning the first time. The signing scripts are ready and waiting on certificates. Check the
  SHA-256 on the download page before you open the file.
- **Linux is new.** Its tests pass and it starts on Ubuntu 24.04, but it hasn't been used on a Linux
  desktop yet. The AppImage needs
  glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later). "Remember on this computer"
  needs GNOME Keyring or KWallet.
- **Android** still has no Remember option, and wallets sync only while Nakos is open.
- Bulk labelling, coin search and tags, a more visual spend planner, saved plans, PSBT QR codes,
  background sync and CSV / BIP-329 exports are planned for the next releases.
