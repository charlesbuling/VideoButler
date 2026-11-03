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

# Shared toolchain paths. Source this; do not run it.
#
# These live here rather than in a shell profile so a checkout builds the same
# way on any machine.

export JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@17}"
export ANDROID_HOME="${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"

# One NDK for everything: cargo-ndk compiles the engine with it and Gradle
# strips symbols with it. The version is pinned to match `ndkVersion` in
# app/android/app/build.gradle.kts; change both together.
VB_NDK_VERSION="29.0.14206865"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  if [ -d "$ANDROID_HOME/ndk/$VB_NDK_VERSION" ]; then
    export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/$VB_NDK_VERSION"
  elif [ -d "$ANDROID_HOME/ndk" ]; then
    # Fall back to the newest installed, which usually works but may not
    # match what Gradle expects.
    ANDROID_NDK_HOME="$(find "$ANDROID_HOME/ndk" -maxdepth 1 -mindepth 1 -type d | sort -V | tail -1)"
    export ANDROID_NDK_HOME
  fi
fi
[ -n "${ANDROID_NDK_HOME:-}" ] && export NDK_HOME="$ANDROID_NDK_HOME"

# Apple toolchain: prefer a full Xcode when one is installed, even if
# xcode-select still points at the Command Line Tools. Repointing that needs
# sudo; DEVELOPER_DIR overrides it for this process tree, which is all a build
# needs, and it leaves the machine's global setting alone.
#
# Only when its license has been accepted, though. An unlicensed Xcode fails
# every compile it touches -- including the Rust and Android builds the
# Command Line Tools handle perfectly well -- so pointing at it blindly breaks
# more than it fixes. `xcrun --show-sdk-path` exits 69 in that state.
if [ -z "${DEVELOPER_DIR:-}" ] && [ -d /Applications/Xcode.app/Contents/Developer ]; then
  if DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer" \
     xcrun --sdk macosx --show-sdk-path >/dev/null 2>&1; then
    export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
  fi
fi

export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$HOME/.cargo/bin:$PATH"

# Homebrew symlinks flutter, dart, ffmpeg and cargo-adjacent tools into its
# bin directory. Adding that directory is what makes this file work from a
# clean shell; searching the Caskroom does not, because the versioned SDK
# directory there is reached through a symlink that `find` will not descend.
for _brew_bin in /opt/homebrew/bin /usr/local/bin; do
  if [ -d "$_brew_bin" ]; then
    case ":$PATH:" in
      *":$_brew_bin:"*) ;;
      *) export PATH="$_brew_bin:$PATH" ;;
    esac
  fi
done
unset _brew_bin
# Windows (Git Bash): cargo wherever CARGO_HOME puts it, and Visual Studio's
# own CMake (whisper.cpp, llama.cpp) and libclang (their bindings) through
# vswhere, so a plain Git Bash builds as a Developer PowerShell does. The
# libclang is the "C++ Clang Compiler" component's; an LLVM install set in
# LIBCLANG_PATH wins.
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    if [ -n "${CARGO_HOME:-}" ]; then
      export PATH="$(cygpath -u "$CARGO_HOME")/bin:$PATH"
    fi
    _vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    if [ -x "$_vswhere" ]; then
      _vs="$("$_vswhere" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
        -property installationPath | tr -d '\r')"
      if [ -n "$_vs" ]; then
        _cmake="$(cygpath -u "$_vs")/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin"
        if ! command -v cmake > /dev/null && [ -x "$_cmake/cmake.exe" ]; then
          export PATH="$_cmake:$PATH"
        fi
        if [ -z "${LIBCLANG_PATH:-}" ] && [ -f "$_vs/VC/Tools/Llvm/x64/bin/libclang.dll" ]; then
          export LIBCLANG_PATH="$_vs\\VC\\Tools\\Llvm\\x64\\bin"
        fi
      fi
    fi
    unset _vswhere _vs _cmake
    # cmake-rs replaces CMake's own Release flags on MSVC with cc's, which
    # carry no optimisation (rust-lang/cmake-rs#240): whisper.cpp came out
    # unoptimised, 30 s of speech taking 124 s. llama-cpp-sys-2 puts the
    # flags back itself; whisper-rs-sys passes any CMAKE_* through. -EHsc
    # because the replaced flags dropped C++'s unwinding too. Written with
    # `-`, which cl takes as it takes `/`: Git Bash rewrites a leading
    # `/O2` into a path under its own install as it starts cargo. A build
    # made without these keeps them out until `cargo clean -p whisper-rs-sys`.
    export CMAKE_C_FLAGS_RELEASE="-O2 -Ob2 -DNDEBUG"
    export CMAKE_CXX_FLAGS_RELEASE="-O2 -Ob2 -DNDEBUG -EHsc"
    ;;
esac
