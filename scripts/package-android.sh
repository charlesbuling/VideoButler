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

# Package the Android app for a phone or tablet with no network, with every
# model the app can choose between.
#
# An APK cannot hold them: APK signing does not accept ZIP64, so an APK
# stops at 4 GB, and Qwen3-VL 8B alone is 5 GB. So the installer is a
# folder. The APK carries the engine, the model host (ONNX Runtime,
# llama.cpp, whisper.cpp) and the small models, and works offline by
# itself; beside it, `models` holds the rest -- the other four speech
# models, Qwen3-VL at 8B, 4B and 2B, and the composition judge when this Mac
# has it -- and an install script puts the APK and those models on a device
# over USB. The models go into the app's OBB folder, which is what Android
# keeps for an APK's large files: the app needs no permission to read it,
# it outlives an update and goes with an uninstall, and the engine reads it
# as it reads a Mac bundle's models (VB_BUNDLED_MODELS), in place -- not
# copied into the library, so nothing is stored twice.
#
# Without a computer, the `models` folder copied to the device is also a
# model pack for Models > Import models from a folder.
#
# Every catalogue file is checked against its SHA-256 before it goes in.
#
# Usage: scripts/package-android.sh              build the APK, then package
#        scripts/package-android.sh --no-build   package the APK already built
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

if [ "${1:-}" != "--no-build" ]; then
  ./scripts/build-android.sh release
fi

APK="app/build/app/outputs/flutter-apk/app-release.apk"
MODELS_DIR="${VB_MODELS_DIR:-$HOME/Library/Application Support/VideoButler/models}"
VERSION="$(sed -n 's/^version: *\([0-9.]*\).*/\1/p' app/pubspec.yaml)"
OUT="dist/VideoButler-$VERSION-android"

if [ ! -f "$APK" ]; then
  echo "No APK at $APK; build it first" >&2
  exit 1
fi

# id, file, sha256 -- every file of the catalogue in vb-pipeline/src/models.rs
# that the APK does not carry (build-android.sh's BUNDLED); a test there
# holds the two lists to the catalogue.
CATALOGUE=(
  "whisper-tiny ggml-tiny.bin be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
  "whisper-base ggml-base.bin 60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
  "whisper-small-q5 ggml-small-q5_1.bin ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"
  "whisper-small ggml-small.bin 1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
  "qwen3-vl-8b-q4 qwen3-vl-8b-instruct-q4_k_m.gguf 67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2"
  "qwen3-vl-8b-q4 qwen3-vl-8b-instruct-mmproj-f16.gguf ca524100ebf825c9a870db1c580d03879e0da0ab2541697e2458e64891cf9d38"
  "qwen3-vl-4b-q4 qwen3-vl-4b-instruct-q4_k_m.gguf 66358cb18bb6b3b1b6675aa412c7a88ef01d228f481184d13668e5201c730a0a"
  "qwen3-vl-4b-q4 qwen3-vl-4b-instruct-mmproj-f16.gguf 256f3a43bd4205ffef48d6b92715e1e70b5b0e9aef06522584967513a9985331"
  "qwen3-vl-2b-q4 qwen3-vl-2b-instruct-q4_k_m.gguf 089d75c52f4b7ffc56ba998ffc50aae89fcafc755f9e7208aacca281dca6c2ae"
  "qwen3-vl-2b-q4 qwen3-vl-2b-instruct-mmproj-f16.gguf c3d5afbef5287953acd57b4043d2269456e5761a4eaccb3b71b062996970aea5"
)
# Converted on the development side (scripts/convert-photoframer.sh), so
# there is no published digest; carried when this Mac has it. A 7B: the app
# runs it only on a device with the memory for it (12 GB).
LOCAL=(photoframer-7b-q4_k_m.gguf photoframer-7b-mmproj-f16.gguf)

# A copy on the same APFS volume is a clone: no time, no space.
copy() { cp -c "$1" "$2" 2>/dev/null || cp "$1" "$2"; }

echo
echo "Making the installer in $OUT"
rm -rf "$OUT"
mkdir -p "$OUT/models"
cp "$APK" "$OUT/VideoButler-$VERSION-android-arm64.apk"

echo "Checking and adding the models from $MODELS_DIR"
missing=()
for entry in "${CATALOGUE[@]}"; do
  read -r id name want <<< "$entry"
  if [ ! -f "$MODELS_DIR/$name" ]; then
    missing+=("$id")
    continue
  fi
  if [ "$(shasum -a 256 "$MODELS_DIR/$name" | cut -d' ' -f1)" != "$want" ]; then
    echo "  $name does not match the catalogue" >&2
    exit 1
  fi
  copy "$MODELS_DIR/$name" "$OUT/models/$name"
  echo "  $name"
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "Not in $MODELS_DIR; download them in the Mac app's Models page, or:" >&2
  for id in $(printf '%s\n' "${missing[@]}" | sort -u); do
    echo "  vb call models.download '{\"id\":\"$id\"}' --wait" >&2
  done
  exit 1
