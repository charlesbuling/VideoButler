# Third-Party Notices / 第三方声明与致谢

Video Butler (视频管家) is Copyright (C) 2026 zhengkuang and is licensed under
**AGPL-3.0-or-later** (see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE)).
It builds on the open-source projects and openly licensed model weights listed
below. Each of them stays under **its own license**; nothing here relicenses
them, and the AGPL of this project does not apply to them. Thank you to every
author and maintainer listed here. 🙏

本项目（视频管家）以 AGPL-3.0-or-later 授权。下列开源项目和模型权重各自保留其原有许可证，
本项目的 AGPL 不改变它们的授权。衷心感谢它们的作者与维护者。

How this file was produced (2026-11-03, version 0.2.0):

- Rust crates: `cargo tree -e normal,build -f '{p} {l}' --prefix none`, run in `core/`
  for the host and for `aarch64-linux-android`, `aarch64-apple-ios`,
  `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`, merged. The license
  column is each crate's own `Cargo.toml` `license` field (cargo metadata).
  Dev-only dependencies are not listed. The same policy is enforced by `cargo deny` with
  `core/deny.toml`.
- Dart packages: `app/pubspec.lock`; the license column is read from each
  package's `LICENSE` file in the local pub cache.
- Native libraries: the build scripts in `scripts/` and the vendored sources in
  the `llama-cpp-sys-2` and `whisper-rs-sys` crates.
- Models: `core/vb-pipeline/src/models.rs` / `models-manifest.json`, checked
  against each Hugging Face model card and LICENSE file.

