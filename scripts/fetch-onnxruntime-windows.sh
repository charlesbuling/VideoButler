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

# Fetch ONNX Runtime for Windows, for the model host's vision engine.
#
# Microsoft's own build fits Windows as it fits Android: the release zip
# carries onnxruntime.dll for x64, CPU provider, MIT-licensed. This takes
# the DLLs out of it, into build/onnxruntime-windows/current/lib/, where a
# checkout's engine finds them (vb-onnx, `load-dynamic`) and the app's
# build copies them beside video_butler.exe (app/windows/CMakeLists.txt).
#
# Windows ships an onnxruntime.dll of its own in System32 (Windows ML's,
# older), so the engine always loads this one by its full path, never by
# name.
#
# The version matches the macOS and Android builds, so all three run the
# same kernels. The zip is checked against the SHA-256 recorded here, read
# from the digest GitHub shows for the release asset.
#
# Usage: scripts/fetch-onnxruntime-windows.sh          fetch and unpack
#        scripts/fetch-onnxruntime-windows.sh --env    print the path, for callers
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="1.30.0"
SHA256="c6ba983baf5681af108599675d2a89c2d145512d02de28aed0bff177cd0ba949"
ROOT="$(pwd)/build/onnxruntime-windows"
CURRENT="$ROOT/current"

if [ "${1:-}" = "--env" ]; then
  echo "export VB_ORT_DYLIB=$CURRENT/lib/onnxruntime.dll"
  exit 0
fi

if [ -f "$CURRENT/lib/onnxruntime.dll" ] && [ "$(cat "$CURRENT/VERSION" 2>/dev/null)" = "$VERSION" ]; then
  echo "ONNX Runtime $VERSION for Windows: already unpacked"
  echo "  $CURRENT"
  exit 0
fi

NAME="onnxruntime-win-x64-$VERSION"
ZIP="$ROOT/$NAME.zip"
mkdir -p "$ROOT"
if [ ! -f "$ZIP" ]; then
  echo "Downloading ONNX Runtime $VERSION for Windows"
  # Resumed and retried on every error: a TLS connection cut off mid-file
  # is the usual failure on some networks, and not one curl counts as
  # transient.
  curl -fL --connect-timeout 20 --retry 8 --retry-all-errors -C - -o "$ZIP.part" \
    "https://github.com/microsoft/onnxruntime/releases/download/v$VERSION/$NAME.zip"
  mv "$ZIP.part" "$ZIP"
fi

if command -v shasum > /dev/null; then
  actual="$(shasum -a 256 "$ZIP" | cut -d' ' -f1)"
else
  actual="$(sha256sum "$ZIP" | cut -d' ' -f1)"
fi
if [ "$actual" != "$SHA256" ]; then
  echo "Checksum mismatch for $ZIP" >&2
  echo "  expected $SHA256" >&2
  echo "  got      $actual" >&2
  rm -f "$ZIP"
  exit 1
fi

work="$ROOT/work"
rm -rf "$work" "$CURRENT"
mkdir -p "$work" "$CURRENT/lib"
unzip -q "$ZIP" "$NAME/lib/*.dll" "$NAME/LICENSE" "$NAME/ThirdPartyNotices.txt" -d "$work"
cp "$work/$NAME"/lib/*.dll "$CURRENT/lib/"
cp "$work/$NAME/LICENSE" "$work/$NAME/ThirdPartyNotices.txt" "$CURRENT/"
echo "$VERSION" > "$CURRENT/VERSION"
rm -rf "$work"

echo "ONNX Runtime $VERSION for Windows:"
for dll in "$CURRENT"/lib/*.dll; do
  echo "  $dll ($(du -h "$dll" | cut -f1))"
done
