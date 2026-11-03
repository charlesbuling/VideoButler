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

# Build an LGPL ffmpeg for iOS, for the linked-libav backend.
#
# iOS allows no subprocess, so the app can only analyse through linked libav,
# and it must ship its own. Linked dynamically, as on the Mac: LGPL code in a
# closed app is shippable when a user can swap the library, which a static
# link would make them relink the whole app to do. iOS takes dynamic
# libraries only as frameworks, so each library is wrapped as one, for the
# device and the simulator, and the two joined into an xcframework.
#
# The components are the macOS build's, exactly: the same footage decodes the
# same way on every Apple device. VideoToolbox is on, as on the Mac: it is the
# phone's own decoder, a system framework.
#
# Out:
#   build/ffmpeg-ios/<version>/<sdk>/{include,lib}   one install per SDK, for
#                                                    the engine's build
#   build/ffmpeg-ios/<version>/xcframeworks/         what the app links and
#                                                    embeds
#   build/ffmpeg-ios/current -> <version>
#
# Usage: scripts/build-ffmpeg-ios.sh          build it
#        scripts/build-ffmpeg-ios.sh --env    print the paths, for callers
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

FFMPEG_VERSION="9.0.1"
FFMPEG_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"

# The oldest iOS the app supports, read from the Xcode project so the
# libraries, the engine and the app cannot disagree.
DEPLOYMENT_TARGET="$(sed -n 's/.*IPHONEOS_DEPLOYMENT_TARGET = \([0-9.]*\);.*/\1/p' \
  app/ios/Runner.xcodeproj/project.pbxproj | head -1)"
DEPLOYMENT_TARGET="${DEPLOYMENT_TARGET:-15.0}"

ROOT="$(pwd)/build/ffmpeg-ios"
SRC_DIR="$(pwd)/build/ffmpeg-src"
TARBALL="$SRC_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz"
BASE="$ROOT/$FFMPEG_VERSION"
CURRENT="$ROOT/current"

if [ "${1:-}" = "--env" ]; then
  echo "export VB_LIBAV_DIR_aarch64_apple_ios=$CURRENT/iphoneos"
  echo "export VB_LIBAV_DIR_aarch64_apple_ios_sim=$CURRENT/iphonesimulator"
  echo "export VB_LIBAV_XCFRAMEWORKS=$CURRENT/xcframeworks"
  exit 0
fi

# Kept identical to scripts/build-ffmpeg-macos.sh and build-ffmpeg-android.sh.
COMPONENTS=(
  --enable-protocol=file
  --enable-demuxer=mov,matroska,mpegts,avi,mpegps
  # Photographs: JPEG, PNG and WebP through the image demuxers, by name and by
  # content. HEIF comes through mov above, as HEVC tiles the shim assembles.
  --enable-demuxer=image2,image_jpeg_pipe,image_png_pipe,image_webp_pipe
  --enable-decoder=png,webp
  --enable-parser=png,webp
  --enable-decoder=h264,hevc,mpeg4,mpeg2video,vp8,vp9,prores,mjpeg
  --enable-parser=h264,hevc,mpeg4video,mpegvideo,vp8,vp9,mjpeg
  --enable-demuxer=mp3,wav,flac,aac,ogg,aiff
  --enable-decoder=aac,aac_latm,mp3,mp3float,alac,flac,opus,vorbis,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le
  --enable-parser=aac,mpegaudio,flac,opus,vorbis
  --enable-encoder=mjpeg,png
  # The built-in editor's export: its sound in AAC, the piece in an MP4,
  # and a clip's sound played at its speed and in the piece's format.
  --enable-encoder=aac
  --enable-muxer=mp4,mov
  --enable-filter=buffer,buffersink,select,scale,format
  --enable-filter=abuffer,abuffersink,atempo,aresample,aformat
  --enable-zlib
)
HWACCEL=(
  --enable-videotoolbox
  --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,vp9_videotoolbox,mpeg2_videotoolbox,mpeg4_videotoolbox
  # And the media engine's encoders, for the editor's export: x264 and x265
  # are GPL and never in this build.
  --enable-encoder=h264_videotoolbox,hevc_videotoolbox
)

# Dependents last: each library's references are rewritten to the ones
# before it.
LIBS=(avutil swresample swscale avcodec avformat avfilter)

CONFIG_ID="$(printf '%s\n' "${COMPONENTS[@]}" "${HWACCEL[@]}" "$DEPLOYMENT_TARGET" | shasum | cut -c1-12)"
if [ -d "$BASE/xcframeworks/libavfilter.xcframework" ] \
  && [ "$(cat "$BASE/.config-id" 2>/dev/null)" = "$CONFIG_ID" ]; then
  ln -sfn "$FFMPEG_VERSION" "$CURRENT"
  echo "LGPL ffmpeg $FFMPEG_VERSION for iOS $DEPLOYMENT_TARGET: already built"
  echo "  $BASE"
  exit 0
fi
rm -rf "$BASE"

mkdir -p "$SRC_DIR"
if [ ! -f "$TARBALL" ]; then
  echo "Downloading ffmpeg $FFMPEG_VERSION"
  curl -fsSL -o "$TARBALL.part" "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
  mv "$TARBALL.part" "$TARBALL"
fi
actual="$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)"
if [ "$actual" != "$FFMPEG_SHA256" ]; then
  echo "Checksum mismatch for $TARBALL: expected $FFMPEG_SHA256, got $actual" >&2
  exit 1
fi

