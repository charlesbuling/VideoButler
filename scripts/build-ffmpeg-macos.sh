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

# Build an LGPL ffmpeg for macOS, for the linked-libav backend.
#
# The app must analyse video on a Mac that has never heard of Homebrew, so it
# links libav and carries it inside the bundle. Homebrew's ffmpeg is a GPLv3
# build and cannot ship; this one is LGPL, and the script refuses to finish if
# configure decides otherwise.
#
# The libraries land in build/ffmpeg-macos/<version>/<arch>/{include,lib} with
# install names under @rpath, which is what lets them sit in the app's
# Frameworks directory and still be found at runtime.
# scripts/build-macos-native.sh links against them and copies them in.
#
# The configuration matches scripts/build-ffmpeg-android.sh exactly: the two
# platforms decode the same footage, and a codec enabled on one and missing on
# the other would be a difference users hit rather than a difference we chose.
#
# Usage: scripts/build-ffmpeg-macos.sh          build it
#        scripts/build-ffmpeg-macos.sh --env    print the paths, for callers
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

FFMPEG_VERSION="9.0.1"
FFMPEG_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"

# The oldest macOS the app supports, read from the Xcode project so the
# libraries, the engine and the app cannot disagree.
DEPLOYMENT_TARGET="$(sed -n 's/.*MACOSX_DEPLOYMENT_TARGET = \([0-9.]*\);.*/\1/p' \
  app/macos/Runner.xcodeproj/project.pbxproj | head -1)"
ARCH="$(uname -m)"

ROOT="$(pwd)/build/ffmpeg-macos"
SRC_DIR="$(pwd)/build/ffmpeg-src"
TARBALL="$SRC_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz"
PREFIX="$ROOT/$FFMPEG_VERSION/$ARCH"

# A stable path for the app's build settings to point at, so the ffmpeg
# version lives in this script alone and nowhere in the Xcode project.
CURRENT="$ROOT/current"

if [ "${1:-}" = "--env" ]; then
  echo "export VB_LIBAV_DIR=$CURRENT"
  exit 0
fi

# What the backend in core/vb-media/csrc/vb_av.c touches, and nothing else.
# Kept identical to scripts/build-ffmpeg-android.sh.
COMPONENTS=(
  --enable-protocol=file
  # .mp4 .mov .m4v / .mkv .webm / .mts .m2ts / .avi / MPEG program streams
  --enable-demuxer=mov,matroska,mpegts,avi,mpegps
  # Photographs: JPEG, PNG and WebP through the image demuxers, by name and by
  # content. HEIF comes through mov above, as HEVC tiles the shim assembles.
  --enable-demuxer=image2,image_jpeg_pipe,image_png_pipe,image_webp_pipe
  --enable-decoder=png,webp
  --enable-parser=png,webp
  --enable-decoder=h264,hevc,mpeg4,mpeg2video,vp8,vp9,prores,mjpeg
  --enable-parser=h264,hevc,mpeg4video,mpegvideo,vp8,vp9,mjpeg
  # Audio, decoded to mono for beat detection and speech recognition: the
  # audio in footage (AAC almost always), and music a user picks to cut to.
  --enable-demuxer=mp3,wav,flac,aac,ogg,aiff
  --enable-decoder=aac,aac_latm,mp3,mp3float,alac,flac,opus,vorbis,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le
  --enable-parser=aac,mpegaudio,flac,opus,vorbis
  # Keyframe stills.
  --enable-encoder=mjpeg,png
  # The built-in editor's export: its sound in AAC, the piece in an MP4,
  # and a clip's sound played at its speed and in the piece's format.
  --enable-encoder=aac
  --enable-muxer=mp4,mov
  # Scene detection, plus the scaler the filter graph inserts on its own.
  --enable-filter=buffer,buffersink,select,scale,format
  --enable-filter=abuffer,abuffersink,atempo,aresample,aformat
  --enable-zlib
)

# The Mac's own video decoder. The one place this build departs from the
# Android one, deliberately: VideoToolbox is the Mac's hardware decoder, a
# system framework, LGPL-compatible, and the difference between analysing
# 4K HEVC at a fifth of real time on the CPU and several times real time on
# the media engine. Android's equivalent is MediaCodec, a separate change.
# The shim falls back to software on its own when a stream will not decode
# in hardware, so a codec listed above without a hwaccel here still works.
HWACCEL=(
  --enable-videotoolbox
  --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,vp9_videotoolbox,mpeg2_videotoolbox,mpeg4_videotoolbox
  # And the media engine's encoders, for the editor's export: x264 and x265
  # are GPL and never in this build.
  --enable-encoder=h264_videotoolbox,hevc_videotoolbox
)

