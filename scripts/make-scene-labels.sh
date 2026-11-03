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

# Make the text side of the SigLIP 2 scene labeller: one embedding per row of
# core/vb-onnx/data/scene-labels.tsv, written to scene-labels.f32 beside it.
#
# SigLIP 2 names a picture by comparing it with sentences. The sentences
# never change, so they are embedded here, once, on the development side,
# and the app carries only the image half of the model (186 MB) instead of
# the text half too (1.1 GB). Nothing is trained: this is the released text
# model, run on the label table.
#
# The files come from onnx-community's ONNX export of
# google/siglip2-base-patch16-224 (Apache-2.0), at a pinned revision, and the
# text model is checked against its SHA-256. The image model the app runs,
# vision_model_fp16.onnx, is from the same revision (vb-pipeline's catalogue).
#
# Needs python3 and about 1.2 GB free. Run it after editing the table and
# commit the table and scene-labels.f32 together.
#
# Usage: scripts/make-scene-labels.sh
set -euo pipefail

cd "$(dirname "$0")/.."
WORK="$(pwd)/build/siglip"
DATA="core/vb-onnx/data"
REV=ba1f3b0843f24bc5417d38e19c37b287d719b2f4
BASE="https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/$REV"
mkdir -p "$WORK/files"

# Resumes: connections to Hugging Face drop here mid-transfer.
fetch() {
  local path=$1 sha=$2 out
  out="$WORK/files/$(basename "$path")"
  if [ ! -f "$out" ] || [ "$(shasum -a 256 "$out" | cut -d' ' -f1)" != "$sha" ]; then
    for attempt in $(seq 1 20); do
      curl -sSL --fail -C - --connect-timeout 20 -o "$out" "$BASE/$path" && break
      echo "  retrying $path ($attempt)"
      sleep 3
    done
  fi
  if [ "$(shasum -a 256 "$out" | cut -d' ' -f1)" != "$sha" ]; then
    echo "$path does not match its SHA-256" >&2
    exit 1
  fi
}
echo "Fetching the text model"
fetch tokenizer.json cb9140fae3ac5122c972d37adf83e1248471a38147ad76f8215c8872c6fd8322
fetch onnx/text_model.onnx baf12d941beabafafb14f7b4adb38dc15be18681b964a84410ec53d9d65e6293

if [ ! -x "$WORK/venv/bin/python" ]; then
  python3 -m venv "$WORK/venv"
  "$WORK/venv/bin/pip" install -q --retries 10 --timeout 60 onnxruntime numpy tokenizers
fi

echo "Embedding the labels"
"$WORK/venv/bin/python" - "$WORK/files" "$DATA/scene-labels.tsv" "$DATA/scene-labels.f32" <<'EOF'
import sys
import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

files, table, out = sys.argv[1:]
rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8")
        if l.strip() and not l.startswith("#")]
assert all(len(r) == 4 for r in rows), "every row needs four columns"
assert all(r[2] in ("place", "subject", "activity") for r in rows)
assert all("," not in r[0] for r in rows), "a label may not hold a comma"

# The tokenizer pads to SigLIP's 64 tokens and ends each sentence with <eos>,
# as the model was trained; the text is lower-cased, as its tokenizer config
# asks.
tokenizer = Tokenizer.from_file(f"{files}/tokenizer.json")
model = ort.InferenceSession(f"{files}/text_model.onnx", providers=["CPUExecutionProvider"])

def embed(sentences):
    ids = np.array([e.ids for e in tokenizer.encode_batch([s.lower() for s in sentences])],
                   dtype=np.int64)
    assert ids.shape[1] == 64, ids.shape
    x = np.concatenate([model.run(["pooler_output"], {"input_ids": ids[i:i + 32]})[0]
                        for i in range(0, len(ids), 32)])
    return x / np.linalg.norm(x, axis=1, keepdims=True)

plain = embed([f"this is a photo of {r[3]}." for r in rows])
# A place or a subject is often behind the people in a holiday picture;
# asking for it in the background as well found half again as many of them
# on the checked frames, as precisely.
behind = embed([f"this is a photo with {r[3]} in the background." for r in rows])
both = plain + behind
both /= np.linalg.norm(both, axis=1, keepdims=True)
kinds = np.array([r[2] for r in rows])
table_embeds = np.where((kinds == "activity")[:, None], plain, both).astype("<f4")
table_embeds.tofile(out)
print(f"{len(rows)} labels, {table_embeds.shape[1]} dimensions -> {out}")
EOF
