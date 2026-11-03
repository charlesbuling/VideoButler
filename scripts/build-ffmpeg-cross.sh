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

# Build an LGPL ffmpeg for Linux or Windows, for the linked-libav backend,
# from macOS, Linux or Windows (MSYS2, for its make), with Zig as the C
# toolchain.
#
# The desktop apps for Linux and Windows carry their own ffmpeg as the Mac
# app does: a distribution's ffmpeg is usually a GPL build, and Windows has
# none. Zig compiles for either from anywhere, so this one script runs on
# the development Mac, the Windows machine and the CI runners alike, and
# they cannot build different things.
#
# The components are the other builds' exactly, and a build whose configure
# came out GPL or nonfree is refused. Windows adds what VideoToolbox is on
# the Mac: Media Foundation's encoders for the editor's export and D3D11
# decoding, both system libraries. There is no hardware decoder on Linux
# yet (VA-API): footage decodes in software there.
#
# Out: build/ffmpeg-<target>/<version>/{include,lib,bin} and a `current`
# link beside it -- a copy on a Windows host, where git carries the build
# (WINDOWS.md). Shared libraries: .so on Linux; on Windows .dll in bin/
# with import libraries in lib/.
#
# Usage: scripts/build-ffmpeg-cross.sh <linux-x86_64|linux-aarch64|windows-x86_64>
#        scripts/build-ffmpeg-cross.sh <target> --env
#
# x86_64 targets need nasm. On Windows, from an MSYS2 shell (Git Bash has
# no make), with Zig on PATH and `pacman -S make diffutils binutils gcc
# nasm` -- gcc only for configure's host compiler, which builds nothing
# that ships.
set -euo pipefail
# The DLLs' export lists are sorted, and sort orders by locale: an MSYS2
# shell put av_pkt_dump_log2 before av_pkt_dump2, where the Mac did not.
export LC_ALL=C

cd "$(dirname "$0")/.."

TARGET="${1:-}"
case "$TARGET" in
  linux-x86_64) ZIG_TARGET=x86_64-linux-gnu.2.28; OS=linux; ARCH=x86_64; RUST=x86_64-unknown-linux-gnu ;;
  linux-aarch64) ZIG_TARGET=aarch64-linux-gnu.2.28; OS=linux; ARCH=aarch64; RUST=aarch64-unknown-linux-gnu ;;
  windows-x86_64) ZIG_TARGET=x86_64-windows-gnu; OS=mingw32; ARCH=x86_64; RUST=x86_64-pc-windows-gnu ;;
  *) echo "usage: $0 <linux-x86_64|linux-aarch64|windows-x86_64> [--env]" >&2; exit 1 ;;
esac

FFMPEG_VERSION="9.0.1"
FFMPEG_SHA256="cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"
ZLIB_VERSION="1.3.1"
ZLIB_SHA256="9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23"

ROOT="$(pwd)/build/ffmpeg-$TARGET"
SRC_DIR="$(pwd)/build/ffmpeg-src"
PREFIX="$ROOT/$FFMPEG_VERSION"
CURRENT="$ROOT/current"

if [ "${2:-}" = "--env" ]; then
  echo "export VB_LIBAV_DIR_${RUST//-/_}=$CURRENT"
  exit 0
fi

command -v zig > /dev/null || { echo "zig is needed: brew install zig, or pip install ziglang" >&2; exit 1; }

# Kept identical to the other ffmpeg builds.
COMPONENTS=(
  --enable-protocol=file
  --enable-demuxer=mov,matroska,mpegts,avi,mpegps
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

# Windows' own video hardware, as the Mac build has VideoToolbox: D3D11
# decoding (the shim falls back to software for a stream it will not take)
# and Media Foundation's encoders for the editor's export -- the GPU's
# where there is one, Microsoft's software H.264 otherwise. Both are system
# libraries, LGPL-compatible; x264 and x265 are GPL and never in this build.
HWACCEL=()
if [ "$OS" = "mingw32" ]; then
  HWACCEL=(
    --enable-d3d11va
    --enable-hwaccel=h264_d3d11va,h264_d3d11va2,hevc_d3d11va,hevc_d3d11va2
    --enable-mediafoundation
    --enable-encoder=h264_mf,hevc_mf
  )
fi

# x86's SIMD -- the decoders', and the scaler's that every frame the
# engine reads goes through -- is written for nasm. Without it the C
# fallbacks run, slower, and round differently from every other ffmpeg on
# the machine. Arm's is assembled by the C compiler and needs nothing.
ASM=()
if [ "$ARCH" = "x86_64" ]; then
  command -v nasm > /dev/null ||
    { echo "nasm is needed for $TARGET: brew install nasm, apt install nasm, or pacman -S nasm" >&2; exit 1; }
  ASM=(--x86asmexe=nasm)
fi

# shasum is Perl's, and MSYS2 has sha1sum and sha256sum instead; both
# print the digest first.
sha() {
  if command -v shasum > /dev/null; then shasum -a "$@"; else "sha$1sum" "${@:2}"; fi
}

# The link where links work. On a Windows host git carries the build itself
# rather than a link to it (WINDOWS.md), so `current` is a copy.
link_current() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) rm -rf "$CURRENT" && cp -R "$PREFIX" "$CURRENT" ;;
    *) ln -sfn "$FFMPEG_VERSION" "$CURRENT" ;;
  esac
}

