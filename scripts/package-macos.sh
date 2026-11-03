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

# Package the macOS app with every model inside it, as one disk image that
# installs and runs on a Mac with no network.
#
# Every model the app can choose between goes in: all five speech models,
# Qwen3-VL at 8B, 4B and 2B, the director's and the pose model, and the
# composition judge when this Mac has it. The app runs the most capable of
# each and the Models page switches to another, so a test build compares
# them without downloading anything. Only SigLIP 2 stays out: on a Mac
# Apple Vision names scenes, and it would never run.
#
# The models go into the bundle's Contents/Resources/models, where the engine
# finds them after its own library (vb-pipeline::models::bundled_dir), read
# only: nothing is copied out on first launch, and deleting the app deletes
# them. They are taken from this Mac's library and every catalogue file is
# checked against its SHA-256 first, so a half-finished download never goes
# into an installer.
#
# The app is signed ad hoc, as the build signs it. With no Developer ID it is
# not notarised, so a Mac opens it the first time through System Settings >
# Privacy & Security > Open Anyway (or right-click > Open); the image's
# instructions say so.
#
# Beside it goes the app alone (`-app.dmg`, about a hundred megabytes), for
# a Mac whose models are in its library already -- downloaded there, or
# imported once from the model pack (`--models`, the models folder alone,
# for Models > Import models from a folder). An update is then the small
# image, not sixteen gigabytes again. A Mac that installed the full image
# has its models inside that app: import the pack before switching it to
# the small one.
#
# Signing: ad hoc, unless VB_SIGN_IDENTITY names a "Developer ID
# Application" identity in this Mac's keychain -- then every binary is
# signed inside out with the hardened runtime and a secure timestamp, and
# with VB_NOTARY_PROFILE (an `xcrun notarytool store-credentials` profile)
# each image is notarised and stapled, so a Mac opens it with no detour
# through Privacy & Security.
#
# Usage: scripts/package-macos.sh              build the app, then package
#        scripts/package-macos.sh --no-build   package the app already built
#        ... --app-only                        the small image only
#        ... --models                          also the model pack
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

BUILD=1
FULL=1
PACK=0
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=0 ;;
    --app-only) FULL=0 ;;
    --models) PACK=1 ;;
    *) echo "unknown option $arg" >&2; exit 2 ;;
  esac
done
if [ "$BUILD" = 1 ]; then
  ./scripts/build-macos-native.sh
fi

APP="app/build/macos/Build/Products/Release/video_butler.app"
MODELS_DIR="${VB_MODELS_DIR:-$HOME/Library/Application Support/VideoButler/models}"
VERSION="$(sed -n 's/^version: *\([0-9.]*\).*/\1/p' app/pubspec.yaml)"
OUT="dist"
STAGE="build/package/macos"
IMAGE="$OUT/VideoButler-$VERSION-macOS-arm64.dmg"
APP_IMAGE="$OUT/VideoButler-$VERSION-macOS-arm64-app.dmg"
PACK_IMAGE="$OUT/VideoButler-models-macOS.dmg"
ENTITLEMENTS="app/macos/Runner/Release.entitlements"

if [ ! -d "$APP" ]; then
  echo "No app at $APP; build it first" >&2
  exit 1
fi

# id, file, sha256 -- every file of the catalogue in vb-pipeline/src/models.rs
# but SigLIP 2's; a test there holds this list to the catalogue.
CATALOGUE=(
  "whisper-tiny ggml-tiny.bin be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
  "whisper-base-q5 ggml-base-q5_1.bin 422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898"
  "whisper-base ggml-base.bin 60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
  "whisper-small-q5 ggml-small-q5_1.bin ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"
  "whisper-small ggml-small.bin 1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
  "depth-anything-v2-small depth-anything-v2-small.onnx afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c"
  "yunet-2023mar face_detection_yunet_2023mar.onnx 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
  "pphumanseg-2023mar human_segmentation_pphumanseg_2023mar.onnx 552d8a984054e59b5d773d24b9b12022b22046ceb2bbc4c9aaeaceb36a9ddf24"
  "rtmo-s rtmo-s.onnx 1cd6a3517658903233e9ccb78666554862f57f0944b6886e2048093594aef4e7"
  "auraface-v1 auraface-v1-glintr100.onnx a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60"
  "qwen3-vl-8b-q4 qwen3-vl-8b-instruct-q4_k_m.gguf 67d1659bfe71b89d50b45a4ad1a9e5b997e5bb16ce5da66a6a6167abd569e9e2"
  "qwen3-vl-8b-q4 qwen3-vl-8b-instruct-mmproj-f16.gguf ca524100ebf825c9a870db1c580d03879e0da0ab2541697e2458e64891cf9d38"
  "qwen3-vl-4b-q4 qwen3-vl-4b-instruct-q4_k_m.gguf 66358cb18bb6b3b1b6675aa412c7a88ef01d228f481184d13668e5201c730a0a"
  "qwen3-vl-4b-q4 qwen3-vl-4b-instruct-mmproj-f16.gguf 256f3a43bd4205ffef48d6b92715e1e70b5b0e9aef06522584967513a9985331"
  "qwen3-vl-2b-q4 qwen3-vl-2b-instruct-q4_k_m.gguf 089d75c52f4b7ffc56ba998ffc50aae89fcafc755f9e7208aacca281dca6c2ae"
  "qwen3-vl-2b-q4 qwen3-vl-2b-instruct-mmproj-f16.gguf c3d5afbef5287953acd57b4043d2269456e5761a4eaccb3b71b062996970aea5"
)
# Converted on the development side (scripts/convert-photoframer.sh), so
# there is no published digest; carried when this Mac has it.
LOCAL=(photoframer-7b-q4_k_m.gguf photoframer-7b-mmproj-f16.gguf)

