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

# Download Video Butler model weights from their official Hugging Face
# URLs and install them where the engine looks.
#
# Reads models-manifest.json (the one beside this script's repository root
# by default; VB_MODELS_MANIFEST names another file or URL). For each model
# it downloads `url` (curl -L -C -, so a dropped connection resumes), checks
# the size and sha256 of the whole file, and installs it.
#
# Sources, in the order tried (--source picks the first):
#   hf      the manifest `url` on huggingface.co, then the same path on
#           hf-mirror.com (the engine's own fallback, vb-cloud download.rs)
#   github  a GitHub Release that mirrors the weights
#           (${VB_RELEASE_BASE:-manifest github_release_base}); files over
#           1.9 GiB are split there and listed in `github_parts`
# If the first source fails, the other is tried.
#
# PhotoFramer (photoframer-7b-*.gguf) has no public host yet: hosting is TBD.
# Its manifest `url` is null; set VB_PHOTOFRAMER_URL and
# VB_PHOTOFRAMER_MMPROJ_URL to where you put the converted files (or use
# --source github once a Release carries them). Without them the files are
# skipped, unless you named them on the command line.
#
# The destination matches vb-pipeline's library (library.rs):
#   VB_LIBRARY_DIR set     -> $VB_LIBRARY_DIR/models
#   else VB_WORKDIR set    -> $VB_WORKDIR/library/models
#   else macOS             -> ~/Library/Application Support/VideoButler/models
#   else Linux and others  -> ${XDG_DATA_HOME:-~/.local/share}/video-butler/models
# --out DIR writes the files into DIR itself (no extra models/ folder).
# That is how an Android build prepares VB_MODELS_DIR.
#
# A file already at the destination with the manifest sha256 is skipped.
# Needs bash, curl, and either sha256sum or shasum. No jq.
#
# Usage:
#   scripts/fetch-models.sh --base --platform macos
#   scripts/fetch-models.sh --platform android
#   scripts/fetch-models.sh --out /tmp/vb-models ggml-base.bin
#   scripts/fetch-models.sh --dry-run --platform android
#   VB_PHOTOFRAMER_URL=https://... VB_PHOTOFRAMER_MMPROJ_URL=https://... \
#     scripts/fetch-models.sh photoframer-7b-q4_k_m.gguf photoframer-7b-mmproj-f16.gguf

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST_SRC="${VB_MODELS_MANIFEST:-${SCRIPT_DIR}/../models-manifest.json}"

PLATFORM=""
OUT=""
SOURCE="hf"
BASE_ONLY=0
DRY_RUN=0
NAMES=()

usage() {
  cat <<'USAGE'
Usage: scripts/fetch-models.sh [--platform NAME] [--base] [--out DIR]
                               [--source hf|github] [--dry-run] [FILE ...]

  --platform NAME   keep models whose manifest platforms include NAME
                    (macos, android, ios)
  --base            only the base set for --platform (what a lite install
                    needs first: the small vision models and one speech model)
  --out DIR         write files into DIR instead of the model library
  --source SRC      hf (default): Hugging Face, then hf-mirror.com, then
                    GitHub; github: the GitHub Release first, then Hugging Face
  --dry-run         list what would be downloaded, from where, and stop
  FILE              install only these manifest file names

Environment:
  VB_MODELS_MANIFEST        manifest path or URL (default: repo models-manifest.json)
  VB_RELEASE_BASE           GitHub Release download URL for --source github
  VB_PHOTOFRAMER_URL        where photoframer-7b-q4_k_m.gguf is hosted (TBD)
  VB_PHOTOFRAMER_MMPROJ_URL where photoframer-7b-mmproj-f16.gguf is hosted (TBD)
  VB_LIBRARY_DIR            engine library root; models go in its models/ folder
  VB_WORKDIR                used when VB_LIBRARY_DIR is unset, as on mobile
  XDG_DATA_HOME             Linux library root is $XDG_DATA_HOME/video-butler
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --platform)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        echo "fetch-models: --platform needs a name" >&2
        exit 2
      fi
      PLATFORM="$2"
      shift 2
      ;;
    --out)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        echo "fetch-models: --out needs a directory" >&2
        exit 2
      fi
      OUT="$2"
      shift 2
      ;;
    --source)
      if [ "$#" -lt 2 ] || { [ "$2" != hf ] && [ "$2" != github ]; }; then
        echo "fetch-models: --source is hf or github" >&2
        exit 2
      fi
      SOURCE="$2"
      shift 2
      ;;
    --base) BASE_ONLY=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --) shift; while [ "$#" -gt 0 ]; do NAMES+=("$1"); shift; done ;;
    -*)
      echo "fetch-models: unknown option $1" >&2
      usage >&2
      exit 2
      ;;
    *) NAMES+=("$1"); shift ;;
  esac
