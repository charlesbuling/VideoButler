#!/usr/bin/env bash
# Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as
# published by the Free Software Foundation, either version 3 of the
# License, or (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.

# Build the Linux app with the engine inside: LGPL ffmpeg, the engine as a
# shared library that finds it beside itself, then Flutter's bundle.
#
# On Linux (x86_64) this builds everything, whisper.cpp included, and leaves
# the app in app/build/linux/x64/release/bundle -- run bundle/video_butler.
# Flutter needs clang, cmake, ninja, pkg-config and GTK 3's headers:
#   sudo apt install clang cmake ninja-build pkg-config libgtk-3-dev
#
# From a Mac it cross-builds the engine alone, with Zig, to check it links;
# whisper.cpp is left out there, because its CMake build takes the Mac's
# compiler flags. Flutter cannot build a Linux app anywhere but on Linux.
#
# Needs zig for ffmpeg (brew install zig, or pip install ziglang).
set -euo pipefail

cd "$(dirname "$0")/.."
if [ -f scripts/env.sh ]; then
  # shellcheck source=/dev/null
  source scripts/env.sh
fi

./scripts/build-ffmpeg-cross.sh linux-x86_64
eval "$(./scripts/build-ffmpeg-cross.sh linux-x86_64 --env)"

TARGET=x86_64-unknown-linux-gnu
# The engine finds ffmpeg in its own directory, whatever the bundle's path.
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='-C link-arg=-Wl,-rpath,$ORIGIN'

if [ "$(uname -s)" = "Linux" ]; then
  echo "Building the engine for $TARGET"
  cargo build --release -p vb-ffi --features libav,whisper --target "$TARGET" \
    --manifest-path core/Cargo.toml
  (cd app && flutter pub get > /dev/null && flutter build linux --release)
  BUNDLE="app/build/linux/x64/release/bundle"
  echo
  echo "App: $BUNDLE/video_butler  ($(du -sh "$BUNDLE" | cut -f1), with LGPL ffmpeg inside)"
else
  command -v cargo-zigbuild > /dev/null || cargo install cargo-zigbuild
  rustup target add "$TARGET" > /dev/null
  echo "Cross-building the engine for $TARGET, without whisper.cpp"
  cargo zigbuild --release -p vb-ffi --features libav --target "$TARGET" \
    --manifest-path core/Cargo.toml
  echo
  echo "Engine: core/target/$TARGET/release/libvb_ffi.so"
  echo "The app itself builds only on Linux: run this script there."
fi
