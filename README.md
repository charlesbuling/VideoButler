<div align="center">

<!-- TODO: 仓库里还没有标志图。有图之后再放居中的 logo，例如 ![Video Butler](docs/logo.png)。不要放一张不存在的图片。 -->

# Video Butler

视频管家 · 在自己的设备上，把素材剪成一份还能继续改的草稿

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release v0.2.0](https://img.shields.io/badge/release-v0.2.0-green.svg)](https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20iOS%20%7C%20Android%20%7C%20Linux-lightgrey.svg)](https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0)

中文 | [English](README_en.md)

</div>

设备端的 AI 视频粗剪。导入本地视频和照片，看懂它们，交回一份**草稿工程**，由你接着剪。可以完全离线，跑在自己的电脑和手机上。

AI 不交出一条已经压扁的成片当作唯一结果。它交出一份剪辑决策（Editing DSL）：每一刀、每一句字幕、每一次转场和音乐，都可以改回来，不必把整条流水线重跑一遍。

目标是一份合格的粗剪，加上人的精修，不是无人值守的导演。

> 仓库在版权人公开之前仍是私有的。下面的 Release 链接按公开后的地址写。Release 还没有挂上时，用「从源码构建」。

---

# 这个项目做什么

* 素材留在你的磁盘上。分析、选片、成片描述都在本机完成。云端接口是可选的加速，不是前提。
* 一条流水线读懂镜头：画面稳不稳、曝光、运动、场景，以及（装了模型之后）遮挡、构图、姿态和一句话字幕。字幕写的是画面里有什么，不是把假期里的话转写成文字。
* 按心情、题材、要不要卡点，从场景里选片段，切在拍到的时刻上。时间在契约里一律是整数微秒。
* 草稿可以交给剪映 / CapCut，或写成 OpenTimelineIO。macOS 上也可以在软件里改完再导出视频；能无损拷贝的成片就直接拷贝原片，不重新编码。
* 视频管家把文件夹里的视频和照片归档：人物是按脸的相似度分组，不是认人；回忆（旅行、一年、孩子长大、结伴出现的人）可以再变成一个工程。

---

# 功能

- **非破坏。** 决策在 Editing DSL 里。内置编辑器的每一步都是一次操作，引擎回完整状态。撤销是整份草稿。
- **离线。** 不下载模型也能用规则剪出一条草稿（`vb run --offline`）。语音、景深、姿态、字幕是装上对应权重之后才有的。
- **看素材。** 扫一遍镜头，按画面变化分段，再按长得像不像、拍得近不近并成场景。导演用景深、脸和人像看有没有东西挡住画面。装了 PhotoFramer 时，对入选画面再打一次构图分。
- **卡点与舞蹈。** 音乐或素材自己的声音可以出拍。舞蹈和「主体衔接」按姿态把同一段动作的多次拍摄接在一条时间上。
- **视频管家。** 给人脸分组、按规则分类、找回忆，并在地图上标出有位置的回忆。底图是 Natural Earth 的公有领域轮廓，不是在线地图。
- **照片也是素材。** 与周围拍摄的视频按时间和画面签名进同一批场景。
- **你接着剪。** macOS 上的内置编辑器可以分割、修剪、变速、文字、音乐，并导出视频、无损拷贝、分条素材，或再写一份剪映 / OTIO 草稿。

还没有的，见下面的现状表，而不是当成已经完成。

---

# 现状

版本 **0.2.0**（`core/Cargo.toml` 与 `app/pubspec.yaml` 相同）。

| 层 | 状态 |
|---|---|
| 两份契约（Footage Graph、Editing DSL）与往返测试 | 已有 |
| 镜头分段、场景、古典画质 | 已有 |
| 心情 × 题材 × 卡点 | 已有，规则编排，不是模型在导演 |
| 节拍 | 已有；卡点时刀落在格子上 |
| 语音（whisper.cpp） | macOS、Android、iOS、本机编译的 Linux 引擎；要自己装模型 |
| 场景标签与人脸（Apple Vision） | 仅 Apple 平台，不用下载 |
| 景深、人脸检测、人像、姿态、SigLIP 场景标签、AuraFace 分组 | 有 ONNX Runtime 的构建（macOS、Android）。iOS 没有。Linux 目前也没有 |
| 字幕（Qwen3-VL）与构图分（PhotoFramer） | 有 llama.cpp 的构建（macOS、Android）。iOS 没有。PhotoFramer 仍是本地转换，目录里不提供下载 |
| 人物分组 | 已有。是分组，不是识别 |
| 剪映 / CapCut 草稿、OTIO | 已有。FCPXML 写入器还没有 |
| 内置编辑器与成片导出 | macOS 上可以渲染。Android、Windows、Linux 还没有接上平台编码器；无损拷贝是这些构建能做的视频导出 |
| 视频管家、回忆、地图、照片 | 已有 |
| 云端（OpenAI 兼容） | 看图可以；还没有用它来驱动剪辑 |
| macOS 应用 | arm64，macOS 12+，包内是 LGPL ffmpeg。未公证 |
| iOS | 模拟器能跑。设备构建能链接，未签名。没有 ONNX Runtime 和 llama.cpp |
| Android | arm64 APK，模拟器和实机跑过。Release 用调试密钥签名 |
| Linux | 只能在 Linux 上编出应用。引擎可以从 Mac 交叉编译。应用还没有实际跑过 |
| Windows | **0.2.0 已提供** x64 安装包（见下方 Release）。源码与 `WINDOWS.md`、`scripts/build-windows.sh` / `package-windows.sh` 在本仓库 |

---

# 更新

- **2026-10-18** — 视频管家收录照片，并可以按调用方给出的文件列表归档；APK 带上 AuraFace。
- **2026-10-17** — 0.2.0：无损导出（拷贝原片，不重新编码）、AGPL、安装包整理。
- **2026-10-12** — 0.1.5：内置编辑器的时间线，渲染字幕、画面和声音，以及变速。
- **2026-10-09** — 0.1.4：照片、视频管家和回忆地图。
- **2026-09-30** — 0.1.1。

更早的记录在 Git 历史里。产品决策以 `docs/AI Video Auto-Editing — Development Documentation.md` 为准。

---

# 下载与安装

0.2.0 的安装包在 GitHub Release **`v0.2.0`**：

`https://github.com/charlesbuling/VideoButler/releases/tag/v0.2.0`

安装包在 `dist/` 里生成，`dist/` 不进 Git。发布到 Release 之后从这里下载，而不是从仓库源码树里找二进制。

| 资产 | 是什么 |
|---|---|
| `VideoButler-0.2.0-macOS-arm64.dmg` | 只有应用（约 35 MB），不含模型 |
| `VideoButler-0.2.0-android-arm64.apk` | arm64 APK |
| `VideoButler-0.2.0-ios-unsigned.ipa` | 未签名的 iOS 包 |
| `SHA256SUMS.txt` | 上面安装包的 sha256 |

模型不作为 Release 资产发布，而是从各自的 Hugging Face 官方地址下载，见下面「模型文件从哪里下载」。

Windows x64：`VideoButler-0.2.0-windows-x64.zip`（约 31 MB，不含模型；模型经 Hugging Face / `scripts/fetch-models.sh`）。

仓库公开、Release 挂上之前，这些 URL 不存在。那时请从源码构建。

## macOS

Apple 芯片，macOS 12 或更新。打开磁盘映像，把 `video_butler.app` 拖到「应用程序」。

应用是 ad hoc 签名，**没有公证**。第一次打开要走系统设置 > 隐私与安全性 > 仍要打开，或在 Finder 里右键 > 打开。签名每次构建都不同，系统会再次询问磁盘、桌面、文稿和下载文件夹的权限。

模型不在这个 DMG 里。用下一节的脚本装进模型库，或在应用的「模型」页下载（那条路要联网，从上游拉取）。

## Android

arm64。这个 APK 用 **调试密钥** 签名（`app/android/app/build.gradle.kts` 里的 release 构建使用 debug 签名），不是应用商店的签名。安装前允许此来源。只有用同一把调试密钥签的包才能覆盖安装。

APK 装的是应用。权重用 `scripts/fetch-models.sh` 从 Hugging Face 下载。体积小的模型在用 `scripts/build-android.sh` 打包时，如果当时模型目录里有，会打进 APK；Qwen 和较大的 Whisper 文件放不进 APK（签名不接受超过 4GB 的 ZIP），所以要另外下载。装到手机上的 OBB 目录、或在应用里「从文件夹导入」，见 `scripts/package-android.sh` 里的安装脚本。

## iOS

Release 里的 IPA **没有签名**。本仓库的 `scripts/build-ios.sh --device` 产出的是未签名的 `Runner.app`，不是 IPA；未签名 IPA 就是把该 `.app` 放进 `Payload/` 再压缩。装到真机上仍然要在 Xcode 里用你自己的签名团队签名。没有 Apple 账号时，用模拟器构建：`scripts/build-ios.sh`。

这部构建没有 ONNX Runtime，也没有 llama.cpp。景深、YuNet、人像分割、姿态、SigLIP、字幕和构图分都不在。Apple Vision、whisper.cpp、节拍和剪辑在。运行结果里会写明哪些适配器没有跑。

## Linux

这次 Release 没有预编译的 Linux 应用。在 Linux 上用 `scripts/build-linux.sh` 从源码构建。这个应用还没有在 Linux 上实际跑过。

## 模型文件从哪里下载

模型**不在 Release 上**。默认从每个模型的 Hugging Face 官方地址下载；地址、字节数和 sha256 与 `core/vb-pipeline/src/models.rs` 一致，也写在仓库根目录的 `models-manifest.json` 里，`scripts/fetch-models.sh` 读的就是它。

```bash
./scripts/fetch-models.sh --dry-run --platform macos   # 只列出会下载什么、多大，不下载
./scripts/fetch-models.sh --base --platform android    # 该平台的基础模型
./scripts/fetch-models.sh --platform macos             # 该平台所有能下载的模型
./scripts/fetch-models.sh ggml-base.bin rtmo-s.onnx    # 指定文件
./scripts/fetch-models.sh --out /path/to/models
./scripts/fetch-models.sh --source github ggml-base.bin  # 先试 GitHub Release 镜像
```

每个文件依次尝试 Hugging Face、`hf-mirror.com`、GitHub Release 镜像（清单里有时）；`--source github` 时先试 GitHub Release。下载可断点续传（`curl -C -`），下完核对字节数和 sha256，不符就不安装。目标目录里已有且 sha256 相符的文件会跳过。

基础模型（`--base`，取自清单里的 `base`）：

- Android：`depth-anything-v2-small.onnx`、`face_detection_yunet_2023mar.onnx`、`human_segmentation_pphumanseg_2023mar.onnx`、`rtmo-s.onnx`、`ggml-base-q5_1.bin`、`siglip2-base-patch16-224-vision-fp16.onnx`、`auraface-v1-glintr100.onnx`，约 621 MiB
- macOS：同上，不含 SigLIP 2，约 443 MiB
- iOS：`ggml-base-q5_1.bin`，约 57 MiB

GitHub Release 镜像是可选的。清单的 `github_release_base` 是 `v0.2.0` 的下载地址，`github_parts` 写明资产名。大于 1.9 GiB 的文件（`qwen3-vl-8b-instruct-q4_k_m.gguf`、`qwen3-vl-4b-instruct-q4_k_m.gguf`、`photoframer-7b-q4_k_m.gguf`）按 `split -b 1900M -d -a 2` 切成 `<文件名>.part00`、`.part01`……，脚本下完各段再拼回去，核对的是整个文件。0.2.0 的 Release 现在没有挂模型文件，所以这条路默认用不上；换镜像设 `VB_RELEASE_BASE`，换清单设 `VB_MODELS_MANIFEST`（路径或 URL）。

手动下载：

```bash
curl -fL -C - -o ggml-base.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin
shasum -a 256 ggml-base.bin   # 和清单里的 sha256 对比
```

模型目录与 `core/vb-pipeline/src/library.rs` 一致：

| 条件 | 目录 |
|---|---|
| 设置了 `VB_LIBRARY_DIR` | `$VB_LIBRARY_DIR/models` |
| 否则设置了 `VB_WORKDIR`（手机由宿主传入） | `$VB_WORKDIR/library/models` |
| macOS | `~/Library/Application Support/VideoButler/models` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/video-butler/models` |

---

# 模型

权重许可只有 MIT、Apache-2.0 或 BSD-3-Clause。字节数和 sha256 来自 `core/vb-pipeline/src/models.rs`，是目录里记录的上游文件，不是估出来的。

| 文件 | 作用 | 字节 | 许可证 | 现在用在 |
|---|---|---:|---|---|
| `ggml-tiny.bin` | 语音 | 77691713 | MIT | macOS、Android、iOS、本机 Linux 引擎 |
| `ggml-base-q5_1.bin` | 语音 | 59707625 | MIT | 同上。Android APK 打包时打进包内的是这一档 |
| `ggml-base.bin` | 语音 | 147951465 | MIT | 同上 |
| `ggml-small-q5_1.bin` | 语音 | 190085487 | MIT | 同上 |
| `ggml-small.bin` | 语音 | 487601967 | MIT | 同上 |
| `depth-anything-v2-small.onnx` | 景深 | 99060839 | Apache-2.0 | macOS、Android |
| `face_detection_yunet_2023mar.onnx` | 人脸检测 | 232589 | MIT | macOS、Android |
| `auraface-v1-glintr100.onnx` | 人脸分组 | 260694151 | Apache-2.0 | macOS、Android |
| `human_segmentation_pphumanseg_2023mar.onnx` | 人像分割 | 6163938 | Apache-2.0 | macOS、Android |
| `rtmo-s.onnx` | 人体姿态 | 39636400 | Apache-2.0 | macOS、Android |
| `siglip2-base-patch16-224-vision-fp16.onnx` | 场景标签 | 186039516 | Apache-2.0 | 没有 Apple Vision 的构建（Android）。Mac 不用它 |
| `qwen3-vl-2b-instruct-q4_k_m.gguf` 及其 mmproj | 字幕 | 1107409952 + 819394848 | Apache-2.0 | macOS、Android |
| `qwen3-vl-4b-instruct-q4_k_m.gguf` 及其 mmproj | 字幕 | 2497281664 + 836180256 | Apache-2.0 | macOS、Android |
| `qwen3-vl-8b-instruct-q4_k_m.gguf` 及其 mmproj | 字幕 | 5027784800 + 1159029824 | Apache-2.0 | macOS、Android |
| `photoframer-7b-q4_k_m.gguf` 及其 mmproj | 构图 | 4683073792 + 1354163232（本机转换产物） | MIT | macOS、Android。暂无公开下载，托管待定 |

Qwen 的投影文件名是 `qwen3-vl-8b-instruct-mmproj-f16.gguf`、`qwen3-vl-4b-instruct-mmproj-f16.gguf`、`qwen3-vl-2b-instruct-mmproj-f16.gguf`。上游 URL 与 sha256 在 `NOTICE` 和 `models.rs`。

PhotoFramer 的上游是 PyTorch 权重，由 `scripts/convert-photoframer.sh` 转成 GGUF。应用在模型库里看到这两个文件才会用：`photoframer-7b-q4_k_m.gguf`、`photoframer-7b-mmproj-f16.gguf`。`models.rs` 的下载目录里没有它们；`models-manifest.json` 记录了转换产物的字节数和 sha256。转换后的 GGUF 在哪里公开托管**还没有定**，所以 `fetch-models.sh` 默认跳过这两个文件。有了地址后，设 `VB_PHOTOFRAMER_URL`（权重）和 `VB_PHOTOFRAMER_MMPROJ_URL`（投影文件）即可下载，并按清单校验。

应用按角色选用已安装且放得进内存的模型。没有字幕模型，成片就没有字幕，运行说明里会写。

---

# 快速开始

命令行只需要 Rust 和 ffmpeg。不装模型、断网也可以出一份 OTIO 草稿：

```bash
./scripts/make-test-media.sh
cargo run -p vb-cli --manifest-path core/Cargo.toml -- doctor
cargo run -p vb-cli --manifest-path core/Cargo.toml -- \
  run test-media/sample.mp4 --out draft.otio --offline
```

这会写出草稿、Footage Graph 和 Editing DSL。后两份是契约，故意写成可读的 JSON。

按应用的方式跑一个文件夹，并给剪映写草稿：

```bash
cargo install --path core/vb-cli --manifest-path core/Cargo.toml  # 或直接用 cargo run
vb run ~/Movies/kyoto --mood happy --topic travel --beat \
  --music ~/Music/track.mp3 --title "Kyoto" --jianying
```

macOS 应用（需要完整的 Xcode）：

```bash
./scripts/build-macos-native.sh
open app/build/macos/Build/Products/Release/video_butler.app
```

没有 Xcode 时，`./scripts/run-macos.sh` 用同一套 Dart 界面和引擎，在本机 `http://127.0.0.1:8787` 提供网页。网页构建能看缩略图，不能播放本地文件。

---

# 从源码构建

共同的东西：Rust（工作区要求 1.85，edition 2024）、Flutter，以及各平台脚本里写明的工具链。`scripts/env.sh` 集中放路径，不要改 shell 配置文件来迁就这一份检出。

发布用的 ffmpeg 必须是这些脚本编出来的 LGPL 构建。开发机上的 Homebrew ffmpeg 可以是 GPL，不能拿去发布。

## macOS

```bash
./scripts/build-macos-native.sh          # 应用，LGPL ffmpeg 打进包里
./scripts/package-macos.sh --app-only    # 只有应用的 DMG，对应 Release 里的那个资产
```

需要 Xcode。部署目标是 macOS 12。未公证。模型另装，见上一节。

## Android

Gradle Wrapper 已经在仓库里（Gradle 9.3.1，`app/android/gradlew`）。还需要 Android SDK、NDK、Rust 的 Android 目标，以及脚本去编的 LGPL ffmpeg。不要指望只跑 `./gradlew`：引擎的 `.so` 由打包脚本编出来。

小模型要先放在 `VB_MODELS_DIR`（默认是 macOS 的模型库）。用 `fetch-models.sh` 从 Hugging Face 准备这个目录（`--base --platform android` 就是这七个文件）：

```bash
./scripts/fetch-models.sh --out "$HOME/android-models" --base --platform android

VB_MODELS_DIR="$HOME/android-models" ./scripts/build-android.sh
```

`build-android.sh` 把上面这些打进 APK。放不进 APK 的权重要放在同一个目录里，再由打包脚本拷到安装目录：

```bash
./scripts/fetch-models.sh --out "$HOME/android-models"
VB_MODELS_DIR="$HOME/android-models" ./scripts/package-android.sh
```

`package-android.sh` 会要齐目录里那些较大的 Whisper 和 Qwen 文件；PhotoFramer 只有本机已经转换过才放进去，没有就跳过。安装到手机的步骤在它生成的 `安装到手机.command` 里：APK 用 adb 安装，模型推进应用的 OBB 目录。

在 Linux 上 `--out` 同样适用。`VB_MODELS_DIR` 指向那个目录，脚本就不会去找 Mac 的 `~/Library/Application Support/VideoButler/models`。

## iOS

只能在 Mac 上编，需要 Xcode。

```bash
./scripts/build-ios.sh             # 模拟器
./scripts/build-ios.sh --device    # 未签名的设备 .app
```

没有 ONNX Runtime，没有 llama.cpp。whisper.cpp 在。部署目标见 Xcode 工程（当前是 iOS 15）。

## Linux

在 Linux 上：

```bash
sudo apt install clang cmake ninja-build pkg-config libgtk-3-dev
./scripts/build-linux.sh
```

产物在 `app/build/linux/x64/release/bundle`，运行 `bundle/video_butler`。本机编译包含 whisper.cpp。ONNX Runtime 和 llama.cpp 还没有接上。应用还没有在 Linux 上跑过。

从 Mac 上跑同一个脚本只会交叉编译引擎（Zig），编不出 Linux 应用。

## Windows

0.2.0 起 Windows x64 应用与本仓库一并发布。在 Windows 上用 Git Bash：

```bash
./scripts/build-windows.sh
./scripts/package-windows.sh --no-build
```

说明见 `WINDOWS.md`（工具链、MSYS2 编 ffmpeg、ONNX Runtime、剪映路径等）。安装包是 Release 目录的 zip：`video_butler.exe` 与旁边的 `vb_ffi.dll`、`vb_llama_engine.dll`、ONNX Runtime 与 LGPL ffmpeg DLL。模型不打进包，用 `scripts/fetch-models.sh` 或应用内「模型」页从 Hugging Face 下载。

历史私有仓库 `charlesbuling/VideoBulter-windows` 仅作开发参考；请以本仓库为准。

---

# 架构

依赖只朝一个方向。`vb-narrative` 只依赖 `vb-contracts` 和 `vb-infer`，有测试守着。

```
contracts/            JSON Schema
    |
vb-contracts          Rust 类型与校验
    |
    +-- vb-media      ffmpeg，MediaBackend（命令行或进程内 libav）
    +-- vb-geo        离线地名与世界轮廓
    +-- vb-infer      适配器接口
    |     +-- vb-whisper    whisper.cpp
    |     +-- vb-apple      Apple Vision
    |     +-- vb-onnx       ONNX Runtime（运行时加载）
    |     +-- vb-llama      加载 vb-llama-engine（llama.cpp，单独的动态库）
    |     +-- vb-cloud      唯一允许联网的 crate
    |
vb-perception         写出 Footage Graph，并找拍
    |
vb-narrative          先验与编排，写出 Editing DSL
    |
    +-- vb-edit       编辑器操作
    +-- vb-render     DSL 到成片（像素在 Rust 里合成）
    |     +-- vb-text
    +-- vb-export     OTIO、剪映 / CapCut 草稿
    |
vb-pipeline           运行、缓存、工程、模型库、视频管家、编辑会话
    |
    +-- vb-cli        vb，含 vb serve 与 vb call
    +-- vb-ffi        C ABI
          |
        app/          一套 Flutter 界面。原生走 FFI，网页走本机 HTTP
```

三套前端共用 `vb-pipeline::api`。加一个能力是在那里加一条分发，而不是给某个前端单独开接口。

两件已经付出过代价的约定：

**时间是整数微秒。** 字段以 `Us` 结尾。契约里没有浮点时长，也没有帧号。

**DSL 不存绝对时间轴位置。** 槽只声明顺序和时长；字和声音锚在 `{slotId, offsetUs}`。转场重叠和片头会让「前面时长之和」不等于成片上的位置。绝对时间只在渲染时走一遍槽算出来。

---

# 仓库结构

```
core/            Rust 工作区（引擎）
app/             Flutter 应用（macOS、iOS、Android、Linux、Windows 壳）
contracts/       JSON Schema 与示例
scripts/         构建、打包、fetch-models.sh
docs/            架构、产品与路线文档
PRD.md           产品需求
prior-store/     叙事先验
NOTICE           版权与第三方
CLA.md           贡献者许可协议
CONTRIBUTING.md
```

产品需求见 [PRD.md](PRD.md)，架构见 [docs/architecture.md](docs/architecture.md)，构建与检查命令见 [CONTRIBUTING.md](CONTRIBUTING.md)。产品决定以 `docs/AI Video Auto-Editing — Development Documentation.md` 为准；与其他文档不一致时，以那份文档为准。

---

# 参与贡献

见 [CONTRIBUTING.md](CONTRIBUTING.md)。拉取请求对着 **`macos-app`**。没有自动 CI，推送前在本地跑 CONTRIBUTING.md 里的检查。

你保留版权。合并的前提是接受 [CLA.md](CLA.md)（拉取请求里勾选，并且每个提交有 `Signed-off-by`）。维护者可以不合并，而自己把想法重新实现。商业授权写 **217010072@link.cuhk.edu.cn**。

行为准则是 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。安全问题写 [SECURITY.md](SECURITY.md)，不要开公开 issue。

---

# 第三方

链接方式在 [NOTICE](NOTICE)，完整的第三方名单、许可证和版权声明在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。短的版本：

- 发布用的 ffmpeg 是 LGPL 构建，动态链接或按动态库放进应用，以便替换。
- whisper.cpp、llama.cpp、ONNX Runtime 是 MIT。llama.cpp 和 ONNX Runtime 在运行时加载，不和引擎静态链在一起。
- 模型权重的许可证和上游 URL 在 `NOTICE` 与 `core/vb-pipeline/src/models.rs`。
- 地图轮廓是 Natural Earth，公有领域。
- Gradle Wrapper 是 Apache-2.0，属于 Gradle，不是本项目的代码。

不要加入 CoTracker3、MMAudio（CC-BY-NC），也不要加入别人的 copyleft 代码。

---

# 🙏 致谢

视频管家站在许多优秀开源项目和开放模型的肩膀上。没有它们，就没有一个能在本机离线剪片的视频管家。衷心感谢这些项目的作者、维护者和贡献者：

- [FFmpeg](https://ffmpeg.org/)：解码、抽帧、封装，几乎所有和视频打交道的地方都靠它。
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) 与 [OpenAI Whisper](https://github.com/openai/whisper)：离线语音识别。
- [llama.cpp / ggml](https://github.com/ggml-org/llama.cpp)：在本机跑视觉语言模型。
- [ONNX Runtime](https://github.com/microsoft/onnxruntime)：运行深度、人脸、人像分割、姿态和场景模型。
- [Flutter](https://flutter.dev/)：一套界面跑在 macOS、iOS、Android 和桌面上。
- [Qwen3-VL](https://huggingface.co/Qwen)（通义千问团队）：看懂画面、写出描述。
- [PhotoFramer](https://zhiyuanyou.github.io/photoframer/)：构图评分。
- [Depth Anything V2](https://github.com/DepthAnything/Depth-Anything-V2)、[YuNet](https://github.com/opencv/opencv_zoo)、[PP-HumanSeg](https://github.com/PaddlePaddle/PaddleSeg)、[AuraFace](https://huggingface.co/fal/AuraFace-v1)、[RTMO / MMPose](https://github.com/open-mmlab/mmpose)、[SigLIP 2](https://huggingface.co/google/siglip2-base-patch16-224)：深度、人脸、人像、姿态与场景理解。
- Rust 生态里的 [ort](https://github.com/pykeio/ort)、[whisper-rs](https://codeberg.org/tazz4843/whisper-rs)、[llama-cpp-rs](https://github.com/utilityai/llama-cpp-rs)、[cosmic-text](https://github.com/pop-os/cosmic-text)、[serde](https://serde.rs/) 等。
- [Natural Earth](https://www.naturalearthdata.com/)：回忆地图的公有领域地图数据。
- 借鉴思路（未使用其代码）：[World Monitor](https://github.com/koala73/worldmonitor) 的地图风格，[LosslessCut](https://github.com/mifi/lossless-cut) 的无损导出思路，剪映的编辑器体验。

完整名单（全部原生库、模型、Rust crate 和 Dart 包，以及各自的许可证与必需的版权声明）见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。模型权重各自保留其原有许可证，不随本项目改变；视频管家本身以 AGPL-3.0-or-later 授权（见 [LICENSE](LICENSE)、[NOTICE](NOTICE)）。

---

# Star

<!-- TODO: 仓库公开并有了 star 之后，再嵌 Star History 图。不要放一张不存在的图片。
     例如 https://api.star-history.com/svg?repos=charlesbuling/VideoButler&type=Date
-->

---

## ⚖️ 版权与商业授权 (Copyright & Licensing)

本项目采用 **GNU Affero General Public License v3.0 (AGPL v3)** 许可证开源。

### 1. 开源与免费使用
* 欢迎个人、研究人员及开源社区在遵守 AGPL v3 协议的前提下免费使用、修改和分发本项目代码。
* **核心约束（远程网络交互传染）：** 如果您修改了本项目的代码，或者将本项目部署在云端/服务器上作为 SaaS（软件即服务）向公众提供网络访问，**您必须无条件将您的整个系统源代码以 AGPL v3 协议向所有服务使用者公开**。

### 2. 商业授权 (Dual-Licensing / 商业化免除)
* **不公开源码的商业用途：** 如果您的企业/组织计划将本项目用于商业闭源产品，或作为不愿开源的商业服务的一部分，**则不能使用当前的 AGPL v3 许可证**。
* 原作者 **zhengkuang** 始终拥有本项目的 100% 完整知识产权。我们提供灵活的 **商业双重许可 (Commercial License)** 方案。
* 任何商业合作、企业私有化部署、闭源授权或赞助需求，请直接通过官方邮箱联系：
  📬 **217010072@link.cuhk.edu.cn**