done

if [ "$BASE_ONLY" -eq 1 ] && [ -z "$PLATFORM" ]; then
  echo "fetch-models: --base needs --platform" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "fetch-models: curl is required" >&2
  exit 1
fi

lower() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

file_sha256() {
  local sum
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum "$1")
  elif command -v shasum >/dev/null 2>&1; then
    sum=$(shasum -a 256 "$1")
  else
    echo "fetch-models: need sha256sum or shasum" >&2
    exit 1
  fi
  printf '%s' "${sum%% *}"
}

file_bytes() {
  wc -c < "$1" | tr -d '[:space:]'
}

urlencode() {
  local raw="$1" i=0 c hex out=""
  while [ "$i" -lt "${#raw}" ]; do
    c=${raw:i:1}
    case "$c" in
      [A-Za-z0-9._~-]) out="${out}${c}" ;;
      *)
        printf -v hex '%02X' "'$c"
        out="${out}%${hex}"
        ;;
    esac
    i=$((i + 1))
  done
  printf '%s' "$out"
}

download_resume() {
  local url="$1" dest="$2" status=0
  mkdir -p "$(dirname "$dest")"
  if curl -fL --retry 5 --retry-delay 2 -C - -o "$dest" "$url"; then
    return 0
  else
    status=$?
  fi
  # 33: the local file is already as long as the server's copy.
  if [ "$status" -eq 33 ] && [ -s "$dest" ]; then
    return 0
  fi
  # 22 with a partial file is often a 416 (range past the end): start over.
  rm -f "$dest"
  curl -fL --retry 5 --retry-delay 2 -o "$dest" "$url"
}

library_models_dir() {
  local root
  if [ -n "${VB_LIBRARY_DIR:-}" ]; then
    root="$VB_LIBRARY_DIR"
  elif [ -n "${VB_WORKDIR:-}" ]; then
    root="${VB_WORKDIR}/library"
  elif [ "$(uname -s)" = "Darwin" ]; then
    root="${HOME}/Library/Application Support/VideoButler"
  else
    root="${XDG_DATA_HOME:-${HOME}/.local/share}/video-butler"
  fi
  printf '%s/models' "$root"
}

in_list() {
  # Comma-separated membership test (no arrays: bash 3.2 + set -u safe).
  case ",$1," in
    *",$2,"*) return 0 ;;
  esac
  return 1
}

want_name() {
  local file="$1" n
  if [ "${#NAMES[@]}" -eq 0 ]; then
    return 0
  fi
  for n in "${NAMES[@]}"; do
    if [ "$n" = "$file" ]; then
      return 0
    fi
  done
  return 1
}

named() {
  [ "${#NAMES[@]}" -gt 0 ] && want_name "$1"
}

DEST="${OUT:-$(library_models_dir)}"

manifest=$(mktemp)
awk_prog=$(mktemp)
rows_file=$(mktemp)
cleanup() {
  rm -f "$manifest" "$awk_prog" "$rows_file"
}
trap cleanup EXIT

