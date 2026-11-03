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

# Cross-compile an LGPL ffmpeg for Android, for the linked-libav backend.
#
# Android cannot spawn the ffmpeg binary, so the engine links libav instead
# (vb-media's `libav` feature). This builds those libraries once per ABI into
# build/ffmpeg-android/<version>/<abi>/{include,lib}, which
# scripts/build-android.sh then links against and packages into the APK.
#
# The configuration is deliberately narrow: --disable-everything, then only
# what analysis uses -- the common camera containers and codecs, a JPEG and a
# PNG encoder for keyframe stills, and the select and scale filters. That
# keeps the libraries small, and it keeps the build LGPL: nothing here needs
# --enable-gpl, and the script refuses to finish if the result is not LGPL.
#
# Codec patents are a separate question from the copyright license checked
# here. H.264 and HEVC decoding in particular needs a decision before release.
#
# Usage: scripts/build-ffmpeg-android.sh [abi ...]   (default: all three)
#        scripts/build-ffmpeg-android.sh --env
#
# --env prints, as shell exports, the VB_LIBAV_DIR_<target> variables that
# point each Rust target at its ABI's build (see core/vb-media/build.rs), and
# VB_FFMPEG_ANDROID_ROOT, under which each ABI's build sits. Callers use these
# rather than keeping their own copy of the paths or the ABI-to-target mapping.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

FFMPEG_VERSION="9.0.1"
FFMPEG_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
# Flutter's default minSdk. The engine is built for the same level; a library
# built for a newer one would fail to load on the oldest supported devices.
API=24

ROOT="$(pwd)/build/ffmpeg-android"
# Shared with scripts/build-ffmpeg-macos.sh: same release, same checksum, one
# download.
SRC_DIR="$(pwd)/build/ffmpeg-src"
TARBALL="$SRC_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz"
OUT_ROOT="$ROOT/$FFMPEG_VERSION"

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  echo "No NDK found. Install one with:  sdkmanager 'ndk;$VB_NDK_VERSION'" >&2
  exit 1
fi
case "$(uname -s)" in
  Darwin) HOST_TAG="darwin-x86_64" ;; # the NDK ships universal binaries here
  Linux) HOST_TAG="linux-x86_64" ;;
  *) echo "Unsupported build host: $(uname -s)" >&2; exit 1 ;;
esac
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG"

rust_target() {
  case "$1" in
    arm64-v8a) echo aarch64_linux_android ;;
    armeabi-v7a) echo armv7_linux_androideabi ;;
    x86_64) echo x86_64_linux_android ;;
    *) echo "Unknown ABI $1" >&2; exit 1 ;;
  esac
}

if [ "${1:-}" = "--env" ]; then
  echo "export VB_FFMPEG_ANDROID_ROOT=$OUT_ROOT"
  for abi in arm64-v8a armeabi-v7a x86_64; do
    echo "export VB_LIBAV_DIR_$(rust_target "$abi")=$OUT_ROOT/$abi"
  done
  exit 0
fi

ABIS=("$@")
[ ${#ABIS[@]} -eq 0 ] && ABIS=(arm64-v8a armeabi-v7a x86_64)

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

# --- configuration -----------------------------------------------------------
# What the backend in core/vb-media/csrc/vb_av.c touches, and nothing else.
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
  --enable-zlib # the PNG encoder needs it; the NDK sysroot provides it
)

configure_abi() {
  local abi="$1" arch cpu cc extra=()
  case "$abi" in
    arm64-v8a)
      arch=aarch64; cpu=armv8-a; cc="aarch64-linux-android$API-clang" ;;
    armeabi-v7a)
      arch=arm; cpu=armv7-a; cc="armv7a-linux-androideabi$API-clang"
      extra=(--enable-neon) ;;
    x86_64)
      # Emulator only. Its assembly needs nasm, and emulator speed is not
      # worth another build dependency.
      arch=x86_64; cpu=x86-64; cc="x86_64-linux-android$API-clang"
      extra=(--disable-x86asm) ;;
    *) echo "Unknown ABI $abi" >&2; exit 1 ;;
  esac

  ./configure \
    --prefix="$OUT_ROOT/$abi" \
    --target-os=android \
    --arch="$arch" \
    --cpu="$cpu" \
    --enable-cross-compile \
    --sysroot="$TOOLCHAIN/sysroot" \
    --cc="$TOOLCHAIN/bin/$cc" \
    --cxx="$TOOLCHAIN/bin/$cc++" \
    --nm="$TOOLCHAIN/bin/llvm-nm" \
    --ar="$TOOLCHAIN/bin/llvm-ar" \
    --ranlib="$TOOLCHAIN/bin/llvm-ranlib" \
    --strip="$TOOLCHAIN/bin/llvm-strip" \
    --enable-shared \
    --disable-static \
    --enable-pic \
    --disable-programs \
    --disable-doc \
    --disable-avdevice \
    --disable-network \
    --disable-autodetect \
    --disable-debug \
    --disable-everything \
    "${COMPONENTS[@]}" \
    ${extra[@]+"${extra[@]}"} # macOS bash 3.2 treats an empty array as unset
}

