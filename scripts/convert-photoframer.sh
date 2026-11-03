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

# Make the composition judge the app runs: PhotoFramer, as a quantised GGUF.
#
# PhotoFramer (CVPR 2026, MIT) is published as full-precision weights for
# PyTorch. The app runs it through llama.cpp, which wants GGUF, so this
# converts it on the development side -- the app never runs Python -- and
# places the two files in the library under the names vb-pipeline's
# `models::LOCAL` expects. Until the converted files are hosted, this is how
# a machine gets the judge.
#
# Everything is pinned: the weights by digest, llama.cpp at the commit
# llama-cpp-sys-2 vendors (so the GGUF matches the engine that reads it), and
# transformers at the version that llama.cpp's converter asks for -- 5.x
# rejects the model's config, and its vision config only converts with the
# 4.x defaults.
#
# Needs python3, cmake and about 40 GB free while it works.
#
# Usage: scripts/convert-photoframer.sh [library-models-dir]
#        VB_HF=https://huggingface.co scripts/convert-photoframer.sh   (no mirror)
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
source scripts/env.sh

MODELS="${1:-$HOME/Library/Application Support/VideoButler/models}"
WORK="$(pwd)/build/photoframer"
HF="${VB_HF:-https://hf-mirror.com}"
REPO="zhiyuanyou/Qwen2.5-VL-7B-GRPO-Composition-Score-Class"
LLAMA_COMMIT="e79e4bf660e19f2ad851e06c6913f7a8c5852621"
mkdir -p "$WORK/weights" "$MODELS"

# --- the weights, resumed across dropped connections -------------------------
get() { # file bytes sha256(optional)
  local file=$1 bytes=$2 sha=${3:-} dest="$WORK/weights/$1"
  for _ in $(seq 1 40); do
    [ -f "$dest" ] && [ "$(stat -f %z "$dest")" = "$bytes" ] && break
    [ -f "$dest" ] && [ "$(stat -f %z "$dest")" -gt "$bytes" ] && rm -f "$dest"
    curl -sSL --http1.1 -C - --connect-timeout 20 -o "$dest" "$HF/$REPO/resolve/main/$file" || true
  done
  [ "$(stat -f %z "$dest")" = "$bytes" ] || { echo "$file did not arrive whole" >&2; exit 1; }
  if [ -n "$sha" ] && [ "$(shasum -a 256 "$dest" | cut -d' ' -f1)" != "$sha" ]; then
    echo "$file: wrong sha256" >&2; exit 1
  fi
}
echo "Fetching PhotoFramer's weights"
get added_tokens.json 605
get chat_template.json 1049
get config.json 1300
get generation_config.json 281
get merges.txt 1671853
get model.safetensors.index.json 57619
get preprocessor_config.json 575
get special_tokens_map.json 613
get tokenizer_config.json 7404
get vocab.json 2776833
get tokenizer.json 11422063 5eee858c5123a4279c3e1f7b81247343f356ac767940b2692a928ad929543214
get model-00001-of-00004.safetensors 4968243304 63fcdf3fc89c4c2b854dd6d6fd71e1f1b02d5ea889c4832762092b76e366df54
get model-00002-of-00004.safetensors 4991495816 aa6024057db766bc75fad76aa67c68d53ee8e9848d1a0d62d1622c4068b3f3d8
get model-00003-of-00004.safetensors 4932751040 aa119476316b2d88816acb5a5e5b7b89c553ee81571f4531092f890c78c90020
get model-00004-of-00004.safetensors 1691924384 74141bfb2bf944ee2e635dd4b5d4c3ca5486b85a330f5c6ba99bce75e77ba8b9

# --- llama.cpp at the vendored commit: its converter and its quantiser -------
if [ ! -d "$WORK/llama.cpp" ]; then
  echo "Fetching llama.cpp at $LLAMA_COMMIT"
  for _ in $(seq 1 10); do
    curl -sSL --http1.1 -o "$WORK/llama.tgz" \
      "https://codeload.github.com/ggml-org/llama.cpp/tar.gz/$LLAMA_COMMIT" || true
    tar -tzf "$WORK/llama.tgz" >/dev/null 2>&1 && break
  done
  mkdir -p "$WORK/llama.cpp"
  tar -xzf "$WORK/llama.tgz" -C "$WORK/llama.cpp" --strip-components 1
fi
if [ ! -x "$WORK/llama.cpp/build/bin/llama-quantize" ]; then
  cmake -S "$WORK/llama.cpp" -B "$WORK/llama.cpp/build" -DCMAKE_BUILD_TYPE=Release \
    -DLLAMA_CURL=OFF -DLLAMA_BUILD_SERVER=OFF -DLLAMA_BUILD_TESTS=OFF >/dev/null
  cmake --build "$WORK/llama.cpp/build" --target llama-quantize -j 8 >/dev/null
fi

# --- the converter's Python, with the transformers it was written for --------
if [ ! -x "$WORK/venv/bin/python" ]; then
  python3 -m venv "$WORK/venv"
  "$WORK/venv/bin/pip" install -q --retries 10 --timeout 60 \
    numpy torch safetensors sentencepiece protobuf "transformers==4.57.6"
fi

echo "Converting to GGUF"
PY="$WORK/venv/bin/python"
"$PY" "$WORK/llama.cpp/convert_hf_to_gguf.py" "$WORK/weights" --outtype f16 \
  --outfile "$WORK/photoframer-7b-f16.gguf"
"$PY" "$WORK/llama.cpp/convert_hf_to_gguf.py" "$WORK/weights" --mmproj --outtype f16 \
  --outfile "$WORK/photoframer-7b-mmproj-f16.gguf"

echo "Quantising to Q4_K_M"
"$WORK/llama.cpp/build/bin/llama-quantize" "$WORK/photoframer-7b-f16.gguf" \
  "$WORK/photoframer-7b-q4_k_m.gguf" Q4_K_M >/dev/null

cp "$WORK/photoframer-7b-q4_k_m.gguf" "$WORK/photoframer-7b-mmproj-f16.gguf" "$MODELS/"
echo "PhotoFramer is in $MODELS:"
ls -la "$MODELS"/photoframer-7b-*.gguf