case "$MANIFEST_SRC" in
  http://*|https://*)
    echo "fetch-models: manifest ${MANIFEST_SRC}" >&2
    curl -fL --retry 5 --retry-delay 2 -o "$manifest" "$MANIFEST_SRC"
    ;;
  *)
    if [ ! -f "$MANIFEST_SRC" ]; then
      echo "fetch-models: no manifest at ${MANIFEST_SRC} (set VB_MODELS_MANIFEST)" >&2
      exit 1
    fi
    cp "$MANIFEST_SRC" "$manifest"
    ;;
esac

cat > "$awk_prog" <<'AWK'
function skip() {
  while (i <= n) {
    c = substr(s, i, 1)
    if (c != " " && c != "\t" && c != "\n" && c != "\r") break
    i++
  }
}
function peek() {
  skip()
  return substr(s, i, 1)
}
function fail(msg) {
  printf "fetch-models: %s (at byte %d)\n", msg, i > "/dev/stderr"
  exit 2
}
function parse_string(    out, e) {
  if (peek() != "\"") fail("expected string")
  i++
  out = ""
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == "\\") {
      i++
      if (i > n) fail("truncated escape")
      e = substr(s, i, 1)
      if (e == "n") out = out "\n"
      else if (e == "r") out = out "\r"
      else if (e == "t") out = out "\t"
      else if (e == "u") {
        if (i + 4 > n) fail("truncated unicode escape")
        i += 4
        out = out "?"
      } else out = out e
      i++
    } else if (c == "\"") {
      i++
      return out
    } else {
      out = out c
      i++
    }
  }
  fail("unterminated string")
}
function parse_number(    start, ch) {
  skip()
  start = i
  while (i <= n) {
    ch = substr(s, i, 1)
    if (ch ~ /[0-9.eE+-]/) i++
    else break
  }
  if (i == start) fail("expected number")
  return substr(s, start, i - start)
}
function skip_literal(word) {
  if (substr(s, i, length(word)) != word) fail("expected " word)
  i += length(word)
}
function parse_string_array(    out, first, val) {
  if (peek() != "[") fail("expected array")
  i++
  out = ""
  first = 1
  while (1) {
    c = peek()
    if (c == "]") { i++; return out }
    val = parse_string()
    if (!first) out = out ","
    first = 0
    out = out val
    skip()
    if (substr(s, i, 1) == ",") i++
  }
}
function skip_value() {
  c = peek()
  if (c == "{") parse_object()
  else if (c == "[") skip_array()
  else if (c == "\"") parse_string()
  else if (c == "n") skip_literal("null")
  else if (c == "t") skip_literal("true")
  else if (c == "f") skip_literal("false")
  else parse_number()
}
function skip_array() {
  if (peek() != "[") fail("expected array")
  i++
  while (1) {
    c = peek()
    if (c == "]") { i++; return }
    skip_value()
    skip()
    if (substr(s, i, 1) == ",") i++
  }
}
function parse_object(    key, val) {
  if (peek() != "{") fail("expected object")
  i++
  file = ""; size = ""; sha = ""; lic = ""; source = ""; urlenv = ""; platforms = ""; parts = ""; base = ""
  while (1) {
    c = peek()
    if (c == "}") { i++; break }
    key = parse_string()
    skip()
    if (substr(s, i, 1) != ":") fail("expected colon")
    i++
    c = peek()
    if (key == "platforms" && c == "[") platforms = parse_string_array()
    else if (key == "github_parts" && c == "[") parts = parse_string_array()
    else if (key == "base" && c == "[") base = parse_string_array()
    else if (c == "\"") {
      val = parse_string()
      if (key == "file") file = val
      else if (key == "sha256") sha = val
      else if (key == "license") lic = val
      else if (key == "url") source = val
      else if (key == "url_env") urlenv = val
      else if (key == "github_release_base") doc_base = val
      else if (key == "version") doc_version = val
    } else if (c == "n") {
      skip_literal("null")
      if (key == "url") source = ""
    } else if (c == "t") skip_literal("true")
    else if (c == "f") skip_literal("false")
    else if (c == "{") parse_object()
    else if (c == "[") skip_array()
    else {
      val = parse_number()
      if (key == "size") size = val
      else if (key == "version") doc_version = val
    }
    skip()
    if (substr(s, i, 1) == ",") i++
  }
  if (file != "" && sha != "") {
    printf "%s\037%s\037%s\037%s\037%s\037%s\037%s\037%s\037%s\n", file, size, sha, lic, source, urlenv, platforms, parts, base
    file = ""
    sha = ""
  }
}
{
  s = s $0 "\n"
}
END {
  n = length(s)
  i = 1
  doc_version = ""
  doc_base = ""
  if (peek() != "{") fail("manifest is not a JSON object")
  parse_object()
  skip()
  if (i <= n && peek() != "") {
    # trailing whitespace only; peek already skipped it
  }
  printf "#version\037%s\037%s\037\037\037\037\037\037\n", doc_version, doc_base
}
AWK