Where a license requires its copyright line or notice to travel with binaries,
the text is reproduced in [Required notices](#required-notices) below. Full license texts are
in each upstream repository and, for crates and Dart packages, in the
packages themselves.

---

## 1. Native libraries / 原生库

| Project | Link | License | How Video Butler uses it |
|---|---|---|---|
| FFmpeg 9.0.1 (libavformat, libavcodec, libavutil, libswscale, libswresample, libavfilter) | <https://ffmpeg.org/> | **LGPL-2.1-or-later** (the release build is configured without `--enable-gpl`/`--enable-nonfree`; `scripts/build-ffmpeg-*.sh` refuse a GPL or nonfree result) | Decoding, stills, muxing, AAC encode. Shipped as replaceable dynamic libraries / frameworks. |
| whisper.cpp + ggml (vendored in `whisper-rs-sys` 0.15.0) | <https://github.com/ggml-org/whisper.cpp> | MIT | Speech recognition (`vb-whisper`). |
| whisper-rs / whisper-rs-sys | <https://codeberg.org/tazz4843/whisper-rs> | Unlicense | Rust bindings to whisper.cpp. |
| llama.cpp + ggml + mtmd (vendored in `llama-cpp-sys-2` 0.1.156) | <https://github.com/ggml-org/llama.cpp> | MIT | Vision-language models (captions, composition judge) in the separate `vb-llama-engine` library. |
| llama-cpp-2 / llama-cpp-sys-2 | <https://github.com/utilityai/llama-cpp-rs> | MIT OR Apache-2.0 | Rust bindings to llama.cpp. |
| ↳ stb_image (vendored in llama.cpp) | <https://github.com/nothings/stb> | Public domain or MIT | Image loading inside llama.cpp's multimodal tools. |
| ↳ miniaudio (vendored in llama.cpp) | <https://github.com/mackron/miniaudio> | Public domain or MIT-0 | Audio input path of llama.cpp's mtmd. |
| ↳ nlohmann/json (vendored in llama.cpp) | <https://github.com/nlohmann/json> | MIT | JSON in llama.cpp's common code. |
| ↳ cpp-httplib, sheredom/subprocess.h (vendored in llama.cpp) | <https://github.com/yhirose/cpp-httplib>, <https://github.com/sheredom/subprocess.h> | MIT; Unlicense | Present in the vendored source tree; the server and curl are switched off in our build (`LLAMA_BUILD_SERVER=OFF`, `LLAMA_CURL=OFF`). Listed for completeness. |
| ONNX Runtime 1.30.0 | <https://github.com/microsoft/onnxruntime> | MIT | Runs the ONNX models (depth, faces, person masks, pose, scene labels). Loaded at runtime (`ort` with `load-dynamic`). macOS: built from source; Android: Microsoft's AAR. ONNX Runtime bundles its own third-party components, listed in its [`ThirdPartyNotices.txt`](https://github.com/microsoft/onnxruntime/blob/main/ThirdPartyNotices.txt), which must travel with the library. |
| ort / ort-sys 2.0.0-rc.13 | <https://github.com/pykeio/ort> | MIT OR Apache-2.0 | Rust bindings to ONNX Runtime. |
| zlib 1.3.1 | <https://zlib.net/> | Zlib | Linked into the Linux/Windows cross-built FFmpeg (PNG). Other platforms use the system zlib. |
| Flutter engine and framework | <https://flutter.dev/> | BSD-3-Clause (engine third-party components under their own licenses, shown by Flutter's license registry) | The app UI on every platform. |
| Material Icons font (bundled by Flutter, `uses-material-design`) | <https://github.com/google/material-design-icons> | Apache-2.0 | App icons. |
| AndroidX Media3 / ExoPlayer (via `video_player_android`) | <https://github.com/androidx/media> | Apache-2.0 | Clip preview on Android. |
| Gradle Wrapper (`app/android/gradle/wrapper`) | <https://gradle.org/> | Apache-2.0 | Android build tool (source only, not in the app). |
| Apple Vision, AVFoundation, VideoToolbox, Core ML | <https://developer.apple.com/> | Apple system frameworks (platform SDK terms) | Called through the OS; no Apple source is vendored. |

## 2. AI models / 模型权重

None of these weights is bundled in this repository. They are downloaded from the
listed upstream URL (or an optional mirror of the same files) after install, and each keeps
its own license.

| Model (file) | Authors / upstream | License (verified on the model card / LICENSE) | Used for |
|---|---|---|---|
| Whisper tiny / base / small, ggml conversions (`ggml-*.bin`) | OpenAI (Whisper); ggml conversion by ggerganov / whisper.cpp — <https://github.com/openai/whisper>, <https://huggingface.co/ggerganov/whisper.cpp> | MIT (OpenAI repo: "Copyright (c) 2022 OpenAI"; HF repo tag `license:mit`) | Speech recognition |
| Qwen3-VL 2B / 4B / 8B Instruct GGUF (`qwen3-vl-*-instruct-q4_k_m.gguf`, `*-mmproj-f16.gguf`) | Qwen team, Alibaba Cloud — <https://huggingface.co/Qwen/Qwen3-VL-8B-Instruct-GGUF> (and 4B / 2B) | Apache-2.0 | Captions and scene understanding |
| PhotoFramer composition model (`photoframer-7b-*.gguf`, converted locally; not in the download catalogue) | Zhiyuan You, Ke Wang, He Zhang, Xin Cai, Jinjin Gu, Tianfan Xue, Chao Dong, Zhoutong Zhang — <https://huggingface.co/zhiyuanyou/Qwen2.5-VL-7B-GRPO-Composition-Score-Class>, paper <https://arxiv.org/abs/2512.00993> | MIT (fine-tune of Qwen2.5-VL-7B-Instruct, Apache-2.0) | Composition judge |
| Depth Anything V2 Small (`depth-anything-v2-small.onnx`) | Depth Anything authors (HKU / TikTok) — <https://huggingface.co/depth-anything/Depth-Anything-V2-Small>; ONNX export by onnx-community | Apache-2.0 (Small only; Base/Large are CC-BY-NC and are **not** used) | Depth |
| YuNet (`face_detection_yunet_2023mar.onnx`) | Shiqi Yu et al., OpenCV Zoo — <https://huggingface.co/opencv/face_detection_yunet> | MIT ("Copyright (c) 2020 Shiqi Yu") | Face detection |
| PP-HumanSeg (`human_segmentation_pphumanseg_2023mar.onnx`) | PaddlePaddle authors (PaddleSeg), OpenCV Zoo — <https://huggingface.co/opencv/human_segmentation_pphumanseg> | Apache-2.0 ("Copyright (c) 2021 PaddlePaddle Authors") | Person masks |
| AuraFace v1 (`glintr100.onnx` only) | fal — <https://huggingface.co/fal/AuraFace-v1> | Apache-2.0 (only `glintr100.onnx` is fetched; the InsightFace files in that repo are not) | Face grouping |
| RTMO-s (`rtmo-s.onnx`) | OpenMMLab MMPose — <https://github.com/open-mmlab/mmpose/tree/main/projects/rtmo>; ONNX export by Xenova — <https://huggingface.co/Xenova/RTMO-s> | Apache-2.0 | Multi-person pose |
| SigLIP 2 base patch16-224, vision tower fp16 (`siglip2-base-patch16-224-vision-fp16.onnx`) and the scene-label embeddings derived from its text tower (`core/vb-onnx/data/scene-labels.f32`) | Google — <https://huggingface.co/google/siglip2-base-patch16-224>; ONNX export by onnx-community | Apache-2.0 | Scene labels |

## 3. Data / 数据

| Data | Link | License | Use |
|---|---|---|---|
| Natural Earth 1:10m (countries, provinces, populated places) | <https://www.naturalearthdata.com/> | Public domain | Memory-map outlines and place names compiled into `vb-geo`. "Made with Natural Earth." |

## 4. Ideas and inspiration (no code used) / 借鉴与灵感（未使用其代码）

| Project | Link | License | What we learned from it |
|---|---|---|---|
| World Monitor | <https://github.com/koala73/worldmonitor> | AGPL-3.0 | The look of the memory map. None of its code is used. |
| LosslessCut | <https://github.com/mifi/lossless-cut> | GPL-2.0-only | The approach to lossless export (cut at keyframes, join parts). None of its code is used. |
| 剪映 / CapCut | <https://www.capcut.com/> | Proprietary | Editor layout and the draft format Video Butler exports to. No code or assets used. |
| OpenTimelineIO | <https://github.com/AcademySoftwareFoundation/OpenTimelineIO> | Apache-2.0 | The `.otio` interchange format Video Butler can write (format only; the library is not linked). |

## 5. Rust crates / Rust 依赖

All crates reachable from the workspace through normal and build dependencies
on any supported target (223 crates, this project's own `vb-*` crates excluded).
Where a crate offers a choice ("MIT OR Apache-2.0"), Video Butler uses it under
any one of the permissive options; `self_cell` is used under Apache-2.0, not GPL-2.0.
No crate ships a separate `NOTICE` file.

| Crate | Version | License (Cargo.toml) | Source |
|---|---|---|---|
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | <https://github.com/oyvindln/adler2> |
| `ahash` | 0.7.8 | MIT OR Apache-2.0 | <https://github.com/tkaitchuck/ahash> |
| `ahash` | 0.8.12 | MIT OR Apache-2.0 | <https://github.com/tkaitchuck/ahash> |
| `aho-corasick` | 1.1.5 | Unlicense OR MIT | <https://github.com/BurntSushi/aho-corasick> |
| `anstream` | 1.0.0 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `anstyle` | 1.0.14 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `anstyle-parse` | 1.0.0 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `anstyle-query` | 1.1.5 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `anstyle-wincon` | 3.0.11 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 | <https://github.com/dtolnay/anyhow> |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 | <https://github.com/bluss/arrayvec> |
| `ascii` | 1.1.0 | Apache-2.0 OR MIT | <https://github.com/tomprogrammer/rust-ascii> |
| `autocfg` | 1.5.1 | Apache-2.0 OR MIT | <https://github.com/cuviper/autocfg> |
| `base64` | 0.22.1 | MIT OR Apache-2.0 | <https://github.com/marshallpierce/rust-base64> |
| `bincode` | 1.3.3 | MIT | <https://github.com/servo/bincode> |
| `bindgen` | 0.72.1 | BSD-3-Clause | <https://github.com/rust-lang/rust-bindgen> |
| `bit-set` | 0.8.0 | Apache-2.0 OR MIT | <https://github.com/contain-rs/bit-set> |
| `bit-vec` | 0.8.0 | Apache-2.0 OR MIT | <https://github.com/contain-rs/bit-vec> |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 | <https://github.com/bitflags/bitflags> |
| `blake3` | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | <https://github.com/BLAKE3-team/BLAKE3> |
| `block-buffer` | 0.10.4 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| `borrow-or-share` | 0.2.4 | MIT-0 | <https://github.com/yescallop/borrow-or-share> |
| `bytecount` | 0.6.9 | Apache-2.0/MIT | <https://github.com/llogiq/bytecount> |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT | <https://github.com/Lokathor/bytemuck> |
| `bytemuck_derive` | 1.12.1 | Zlib OR Apache-2.0 OR MIT | <https://github.com/Lokathor/bytemuck> |
| `cc` | 1.4.7 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cc-rs> |
| `cexpr` | 0.6.0 | Apache-2.0/MIT | <https://github.com/jethrogb/rust-cexpr> |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cfg-if> |
| `chunked_transfer` | 1.5.0 | MIT OR Apache-2.0 | <https://github.com/frewsxcv/rust-chunked-transfer> |
| `clang-sys` | 1.9.1 | Apache-2.0 | <https://github.com/KyleMayes/clang-sys> |
| `clap` | 4.6.7 | MIT OR Apache-2.0 | <https://github.com/clap-rs/clap> |
| `clap_builder` | 4.6.7 | MIT OR Apache-2.0 | <https://github.com/clap-rs/clap> |
| `clap_derive` | 4.6.7 | MIT OR Apache-2.0 | <https://github.com/clap-rs/clap> |
| `clap_lex` | 1.1.1 | MIT OR Apache-2.0 | <https://github.com/clap-rs/clap> |
| `cmake` | 0.1.58 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cmake-rs> |
| `colorchoice` | 1.0.5 | MIT OR Apache-2.0 | <https://github.com/rust-cli/anstyle.git> |
| `constant_time_eq` | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 | <https://github.com/cesarb/constant_time_eq> |
| `core_maths` | 0.1.1 | MIT | <https://github.com/robertbastian/core_maths> |
| `cosmic-text` | 0.19.0 | MIT OR Apache-2.0 | <https://github.com/pop-os/cosmic-text> |
| `cpufeatures` | 0.2.17 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| `cpufeatures` | 0.3.1 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/utils> |
| `crc32fast` | 1.5.2 | MIT OR Apache-2.0 | <https://github.com/srijs/rust-crc32fast> |
| `crossbeam-deque` | 0.8.8 | MIT OR Apache-2.0 | <https://github.com/crossbeam-rs/crossbeam> |
| `crossbeam-epoch` | 0.9.21 | MIT OR Apache-2.0 | <https://github.com/crossbeam-rs/crossbeam> |
| `crossbeam-utils` | 0.8.23 | MIT OR Apache-2.0 | <https://github.com/crossbeam-rs/crossbeam> |
| `crypto-common` | 0.1.7 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| `deranged` | 0.5.8 | MIT OR Apache-2.0 | <https://github.com/jhpratt/deranged> |
| `digest` | 0.10.7 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/traits> |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 | <https://github.com/yaahc/displaydoc> |
| `either` | 1.18.0 | MIT OR Apache-2.0 | <https://github.com/rayon-rs/either> |
| `email_address` | 0.2.9 | MIT | <https://github.com/johnstonskj/rust-email_address.git> |
| `encoding_rs` | 0.8.35 | (Apache-2.0 OR MIT) AND BSD-3-Clause | <https://github.com/hsivonen/encoding_rs> |
| `enumflags2` | 0.7.12 | MIT OR Apache-2.0 | <https://github.com/meithecatte/enumflags2> |
| `enumflags2_derive` | 0.7.12 | MIT OR Apache-2.0 | <https://github.com/meithecatte/enumflags2> |
| `fancy-regex` | 0.14.0 | MIT | <https://github.com/fancy-regex/fancy-regex> |
| `fast2s` | 0.3.1 | MIT | <https://github.com/tyrchen/fast2s> |
| `fdeflate` | 0.3.7 | MIT OR Apache-2.0 | <https://github.com/image-rs/fdeflate> |
| `find-msvc-tools` | 0.1.13 | MIT OR Apache-2.0 | <https://github.com/rust-lang/cc-rs> |
| `find_cuda_helper` | 0.2.0 | MIT OR Apache-2.0 | <https://github.com/Rust-GPU/Rust-CUDA> |
| `flate2` | 1.1.10 | MIT OR Apache-2.0 | <https://github.com/rust-lang/flate2-rs> |
| `fluent-uri` | 0.3.2 | MIT | <https://github.com/yescallop/fluent-uri-rs> |
| `font-types` | 0.11.3 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `font-types` | 0.12.6 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `fontdb` | 0.23.0 | MIT | <https://github.com/RazrFalcon/fontdb> |
| `form_urlencoded` | 1.2.2 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| `fraction` | 0.15.4 | MIT OR Apache-2.0 | <https://github.com/dnsl48/fraction.git> |
| `fs_extra` | 1.3.0 | MIT | <https://github.com/webdesus/fs_extra> |
| `generic-array` | 0.14.7 | MIT | <https://github.com/fizyk20/generic-array.git> |
| `getrandom` | 0.2.17 | MIT OR Apache-2.0 | <https://github.com/rust-random/getrandom> |
| `getrandom` | 0.3.4 | MIT OR Apache-2.0 | <https://github.com/rust-random/getrandom> |
| `glob` | 0.3.4 | MIT OR Apache-2.0 | <https://github.com/rust-lang/glob> |
| `harfrust` | 0.5.2 | MIT | <https://github.com/harfbuzz/harfrust> |
| `hashbrown` | 0.12.3 | MIT OR Apache-2.0 | <https://github.com/rust-lang/hashbrown> |
| `heck` | 0.5.0 | MIT OR Apache-2.0 | <https://github.com/withoutboats/heck> |
| `httpdate` | 1.0.3 | MIT OR Apache-2.0 | <https://github.com/pyfisch/httpdate> |
| `icu_collections` | 2.1.1 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_locale_core` | 2.1.1 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_normalizer` | 2.1.1 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_normalizer_data` | 2.1.1 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_properties` | 2.1.2 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_properties_data` | 2.1.2 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `icu_provider` | 2.1.1 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `idna` | 1.1.0 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url/> |
| `idna_adapter` | 1.2.1 | Apache-2.0 OR MIT | <https://github.com/hsivonen/idna_adapter> |
| `is_terminal_polyfill` | 1.70.2 | MIT OR Apache-2.0 | <https://github.com/polyfill-rs/is_terminal_polyfill> |
| `itertools` | 0.13.0 | MIT OR Apache-2.0 | <https://github.com/rust-itertools/itertools> |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 | <https://github.com/dtolnay/itoa> |
| `jobserver` | 0.1.35 | MIT OR Apache-2.0 | <https://github.com/rust-lang/jobserver-rs> |
| `jsonschema` | 0.26.2 | MIT | <https://github.com/Stranger6667/jsonschema> |
| `kamadak-exif` | 0.6.1 | BSD-2-Clause | <https://github.com/kamadak/exif-rs> |
| `lazy_static` | 1.5.0 | MIT OR Apache-2.0 | <https://github.com/rust-lang-nursery/lazy-static.rs> |
| `libc` | 0.2.189 | MIT OR Apache-2.0 | <https://github.com/rust-lang/libc> |
| `libloading` | 0.8.9 | ISC | <https://github.com/nagisa/rust_libloading/> |
| `libloading` | 0.9.0 | ISC | <https://github.com/nagisa/rust_libloading/> |
| `libm` | 0.2.16 | MIT | <https://github.com/rust-lang/compiler-builtins> |
| `linebender_resource_handle` | 0.1.1 | Apache-2.0 OR MIT | <https://github.com/linebender/raw_resource_handle> |
| `litemap` | 0.8.3 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `llama-cpp-2` | 0.1.156 | MIT OR Apache-2.0 | <https://github.com/utilityai/llama-cpp-rs> |
| `llama-cpp-sys-2` | 0.1.156 | MIT OR Apache-2.0 | <https://github.com/utilityai/llama-cpp-rs> |
| `log` | 0.4.34 | MIT OR Apache-2.0 | <https://github.com/rust-lang/log> |
| `matchers` | 0.2.0 | MIT | <https://github.com/hawkw/matchers> |
| `memchr` | 2.8.3 | Unlicense OR MIT | <https://github.com/BurntSushi/memchr> |
| `memmap2` | 0.9.11 | MIT OR Apache-2.0 | <https://github.com/RazrFalcon/memmap2-rs> |
| `minimal-lexical` | 0.2.1 | MIT/Apache-2.0 | <https://github.com/Alexhuszagh/minimal-lexical> |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 | <https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide> |
| `mutate_once` | 0.1.2 | BSD-2-Clause | <https://github.com/kamadak/mutate_once-rs> |
| `nom` | 7.1.3 | MIT | <https://github.com/Geal/nom> |
| `nu-ansi-term` | 0.50.3 | MIT | <https://github.com/nushell/nu-ansi-term> |
| `num` | 0.4.3 | MIT OR Apache-2.0 | <https://github.com/rust-num/num> |
| `num-bigint` | 0.4.8 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-bigint> |
| `num-cmp` | 0.1.0 | MIT/Apache-2.0 | <https://github.com/lifthrasiir/num-cmp> |
| `num-complex` | 0.4.6 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-complex> |
| `num-conv` | 0.1.0 | MIT OR Apache-2.0 | <https://github.com/jhpratt/num-conv> |
| `num-integer` | 0.1.47 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-integer> |
| `num-iter` | 0.1.46 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-iter> |
| `num-rational` | 0.4.2 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-rational> |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 | <https://github.com/rust-num/num-traits> |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 | <https://github.com/matklad/once_cell> |
| `once_cell_polyfill` | 1.70.2 | MIT OR Apache-2.0 | <https://github.com/polyfill-rs/once_cell_polyfill> |
| `ort` | 2.0.0-rc.13 | MIT OR Apache-2.0 | <https://github.com/pykeio/ort> |
| `ort-sys` | 2.0.0-rc.13 | MIT OR Apache-2.0 | <https://github.com/pykeio/ort> |
| `outref` | 0.5.2 | MIT | <https://github.com/Nugine/outref> |
| `percent-encoding` | 2.3.2 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url/> |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT | <https://github.com/taiki-e/pin-project-lite> |
| `png` | 0.18.1 | MIT OR Apache-2.0 | <https://github.com/image-rs/image-png> |
| `potential_utf` | 0.1.6 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `powerfmt` | 0.2.0 | MIT OR Apache-2.0 | <https://github.com/jhpratt/powerfmt> |
| `prettyplease` | 0.2.37 | MIT OR Apache-2.0 | <https://github.com/dtolnay/prettyplease> |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 | <https://github.com/dtolnay/proc-macro2> |
| `quote` | 1.0.47 | MIT OR Apache-2.0 | <https://github.com/dtolnay/quote> |
| `rangemap` | 1.8.0 | MIT/Apache-2.0 | <https://github.com/jeffparsons/rangemap> |
| `rayon-core` | 1.13.0 | MIT OR Apache-2.0 | <https://github.com/rayon-rs/rayon> |
| `read-fonts` | 0.37.0 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `read-fonts` | 0.41.0 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `ref-cast` | 1.0.27 | MIT OR Apache-2.0 | <https://github.com/dtolnay/ref-cast> |
| `ref-cast-impl` | 1.0.27 | MIT OR Apache-2.0 | <https://github.com/dtolnay/ref-cast> |
| `referencing` | 0.26.2 | MIT | <https://github.com/Stranger6667/jsonschema> |
| `regex` | 1.13.1 | MIT OR Apache-2.0 | <https://github.com/rust-lang/regex> |
| `regex-automata` | 0.4.18 | MIT OR Apache-2.0 | <https://github.com/rust-lang/regex> |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 | <https://github.com/rust-lang/regex> |
| `ring` | 0.17.14 | Apache-2.0 AND ISC | <https://github.com/briansmith/ring> |
| `rustc-hash` | 2.1.3 | Apache-2.0 OR MIT | <https://github.com/rust-lang/rustc-hash> |
| `rustls` | 0.23.45 | Apache-2.0 OR ISC OR MIT | <https://github.com/rustls/rustls> |
| `rustls-pki-types` | 1.15.1 | MIT OR Apache-2.0 | <https://github.com/rustls/pki-types> |
| `rustls-webpki` | 0.103.15 | ISC | <https://github.com/rustls/webpki> |
| `same-file` | 1.0.6 | Unlicense/MIT | <https://github.com/BurntSushi/same-file> |
| `self_cell` | 1.3.0 | Apache-2.0 OR GPL-2.0-only | <https://github.com/Voultapher/self_cell> |
| `semver` | 1.0.28 | MIT OR Apache-2.0 | <https://github.com/dtolnay/semver> |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 | <https://github.com/serde-rs/serde> |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | <https://github.com/serde-rs/json> |
| `sha2` | 0.10.9 | MIT OR Apache-2.0 | <https://github.com/RustCrypto/hashes> |
| `sharded-slab` | 0.1.7 | MIT | <https://github.com/hawkw/sharded-slab> |
| `shlex` | 1.3.0 | MIT OR Apache-2.0 | <https://github.com/comex/rust-shlex> |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 | <https://github.com/comex/rust-shlex> |
| `simd-adler32` | 0.3.10 | MIT | <https://github.com/mcountryman/simd-adler32> |
| `skrifa` | 0.40.0 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `skrifa` | 0.44.0 | MIT OR Apache-2.0 | <https://github.com/googlefonts/fontations> |
| `slotmap` | 1.1.1 | Zlib | <https://github.com/orlp/slotmap> |
| `smallvec` | 1.16.1 | MIT OR Apache-2.0 | <https://github.com/servo/rust-smallvec> |
| `smol_str` | 0.3.2 | MIT OR Apache-2.0 | <https://github.com/rust-analyzer/smol_str> |
| `stable_deref_trait` | 1.2.1 | MIT OR Apache-2.0 | <https://github.com/storyyeller/stable_deref_trait> |
| `strsim` | 0.11.1 | MIT | <https://github.com/rapidfuzz/strsim-rs> |
| `subtle` | 2.6.1 | BSD-3-Clause | <https://github.com/dalek-cryptography/subtle> |
| `swash` | 0.2.10 | Apache-2.0 OR MIT | <https://github.com/dfrg/swash> |
| `syn` | 2.0.119 | MIT OR Apache-2.0 | <https://github.com/dtolnay/syn> |
| `syn` | 3.0.6 | MIT OR Apache-2.0 | <https://github.com/dtolnay/syn> |
| `synstructure` | 0.14.0 | MIT | <https://github.com/mystor/synstructure> |
| `sys-locale` | 0.3.2 | MIT OR Apache-2.0 | <https://github.com/1Password/sys-locale> |
| `thiserror` | 2.0.20 | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |
| `thiserror-impl` | 2.0.20 | MIT OR Apache-2.0 | <https://github.com/dtolnay/thiserror> |
| `thread_local` | 1.1.10 | MIT OR Apache-2.0 | <https://github.com/Amanieu/thread_local-rs> |
| `time` | 0.3.45 | MIT OR Apache-2.0 | <https://github.com/time-rs/time> |
| `time-core` | 0.1.7 | MIT OR Apache-2.0 | <https://github.com/time-rs/time> |
| `time-macros` | 0.2.25 | MIT OR Apache-2.0 | <https://github.com/time-rs/time> |
| `tiny_http` | 0.12.0 | MIT OR Apache-2.0 | <https://github.com/tiny-http/tiny-http> |
| `tinystr` | 0.8.4 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `tinyvec` | 1.13.3 | Zlib OR Apache-2.0 OR MIT | <https://github.com/Lokathor/tinyvec> |
| `tracing` | 0.1.44 | MIT | <https://github.com/tokio-rs/tracing> |
| `tracing-attributes` | 0.1.31 | MIT | <https://github.com/tokio-rs/tracing> |
| `tracing-core` | 0.1.36 | MIT | <https://github.com/tokio-rs/tracing> |
| `tracing-log` | 0.2.0 | MIT | <https://github.com/tokio-rs/tracing> |
| `tracing-subscriber` | 0.3.23 | MIT | <https://github.com/tokio-rs/tracing> |
| `ttf-parser` | 0.25.1 | MIT OR Apache-2.0 | <https://github.com/harfbuzz/ttf-parser> |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 | <https://github.com/paholg/typenum> |
| `unicode-bidi` | 0.3.18 | MIT OR Apache-2.0 | <https://github.com/servo/unicode-bidi> |
| `unicode-ident` | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | <https://github.com/dtolnay/unicode-ident> |
| `unicode-linebreak` | 0.1.5 | Apache-2.0 | <https://github.com/axelf4/unicode-linebreak> |
| `unicode-script` | 0.5.8 | MIT OR Apache-2.0 | <https://github.com/unicode-rs/unicode-script> |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 | <https://github.com/unicode-rs/unicode-segmentation> |
| `untrusted` | 0.9.0 | ISC | <https://github.com/briansmith/untrusted> |
| `ureq` | 2.12.1 | MIT OR Apache-2.0 | <https://github.com/algesten/ureq> |
| `url` | 2.5.8 | MIT OR Apache-2.0 | <https://github.com/servo/rust-url> |
| `utf8_iter` | 1.0.4 | Apache-2.0 OR MIT | <https://github.com/hsivonen/utf8_iter> |
| `utf8parse` | 0.2.2 | Apache-2.0 OR MIT | <https://github.com/alacritty/vte> |
| `uuid` | 1.26.1 | Apache-2.0 OR MIT | <https://github.com/uuid-rs/uuid> |
| `uuid-simd` | 0.8.0 | MIT | <https://github.com/Nugine/simd> |
| `version_check` | 0.9.5 | MIT/Apache-2.0 | <https://github.com/SergioBenitez/version_check> |
| `vsimd` | 0.8.0 | MIT | <https://github.com/Nugine/simd> |
| `walkdir` | 2.5.0 | Unlicense/MIT | <https://github.com/BurntSushi/walkdir> |
| `webpki-roots` | 0.26.11 | CDLA-Permissive-2.0 | <https://github.com/rustls/webpki-roots> |
| `webpki-roots` | 1.0.9 | CDLA-Permissive-2.0 | <https://github.com/rustls/webpki-roots> |
| `whisper-rs` | 0.16.0 | Unlicense | <https://codeberg.org/tazz4843/whisper-rs> |
| `whisper-rs-sys` | 0.15.0 | Unlicense | <https://codeberg.org/tazz4843/whisper-rs> |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 | <https://github.com/microsoft/windows-rs> |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | <https://github.com/microsoft/windows-rs> |
| `writeable` | 0.6.4 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `yazi` | 0.2.1 | Apache-2.0 OR MIT | <https://github.com/dfrg/yazi> |
| `yoke` | 0.8.3 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `yoke-derive` | 0.8.3 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zeno` | 0.3.3 | Apache-2.0 OR MIT | <https://github.com/dfrg/zeno> |
| `zerocopy` | 0.8.57 | BSD-2-Clause OR Apache-2.0 OR MIT | <https://github.com/google/zerocopy> |
| `zerofrom` | 0.1.8 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zerofrom-derive` | 0.1.8 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zeroize` | 1.9.0 | Apache-2.0 OR MIT | <https://github.com/RustCrypto/utils> |
| `zerotrie` | 0.2.5 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zerovec` | 0.11.8 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zerovec-derive` | 0.11.6 | Unicode-3.0 | <https://github.com/unicode-org/icu4x> |
| `zmij` | 1.0.23 | MIT | <https://github.com/dtolnay/zmij> |
| `zune-core` | 0.5.3 | MIT OR Apache-2.0 OR Zlib | <https://github.com/etemesi254/zune-image> |
| `zune-jpeg` | 0.5.15 | MIT OR Apache-2.0 OR Zlib | <https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg> |

## 6. Dart / Flutter packages / Dart 依赖

From `app/pubspec.lock`. "direct main" packages ship in the app; "direct dev"
and most transitive test packages (for example `fake_async`, `webdriver`, `leak_tracker*`) are build or
test only. Packages from the Flutter SDK are BSD-3-Clause.

| Package | Version | Dependency | License (LICENSE file) | Source |
|---|---|---|---|---|
| `async` | 2.13.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/async> |
| `boolean_selector` | 2.1.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/boolean_selector> |
| `characters` | 1.4.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/characters> |
| `clock` | 1.1.3 | transitive | Apache-2.0 | <https://pub.dev/packages/clock> |
| `collection` | 1.19.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/collection> |
| `cross_file` | 0.3.5+5 | transitive | BSD-3-Clause | <https://pub.dev/packages/cross_file> |
| `csslib` | 1.0.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/csslib> |
| `fake_async` | 1.3.3 | transitive | Apache-2.0 | <https://pub.dev/packages/fake_async> |
| `ffi` | 2.2.0 | direct main | BSD-3-Clause | <https://pub.dev/packages/ffi> |
| `file` | 7.0.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/file> |
| `file_selector_linux` | 0.9.4+1 | transitive | BSD-3-Clause | <https://pub.dev/packages/file_selector_linux> |
| `file_selector_macos` | 0.9.5+1 | transitive | BSD-3-Clause | <https://pub.dev/packages/file_selector_macos> |
| `file_selector_platform_interface` | 2.7.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/file_selector_platform_interface> |
| `file_selector_windows` | 0.9.3+6 | transitive | BSD-3-Clause | <https://pub.dev/packages/file_selector_windows> |
| `flutter_lints` | 4.0.0 | direct dev | BSD-3-Clause | <https://pub.dev/packages/flutter_lints> |
| `flutter_plugin_android_lifecycle` | 2.0.35 | transitive | BSD-3-Clause | <https://pub.dev/packages/flutter_plugin_android_lifecycle> |
| `html` | 0.15.7 | transitive | MIT | <https://pub.dev/packages/html> |
| `http` | 1.6.0 | direct main | BSD-3-Clause | <https://pub.dev/packages/http> |
| `http_parser` | 4.1.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/http_parser> |
| `image_picker` | 1.2.3 | direct main | BSD-3-Clause | <https://pub.dev/packages/image_picker> |
| `image_picker_android` | 0.8.13+23 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_android> |
| `image_picker_for_web` | 3.1.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_for_web> |
| `image_picker_ios` | 0.8.13+8 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_ios> |
| `image_picker_linux` | 0.2.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_linux> |
| `image_picker_macos` | 0.2.2+1 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_macos> |
| `image_picker_platform_interface` | 2.11.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_platform_interface> |
| `image_picker_windows` | 0.2.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/image_picker_windows> |
| `intl` | 0.20.3 | transitive | BSD-3-Clause | <https://pub.dev/packages/intl> |
| `leak_tracker` | 11.0.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/leak_tracker> |
| `leak_tracker_flutter_testing` | 3.0.10 | transitive | BSD-3-Clause | <https://pub.dev/packages/leak_tracker_flutter_testing> |
| `leak_tracker_testing` | 3.0.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/leak_tracker_testing> |
| `lints` | 4.0.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/lints> |
| `matcher` | 0.12.20 | transitive | BSD-3-Clause | <https://pub.dev/packages/matcher> |
| `material_color_utilities` | 0.13.0 | transitive | Apache-2.0 | <https://pub.dev/packages/material_color_utilities> |
| `meta` | 1.18.3 | transitive | BSD-3-Clause | <https://pub.dev/packages/meta> |
| `mime` | 2.1.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/mime> |
| `path` | 1.9.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/path> |
| `platform` | 3.2.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/platform> |
| `plugin_platform_interface` | 2.1.8 | transitive | BSD-3-Clause | <https://pub.dev/packages/plugin_platform_interface> |
| `process` | 5.0.6 | transitive | BSD-3-Clause | <https://pub.dev/packages/process> |
| `source_span` | 1.10.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/source_span> |
| `stack_trace` | 1.12.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/stack_trace> |
| `stream_channel` | 2.1.4 | transitive | BSD-3-Clause | <https://pub.dev/packages/stream_channel> |
| `string_scanner` | 1.4.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/string_scanner> |
| `sync_http` | 0.3.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/sync_http> |
| `term_glyph` | 1.2.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/term_glyph> |
| `test_api` | 0.7.12 | transitive | BSD-3-Clause | <https://pub.dev/packages/test_api> |
| `typed_data` | 1.4.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/typed_data> |
| `vector_math` | 2.4.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/vector_math> |
| `video_player` | 2.14.0 | direct main | BSD-3-Clause | <https://pub.dev/packages/video_player> |
| `video_player_android` | 2.12.2 | transitive | BSD-3-Clause | <https://pub.dev/packages/video_player_android> |
| `video_player_avfoundation` | 2.12.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/video_player_avfoundation> |
| `video_player_platform_interface` | 6.9.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/video_player_platform_interface> |
| `video_player_web` | 2.4.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/video_player_web> |
| `vm_service` | 15.3.0 | transitive | BSD-3-Clause | <https://pub.dev/packages/vm_service> |
| `web` | 1.1.1 | transitive | BSD-3-Clause | <https://pub.dev/packages/web> |
| `webdriver` | 3.2.0 | transitive | Apache-2.0 | <https://pub.dev/packages/webdriver> |

---

## Required notices

The licenses above that ask for it require their copyright notice and license
to accompany copies of the software. The notices are:

**MIT** — whisper.cpp / ggml: Copyright (c) 2023-2024 The ggml authors.
llama.cpp / ggml: Copyright (c) 2023-2026 The ggml authors.
ONNX Runtime: Copyright (c) Microsoft Corporation.
OpenAI Whisper: Copyright (c) 2022 OpenAI.
YuNet: Copyright (c) 2020 Shiqi Yu.
nlohmann/json: Copyright (c) 2013-2025 Niels Lohmann.
cpp-httplib: Copyright (c) 2017 yhirose.
PhotoFramer: the authors listed above.

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

**BSD-3-Clause** — Flutter and the Flutter team's packages (`video_player`,
`image_picker`, `http`, `ffi`, …): Copyright 2014 The Flutter Authors. All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the conditions of the BSD-3-Clause
license are met (retain this notice, reproduce it in documentation for binary
forms, and do not use the names of the copyright holders to endorse derived
products without permission). The software is provided "as is" without
warranty of any kind.

**Apache-2.0** — Qwen3-VL (Qwen team, Alibaba Cloud), Depth Anything V2 Small,
PP-HumanSeg (Copyright (c) 2021 PaddlePaddle Authors), AuraFace v1 (fal),
RTMO (OpenMMLab), SigLIP 2 (Google), Material Icons (Google), AndroidX Media3,
the Gradle Wrapper, and the Apache-licensed crates and Dart packages listed
above. Licensed under the Apache License, Version 2.0
(<https://www.apache.org/licenses/LICENSE-2.0>); distributed on an "AS IS"
basis, without warranties or conditions of any kind. None of the model
repositories above publishes a `NOTICE` file, and no crate in the tree ships one;
if an upstream adds one, its content must be reproduced here.

**LGPL-2.1-or-later** — FFmpeg (<https://ffmpeg.org/legal.html>). This
application uses libraries from the FFmpeg project under the LGPLv2.1. The
FFmpeg source for the version shipped (9.0.1) is <https://ffmpeg.org/releases/ffmpeg-9.0.1.tar.xz>,
and the exact configure flags are in `scripts/build-ffmpeg-*.sh`. The
libraries are dynamic and can be replaced by the user.

**Zlib** — zlib: Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler.

**Public domain / Unlicense / MIT-0** — stb_image (Sean Barrett), miniaudio
(David Reid), subprocess.h (Neil Henning), whisper-rs, Natural Earth: no
notice is required; credited here with thanks.
