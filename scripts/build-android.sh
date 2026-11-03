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

# Build the Android app: cross-compile the Rust engine, then assemble the APK.
#
# The engine ships as one .so per ABI under jniLibs, which is where Flutter's
# Gradle build expects native libraries and where dart:ffi's
# `DynamicLibrary.open("libvb_ffi.so")` will find it at runtime.
#
# Android cannot spawn the ffmpeg binary, so by default the engine is built
# with the linked-libav backend against an LGPL ffmpeg cross-compiled by
# scripts/build-ffmpeg-android.sh, and those libraries are packaged next to
# it. The first run compiles ffmpeg, about half a minute per ABI; later runs
# reuse it. VB_ANDROID_LIBAV=0 builds the engine without it, which yields an
# app that runs but cannot analyse video.
#
# The model host comes along as it does on the Mac: whisper.cpp linked into
# the engine; llama.cpp in libvb_llama_engine.so, which the engine loads at
# runtime because the two carry different ggml; ONNX Runtime from
# Microsoft's Android build (scripts/fetch-onnxruntime-android.sh), loaded at
# runtime too; and libc++_shared.so, which the C++ in all three needs.
#
# The small models -- depth, faces and whose face each is, people, body pose,
# scene labels and one speech model, about 650 MB -- go into the APK as
# assets, and the app copies them into its library on first launch, so it
# works offline from the start, the video butler's face grouping included.
# They are taken from VB_MODELS_DIR (the Mac app's library by default) and
# checked against the catalogue's SHA-256. The rest is gigabytes, more than
# an APK can hold, and goes beside it: scripts/package-android.sh.
#
# One ABI by default, arm64-v8a: it is every phone that can hold these
# models, and the emulator on Apple silicon. VB_ANDROID_ABIS widens it.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

MODE="${1:-debug}"
JNI_DIR="app/android/app/src/main/jniLibs"

if ! command -v flutter >/dev/null; then
  echo "Flutter was not found. Install it with:  brew install --cask flutter" >&2
  exit 1
fi
if [ ! -d "$ANDROID_HOME/platform-tools" ]; then
  echo "The Android SDK looks incomplete at $ANDROID_HOME." >&2
  echo "Install the pieces with:" >&2
  echo "  sdkmanager 'platform-tools' 'platforms;android-36' 'build-tools;36.1.0'" >&2
  exit 1
fi
if ! command -v cargo-ndk >/dev/null; then
  echo "cargo-ndk is missing. Install it with:  cargo install cargo-ndk" >&2
  exit 1
fi
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  echo "No NDK found under $ANDROID_HOME/ndk. Install one with:" >&2
  echo "  sdkmanager 'ndk;$VB_NDK_VERSION'" >&2
  exit 1
fi

read -r -a ABIS <<< "${VB_ANDROID_ABIS:-arm64-v8a}"
# Flutter's default minSdk, and the level scripts/build-ffmpeg-android.sh
# builds for. Everything in the APK has to agree on it.
API=24
LIBAV="${VB_ANDROID_LIBAV:-1}"

echo "Building the engine for Android ($MODE)"
echo "  NDK: $ANDROID_NDK_HOME"

mkdir -p "$JNI_DIR"

FEATURES=(whisper)
if [ "$LIBAV" = "1" ]; then
  ./scripts/build-ffmpeg-android.sh "${ABIS[@]}"
  # One prefix per Rust target, so a single cargo-ndk invocation links every
  # ABI against its own libraries. See core/vb-media/build.rs.
  eval "$(./scripts/build-ffmpeg-android.sh --env)"
  FEATURES+=(libav)
  echo "  media: linked libav"
else
  echo "  media: none (VB_ANDROID_LIBAV=0); the app will not be able to analyse video"
fi

./scripts/fetch-onnxruntime-android.sh
eval "$(./scripts/fetch-onnxruntime-android.sh --env)"

# whisper-rs-sys decides whether to link ggml's BLAS backend by the build
# machine's OS rather than the target's, so from a Mac it asks for a
# libggml-blas.a that an Android build never makes. An empty archive answers
# it; nothing in the Android build references BLAS.
SHIMS="$(pwd)/build/android-shims"
mkdir -p "$SHIMS"
"$(ls "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin/llvm-ar | head -1)" rcs "$SHIMS/libggml-blas.a"
for abi in "${ABIS[@]}"; do
  case "$abi" in
    arm64-v8a) triple=AARCH64_LINUX_ANDROID ;;
    armeabi-v7a) triple=ARMV7_LINUX_ANDROIDEABI ;;
    x86_64) triple=X86_64_LINUX_ANDROID ;;
    *) echo "unknown ABI $abi" >&2; exit 1 ;;
  esac
  export "CARGO_TARGET_${triple}_RUSTFLAGS=-L native=$SHIMS"
done
# llama.cpp's build script finds the NDK under this name.
export ANDROID_NDK="$ANDROID_NDK_HOME"