if ! awk -f "$awk_prog" "$manifest" > "$rows_file"; then
  echo "fetch-models: could not parse models-manifest.json" >&2
  exit 1
fi

GH_BASE="${VB_RELEASE_BASE:-}"
all_files=" "
while IFS=$'\037' read -r file size sha lic url urlenv platforms parts base; do
  [ -n "$file" ] || continue
  if [ "$file" = "#version" ]; then
    [ -n "$GH_BASE" ] || GH_BASE="$sha"
    continue
  fi
  all_files="${all_files}${file} "
done < "$rows_file"
GH_BASE="${GH_BASE%/}"

if [ "${#NAMES[@]}" -gt 0 ]; then
  for n in "${NAMES[@]}"; do
    case "$all_files" in
      *" ${n} "*) ;;
      *)
        echo "fetch-models: ${n} is not in the manifest" >&2
        exit 1
        ;;
    esac
  done
fi

[ "$DRY_RUN" -eq 1 ] || mkdir -p "$DEST"

# Fetch the whole file from one URL into $2. Returns non-zero on failure.
fetch_url() {
  local url="$1" out="$2"
  echo "fetch-models:   ${url}" >&2
  download_resume "$url" "$out"
}

# Fetch the GitHub Release parts into the stage and join them into $2.
fetch_github() {
  local stage="$1" out="$2" part enc
  [ -n "$GH_BASE" ] && [ -n "$parts" ] || return 1
  IFS=',' read -r -a part_names <<PARTS
${parts}
PARTS
  for part in "${part_names[@]}"; do
    [ -n "$part" ] || return 1
    enc=$(urlencode "$part")
    echo "fetch-models:   ${GH_BASE}/${enc}" >&2
    download_resume "${GH_BASE}/${enc}" "${stage}/${part}" || return 1
  done
  : > "$out"
  for part in "${part_names[@]}"; do
    cat "${stage}/${part}" >> "$out"
  done
}

hf_mirror() {
  case "$1" in
    https://huggingface.co/*) printf 'https://hf-mirror.com/%s' "${1#https://huggingface.co/}" ;;
    *) return 1 ;;
  esac
}