fi
local_files=0
for name in "${LOCAL[@]}"; do
  if [ -f "$MODELS_DIR/$name" ]; then
    copy "$MODELS_DIR/$name" "$OUT/models/$name"
    local_files=$((local_files + 1))
    echo "  $name"
  fi
done
# Half a local model would read as installed and fail when loaded.
if [ "$local_files" != "${#LOCAL[@]}" ]; then
  for name in "${LOCAL[@]}"; do rm -f "$OUT/models/$name"; done
  echo "  (no PhotoFramer on this Mac; the composition judge is left out)"
fi

# The install script, double-clicked in Finder (a .command opens in
# Terminal) or run with bash anywhere adb runs.
cat > "$OUT/安装到手机.command" <<'EOF'
#!/bin/bash
# Install Video Butler and every model on the Android device connected by
# USB: the APK, then the models into the app's OBB folder, where it reads
# them in place. A model already there at its full size is skipped, so this
# runs again after an interruption, or to update the app alone.
set -euo pipefail
cd "$(dirname "$0")"

PACKAGE=dev.videobutler.video_butler
OBB=/sdcard/Android/obb/$PACKAGE
DEST=$OBB/models

ADB=""
for candidate in "$(command -v adb || true)" \
    "${ANDROID_HOME:-}/platform-tools/adb" \
    "${ANDROID_SDK_ROOT:-}/platform-tools/adb" \
    "$HOME/Library/Android/sdk/platform-tools/adb" \
    /opt/homebrew/share/android-commandlinetools/platform-tools/adb \
    /opt/homebrew/bin/adb /usr/local/bin/adb; do
  if [ -n "$candidate" ] && [ -x "$candidate" ]; then
    ADB=$candidate
    break
  fi
done
if [ -z "$ADB" ]; then
  echo "没有找到 adb。在 Mac 上用 Homebrew 安装：brew install --cask android-platform-tools"
  echo "No adb found. On a Mac: brew install --cask android-platform-tools"
  exit 1
fi

devices=$("$ADB" devices | awk 'NR > 1 && $2 == "device" { print $1 }')
count=$(printf '%s' "$devices" | grep -c . || true)
if [ "$count" = 0 ]; then
  echo "没有连接的设备：打开「开发者选项 → USB 调试」，用数据线连上电脑，并在设备上点「允许」。"
  echo "No device: turn on USB debugging, connect it, and allow this computer on the device."
  exit 1
fi
if [ "$count" -gt 1 ] && [ -z "${ANDROID_SERIAL:-}" ]; then
  echo "连接了多台设备，用 ANDROID_SERIAL=<序列号> 指定一台 / More than one device; name one with ANDROID_SERIAL:"
  echo "$devices"
  exit 1
fi
device() { "$ADB" shell "$@" | tr -d '\r'; }
echo "设备 / Device: $(device getprop ro.product.model)，Android $(device getprop ro.build.version.release)"

echo
echo "安装 App / Installing the app"
apk=$(ls VideoButler-*-android-arm64.apk)
if ! "$ADB" install -r "$apk"; then
  echo "安装失败。如果提示签名不一致，先在设备上卸载旧的 Video Butler 再运行一次。"
  echo "Install failed. If the signatures differ, uninstall the old app first and run this again."
  exit 1
fi

# The app makes its models folder itself when it starts, so that the folder
# is its own and it can read what goes in.
device am start -n "$PACKAGE/.MainActivity" >/dev/null
made() { [ "$(device "[ -d $DEST ] && echo yes" || true)" = "yes" ]; }
for _ in $(seq 1 30); do
  made && break
  sleep 1
done
if ! made; then
  echo "App 没有建好它的模型目录（$DEST）。在设备上打开一次 Video Butler，再运行这个脚本。"
  echo "The app has not made its models folder ($DEST). Open Video Butler once on the device and run this again."
  exit 1
fi