# A copy on the same APFS volume is a clone: no time, no space.
copy() { cp -c "$1" "$2" 2>/dev/null || cp "$1" "$2"; }

# Sign an app: inside out with a Developer ID, else ad hoc as the build did.
sign_app() {
  local app="$1"
  if [ -n "${VB_SIGN_IDENTITY:-}" ]; then
    local id="$VB_SIGN_IDENTITY"
    # Every library and nested bundle first: a seal is only valid over
    # signed parts.
    find "$app/Contents/Frameworks" -maxdepth 1 -type f -name '*.dylib' -print0 |
      while IFS= read -r -d '' lib; do
        codesign --force --options runtime --timestamp --sign "$id" "$lib"
      done
    find "$app/Contents/Frameworks" -maxdepth 1 -type d -name '*.framework' -print0 |
      while IFS= read -r -d '' framework; do
        codesign --force --options runtime --timestamp --sign "$id" "$framework"
      done
    codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
      --sign "$id" "$app"
  else
    codesign --force --deep --sign - "$app"
  fi
  codesign --verify --deep --strict "$app"
}

# Notarise and staple an image, when there is a Developer ID to do it with.
notarise() {
  local image="$1"
  if [ -n "${VB_SIGN_IDENTITY:-}" ] && [ -n "${VB_NOTARY_PROFILE:-}" ]; then
    codesign --force --timestamp --sign "$VB_SIGN_IDENTITY" "$image"
    echo "Notarising $image (Apple's service; minutes for a small image, long for a large one)"
    xcrun notarytool submit "$image" --keychain-profile "$VB_NOTARY_PROFILE" --wait
    xcrun stapler staple "$image"
  fi
}

# How an image's notes say to open the app, as signing left it.
if [ -n "${VB_SIGN_IDENTITY:-}" ] && [ -n "${VB_NOTARY_PROFILE:-}" ]; then
  OPEN_ZH="2. 这个 App 经过苹果公证，双击即可打开。"
  OPEN_EN="2. The app is notarised: double-click it to open."
else
  OPEN_ZH="2. 第一次打开：这个 App 没有经过苹果公证，双击会被拦下。请打开
   「系统设置 → 隐私与安全性」，在底部点「仍要打开」；或在「应用程序」里
   右键点它，选「打开」。之后就能正常双击打开。"
  OPEN_EN="2. The app is not notarised: the first time, open System Settings > Privacy &
   Security and click Open Anyway, or right-click it in Applications and
   choose Open."
fi