# One SDK: configure, check the licence and the hardware decoder, build,
# install into $BASE/<sdk>.
build_sdk() {
  local sdk="$1" target="$2"
  local prefix="$BASE/$sdk" work="$ROOT/work-$sdk"
  local sysroot cc
  sysroot="$(xcrun --sdk "$sdk" --show-sdk-path)"
  cc="$(xcrun --sdk "$sdk" -f clang)"
  echo "Building LGPL ffmpeg $FFMPEG_VERSION for $sdk ($target)"
  rm -rf "$work"
  mkdir -p "$work"
  tar -xf "$TARBALL" -C "$work" --strip-components=1
  (
    cd "$work"
    ./configure \
      --prefix="$prefix" \
      --enable-cross-compile \
      --target-os=darwin \
      --arch=aarch64 \
      --cc="$cc" \
      --sysroot="$sysroot" \
      --extra-cflags="-target $target" \
      --extra-ldflags="-target $target" \
      --enable-shared \
      --disable-static \
      --enable-pic \
      --install-name-dir=@rpath \
      --disable-programs \
      --disable-doc \
      --disable-avdevice \
      --disable-network \
      --disable-autodetect \
      --disable-debug \
      --disable-everything \
      "${COMPONENTS[@]}" \
      "${HWACCEL[@]}" \
      > "$work/configure.log" 2>&1 || {
        tail -20 "$work/configure.log" >&2
        echo "configure failed; full log at $work/configure.log" >&2
        exit 1
      }
    if ! grep -q '^#define CONFIG_GPL 0' config.h || ! grep -q '^#define CONFIG_NONFREE 0' config.h; then
      echo "configure produced a GPL or nonfree build; refusing to continue" >&2
      exit 1
    fi
    if ! grep -q '^#define CONFIG_HEVC_VIDEOTOOLBOX_HWACCEL 1' config_components.h; then
      echo "configure left out VideoToolbox; refusing to continue" >&2
      exit 1
    fi
    make -j"$(sysctl -n hw.ncpu)" > "$work/make.log" 2>&1 || {
      tail -20 "$work/make.log" >&2
      echo "make failed; full log at $work/make.log" >&2
      exit 1
    }
    make install > /dev/null
  )
  rm -rf "$work"
}

# Wrap one SDK's dylibs as frameworks: lib<name>.framework/lib<name>, its
# install name and its references to the others rewritten to match.
frameworks_for() {
  local sdk="$1" platform="$2"
  local out="$BASE/frameworks-$sdk"
  mkdir -p "$out"
  for name in "${LIBS[@]}"; do
    local lib="lib$name"
    local real
    real="$(cd "$BASE/$sdk/lib" && ls "$lib".*.dylib | grep -E "^$lib\.[0-9]+\.dylib$" | head -1)"
    local fw="$out/$lib.framework"
    mkdir -p "$fw"
    cp "$BASE/$sdk/lib/$real" "$fw/$lib"
    install_name_tool -id "@rpath/$lib.framework/$lib" "$fw/$lib"
    for dep in "${LIBS[@]}"; do
      local deplib="lib$dep"
      local ref
      ref="$(otool -L "$fw/$lib" | awk '{print $1}' | grep -E "^@rpath/$deplib\.[0-9]+\.dylib$" || true)"
      if [ -n "$ref" ]; then
        install_name_tool -change "$ref" "@rpath/$deplib.framework/$deplib" "$fw/$lib"
      fi
    done
    cat > "$fw/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>$lib</string>
  <key>CFBundleIdentifier</key><string>dev.videobutler.ffmpeg.$lib</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$lib</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>$FFMPEG_VERSION</string>
  <key>CFBundleVersion</key><string>$FFMPEG_VERSION</string>
  <key>CFBundleSupportedPlatforms</key><array><string>$platform</string></array>
  <key>MinimumOSVersion</key><string>$DEPLOYMENT_TARGET</string>
</dict>
</plist>
PLIST
  done
}

build_sdk iphoneos "arm64-apple-ios$DEPLOYMENT_TARGET"
build_sdk iphonesimulator "arm64-apple-ios$DEPLOYMENT_TARGET-simulator"
frameworks_for iphoneos iPhoneOS
frameworks_for iphonesimulator iPhoneSimulator

mkdir -p "$BASE/xcframeworks"
for name in "${LIBS[@]}"; do
  lib="lib$name"
  xcodebuild -create-xcframework \
    -framework "$BASE/frameworks-iphoneos/$lib.framework" \
    -framework "$BASE/frameworks-iphonesimulator/$lib.framework" \
    -output "$BASE/xcframeworks/$lib.xcframework" > /dev/null
done
rm -rf "$BASE/frameworks-iphoneos" "$BASE/frameworks-iphonesimulator"

# Each slice must be marked for the app's oldest iOS, or the App Store and
# older phones refuse it.
for slice in "$BASE"/xcframeworks/libavcodec.xcframework/*/libavcodec.framework/libavcodec; do
  minos="$(otool -l "$slice" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')"
  if [ "$minos" != "$DEPLOYMENT_TARGET" ]; then
    echo "$slice is marked for iOS $minos, not $DEPLOYMENT_TARGET; refusing it" >&2
    exit 1
  fi
done

echo "$CONFIG_ID" > "$BASE/.config-id"
ln -sfn "$FFMPEG_VERSION" "$CURRENT"

echo
echo "LGPL ffmpeg $FFMPEG_VERSION for iOS $DEPLOYMENT_TARGET and later:"
echo "  $BASE/xcframeworks  ($(du -sh "$BASE/xcframeworks" | cut -f1), device and simulator)"
