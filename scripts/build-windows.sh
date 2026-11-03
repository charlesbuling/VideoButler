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
# C++ tools: builds everything, whisper.cpp included, into
# app/build/windows/x64/runner/Release -- run video_butler.exe there.
# ffmpeg itself is cross-built with Zig on Linux or a Mac (Windows has no
# shell its configure runs well in), and the Windows repository carries
# the result in build/ffmpeg-windows-x86_64/current.
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
        "windows-x86_64 on Linux or a Mac and copy build/ffmpeg-windows-x86_64 here." >&2
      exit 1
    fi
    VB_LIBAV_DIR_x86_64_pc_windows_msvc="$(cd "$LIBAV" && pwd -W 2>/dev/null || pwd)"
    export VB_LIBAV_DIR_x86_64_pc_windows_msvc
    echo "Building the engine for x86_64-pc-windows-msvc"
    cargo build --release -p vb-ffi --features libav,whisper --target x86_64-pc-windows-msvc \
      --manifest-path core/Cargo.toml
    (cd app && flutter pub get > /dev/null && flutter build windows --release)
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
    echo "The app itself builds only on Windows: run this script there."
    ;;
esac
