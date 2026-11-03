#!/bin/sh
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

# Copy the engine's dynamic libraries into the iOS app and sign them.
#
# Run by the Runner target's "Embed engine libraries" build phase, after
# Flutter embeds its own. The LGPL ffmpeg is the slice of each xcframework
# for the platform being built -- device or simulator -- and it is signed
# with the identity the app is signed with, which iOS requires of every
# framework it loads.
set -eu

SLICE=ios-arm64
if [ "${PLATFORM_NAME:-}" = "iphonesimulator" ]; then
  SLICE=ios-arm64-simulator
fi
XCFRAMEWORKS="$SRCROOT/../../build/ffmpeg-ios/current/xcframeworks"
DEST="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"
mkdir -p "$DEST"

for xc in "$XCFRAMEWORKS"/lib*.xcframework; do
  fw="$xc/$SLICE/$(basename "$xc" .xcframework).framework"
  if [ ! -d "$fw" ]; then
    echo "error: $fw is missing; run scripts/build-ffmpeg-ios.sh" >&2
    exit 1
  fi
  rm -rf "$DEST/$(basename "$fw")"
  cp -R "$fw" "$DEST/"
  if [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" --preserve-metadata=identifier,entitlements \
      "$DEST/$(basename "$fw")"
  fi
done
