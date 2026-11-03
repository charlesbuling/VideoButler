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

# Run the desktop app on macOS without Xcode.
#
# Flutter cannot build a macOS bundle without full Xcode, but its web target
# needs no Apple toolchain at all. This compiles the same Dart UI to web and
# serves it from the engine over loopback, so the app works today. When Xcode
# is installed, scripts/build-macos-native.sh produces a native bundle from
# the identical source.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

PORT="${1:-8787}"

if ! command -v flutter >/dev/null; then
  cat >&2 <<'MSG'
Flutter was not found. Install it with:

  brew install --cask flutter

Then re-run this script. If it is installed somewhere unusual, add its bin
directory to PATH in scripts/env.sh.
MSG
  exit 1
fi

if ! command -v cargo >/dev/null; then
  echo "Rust was not found. Install it from https://rustup.rs" >&2
  exit 1
fi

if ! command -v ffmpeg >/dev/null; then
  echo "Warning: ffmpeg is not on PATH, so the app will not be able to" >&2
  echo "analyse video. Install it with:  brew install ffmpeg" >&2
  echo >&2
fi

echo "Building the UI for web"
cd app
flutter pub get
flutter build web --release
cd ..

echo "Building the engine"
cargo build --release -p vb-cli --manifest-path core/Cargo.toml

echo
exec ./core/target/release/vb serve --port "$PORT" --web-root app/build/web