# --- build -------------------------------------------------------------------
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

# What configuration a build was made with, so changing COMPONENTS rebuilds
# rather than keeping a library that lacks what the shim now calls.
CONFIG_ID="$(printf '%s\n' "${COMPONENTS[@]}" | shasum | cut -c1-12)"

for abi in "${ABIS[@]}"; do
  if [ -f "$OUT_ROOT/$abi/lib/libavformat.so" ] \
    && [ "$(cat "$OUT_ROOT/$abi/.config-id" 2>/dev/null)" = "$CONFIG_ID" ]; then
    echo "ffmpeg $FFMPEG_VERSION for $abi: already built"
    continue
  fi
  # A build from another configuration is replaced, not added to: a stale
  # library left beside the new ones is linked by name and wins.
  rm -rf "${OUT_ROOT:?}/$abi"
  echo "Building ffmpeg $FFMPEG_VERSION for $abi"
  work="$ROOT/work/$abi"
  rm -rf "$work"
  mkdir -p "$work"
  tar -xf "$TARBALL" -C "$work" --strip-components=1

  (
    cd "$work"
    configure_abi "$abi" > "$work/configure.log" 2>&1 || {
      tail -20 "$work/configure.log" >&2
      echo "configure failed for $abi; full log at $work/configure.log" >&2
      exit 1
    }

    # The shipping build must be LGPL. Checked on what configure actually
    # decided rather than on the flags passed, so a dependency that silently
    # turned on GPL code would still be caught.
    if ! grep -q '^#define CONFIG_GPL 0' config.h || ! grep -q '^#define CONFIG_NONFREE 0' config.h; then
      echo "configure produced a GPL or nonfree build for $abi; refusing to continue" >&2
      exit 1
    fi

    make -j"$JOBS" > "$work/make.log" 2>&1 || {
      tail -20 "$work/make.log" >&2
      echo "make failed for $abi; full log at $work/make.log" >&2
      exit 1
    }
    make install > /dev/null
  )

  # Android packages only files named lib*.so, and loads a library's
  # dependencies by the exact name in DT_NEEDED. ffmpeg's android target
  # already sets unversioned sonames; check rather than trust it.
  soname="$("$TOOLCHAIN/bin/llvm-readelf" -d "$OUT_ROOT/$abi/lib/libavformat.so" \
    | sed -n 's/.*Library soname: \[\(.*\)\].*/\1/p')"
  if [ "$soname" != "libavformat.so" ]; then
    echo "libavformat for $abi has soname '$soname'; Android needs 'libavformat.so'" >&2
    exit 1
  fi
  echo "$CONFIG_ID" > "$OUT_ROOT/$abi/.config-id"
  rm -rf "$work"
done

echo
echo "LGPL ffmpeg $FFMPEG_VERSION for Android:"
for abi in "${ABIS[@]}"; do
  size="$(du -ch "$OUT_ROOT/$abi"/lib/lib*.so | tail -1 | cut -f1)"
  echo "  $abi  $OUT_ROOT/$abi  ($size of libraries)"
done
