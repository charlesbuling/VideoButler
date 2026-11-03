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

# Package the Windows x64 app into VideoButler-<version>-windows-x64.zip.
# The zip is the Release folder (video_butler.exe plus vb_ffi.dll,
# vb_llama_engine.dll, ONNX Runtime, and LGPL ffmpeg DLLs). Models are not
# bundled: download them with scripts/fetch-models.sh or from the app's
# Models page (Hugging Face).
#
# Usage: scripts/package-windows.sh              build then package
#        scripts/package-windows.sh --no-build   package an existing Release
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

NO_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --no-build) NO_BUILD=1 ;;
    -h|--help)
      sed -n '1,30p' "$0"
      exit 0
      ;;
  esac
done

VERSION="$(sed -n 's/^version: //p' app/pubspec.yaml | head -1)"
VERSION="${VERSION:-0.2.0}"
OUT_NAME="VideoButler-${VERSION}-windows-x64"
RELEASE_DIR="app/build/windows/x64/runner/Release"
STAGING="build/package-windows/${OUT_NAME}"
ZIP="build/package-windows/${OUT_NAME}.zip"

if [ "$NO_BUILD" -eq 0 ]; then
  ./scripts/build-windows.sh
fi

if [ ! -f "$RELEASE_DIR/video_butler.exe" ]; then
  echo "No Release build at $RELEASE_DIR/video_butler.exe" >&2
  exit 1
fi

rm -rf "$STAGING" "$ZIP"
mkdir -p "$(dirname "$STAGING")"
cp -R "$RELEASE_DIR" "$STAGING"

# Prefer zip(1); fall back to PowerShell Compress-Archive on Git Bash.
(
  cd build/package-windows
  if command -v zip >/dev/null 2>&1; then
    rm -f "${OUT_NAME}.zip"
    zip -r "${OUT_NAME}.zip" "$OUT_NAME"
  else
    powershell.exe -NoProfile -Command \
      "Compress-Archive -Path '$OUT_NAME' -DestinationPath '${OUT_NAME}.zip' -Force"
  fi
)

echo "Wrote $ZIP"