# Every catalogue model into a folder, each checked against its SHA-256;
# PhotoFramer when this Mac has both its files.
stage_models() {
  local dest="$1"
  mkdir -p "$dest"
  echo "Checking and adding the models from $MODELS_DIR"
  local missing=()
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
    copy "$MODELS_DIR/$name" "$dest/$name"
    echo "  $name"
  done
  if [ ${#missing[@]} -gt 0 ]; then
    echo "Not in $MODELS_DIR; download them in the app's Models page, or:" >&2
    for id in $(printf '%s\n' "${missing[@]}" | sort -u); do
      echo "  vb call models.download '{\"id\":\"$id\"}' --wait" >&2
    done
    exit 1
  fi
  local local_files=0
  for name in "${LOCAL[@]}"; do
    if [ -f "$MODELS_DIR/$name" ]; then
      copy "$MODELS_DIR/$name" "$dest/$name"
      local_files=$((local_files + 1))
      echo "  $name"
    fi
  done
  # Half a local model would read as installed and fail when loaded.
  if [ "$local_files" != "${#LOCAL[@]}" ]; then
    for name in "${LOCAL[@]}"; do rm -f "$dest/$name"; done
    echo "  (no PhotoFramer on this Mac; the composition judge is left out)"
  fi
}

made=()

# --- the app with every model inside ---------------------------------------
if [ "$FULL" = 1 ]; then
  echo "Staging the app with every model"
  rm -rf "$STAGE"
  mkdir -p "$STAGE" "$OUT"
  ditto "$APP" "$STAGE/Video Butler.app"
  stage_models "$STAGE/Video Butler.app/Contents/Resources/models"
  # The bundle changed, so its signature is out of date.
  sign_app "$STAGE/Video Butler.app"
  ln -s /Applications "$STAGE/Applications"
  cat > "$STAGE/安装说明.txt" <<NOTES
Video Butler（macOS，Apple 芯片）离线安装

1. 把「Video Butler」拖到「Applications」（应用程序）。
$OPEN_ZH
3. 所有模型都已经在 App 里面，不需要联网，也不需要下载或导入：
   语音 Whisper 五档（微型、基础版、基础版量化、小型、小型量化），
   景深、人脸、人像、人体姿态，字幕 Qwen3-VL 8B / 4B / 2B，
   构图评分 PhotoFramer。每类默认用最强的一个；在「模型」页点「使用」
   可以换成别的一档（例如换小一点的字幕模型，速度更快）。
4. 以后更新：模型已经在「资源库」里的 Mac，只需要装一百兆左右的精简安装包
   （文件名以 -app.dmg 结尾）。用这个完整安装包装的 Mac，模型在这个 App
   里面——换成精简版之前，先在「模型」页「从文件夹导入模型」导入一次模型包。

需要 macOS 12 或更新、Apple 芯片（M1 及以后），建议 16GB 以上内存。

Video Butler for macOS (Apple silicon), offline install

1. Drag Video Butler onto Applications.
$OPEN_EN
3. Every model is inside the app; nothing is downloaded or imported:
   five Whisper speech models, depth, faces, people, body pose, Qwen3-VL
   8B / 4B / 2B for captions, and the PhotoFramer composition judge. The
   most capable of each is used; Models > Use switches to another.
4. Later updates: a Mac with its models in the library needs only the small
   image (ending -app.dmg). This full app keeps its models inside itself:
   import the model pack (Models > Import models from a folder) before
   switching to the small one.

Needs macOS 12 or later on Apple silicon; 16 GB of memory recommended.
NOTES
  echo "Making the disk image (several minutes: the models are gigabytes)"
  rm -f "$IMAGE"
  hdiutil create -volname "Video Butler" -srcfolder "$STAGE" -fs HFS+ -format ULFO "$IMAGE" >/dev/null
  rm -rf "$STAGE"
  notarise "$IMAGE"
  made+=("$IMAGE")
fi

# --- the app alone: an update in a hundred megabytes ------------------------
echo "Staging the app alone"
rm -rf "$STAGE"
mkdir -p "$STAGE" "$OUT"
ditto "$APP" "$STAGE/Video Butler.app"
sign_app "$STAGE/Video Butler.app"
ln -s /Applications "$STAGE/Applications"
cat > "$STAGE/安装说明.txt" <<NOTES
Video Butler（macOS，Apple 芯片）精简安装包：只有 App，不带模型

1. 把「Video Butler」拖到「Applications」（应用程序），替换旧版本。
$OPEN_ZH
3. 模型从这台 Mac 的资源库读取（~/Library/Application Support/VideoButler/models）：
   在 App 里下载过、或用「模型」页「从文件夹导入模型」导入过模型包的 Mac，
   直接就能用。之前用完整安装包装的 Mac，模型在旧 App 里面，替换前请先导入
   一次模型包（VideoButler-models-macOS.dmg 里的 models 文件夹）。
   没有模型也能剪：「模型」页会写清楚缺了什么，成片里也会说明哪些没用上。

Video Butler for macOS (Apple silicon), the app alone, no models

1. Drag Video Butler onto Applications, replacing the old one.
$OPEN_EN
3. The models are read from this Mac's library
   (~/Library/Application Support/VideoButler/models): downloaded in the app,
   or imported from the model pack (Models > Import models from a folder).
   A Mac that installed the full image has its models inside that app:
   import the pack first. Without models a cut is still made, and says
   what it went without.
NOTES
rm -f "$APP_IMAGE"
hdiutil create -volname "Video Butler" -srcfolder "$STAGE" -fs HFS+ -format ULFO "$APP_IMAGE" >/dev/null
rm -rf "$STAGE"
notarise "$APP_IMAGE"
made+=("$APP_IMAGE")

# --- the model pack -----------------------------------------------------------
if [ "$PACK" = 1 ]; then
  echo "Staging the model pack"
  rm -rf "$STAGE"
  mkdir -p "$STAGE" "$OUT"
  stage_models "$STAGE/models"
  cat > "$STAGE/请先读我.txt" <<'NOTES'
Video Butler 模型包（macOS）

在 App 的「模型」页点「从文件夹导入模型」，选这里的 models 文件夹。每个文件
都会核对大小和校验值；导入资源库以后，更新 App 只需要一百兆左右的精简安装包。

Video Butler model pack (macOS)

In the app, Models > Import models from a folder, and choose the models folder
here. Each file is checked by size and SHA-256; once in the library, later
updates need only the small app image.
NOTES
  echo "Making the model pack (several minutes)"
  rm -f "$PACK_IMAGE"
  hdiutil create -volname "Video Butler Models" -srcfolder "$STAGE" -fs HFS+ -format ULFO "$PACK_IMAGE" >/dev/null
  rm -rf "$STAGE"
  made+=("$PACK_IMAGE")
fi

echo
for image in "${made[@]}"; do
  echo "Installer: $image ($(du -h "$image" | cut -f1))"
done