size_of() { wc -c < "$1" | tr -d ' '; }
gb() { awk -v k="$1" 'BEGIN { printf "%.1f", k / 1048576 }'; }
need=0
for file in models/*; do
  name=$(basename "$file")
  there=$(device "stat -c %s '$DEST/$name' 2>/dev/null" || true)
  [ "$there" = "$(size_of "$file")" ] || need=$((need + $(size_of "$file") / 1024))
done
free=$(device df -k /sdcard | awk 'NR == 2 { print $4 }')
if [ -n "$free" ] && [ "$need" -gt "$free" ]; then
  echo "设备空间不够：还要 $(gb "$need") GB，只剩 $(gb "$free") GB。"
  echo "Not enough space on the device: $(gb "$need") GB needed, $(gb "$free") GB free."
  exit 1
fi

echo
echo "拷入模型 / Copying the models ($(gb "$need") GB)"
for file in models/*; do
  name=$(basename "$file")
  size=$(size_of "$file")
  there=$(device "stat -c %s '$DEST/$name' 2>/dev/null" || true)
  if [ "$there" = "$size" ]; then
    echo "  已有 / already there: $name"
    continue
  fi
  echo "  $name ($((size / 1000000)) MB)"
  # Under another name until it is whole: a model is installed when its
  # file is there at full size, and a cut-off copy must never look it.
  "$ADB" push "$file" "$DEST/$name.part" >/dev/null
  device mv "$DEST/$name.part" "$DEST/$name"
done

echo
echo "完成。打开 Video Butler →「模型」，每个模型都显示「已安装」；每类默认用这台设备跑得动的最强的一个，点「使用」可以换。"
echo "Done. Video Butler > Models shows every model installed; the most capable this device can run is used, and Use switches."
EOF
chmod +x "$OUT/安装到手机.command"

cat > "$OUT/请先读我.txt" <<EOF
Video Butler $VERSION 安卓安装包（离线用）

这个文件夹里有：
  VideoButler-$VERSION-android-arm64.apk
      App 本身。自带景深、人脸、人像、人体姿态、场景标签模型和语音
      Whisper 基础版（量化），只装它也能离线分析、卡点剪辑、挑出跳舞画面。
  models
      其余所有模型：语音 Whisper 微型、基础版、小型、小型（量化），
      字幕 Qwen3-VL 8B / 4B / 2B，构图评分 PhotoFramer。
  安装到手机.command
      一键安装：App 和 models 里的全部模型。

为什么模型不在 APK 里：一个 APK 最大只能 4GB，而 8B 字幕模型一个文件就有
5GB。所以模型放在 APK 旁边，由安装脚本直接放进 App 自己的模型目录
（Android/obb/dev.videobutler.video_butler/models）。App 原地读取，不需要
在 App 里导入，也不会存两份；卸载 App 时一起删除。

方法一：用电脑一键安装（推荐）
1. 在设备上打开「设置 → 开发者选项 → USB 调试」，用数据线连上 Mac，
   设备上弹出提示时点「允许」。
2. 双击「安装到手机.command」。它会安装（或更新）App，再把 models 里的
   模型拷进去；已经拷过的会跳过，中途断了再运行一次即可。
   一共约 $(du -sh "$OUT/models" | awk '{ print $1 }')B，USB 3 线几分钟。
3. 打开 Video Butler →「模型」：每个模型都显示「已安装」「随安装包提供」。
   每类默认用这台设备跑得动的最强的一个，点「使用」可以换成别的一档。
需要 adb：brew install --cask android-platform-tools。Windows 或 Linux 上
用 Git Bash / 终端运行：bash 安装到手机.command

方法二：不用电脑命令行
1. 把 APK 拷到设备上安装。
2. 把 models 文件夹拷到设备的「下载」（Download）里。
3. 打开 Video Butler →「模型」→「从文件夹导入模型」→「选择文件夹」，
   选中它，点「使用此文件夹」并允许访问。这样会复制一份进 App，字幕模型
   只导入这台设备跑得动的最大一档；导入完，「下载」里的那份可以删掉。

内存（按设备标称的运行内存算）：12GB 及以上能跑字幕 8B 和构图评分
PhotoFramer，8GB 跑 4B，6GB 跑 2B。「内存扩展」「虚拟内存」不算在内：
它是拿存储空间当内存用，模型写每个字都要把全部权重读一遍，必须放在真内存里。
跑不动的模型在「模型」页标着「这台设备内存不够」，不会被用到。
需要 64 位（arm64）安卓设备，Android 7.0 或更新。
EOF

echo
echo "Installer: $OUT ($(du -sh "$OUT" | cut -f1))"
echo "  VideoButler-$VERSION-android-arm64.apk ($(du -h "$OUT/VideoButler-$VERSION-android-arm64.apk" | cut -f1))"
echo "  models ($(du -sh "$OUT/models" | cut -f1)), 安装到手机.command, 请先读我.txt"
