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

# Build a native macOS app bundle. Requires full Xcode.
#
# This is the path that produces a real .app. It is separate from
# run-macos.sh because it needs a toolchain that run-macos.sh deliberately
# avoids; the Dart source both build from is identical.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

# An installed Xcode whose license has not been accepted answers -version
# happily and then fails every compile, so check the SDK instead.
if [ -d /Applications/Xcode.app/Contents/Developer ] && [ -z "${DEVELOPER_DIR:-}" ]; then
  cat >&2 <<'MSG'
Xcode is installed but its license has not been accepted, so no compiler will
run. Accept it once, in a terminal:

  sudo xcodebuild -license accept
  sudo xcodebuild -runFirstLaunch

Then run this script again. Until then, scripts/run-macos.sh gives you the
same app with no Xcode.
MSG
  exit 1
fi

if ! xcodebuild -version >/dev/null 2>&1; then
  cat >&2 <<'MSG'
Full Xcode is required and was not found. Only the Command Line Tools are
installed, and Flutter cannot build a macOS bundle with those alone.

Two ways to install it without the App Store:

  xcodes install --latest          # needs an Apple ID, downloads ~10 GB
  xcodes select <version>          # then point xcode-select at it

Or from https://developer.apple.com/download/all/ as a .xip.

Afterwards:
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -runFirstLaunch

Until then, scripts/run-macos.sh gives you the same app with no Xcode.
MSG
  exit 1
fi

# The engine and the app must agree on the oldest macOS they support. Cargo
# defaults to the build machine's version, which would quietly raise the app's
# minimum to whatever this Mac happens to run, so take the number from the
# Xcode project instead of hardcoding it in two places.
MACOSX_DEPLOYMENT_TARGET="$(sed -n 's/.*MACOSX_DEPLOYMENT_TARGET = \([0-9.]*\);.*/\1/p' \
  app/macos/Runner.xcodeproj/project.pbxproj | head -1)"
if [ -z "$MACOSX_DEPLOYMENT_TARGET" ]; then
  echo "Could not read MACOSX_DEPLOYMENT_TARGET from the Xcode project." >&2
  exit 1
fi
export MACOSX_DEPLOYMENT_TARGET
# whisper.cpp builds through CMake, which takes the deployment target for C
# and C++ from the variable above but not for assembly: its embedded Metal
# shaders come out marked for this Mac's own version, and the linker warns.
# CMake seeds its assembler flags from ASMFLAGS.
export ASMFLAGS="-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"

# The app carries its own ffmpeg, so it works on a Mac with no Homebrew, and
# an LGPL one, so it is shippable. The script skips the build when it is
# already there.
./scripts/build-ffmpeg-macos.sh
eval "$(./scripts/build-ffmpeg-macos.sh --env)"

# And ONNX Runtime, which the engine loads at runtime to run the director's
# vision models -- built from source at this deployment target, because every
# prebuilt Microsoft publishes needs a newer macOS than the app supports. The
# script skips the build when it is already there.
./scripts/build-onnxruntime-macos.sh
eval "$(./scripts/build-onnxruntime-macos.sh --env)"

echo "Building the engine as a static library for macOS $MACOSX_DEPLOYMENT_TARGET and later"
# libav: the bundled LGPL ffmpeg, in-process. whisper: on-device speech
# recognition with a model from the library. The frameworks these two add to
# the link are listed in app/macos/Runner/Configs/Engine.xcconfig.
cargo build --release -p vb-ffi --features libav,whisper --manifest-path core/Cargo.toml

# llama.cpp, in a library of its own that the engine loads at runtime for the
# vision-language models. Never linked into the engine: whisper.cpp is, and
# the two carry different versions of ggml under the same symbol names.
echo "Building llama.cpp's library for macOS $MACOSX_DEPLOYMENT_TARGET and later"
cargo build --release -p vb-llama-engine --manifest-path core/Cargo.toml
LLAMA_DYLIB="core/target/release/libvb_llama_engine.dylib"

(cd app && flutter pub get && flutter build macos --release)

APP="app/build/macos/Build/Products/Release/video_butler.app"

# Carry libav inside the bundle. Its install names are @rpath and the Flutter
# runner already searches @executable_path/../Frameworks, so this is all it
# takes for the app to find them at runtime. Copied with -a to keep the
# version symlinks the dylibs refer to each other by.
cp -a "$VB_LIBAV_DIR"/lib/lib*.dylib "$APP/Contents/Frameworks/"
# ONNX Runtime beside it, with its version symlinks. Nothing links it; the
# engine looks for it in Contents/Frameworks.
cp -a "$(dirname "$VB_ORT_DYLIB")"/libonnxruntime*.dylib "$APP/Contents/Frameworks/"
# llama.cpp's library likewise, named for where it now lives.
cp "$LLAMA_DYLIB" "$APP/Contents/Frameworks/"
install_name_tool -id @rpath/libvb_llama_engine.dylib \
  "$APP/Contents/Frameworks/libvb_llama_engine.dylib"

# Adding files invalidates the bundle's signature, so sign again. Ad-hoc is
# enough to run here; a release needs a Developer ID identity and
# notarisation, without which another Mac refuses to open it.
codesign --force --sign - "$APP/Contents/Frameworks"/lib*.dylib
codesign --force --sign - "$APP"

echo
echo "App: $APP"
echo "     $(du -sh "$APP" | cut -f1), macOS $MACOSX_DEPLOYMENT_TARGET and later, with LGPL ffmpeg, ONNX Runtime and llama.cpp inside"