# What configuration a build was made with, so changing COMPONENTS rebuilds
# rather than keeping a library that lacks what the shim now calls.
CONFIG_ID="$(printf '%s\n' "${COMPONENTS[@]}" "${HWACCEL[@]}" | shasum | cut -c1-12)"

if [ -f "$PREFIX/lib/libavformat.dylib" ] \
  && [ "$(cat "$PREFIX/.config-id" 2>/dev/null)" = "$CONFIG_ID" ]; then
  ln -sfn "$FFMPEG_VERSION/$ARCH" "$CURRENT"
  echo "LGPL ffmpeg $FFMPEG_VERSION for macOS/$ARCH: already built"
  echo "  $PREFIX"
  exit 0
fi
# A build from another configuration is replaced, not added to.
rm -rf "$PREFIX"

# --- source ------------------------------------------------------------------
mkdir -p "$SRC_DIR"
if [ ! -f "$TARBALL" ]; then
  echo "Downloading ffmpeg $FFMPEG_VERSION"
  curl -fsSL -o "$TARBALL.part" "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
  mv "$TARBALL.part" "$TARBALL"
fi
actual="$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)"
if [ "$actual" != "$FFMPEG_SHA256" ]; then
  echo "Checksum mismatch for $TARBALL" >&2
  echo "  expected $FFMPEG_SHA256" >&2
  echo "  got      $actual" >&2
  exit 1
fi

# --- build -------------------------------------------------------------------
echo "Building LGPL ffmpeg $FFMPEG_VERSION for macOS/$ARCH (deployment target $DEPLOYMENT_TARGET)"
work="$ROOT/work"
rm -rf "$work"
mkdir -p "$work"
tar -xf "$TARBALL" -C "$work" --strip-components=1

(
  cd "$work"
  ./configure \
    --prefix="$PREFIX" \
    --enable-shared \
    --disable-static \
    --enable-pic \
    --install-name-dir=@rpath \
    --extra-cflags="-mmacosx-version-min=$DEPLOYMENT_TARGET" \
    --extra-ldflags="-mmacosx-version-min=$DEPLOYMENT_TARGET" \
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

  # Checked on what configure decided, not on the flags passed, so a
  # dependency that quietly pulled in GPL code is still caught.
  if ! grep -q '^#define CONFIG_GPL 0' config.h || ! grep -q '^#define CONFIG_NONFREE 0' config.h; then
    echo "configure produced a GPL or nonfree build; refusing to continue" >&2
    exit 1
  fi
  # A build that silently lost the hardware decoder analyses five times
  # slower and says nothing; refuse it instead.
  if ! grep -q '^#define CONFIG_HEVC_VIDEOTOOLBOX_HWACCEL 1' config_components.h; then
    echo "configure left out VideoToolbox; refusing to continue" >&2
    exit 1
  fi
  # Nor one that cannot export what the editor makes.
  for component in H264_VIDEOTOOLBOX_ENCODER AAC_ENCODER MP4_MUXER ATEMPO_FILTER; do
    if ! grep -q "^#define CONFIG_$component 1" config_components.h; then
      echo "configure left out $component; refusing to continue" >&2
      exit 1
    fi
  done

  make -j"$(sysctl -n hw.ncpu)" > "$work/make.log" 2>&1 || {
    tail -20 "$work/make.log" >&2
    echo "make failed; full log at $work/make.log" >&2
    exit 1
  }
  make install > /dev/null
)
rm -rf "$work"

# The bundle can only find these by @rpath; an absolute install name would
# work on this machine and nowhere else, which is the kind of bug that ships.
for lib in "$PREFIX"/lib/lib*.dylib; do
  name="$(otool -D "$lib" | tail -1)"
  case "$name" in
    @rpath/*) ;;
    *)
      echo "$(basename "$lib") has install name '$name'; the app needs @rpath" >&2
      exit 1
      ;;
  esac
done

echo "$CONFIG_ID" > "$PREFIX/.config-id"
ln -sfn "$FFMPEG_VERSION/$ARCH" "$CURRENT"

echo
echo "LGPL ffmpeg $FFMPEG_VERSION for macOS/$ARCH:"
echo "  $PREFIX  ($(du -ch "$PREFIX"/lib/*.dylib | tail -1 | cut -f1) of libraries)"
