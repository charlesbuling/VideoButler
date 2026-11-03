<div align="center">

<!-- TODO: There is no logo in the repo yet. When one exists, put it here, for example ![Video Butler](docs/logo.png). Do not link an image that is not in the tree. -->

# Video Butler

On-device AI video auto-editing. A draft you keep editing.

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release v0.2.0](https://img.shields.io/badge/release-v0.2.0-green.svg)](https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20iOS%20%7C%20Android%20%7C%20Linux-lightgrey.svg)](https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0)

[中文](README.md) | English

</div>

On-device AI rough cuts. Import local video and photos, understand them, and get back a **draft project** you keep editing. A complete result does not need a network. It runs on your own computer and phone.

The AI does not hand you a flattened render as the only result. It hands you an editing decision (the Editing DSL). Every cut, subtitle, transition, and music choice can be undone without re-running the pipeline.

The bar is a qualified rough cut plus human finishing, not an unattended director.

> The repository stays private until the copyright holder publishes it. The Release links below are the addresses after that. If the Release is not up yet, build from source.

---

# What it does

* Footage stays on your disk. Analysis, selection, and the description of the cut run on the device. A cloud endpoint is an optional accelerator, not a requirement.
* One pipeline reads the shots: stability, exposure, motion, scenes, and — once the models are installed — occlusion, composition, pose, and a one-sentence caption. Captions say what the picture shows, not a transcript of holiday speech.
* Mood, topic, and whether to cut on the beat choose segments from scenes. In the contracts, time is integer microseconds.
* A draft can go to 剪映 / CapCut, or to OpenTimelineIO. On macOS you can also finish it in the built-in editor and export a video. A cut that can be copied losslessly is copied, not re-encoded.
* The video butler files videos and photos in a folder. People are grouped by how alike their faces look, not recognised. Memories (a trip, a year, a child growing up, people who appear together) can become a project.

---

# Features

- **Non-destructive.** Decisions live in the Editing DSL. Each step in the built-in editor is one operation, and the engine answers with the whole state. Undo is a whole draft.
- **Offline.** A rule-based draft works with no model weights and no network (`vb run --offline`). Speech, depth, pose, and captions exist only after those weights are installed.
- **Reads the footage.** One sweep, segments where the view changes, then scenes by look and by when they were filmed. The director uses depth, faces, and a person mask to see what blocks the frame. With PhotoFramer installed, shortlisted frames get a second composition score.
- **Beat and dance.** Music, or the footage's own sound, can supply a beat. Dance and subject-continuity place several takes of one action on one timeline by pose.
- **Video butler.** Groups faces, files clips by rules, finds memories, and draws the located ones on a map. The outlines are Natural Earth's public-domain data, not an online map.
- **Photos are footage.** They join the scenes of the videos filmed around them, by time and by a colour-and-layout signature.
- **You keep editing.** On macOS the built-in editor splits, trims, retimes, and sets text and music, then exports a video, a lossless copy, individual clips, or another 剪映 / OTIO draft.

What is not built yet is in the status table. It is not described as done.

---

# Status

Version **0.2.0** (`core/Cargo.toml` and `app/pubspec.yaml` match).

| Layer | State |
|---|---|
| Both contracts (Footage Graph, Editing DSL) and round-trip tests | done |
| Segments, scenes, classical quality | done |
| Mood × topic × beat | done, rule-based; a model does not direct the cut |
| Beat | done; on a beat cut, cuts land on the grid |
| Speech (whisper.cpp) | macOS, Android, iOS, and a natively built Linux engine; you install the model |
| Scene labels and faces (Apple Vision) | Apple platforms only; nothing to download |
| Depth, face detection, person masks, pose, SigLIP labels, AuraFace grouping | builds that have ONNX Runtime (macOS, Android). Not iOS. Not the Linux build yet |
| Captions (Qwen3-VL) and composition (PhotoFramer) | builds that have llama.cpp (macOS, Android). Not iOS. PhotoFramer is still a local conversion and is not offered as a catalogue download |
| People | done. Grouping, not recognition |
| 剪映 / CapCut drafts, OTIO | done. No FCPXML writer yet |
| Built-in editor and rendered export | rendering works on macOS. Android, Windows, and Linux do not have a platform encoder wired; lossless copy is the video export those builds have |
| Video butler, memories, map, photos | done |
| Cloud (OpenAI-compatible) | vision over it works; nothing drives the cut with it yet |
| macOS app | arm64, macOS 12+, LGPL ffmpeg inside the bundle. Not notarized |
| iOS | runs in the simulator. The device build links and is unsigned. No ONNX Runtime and no llama.cpp |
| Android | arm64 APK, run on an emulator and on phones. The release build is signed with the debug key |
| Linux | the app builds only on Linux. The engine cross-builds from a Mac. The app has never been run |
| Windows | **Included in 0.2.0** as an x64 zip (see Release below). Source, `WINDOWS.md`, and `scripts/build-windows.sh` / `package-windows.sh` live in this repo |

---

# News

- **2026-10-18** — The video butler files photos and can take the caller's file list. The APK carries AuraFace.
- **2026-10-17** — 0.2.0: lossless export (copy the footage, do not re-encode), AGPL, packaging.
- **2026-10-12** — 0.1.5: the editor timeline, rendering captions, picture, and sound, and retiming.
- **2026-10-09** — 0.1.4: photos, the video butler, and the memory map.
- **2026-09-30** — 0.1.1.

Older history is in Git. Product decisions live in `docs/AI Video Auto-Editing — Development Documentation.md`.

---

# Download and install

The 0.2.0 installers are GitHub Release **`v0.2.0`**:

`https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0`

Installers are produced in `dist/`, which Git ignores. After they are published, download them from the Release. They are not files in the source tree.

| Asset | What it is |
|---|---|
| `VideoButler-0.2.0-macOS-arm64.dmg` | the app only (about 35 MB), models not included |
| `VideoButler-0.2.0-android-arm64.apk` | arm64 APK |
| `VideoButler-0.2.0-ios-unsigned.ipa` | unsigned iOS package |
| `SHA256SUMS.txt` | sha256 of the installers above |

Models are not Release assets. They download from each model's official Hugging Face URL; see "Where model files come from" below.

Windows x64: `VideoButler-0.2.0-windows-x64.zip` (~31 MB, no models; get models from Hugging Face / `scripts/fetch-models.sh`).

Until the repository is public and the Release exists, these URLs do not resolve. Build from source.

## macOS

Apple silicon, macOS 12 or later. Open the disk image and drag `video_butler.app` to Applications.

The app is signed ad hoc and **is not notarized**. The first open goes through System Settings > Privacy & Security > Open Anyway, or right-click > Open in Finder. Each build is a new signature, so macOS asks again before the app reads a disk, the Desktop, Documents, or Downloads.

Models are not in this DMG. Install them with the script in the next section, or download them from the app's Models page (that path needs a network and fetches upstream).

## Android

arm64. This APK is signed with the **debug key** (`app/android/app/build.gradle.kts` signs the release build with the debug config), not a store key. Allow installation from this source. Only a build signed with the same debug key can replace it.

The APK is the application. Weights download from Hugging Face with `scripts/fetch-models.sh`. When `scripts/build-android.sh` packages the APK, the small models are copied in if they are in the model directory at build time. Qwen and the larger Whisper files do not fit in an APK (signing rejects a ZIP over 4 GB), so they are downloaded separately. Put them in the app's OBB directory, or import a folder from the Models page. The install script written by `scripts/package-android.sh` does the USB copy.

## iOS

The Release IPA is **unsigned**. `scripts/build-ios.sh --device` in this repo produces an unsigned `Runner.app`, not an IPA. An unsigned IPA is that `.app` inside `Payload/`, zipped. Installing it on a phone still means signing it in Xcode with your own team. With no Apple account, build for the simulator: `scripts/build-ios.sh`.

This build has no ONNX Runtime and no llama.cpp. Depth, YuNet, person masks, pose, SigLIP, captions, and the composition judge are absent. Apple Vision, whisper.cpp, beat detection, and the cut are present. A run says which adapters did not run.

## Linux

This Release has no prebuilt Linux app. Build it on Linux with `scripts/build-linux.sh`. The app has never been run on Linux.

## Where model files come from

Models are **not on the Release**. By default each one downloads from its official Hugging Face URL. The URLs, byte counts, and sha256 values match `core/vb-pipeline/src/models.rs` and are recorded in `models-manifest.json` at the repository root, which `scripts/fetch-models.sh` reads.

```bash
./scripts/fetch-models.sh --dry-run --platform macos   # list what would download and how big; download nothing
./scripts/fetch-models.sh --base --platform android    # the platform's base models
./scripts/fetch-models.sh --platform macos             # every downloadable model for the platform
./scripts/fetch-models.sh ggml-base.bin rtmo-s.onnx    # named files
./scripts/fetch-models.sh --out /path/to/models
./scripts/fetch-models.sh --source github ggml-base.bin  # try the GitHub Release mirror first
```

Each file tries Hugging Face, then `hf-mirror.com`, then the GitHub Release mirror when the manifest has one. `--source github` tries the GitHub Release first. Downloads resume (`curl -C -`). A finished file is checked against the byte count and sha256 and is not installed on a mismatch. A file already at the destination with the matching sha256 is skipped.

Base models (`--base`, from the manifest's `base` field):

- Android: `depth-anything-v2-small.onnx`, `face_detection_yunet_2023mar.onnx`, `human_segmentation_pphumanseg_2023mar.onnx`, `rtmo-s.onnx`, `ggml-base-q5_1.bin`, `siglip2-base-patch16-224-vision-fp16.onnx`, `auraface-v1-glintr100.onnx`, about 621 MiB
- macOS: the same without SigLIP 2, about 443 MiB
- iOS: `ggml-base-q5_1.bin`, about 57 MiB

The GitHub Release mirror is optional. The manifest's `github_release_base` is the `v0.2.0` download URL and `github_parts` names the assets. Files over 1.9 GiB (`qwen3-vl-8b-instruct-q4_k_m.gguf`, `qwen3-vl-4b-instruct-q4_k_m.gguf`, `photoframer-7b-q4_k_m.gguf`) are split with `split -b 1900M -d -a 2` into `<filename>.part00`, `.part01`, and so on. The script downloads the parts, joins them, and checks the whole file. The 0.2.0 Release does not carry model files now, so this path is unused by default. Point it at another mirror with `VB_RELEASE_BASE`, or at another manifest with `VB_MODELS_MANIFEST` (a path or URL).

By hand:

```bash
curl -fL -C - -o ggml-base.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin
shasum -a 256 ggml-base.bin   # compare with the manifest's sha256
```

The directory matches `core/vb-pipeline/src/library.rs`:

| Condition | Directory |
|---|---|
| `VB_LIBRARY_DIR` is set | `$VB_LIBRARY_DIR/models` |
| else `VB_WORKDIR` is set (the mobile host passes this) | `$VB_WORKDIR/library/models` |
| macOS | `~/Library/Application Support/VideoButler/models` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/video-butler/models` |

---

# Models

A weight's license is MIT, Apache-2.0, or BSD-3-Clause. Byte counts and the sha256 live in `core/vb-pipeline/src/models.rs`. They are the upstream files the catalogue records.

| File | Role | Bytes | License | Used on |
|---|---|---:|---|---|
| `ggml-tiny.bin` | speech | 77691713 | MIT | macOS, Android, iOS, native Linux engine |
| `ggml-base-q5_1.bin` | speech | 59707625 | MIT | the same. This is the speech model `build-android.sh` copies into the APK |
| `ggml-base.bin` | speech | 147951465 | MIT | the same |
| `ggml-small-q5_1.bin` | speech | 190085487 | MIT | the same |
| `ggml-small.bin` | speech | 487601967 | MIT | the same |
| `depth-anything-v2-small.onnx` | depth | 99060839 | Apache-2.0 | macOS, Android |
| `face_detection_yunet_2023mar.onnx` | face detection | 232589 | MIT | macOS, Android |
| `auraface-v1-glintr100.onnx` | face grouping | 260694151 | Apache-2.0 | macOS, Android |
| `human_segmentation_pphumanseg_2023mar.onnx` | person mask | 6163938 | Apache-2.0 | macOS, Android |
| `rtmo-s.onnx` | body pose | 39636400 | Apache-2.0 | macOS, Android |
| `siglip2-base-patch16-224-vision-fp16.onnx` | scene labels | 186039516 | Apache-2.0 | builds without Apple Vision (Android). A Mac does not use it |
| `qwen3-vl-2b-instruct-q4_k_m.gguf` and its mmproj | captions | 1107409952 + 819394848 | Apache-2.0 | macOS, Android |
| `qwen3-vl-4b-instruct-q4_k_m.gguf` and its mmproj | captions | 2497281664 + 836180256 | Apache-2.0 | macOS, Android |
| `qwen3-vl-8b-instruct-q4_k_m.gguf` and its mmproj | captions | 5027784800 + 1159029824 | Apache-2.0 | macOS, Android |
| `photoframer-7b-q4_k_m.gguf` and its mmproj | composition | 4683073792 + 1354163232 (local conversion) | MIT | macOS, Android. No public download yet; hosting TBD |

The Qwen projector file names are `qwen3-vl-8b-instruct-mmproj-f16.gguf`, `qwen3-vl-4b-instruct-mmproj-f16.gguf`, and `qwen3-vl-2b-instruct-mmproj-f16.gguf`. Upstream URLs and sha256 values are in `NOTICE` and `models.rs`.

PhotoFramer's upstream is PyTorch weights, converted to GGUF by `scripts/convert-photoframer.sh`. The app uses the two files when they are in the model library: `photoframer-7b-q4_k_m.gguf` and `photoframer-7b-mmproj-f16.gguf`. They are not in the download catalogue in `models.rs`; `models-manifest.json` records the byte counts and sha256 of the converted files. Where the converted GGUF files will be hosted **is not decided yet**, so `fetch-models.sh` skips them by default. Once there is a URL, set `VB_PHOTOFRAMER_URL` (the weight) and `VB_PHOTOFRAMER_MMPROJ_URL` (the projector) to download them, checked against the manifest.

The app uses, for each role, an installed model that fits in memory. With no caption model the cut has no captions, and the run says so.

---

# Quick start

The command line needs Rust and ffmpeg. With no models and no network it still writes an OTIO draft:

```bash
./scripts/make-test-media.sh
cargo run -p vb-cli --manifest-path core/Cargo.toml -- doctor
cargo run -p vb-cli --manifest-path core/Cargo.toml -- \
  run test-media/sample.mp4 --out draft.otio --offline
```

That writes the draft, the Footage Graph, and the Editing DSL. The last two are the contracts, and they are readable JSON on purpose.

A folder the way the app runs one, with a 剪映 draft:

```bash
cargo install --path core/vb-cli --manifest-path core/Cargo.toml  # or cargo run
vb run ~/Movies/kyoto --mood happy --topic travel --beat \
  --music ~/Music/track.mp3 --title "Kyoto" --jianying
```

The macOS app (full Xcode):

```bash
./scripts/build-macos-native.sh
open app/build/macos/Build/Products/Release/video_butler.app
```

Without Xcode, `./scripts/run-macos.sh` serves the same Dart UI and engine at `http://127.0.0.1:8787`. The web build shows thumbnails and cannot play a local file.

---

# Build from source

In common: Rust (the workspace asks for 1.85, edition 2024), Flutter, and the tools each platform script names. `scripts/env.sh` holds the paths. Do not edit a shell profile to match one checkout.

The ffmpeg in a release must be the LGPL build those scripts produce. Homebrew's ffmpeg on a development machine may be GPL. It is not what ships.

## macOS

```bash
./scripts/build-macos-native.sh          # the app, LGPL ffmpeg inside
./scripts/package-macos.sh --app-only    # the app-only DMG, the Release asset
```

Needs Xcode. The deployment target is macOS 12. Not notarized. Install models separately.

## Android

The Gradle wrapper is in the repo (Gradle 9.3.1, `app/android/gradlew`). You still need the Android SDK and NDK, the Rust Android targets, and the LGPL ffmpeg the script builds. `./gradlew` alone does not build the app: the engine `.so` files come from the packaging script.

The small models have to be in `VB_MODELS_DIR` first (on a Mac that defaults to the library models directory). Prepare it from the Release:

```bash
./scripts/fetch-models.sh --out "$HOME/android-models" \
  depth-anything-v2-small.onnx \
  face_detection_yunet_2023mar.onnx \
  human_segmentation_pphumanseg_2023mar.onnx \
  rtmo-s.onnx \
  ggml-base-q5_1.bin \
  siglip2-base-patch16-224-vision-fp16.onnx \
  auraface-v1-glintr100.onnx

VB_MODELS_DIR="$HOME/android-models" ./scripts/build-android.sh
```

`build-android.sh` copies those into the APK. Weights that do not fit in the APK go in the same directory, and the packaging script copies them into the installer folder:

```bash
./scripts/fetch-models.sh --out "$HOME/android-models"
VB_MODELS_DIR="$HOME/android-models" ./scripts/package-android.sh
```

`package-android.sh` expects the larger Whisper and Qwen files in that directory. PhotoFramer is included only when this machine already has the converted files; otherwise it is left out. The generated `安装到手机.command` installs the APK with adb and pushes models into the app's OBB directory.

`--out` is the same on Linux. Point `VB_MODELS_DIR` at that directory so the script does not look for the Mac path `~/Library/Application Support/VideoButler/models`.

## iOS

A Mac, with Xcode.

```bash
./scripts/build-ios.sh             # simulator
./scripts/build-ios.sh --device    # unsigned device .app
```

No ONNX Runtime and no llama.cpp. whisper.cpp is included. The deployment target is in the Xcode project (iOS 15 at the moment).

## Linux

On Linux:

```bash
sudo apt install clang cmake ninja-build pkg-config libgtk-3-dev
./scripts/build-linux.sh
```

The bundle is `app/build/linux/x64/release/bundle`. Run `bundle/video_butler`. A native build includes whisper.cpp. ONNX Runtime and llama.cpp are not wired. The app has not been run on Linux.

On a Mac the same script only cross-builds the engine (Zig). Flutter cannot build a Linux app there.

## Windows

From 0.2.0 the Windows x64 app ships from this repository. On Windows, in Git Bash:

```bash
./scripts/build-windows.sh
./scripts/package-windows.sh --no-build
```

See `WINDOWS.md` (toolchain, MSYS2 ffmpeg, ONNX Runtime, 剪映 paths). The installer zip is the Release folder: `video_butler.exe` beside `vb_ffi.dll`, `vb_llama_engine.dll`, ONNX Runtime and LGPL ffmpeg DLLs. Models are not bundled — use `scripts/fetch-models.sh` or the in-app Models page (Hugging Face).

The older private `charlesbuling/VideoBulter-windows` repo is historical; this repository is the source of truth.

---

# Architecture

Dependencies flow one way. `vb-narrative` may depend only on `vb-contracts` and `vb-infer`. A test checks that.

```
contracts/            JSON Schema
    |
vb-contracts          Rust types and validators
    |
    +-- vb-media      ffmpeg behind MediaBackend (CLI or in-process libav)
    +-- vb-geo        offline place names and world outlines
    +-- vb-infer      adapter traits
    |     +-- vb-whisper    whisper.cpp
    |     +-- vb-apple      Apple Vision
    |     +-- vb-onnx       ONNX Runtime, loaded at runtime
    |     +-- vb-llama      loads vb-llama-engine (llama.cpp, its own dylib)
    |     +-- vb-cloud      the only crate allowed a network client
    |
vb-perception         writes the Footage Graph, and finds the beat
    |
vb-narrative          priors and the orchestrator, writes the Editing DSL
    |
    +-- vb-edit       editor operations
    +-- vb-render     DSL to a finished video (frames composed in Rust)
    |     +-- vb-text
    +-- vb-export     OTIO, 剪映 / CapCut drafts
    |
vb-pipeline           runs, cache, projects, model library, video butler, editor sessions
    |
    +-- vb-cli        vb, including vb serve and vb call
    +-- vb-ffi        C ABI
          |
        app/          one Flutter UI. FFI on native, loopback HTTP on the web
```

Three front ends call `vb-pipeline::api`. A new capability is a match arm there, not a new route in one front end.

Two decisions that already cost real bugs:

**Time is integer microseconds.** Field names end in `Us`. The contracts have no float durations and no frame numbers.

**The DSL stores no absolute timeline positions.** A slot declares order and duration. Text and audio anchor to `{slotId, offsetUs}`. Transition overlap and a title card mean the sum of earlier durations is not the position on the finished timeline. Absolute time is derived once, by walking the slots at render time.

---

# Repository layout

```
core/            Rust workspace (the engine)
app/             Flutter app (macOS, iOS, Android, Linux, and Windows shells)
contracts/       JSON Schema and examples
scripts/         builds, packaging, fetch-models.sh
docs/            architecture, product and roadmap
PRD.md           product requirements (Chinese)
prior-store/     narrative priors
NOTICE           copyright and third parties
CLA.md           contributor license agreement
CONTRIBUTING.md
```

Product requirements are in [PRD.md](PRD.md) (Chinese), the architecture in [docs/architecture.md](docs/architecture.md), and build and check commands in [CONTRIBUTING.md](CONTRIBUTING.md). Product decisions are in `docs/AI Video Auto-Editing — Development Documentation.md`. When another document disagrees with it, that doc wins.

---

# Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Pull requests target **`macos-app`**. There is no automatic CI. Run the checks in CONTRIBUTING.md locally before you push.

You keep your copyright. A merge requires [CLA.md](CLA.md): tick it in the pull request, and put `Signed-off-by` on every commit. Maintainers may reimplement an idea instead of merging. Commercial licensing: **217010072@link.cuhk.edu.cn**.

The code of conduct is [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports go by [SECURITY.md](SECURITY.md), not a public issue.

---

# Third parties

How each piece is linked or bundled is in [NOTICE](NOTICE); the full third-party list, licenses and required notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Short form:

- The ffmpeg in a release is an LGPL build, linked dynamically or shipped as replaceable dynamic libraries.
- whisper.cpp, llama.cpp, and ONNX Runtime are MIT. llama.cpp and ONNX Runtime are loaded at runtime, not statically linked into the engine.
- Model licenses and upstream URLs are in `NOTICE` and `core/vb-pipeline/src/models.rs`.
- Map outlines are Natural Earth, public domain.
- The Gradle wrapper is Apache-2.0, Gradle's, not this project's code.

Do not add CoTracker3 or MMAudio (CC-BY-NC), and do not add someone else's copyleft.

---

# Acknowledgements

Video Butler stands on the shoulders of wonderful open-source projects and openly licensed models. An editor that cuts your footage offline, on your own device, would not exist without them. Heartfelt thanks to their authors, maintainers and contributors:

- [FFmpeg](https://ffmpeg.org/): decoding, stills and muxing — nearly everything that touches video.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) and [OpenAI Whisper](https://github.com/openai/whisper): offline speech recognition.
- [llama.cpp / ggml](https://github.com/ggml-org/llama.cpp): vision-language models on the device.
- [ONNX Runtime](https://github.com/microsoft/onnxruntime): the depth, face, person-mask, pose and scene models.
- [Flutter](https://flutter.dev/): one UI on macOS, iOS, Android and desktop.
- [Qwen3-VL](https://huggingface.co/Qwen) by the Qwen team: understanding and describing what is on screen.
- [PhotoFramer](https://zhiyuanyou.github.io/photoframer/): composition scores.
- [Depth Anything V2](https://github.com/DepthAnything/Depth-Anything-V2), [YuNet](https://github.com/opencv/opencv_zoo), [PP-HumanSeg](https://github.com/PaddlePaddle/PaddleSeg), [AuraFace](https://huggingface.co/fal/AuraFace-v1), [RTMO / MMPose](https://github.com/open-mmlab/mmpose) and [SigLIP 2](https://huggingface.co/google/siglip2-base-patch16-224): depth, faces, people, pose and scenes.
- From the Rust ecosystem: [ort](https://github.com/pykeio/ort), [whisper-rs](https://codeberg.org/tazz4843/whisper-rs), [llama-cpp-rs](https://github.com/utilityai/llama-cpp-rs), [cosmic-text](https://github.com/pop-os/cosmic-text), [serde](https://serde.rs/) and many more.
- [Natural Earth](https://www.naturalearthdata.com/): the public-domain map data behind the memory map.
- Ideas we learned from (no code used): [World Monitor](https://github.com/koala73/worldmonitor)'s map style, [LosslessCut](https://github.com/mifi/lossless-cut)'s approach to lossless export, and CapCut / 剪映's editor.

The full list — every native library, model, Rust crate and Dart package, with its license and any required copyright notice — is in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Model weights keep their own licenses; Video Butler itself is AGPL-3.0-or-later (see [LICENSE](LICENSE) and [NOTICE](NOTICE)).

---

# Star

<!-- TODO: After the repository is public and has stars, embed a Star History chart. Do not link an image that is not in the tree.
     For example https://api.star-history.com/svg?repos=charlesbuling/VideoButler&type=Date
-->

---

## ⚖️ Copyright & Licensing

This project is open source under the **GNU Affero General Public License v3.0 (AGPL v3)**.

### 1. Open-source and free use
* Individuals, researchers, and the open-source community are welcome to use, modify, and distribute this project's code free of charge, provided they comply with AGPL v3.
* **Core constraint (remote network-interaction copyleft):** If you modify this project's code, or deploy this project on a cloud or server as SaaS (software as a service) and offer network access to the public, **you must unconditionally make your entire system's source code available to all users of that service under AGPL v3**.

### 2. Commercial licensing (dual-licensing / commercial exemption)
* **Commercial use without publishing source:** If your company or organization plans to use this project in a closed-source commercial product, or as part of a commercial service you do not want to open-source, **you cannot use the current AGPL v3 license**.
* The original author **zhengkuang** always retains 100% of the intellectual property in this project. We offer a flexible **commercial dual license (Commercial License)**.
* For commercial cooperation, private enterprise deployment, closed-source licensing, or sponsorship, contact the official email directly:
  📬 **217010072@link.cuhk.edu.cn**
