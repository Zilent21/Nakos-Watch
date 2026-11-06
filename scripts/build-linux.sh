#!/usr/bin/env bash
# Builds the Linux bundles: AppImage (portable), .deb and .rpm, then prints their SHA-256 sums.
# Run on Ubuntu 22.04 (natively, or in scripts/linux.Dockerfile). Output: release-linux/
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

version=$(node -p "require('./package.json').version")
arch=$(uname -m)   # x86_64 (published) or aarch64

echo "Nakos $version for Linux ($arch)"
npm ci
(cd src-tauri && cargo test --release --quiet)
npx tauri build --bundles appimage,deb,rpm

out=release-linux
mkdir -p "$out"
bundle=${CARGO_TARGET_DIR:-src-tauri/target}/release/bundle
cp "$bundle"/appimage/*.AppImage "$out/Nakos_${version}_${arch}.AppImage"
cp "$bundle"/deb/*.deb           "$out/Nakos_${version}_$( [[ $arch == x86_64 ]] && echo amd64 || echo arm64 ).deb"
cp "$bundle"/rpm/*.rpm           "$out/Nakos-${version}-1.${arch}.rpm"
chmod +x "$out"/*.AppImage

(cd "$out" && sha256sum * | tee SHA256SUMS.linux)
echo
echo "Smoke test: $out/Nakos_${version}_${arch}.AppImage (needs a desktop session)."
echo "Keychain helper: needs a Secret Service (gnome-keyring or KWallet). Without one, Remember fails cleanly."
