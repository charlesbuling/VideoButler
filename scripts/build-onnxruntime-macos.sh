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

# Build ONNX Runtime for macOS, the engine the model host runs vision models on.
#
# From source, because every prebuilt Microsoft publishes needs a newer macOS
# than the app supports: 1.20 through 1.23 want 13.3 or later, 1.24 and after
# want 14. Built here at the app's own deployment target, with the CoreML
# execution provider, it loads wherever the app does. MIT, like the release.
#
# The engine loads the library at runtime rather than linking it, so the Rust
# build never downloads anything: `ort`'s default feature fetches binaries
# through ureq at build time, which deny.toml bans outside vb-cloud.
#
# The library lands in build/onnxruntime-macos/<version>/lib, with a stable
# build/onnxruntime-macos/current link. scripts/build-macos-native.sh runs this
# and copies the library into the app's Frameworks directory, where the engine
# finds it; a checkout's tests find it through the same link.
#
# ONNX Runtime's build fetches its own dependencies from GitHub. On a network
# that drops connections, run this again: what arrived is kept.
#
# Usage: scripts/build-onnxruntime-macos.sh          build it (skipped if built)
#        scripts/build-onnxruntime-macos.sh --env    print the path, for callers
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

ORT_VERSION="1.30.0"
# GitHub's archive of the v1.30.0 tag.
ORT_SHA256="f6681ecbddf53898adf0cc9e8e9e84657485b84d2eca3c8aa353de6d7dd417ef"

# The oldest macOS the app supports, read from the Xcode project so the
# library, the engine and the app cannot disagree.
DEPLOYMENT_TARGET="$(sed -n 's/.*MACOSX_DEPLOYMENT_TARGET = \([0-9.]*\);.*/\1/p' \
  app/macos/Runner.xcodeproj/project.pbxproj | head -1)"
ARCH="$(uname -m)"

ROOT="$(pwd)/build/onnxruntime-macos"
SRC_ROOT="$(pwd)/build/onnxruntime-src"
TARBALL="$SRC_ROOT/ort-$ORT_VERSION.tgz"
SRC="$SRC_ROOT/onnxruntime-$ORT_VERSION"
PREFIX="$ROOT/$ORT_VERSION"
CURRENT="$ROOT/current"
DYLIB="$CURRENT/lib/libonnxruntime.dylib"

if [ "${1:-}" = "--env" ]; then
  echo "export VB_ORT_DYLIB=$DYLIB"
  exit 0
fi

# Built already, for this deployment target: nothing to do.
if [ -e "$PREFIX/lib/libonnxruntime.dylib" ]; then
  built="$(otool -l "$PREFIX/lib/libonnxruntime.dylib" | awk '/minos/ {print $2; exit}')"
  if [ "$built" = "$DEPLOYMENT_TARGET" ]; then
    ln -sfn "$ORT_VERSION" "$CURRENT"
    echo "ONNX Runtime $ORT_VERSION for macOS $built: $DYLIB"
    exit 0
  fi
fi

mkdir -p "$SRC_ROOT"
if [ ! -d "$SRC" ]; then
  if [ ! -f "$TARBALL" ] || [ "$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)" != "$ORT_SHA256" ]; then
    # A generated archive: no byte ranges, so each attempt fetches it whole.
    for _ in $(seq 1 10); do
      curl -sSL --http1.1 -o "$TARBALL" \
        "https://codeload.github.com/microsoft/onnxruntime/tar.gz/refs/tags/v$ORT_VERSION" || true
      [ "$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)" = "$ORT_SHA256" ] && break
    done
  fi
  got="$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)"
  if [ "$got" != "$ORT_SHA256" ]; then
    echo "$TARBALL: sha256 $got, expected $ORT_SHA256" >&2
    exit 1
  fi
  tar -xzf "$TARBALL" -C "$SRC_ROOT"
fi

