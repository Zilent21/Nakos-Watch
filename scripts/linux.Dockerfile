# Build environment for the Linux bundles (AppImage, .deb, .rpm).
#
# Ubuntu 22.04 is the oldest base Tauri 2 supports (WebKitGTK 4.1). Building on the oldest
# supported glibc (2.35) is what makes the AppImage run on newer distributions too.
#
#   docker build -f scripts/linux.Dockerfile -t nakos-linux-build .
#   docker run --rm -v "$PWD":/src -v nakos-linux-tauri-cache:/root/.cache/tauri -w /src \
#     nakos-linux-build sh -c 'mkdir -p /tmp/bundle src-tauri/target/release &&
#       rm -rf src-tauri/target/release/bundle && ln -s /tmp/bundle src-tauri/target/release/bundle &&
#       scripts/build-linux.sh && cp -r /tmp/bundle src-tauri/target/linux-bundle'
#
# The bundle folder is a symlink into the container's /tmp: on a folder shared from macOS,
# linuxdeploy fails with "Permission denied" on the libraries it copies into the AppDir. Don't
# move the whole target folder into a Docker volume instead: colima keeps volumes in its disk
# image on the Mac's internal disk, which grows and isn't trimmed (it filled the disk once).
# The VM needs about 5 GB of memory (`colima start --memory 5`); with 2 GB rustc is killed.
#
# On Apple Silicon add `--platform linux/amd64` to both commands: the published bundles are x86_64.
# Under Rosetta, AppImages whose ELF header carries the "AI\x02" magic at byte 8 fail with "Exec
# format error". Tauri clears it in linuxdeploy but not in linuxdeploy-plugin-appimage: after the
# first (failed) run, clear it in the cached copy, then run again:
#   docker run --rm -v nakos-linux-tauri-cache:/c ubuntu:22.04 \
#     sh -c 'printf "\0\0\0" | dd of=/c/linuxdeploy-plugin-appimage.AppImage bs=1 seek=8 conv=notrunc'
# This touches only the build tool; the AppImage Nakos ships is not changed.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl wget file ca-certificates git pkg-config \
      libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
      libayatana-appindicator3-dev librsvg2-dev \
      libdbus-1-dev \
      rpm patchelf xdg-utils \
    && rm -rf /var/lib/apt/lists/*

# Node 22 LTS and the Rust stable toolchain.
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && apt-get install -y nodejs && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

# linuxdeploy (used for the AppImage) is itself an AppImage, and containers have no FUSE.
ENV APPIMAGE_EXTRACT_AND_RUN=1
