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

# Build the iOS app with the engine inside: LGPL ffmpeg as frameworks, the
# engine as a static library for the device and the simulator, then Flutter.
#
# What it makes:
#   --simulator (default)  app/build/ios/iphonesimulator/Runner.app, runnable
#                          in any arm64 simulator with no Apple account
#   --device               app/build/ios/iphoneos/Runner.app, unsigned: sign
#                          it in Xcode with a team (Runner > Signing), or pass
#                          --codesign once one is set there
#
# Not yet in the iOS build: ONNX Runtime and llama.cpp, so the director's
# models, the composition judge and captions are absent there and every run
# says so. Everything else -- LibAv with VideoToolbox, Apple Vision, the
# faces, whisper.cpp on Metal, beats, the cut and its drafts -- is the Mac's.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

MODE="simulator"
SIGN="--no-codesign"
for arg in "$@"; do
  case "$arg" in
    --simulator) MODE="simulator" ;;
    --device) MODE="device" ;;
    --codesign) SIGN="" ;;
    *) echo "unknown option $arg" >&2; exit 1 ;;
  esac
done

DEPLOYMENT_TARGET="$(sed -n 's/.*IPHONEOS_DEPLOYMENT_TARGET = \([0-9.]*\);.*/\1/p' \
  app/ios/Runner.xcodeproj/project.pbxproj | head -1)"
export IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET"

./scripts/build-ffmpeg-ios.sh
eval "$(./scripts/build-ffmpeg-ios.sh --env)"

if [ "$MODE" = "simulator" ]; then
  TARGET=aarch64-apple-ios-sim
else
  TARGET=aarch64-apple-ios
fi
rustup target add "$TARGET" > /dev/null
echo "Building the engine for $TARGET, iOS $DEPLOYMENT_TARGET and later"
cargo build --release -p vb-ffi --features libav,whisper --target "$TARGET" \
  --manifest-path core/Cargo.toml

cd app
flutter pub get > /dev/null
if [ "$MODE" = "simulator" ]; then
  flutter build ios --simulator --debug
  APP="app/build/ios/iphonesimulator/Runner.app"
else
  # shellcheck disable=SC2086
  flutter build ios --release $SIGN
  APP="app/build/ios/iphoneos/Runner.app"
fi
cd ..

echo
echo "App: $APP"
echo "     $(du -sh "$APP" | cut -f1), iOS $DEPLOYMENT_TARGET and later, with LGPL ffmpeg inside"
