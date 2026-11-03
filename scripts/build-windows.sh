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

# Build the Windows app with the engine inside: LGPL ffmpeg DLLs, the engine
# as vb_ffi.dll, then Flutter's Windows build, which copies both beside the
# executable (app/windows/CMakeLists.txt).
#
# On Windows, from Git Bash, with Rust (MSVC), Flutter and Visual Studio's
# C++ tools: builds everything -- whisper.cpp linked in, llama.cpp's engine
# library, ONNX Runtime fetched -- into app/build/windows/x64/runner/Release;
# run video_butler.exe there. ffmpeg is the LGPL build under
# build/ffmpeg-windows-x86_64/current (WINDOWS.md), which
# scripts/build-ffmpeg-cross.sh makes again from an MSYS2 shell, or from
# Linux or a Mac.
#
# From a Mac it cross-builds ffmpeg and the engine, with Zig, to check they
# link; whisper.cpp is left out there. Flutter builds a Windows app only on
# Windows.
set -euo pipefail

cd "$(dirname "$0")/.."
if [ -f scripts/env.sh ]; then
  # shellcheck source=/dev/null
  source scripts/env.sh
fi

LIBAV="build/ffmpeg-windows-x86_64/current"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    if [ ! -f "$LIBAV/lib/avformat.lib" ]; then
      echo "No LGPL ffmpeg at $LIBAV. Build it with scripts/build-ffmpeg-cross.sh" \
        "windows-x86_64 from an MSYS2 shell (WINDOWS.md)." >&2
      exit 1
    fi
    VB_LIBAV_DIR_x86_64_pc_windows_msvc="$(cd "$LIBAV" && pwd -W 2>/dev/null || pwd)"
    export VB_LIBAV_DIR_x86_64_pc_windows_msvc
    echo "Building the engine for x86_64-pc-windows-msvc"
    cargo build --release -p vb-ffi --features libav,whisper --target x86_64-pc-windows-msvc \
      --manifest-path core/Cargo.toml
    # The model host: llama.cpp in a library of its own, and ONNX Runtime,
    # both loaded at runtime from beside the executable.
    cargo build --release -p vb-llama-engine --target x86_64-pc-windows-msvc \
      --manifest-path core/Cargo.toml
    ./scripts/fetch-onnxruntime-windows.sh
    # A pub mirror (PUB_HOSTED_URL, as from mainland China) is written into
    # pubspec.lock in place of pub.dev, and a lock naming another host is
    # resolved afresh, newer versions and all. So the lock names the mirror
    # while pub runs, and pub.dev again after, whatever happens in between.
    if [ -n "${PUB_HOSTED_URL:-}" ] && [ "${PUB_HOSTED_URL%/}" != "https://pub.dev" ]; then
      mirror="${PUB_HOSTED_URL%/}"
      sed -i "s#url: \"https://pub.dev\"#url: \"$mirror\"#" app/pubspec.lock
      trap 'sed -i "s#url: \"$mirror\"#url: \"https://pub.dev\"#" app/pubspec.lock' EXIT
    fi
    (cd app && flutter pub get > /dev/null && flutter build windows --release --no-pub)
    OUT="app/build/windows/x64/runner/Release"
    echo
    echo "App: $OUT/video_butler.exe  ($(du -sh "$OUT" | cut -f1), with LGPL ffmpeg inside)"
    ;;
  *)
    ./scripts/build-ffmpeg-cross.sh windows-x86_64
    eval "$(./scripts/build-ffmpeg-cross.sh windows-x86_64 --env)"
    command -v cargo-zigbuild > /dev/null || cargo install cargo-zigbuild
    rustup target add x86_64-pc-windows-gnu > /dev/null
    echo "Cross-building the engine for x86_64-pc-windows-gnu, without whisper.cpp"
    cargo zigbuild --release -p vb-ffi --features libav --target x86_64-pc-windows-gnu \
      --manifest-path core/Cargo.toml
    echo
    echo "Engine: core/target/x86_64-pc-windows-gnu/release/vb_ffi.dll"
    echo "The app itself builds only on Windows: run this script there, or let CI."
    ;;
esac