# arm64 covers essentially every current device; armeabi-v7a covers older
# hardware and x86_64 covers the emulator, which is what most testing uses.
#
# A debug build carries full symbols and lands around 80 MB per ABI; release
# is an order of magnitude smaller, which is why it is the shipping mode.
if [ "$MODE" = "release" ]; then
  PROFILE=(--release)
else
  PROFILE=()
fi
BUILD=(build ${PROFILE[@]+"${PROFILE[@]}"} -p vb-ffi --features "$(IFS=,; echo "${FEATURES[*]}")")
# llama.cpp is always built optimised: a debug build is too slow to run a
# vision-language model at all.
LLAMA=(build --release -p vb-llama-engine)

OUT="$(pwd)/$JNI_DIR"
TARGETS=()
for abi in "${ABIS[@]}"; do TARGETS+=(-t "$abi"); done
# cargo-ndk resolves the workspace from the working directory, so this runs
# from core/ rather than passing --manifest-path.
(cd core && cargo ndk "${TARGETS[@]}" --platform "$API" --link-libcxx-shared -o "$OUT" "${BUILD[@]}")
(cd core && cargo ndk "${TARGETS[@]}" --platform "$API" --link-libcxx-shared -o "$OUT" "${LLAMA[@]}")

# The engine loads libav by soname at startup, so the libraries must sit next
# to it in the APK. A build without libav removes them, rather than leaving a
# stale copy from an earlier build to be packaged for nothing.
for abi in "${ABIS[@]}"; do
  rm -f "$JNI_DIR/$abi"/libav*.so "$JNI_DIR/$abi"/libswscale.so
  if [ "$LIBAV" = "1" ]; then
    cp "$VB_FFMPEG_ANDROID_ROOT/$abi"/lib/lib*.so "$JNI_DIR/$abi/"
  fi
  cp "$VB_ORT_ANDROID_ROOT/$abi/libonnxruntime.so" "$JNI_DIR/$abi/"
done
# An ABI no longer built must not be packaged from an earlier build.
for dir in "$JNI_DIR"/*/; do
  abi="$(basename "$dir")"
  case " ${ABIS[*]} " in *" $abi "*) ;; *) rm -rf "$dir" ;; esac
done

# --- the small models, as assets ------------------------------------------
MODELS_DIR="${VB_MODELS_DIR:-$HOME/Library/Application Support/VideoButler/models}"
ASSETS="app/android/app/src/main/assets/models"
mkdir -p "$ASSETS"
BUNDLED=(
  "depth-anything-v2-small.onnx afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c"
  "face_detection_yunet_2023mar.onnx 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
  "human_segmentation_pphumanseg_2023mar.onnx 552d8a984054e59b5d773d24b9b12022b22046ceb2bbc4c9aaeaceb36a9ddf24"
  "rtmo-s.onnx 1cd6a3517658903233e9ccb78666554862f57f0944b6886e2048093594aef4e7"
  "ggml-base-q5_1.bin 422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898"
  "siglip2-base-patch16-224-vision-fp16.onnx a1959f7bd3993a607e48839f6d01e25b876fe76afda301b028b78eef68aabd95"
  "auraface-v1-glintr100.onnx a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60"
)
echo
echo "Bundling the small models from $MODELS_DIR"
# A model no longer bundled must not ride along from an earlier build.
for file in "$ASSETS"/*; do
  [ -e "$file" ] || continue
  case " ${BUNDLED[*]} " in *" $(basename "$file") "*) ;; *) rm -f "$file" ;; esac
done
for entry in "${BUNDLED[@]}"; do
  name="${entry%% *}"
  want="${entry##* }"
  if [ -f "$ASSETS/$name" ] && [ "$(shasum -a 256 "$ASSETS/$name" | cut -d' ' -f1)" = "$want" ]; then
    continue
  fi
  if [ ! -f "$MODELS_DIR/$name" ]; then
    echo "  $name is not in $MODELS_DIR; download it (the Mac app's Models page, or" >&2
    echo "  vb call models.download '{\"id\":\"<id>\"}' --wait), or set VB_MODELS_DIR" >&2
    exit 1
  fi
  if [ "$(shasum -a 256 "$MODELS_DIR/$name" | cut -d' ' -f1)" != "$want" ]; then
    echo "  $name in $MODELS_DIR does not match the catalogue" >&2
    exit 1
  fi
  cp "$MODELS_DIR/$name" "$ASSETS/$name"
done
du -sh "$ASSETS" | awk '{print "  " $1 " of models in the APK"}'

echo
echo "Engine libraries:"
find "$JNI_DIR" -name "*.so" -exec ls -lh {} \; | awk '{print "  " $NF " (" $5 ")"}' | sort

echo
echo "Building the Flutter APK ($MODE)"
cd app
flutter pub get
if [ "$MODE" = "release" ]; then
  flutter build apk --release
  echo
  echo "APK: app/build/app/outputs/flutter-apk/app-release.apk"
else
  flutter build apk --debug
  echo
  echo "APK: app/build/app/outputs/flutter-apk/app-debug.apk"
fi
