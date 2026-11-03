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

# Fetch ONNX Runtime for Android, for the model host's vision engine.
#
# On macOS the app carries an ONNX Runtime built from source, because
# Microsoft's builds need a newer macOS than the app supports. On Android
# Microsoft's own build fits: the AAR on Maven Central carries one
# libonnxruntime.so per ABI, built for API 21 and up, MIT-licensed. This
# takes the .so files out of it, into build/onnxruntime-android/<version>/<abi>/,
# and scripts/build-android.sh packages them beside the engine, which loads
# the library at runtime (vb-onnx, `load-dynamic`) as it does on the Mac.
#
# The version matches the macOS build, so both run the same kernels. The
# AAR is checked against the SHA-256 recorded here, read from the file
# Maven serves; a mirror is tried when Maven Central does not answer.
#
# Usage: scripts/fetch-onnxruntime-android.sh          fetch and unpack
#        scripts/fetch-onnxruntime-android.sh --env    print the path, for callers
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="1.30.0"
SHA256="e7fb945e402205f6db858d65bb78d2bdb0812317b383976c9e3bceb4862c73f1"
ROOT="$(pwd)/build/onnxruntime-android"
PREFIX="$ROOT/$VERSION"

if [ "${1:-}" = "--env" ]; then
  echo "export VB_ORT_ANDROID_ROOT=$PREFIX"
  exit 0
fi

ABIS=(arm64-v8a armeabi-v7a x86_64)

if [ -f "$PREFIX/arm64-v8a/libonnxruntime.so" ]; then
  echo "ONNX Runtime $VERSION for Android: already unpacked"
  echo "  $PREFIX"
  exit 0
fi

AAR="$ROOT/onnxruntime-android-$VERSION.aar"
mkdir -p "$ROOT"
if [ ! -f "$AAR" ]; then
  path="com/microsoft/onnxruntime/onnxruntime-android/$VERSION/onnxruntime-android-$VERSION.aar"
  for base in https://repo1.maven.org/maven2 https://maven.aliyun.com/repository/public; do
    echo "Downloading ONNX Runtime $VERSION for Android from $base"
    if curl -fL --connect-timeout 20 --retry 3 -C - -o "$AAR.part" "$base/$path"; then
      mv "$AAR.part" "$AAR"
      break
    fi
  done
fi
if [ ! -f "$AAR" ]; then
  echo "Could not download the ONNX Runtime AAR" >&2
  exit 1
fi

actual="$(shasum -a 256 "$AAR" | cut -d' ' -f1)"
if [ "$actual" != "$SHA256" ]; then
  echo "Checksum mismatch for $AAR" >&2
  echo "  expected $SHA256" >&2
  echo "  got      $actual" >&2
  rm -f "$AAR"
  exit 1
fi

rm -rf "$PREFIX"
work="$ROOT/work"
rm -rf "$work"
mkdir -p "$work"
unzip -q "$AAR" 'jni/*' -d "$work"
for abi in "${ABIS[@]}"; do
  mkdir -p "$PREFIX/$abi"
  cp "$work/jni/$abi/libonnxruntime.so" "$PREFIX/$abi/"
done
rm -rf "$work"

echo "ONNX Runtime $VERSION for Android:"
for abi in "${ABIS[@]}"; do
  echo "  $PREFIX/$abi/libonnxruntime.so ($(du -h "$PREFIX/$abi/libonnxruntime.so" | cut -f1))"
done