CONFIG_ID="$(printf '%s\n' "${COMPONENTS[@]}" ${HWACCEL[@]+"${HWACCEL[@]}"} ${ASM[@]+"${ASM[@]}"} \
  "$ZIG_TARGET" | sha 1 | cut -c1-12)"
if [ -f "$PREFIX/.config-id" ] && [ "$(cat "$PREFIX/.config-id")" = "$CONFIG_ID" ]; then
  link_current
  echo "LGPL ffmpeg $FFMPEG_VERSION for $TARGET: already built"
  echo "  $PREFIX"
  exit 0
fi
if [ -f "$CURRENT/.config-id" ] && [ "$(cat "$CURRENT/.config-id")" = "$CONFIG_ID" ]; then
  echo "LGPL ffmpeg $FFMPEG_VERSION for $TARGET: already built"
  echo "  $CURRENT"
  exit 0
fi
rm -rf "$PREFIX"

fetch() {
  local url="$1" file="$2" sha="$3"
  mkdir -p "$SRC_DIR"
  if [ ! -f "$file" ]; then
    echo "Downloading $(basename "$file")"
    # Every error retried: a TLS handshake cut off is not one curl counts
    # as transient, and on some networks it is the usual failure.
    curl -fsSL --retry 5 --retry-all-errors -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  local actual
  actual="$(sha 256 "$file" | cut -d' ' -f1)"
  if [ "$actual" != "$sha" ]; then
    echo "Checksum mismatch for $file: expected $sha, got $actual" >&2
    exit 1
  fi
}
fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" \
  "$SRC_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256"
fetch "https://zlib.net/fossils/zlib-$ZLIB_VERSION.tar.gz" \
  "$SRC_DIR/zlib-$ZLIB_VERSION.tar.gz" "$ZLIB_SHA256"

# Zig's compiler and archiver as ordinary commands, which is how configure
# scripts expect to find them.
TOOLS="$ROOT/tools"
mkdir -p "$TOOLS"
# ffmpeg's configure asks the MinGW linker for --pic-executable, which
# Zig's linker does not know and which a DLL build does not need: the cc
# wrapper drops it from any -Wl list.
cat > "$TOOLS/cc" <<WRAP
#!/bin/sh
for arg do
  shift
  case "\$arg" in
    -Wl,*--pic-executable*)
      arg="\$(printf '%s' "\$arg" | sed -e 's/--pic-executable,//' -e 's/,--pic-executable//' -e 's/^-Wl,--pic-executable\$//')"
      [ -z "\$arg" ] && continue ;;
  esac
  set -- "\$@" "\$arg"