installed=0
skipped=0
total_bytes=0
selected_files=""
while IFS=$'\037' read -r file size sha lic url urlenv platforms parts base; do
  [ -n "$file" ] || continue
  [ "$file" != "#version" ] || continue
  want_name "$file" || continue
  if [ -n "$PLATFORM" ] && ! in_list "$platforms" "$PLATFORM"; then
    continue
  fi
  if [ "$BASE_ONLY" -eq 1 ] && ! in_list "$base" "$PLATFORM"; then
    continue
  fi
  case "$file" in
    */*|*" "*|"")
      echo "fetch-models: refusing unusual file name: ${file}" >&2
      exit 1
      ;;
  esac
  if [ -z "$sha" ]; then
    echo "fetch-models: ${file} has no sha256 in the manifest" >&2
    exit 1
  fi

  # A file with no public URL (PhotoFramer, hosting TBD) takes its URL from
  # the variable the manifest names.
  if [ -z "$url" ] && [ -n "$urlenv" ]; then
    url="${!urlenv:-}"
  fi
  if [ -z "$url" ] && [ "$SOURCE" != github ]; then
    if named "$file"; then
      echo "fetch-models: ${file} has no public download yet (hosting TBD); set ${urlenv:-its URL}" >&2
      exit 1
    fi
    echo "fetch-models: skipping ${file}: no public download yet (hosting TBD); set ${urlenv:-its URL} to fetch it" >&2
    skipped=$((skipped + 1))
    continue
  fi

  dest="${DEST}/${file}"
  selected_files="${selected_files}${file} "
  if [ -f "$dest" ] && [ "$(lower "$(file_sha256 "$dest")")" = "$(lower "$sha")" ]; then
    echo "fetch-models: ${file} already installed" >&2
    installed=$((installed + 1))
    continue
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    if [ "$SOURCE" = "github" ] && [ -n "$parts" ] && [ -n "$GH_BASE" ]; then
      plan="${GH_BASE}/{${parts}}"
    else
      plan="${url:-${GH_BASE}/{${parts}}}"
    fi
    printf 'would fetch %s (%s bytes, %s) from %s\n' "$file" "$size" "$lic" "$plan"
    total_bytes=$((total_bytes + ${size:-0}))
    installed=$((installed + 1))
    continue
  fi

  stage="${DEST}/.vb-fetch/${file}"
  mkdir -p "$stage"
  assembled="${stage}/assembled"
  echo "fetch-models: ${file}" >&2

  ok=0
  mirror=$(hf_mirror "$url" || true)
  if [ "$SOURCE" = github ]; then
    fetch_github "$stage" "$assembled" && ok=1
    if [ "$ok" -eq 0 ] && [ -n "$url" ]; then
      fetch_url "$url" "$assembled" && ok=1
    fi
  else
    fetch_url "$url" "$assembled" && ok=1
    if [ "$ok" -eq 0 ] && [ -n "$mirror" ]; then
      fetch_url "$mirror" "$assembled" && ok=1
    fi
    if [ "$ok" -eq 0 ]; then
      fetch_github "$stage" "$assembled" && ok=1
    fi
  fi
  if [ "$ok" -eq 0 ]; then
    echo "fetch-models: could not download ${file}; run again to resume" >&2
    exit 1
  fi

  if [ -n "$size" ]; then
    got_size=$(file_bytes "$assembled")
    if [ "$got_size" != "$size" ]; then
      echo "fetch-models: ${file} is ${got_size} bytes, manifest says ${size}" >&2
      rm -rf "$stage"
      exit 1
    fi
  fi
  have=$(file_sha256 "$assembled")
  if [ "$(lower "$have")" != "$(lower "$sha")" ]; then
    echo "fetch-models: ${file} sha256 ${have} != ${sha}" >&2
    rm -rf "$stage"
    exit 1
  fi
  mv -f "$assembled" "$dest"
  rm -rf "$stage"
  echo "fetch-models: installed ${dest} (${lic})" >&2
  installed=$((installed + 1))
done < "$rows_file"

if [ "$installed" -eq 0 ]; then
  echo "fetch-models: no models matched" >&2
  exit 1
fi

# A requested name that the platform filter dropped is an error.
if [ "${#NAMES[@]}" -gt 0 ]; then
  for n in "${NAMES[@]}"; do
    case " ${selected_files}" in
      *" ${n} "*) ;;
      *)
        echo "fetch-models: ${n} was not selected (platform '${PLATFORM}'$([ "$BASE_ONLY" -eq 1 ] && echo ', base only'))" >&2
        exit 1
        ;;
    esac
  done
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "fetch-models: dry run: ${installed} file(s), $((total_bytes / 1048576)) MiB to download into ${DEST}; ${skipped} skipped" >&2
else
  rmdir "${DEST}/.vb-fetch" 2>/dev/null || true
  echo "fetch-models: ${installed} model file(s) in ${DEST}; ${skipped} skipped" >&2
fi