# ONNX Runtime's CMake downloads its dependencies itself, once each, and gives
# up on the first dropped connection. Fetch them here instead, retrying, check
# each against the SHA-1 in its deps.txt, and point deps.txt at the local
# copies. Only what a macOS CoreML build uses: nothing for CUDA, DirectX,
# WebGPU, Windows or telemetry.
DEPS="$SRC_ROOT/deps"
mkdir -p "$DEPS"
python3 - "$SRC/cmake/deps.txt" "$DEPS" <<'EOF'
import hashlib, os, subprocess, sys, time
deps_txt, out = sys.argv[1], sys.argv[2]
skip = ('cutlass', 'deep_gemm', 'dawn', 'directx', 'cudnn', 'onnx_tensorrt', 'telemetry',
        'curl', 'mbedtls', 'protoc_win', 'protoc_linux', 'tensorboard', 'vulkan', 'pybind11',
        'microsoft_wil', 'googletest', 'google_benchmark', 'googlexnnpack', 'mimalloc',
        'extensions', 'kleidiai-qmx')
lines = open(deps_txt).read().splitlines()
rewritten = []
for line in lines:
    parts = line.split(';')
    if line.startswith('#') or len(parts) < 3 or parts[1].startswith('file://') \
            or any(parts[0].startswith(s) for s in skip):
        rewritten.append(line)
        continue
    name, url, sha1 = parts[0], parts[1], parts[2]
    dest = os.path.join(out, name + '-' + os.path.basename(url.split('?')[0]))
    def ok():
        return os.path.exists(dest) and hashlib.sha1(open(dest, 'rb').read()).hexdigest() == sha1
    for attempt in range(12):
        if ok():
            break
        subprocess.run(['curl', '-sSL', '--http1.1', '-m', '600', '-o', dest, url])
        time.sleep(1)
    if not ok():
        sys.exit(f'{name}: could not fetch {url} intact; run the script again')
    print(f'  {name}')
    rewritten.append(';'.join([name, 'file://' + dest, sha1] + parts[3:]))
open(deps_txt, 'w').write('\n'.join(rewritten) + '\n')
EOF

echo "Building ONNX Runtime $ORT_VERSION for macOS $DEPLOYMENT_TARGET ($ARCH)"
# CMake 4 refuses projects that still declare support for CMake before 3.5,
# which several of ONNX Runtime's dependencies do; the policy floor lets them
# configure without editing them. Its own unit tests are not built: they are
# ONNX Runtime's to run, and they would pull in googletest. The deployment
# target goes to CMake directly: build.py's --apple_deploy_target reaches
# only its iOS builds, and without this the library is marked for this Mac's
# own macOS.
python3 "$SRC/tools/ci_build/build.py" \
  --build_dir "$SRC/build/macos-$DEPLOYMENT_TARGET" \
  --config Release \
  --build_shared_lib \
  --parallel \
  --skip_tests \
  --compile_no_warning_as_error \
  --osx_arch "$ARCH" \
  --apple_deploy_target "$DEPLOYMENT_TARGET" \
  --use_coreml \
  --cmake_extra_defines CMAKE_POLICY_VERSION_MINIMUM=3.5 onnxruntime_BUILD_UNIT_TESTS=OFF \
    CMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET"

OUT="$SRC/build/macos-$DEPLOYMENT_TARGET/Release"
real="$(find "$OUT" -maxdepth 1 -name 'libonnxruntime.*.*.*.dylib' | head -1)"
if [ -z "$real" ]; then
  echo "the build produced no libonnxruntime; see its output above" >&2
  exit 1
fi
built="$(otool -l "$real" | awk '/minos/ {print $2; exit}')"
if [ "$built" != "$DEPLOYMENT_TARGET" ]; then
  echo "built for macOS $built, not $DEPLOYMENT_TARGET" >&2
  exit 1
fi

rm -rf "$PREFIX"
mkdir -p "$PREFIX/lib"
cp "$real" "$PREFIX/lib/"
name="$(basename "$real")"
# The release's names: the versioned file, and the two links to it that the
# install name and the engine use.
ln -sfn "$name" "$PREFIX/lib/libonnxruntime.1.dylib"
ln -sfn "$name" "$PREFIX/lib/libonnxruntime.dylib"
cp "$SRC/LICENSE" "$SRC/ThirdPartyNotices.txt" "$PREFIX/"
ln -sfn "$ORT_VERSION" "$CURRENT"

echo "ONNX Runtime $ORT_VERSION for macOS $built: $DYLIB"