done
exec zig cc -target $ZIG_TARGET "\$@"
WRAP
printf '#!/bin/sh\nexec zig c++ -target %s "$@"\n' "$ZIG_TARGET" > "$TOOLS/cxx"
printf '#!/bin/sh\nexec zig ar "$@"\n' > "$TOOLS/ar"
printf '#!/bin/sh\nexec zig ranlib "$@"\n' > "$TOOLS/ranlib"
# The MSVC import libraries (.lib) a Windows build makes beside each DLL,
# which is what an engine built with MSVC links against.
printf '#!/bin/sh\nexec zig dlltool "$@"\n' > "$TOOLS/dlltool"
chmod +x "$TOOLS"/*
JOBS="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

work="$ROOT/work"
rm -rf "$work"
mkdir -p "$work"

# zlib, static and position-independent, folded into libavcodec. Its own
# configure looks at the machine it runs on, not the target, so its sources
# are compiled directly: they are plain C, the same for every target.
echo "Building zlib $ZLIB_VERSION for $TARGET"
tar -xzf "$SRC_DIR/zlib-$ZLIB_VERSION.tar.gz" -C "$work"
(
  cd "$work/zlib-$ZLIB_VERSION"
  mkdir -p "$work/zlib/include" "$work/zlib/lib"
  for src in adler32 compress crc32 deflate gzclose gzlib gzread gzwrite infback \
    inffast inflate inftrees trees uncompr zutil; do
    "$TOOLS/cc" -O2 -fPIC -DHAVE_UNISTD_H -c "$src.c" -o "$src.o"
  done
  "$TOOLS/ar" rcs "$work/zlib/lib/libz.a" ./*.o
  cp zlib.h zconf.h "$work/zlib/include/"
) > "$work/zlib.log" 2>&1 || { tail -20 "$work/zlib.log" >&2; exit 1; }

echo "Building LGPL ffmpeg $FFMPEG_VERSION for $TARGET"
mkdir -p "$work/ffmpeg"
tar -xf "$SRC_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$work/ffmpeg" --strip-components=1
(
  cd "$work/ffmpeg"
  ./configure \
    --prefix="$PREFIX" \
    --enable-cross-compile \
    --target-os="$OS" \
    --arch="$ARCH" \
    --cc="$TOOLS/cc" \
    --cxx="$TOOLS/cxx" \
    --ar="$TOOLS/ar" \
    --ranlib="$TOOLS/ranlib" \
    --nm="nm" \
    --strip="true" \
    --pkg-config=false \
    --extra-cflags="-I$work/zlib/include" \
    --extra-ldflags="-L$work/zlib/lib" \
    ${ASM[@]+"${ASM[@]}"} \
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
    ${HWACCEL[@]+"${HWACCEL[@]}"} \
    > "$work/configure.log" 2>&1 || {
      tail -20 "$work/configure.log" >&2
      echo "configure failed; full log at $work/configure.log" >&2
      exit 1
    }
  if ! grep -q '^#define CONFIG_GPL 0' config.h || ! grep -q '^#define CONFIG_NONFREE 0' config.h; then
    echo "configure produced a GPL or nonfree build; refusing to continue" >&2
    exit 1
  fi
  # configure finds sysctl by linking it, and glibc still exports the symbol
  # for old programs, but the header is gone from current glibc and from
  # Zig's. Linux counts CPUs with sched_getaffinity, which ffmpeg uses too.
  if [ "$OS" = "linux" ]; then
    sed -i.bak 's/^#define HAVE_SYSCTL 1$/#define HAVE_SYSCTL 0/' config.h && rm -f config.h.bak
  fi
  if ! grep -q '^#define CONFIG_ZLIB 1' config.h; then
    echo "configure left out zlib, which PNG needs; refusing to continue" >&2
    exit 1
  fi
  if [ "$ARCH" = "x86_64" ] && ! grep -q '^#define HAVE_X86ASM 1' config.h; then
    echo "configure left out x86 assembly; refusing to continue" >&2
    exit 1
  fi
  # A Windows build that silently lost the hardware decoder, or the encoder
  # the editor's export needs, says nothing until a user waits or fails.
  if [ "$OS" = "mingw32" ]; then
    for component in H264_D3D11VA2_HWACCEL HEVC_D3D11VA2_HWACCEL H264_MF_ENCODER \
      HEVC_MF_ENCODER AAC_ENCODER MP4_MUXER ATEMPO_FILTER; do
      if ! grep -q "^#define CONFIG_$component 1" config_components.h; then
        echo "configure left out $component; refusing to continue" >&2
        exit 1
      fi
    done
  fi
  make -j"$JOBS" DLLTOOL="$TOOLS/dlltool" > "$work/make.log" 2>&1 || {
    tail -20 "$work/make.log" >&2
    echo "make failed; full log at $work/make.log" >&2
    exit 1
  }
  make install DLLTOOL="$TOOLS/dlltool" > /dev/null
)
rm -rf "$work"
# ffmpeg installs the MSVC import libraries beside the DLLs; a linker looks
# for them with the rest, in lib/.
if [ "$OS" = "mingw32" ]; then
  cp "$PREFIX"/bin/*.lib "$PREFIX/lib/"
fi

echo "$CONFIG_ID" > "$PREFIX/.config-id"
link_current

echo
echo "LGPL ffmpeg $FFMPEG_VERSION for $TARGET:"
echo "  $PREFIX  ($(du -sh "$PREFIX" | cut -f1))"
