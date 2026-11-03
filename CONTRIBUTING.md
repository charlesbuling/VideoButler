# 参与贡献 / Contributing

Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>

Video Butler 以 **AGPL-3.0-or-later** 对公众授权。贡献者保留自己的版权，
同时必须接受 [`CLA.md`](CLA.md)：向 zhengkuang 授予永久、全球、不可撤销、
免许可费的版权许可和专利许可，包括再许可，以及按其他条款（含专有许可和
商业许可）再授权的权利。没有这份授权，拉取请求不会被合并。维护者也可以
不合并提交，而自行把同一个想法重新实现一遍。

商业授权、闭源使用、企业部署或赞助，不走拉取请求，请写
**217010072@link.cuhk.edu.cn**。

中文在前，英文在后。

## 中文

### 取得代码

仓库目前仍可能是私有的。能访问时：

```bash
git clone https://github.com/charlesbuling/VideoButler.git
cd VideoButler
git checkout macos-app
```

在 GitHub 上先 Fork，再从你的 Fork 克隆。开发分支从 `macos-app` 拉出。
不要直接在 `macos-app` 上提交。Windows 应用在另一个仓库
`charlesbuling/VideoBulter-windows`（分支 `windows`），不属于本仓库的
0.2.0 发布。

### 分支名

用简短、小写、能看出意图的名字，例如 `fix/draft-overlap`、
`feat/caption-locale`。不要只用 `update` 或 `wip`。

### 本地检查

本仓库没有自动 CI。`.github/workflows/ci.yml` 只能手动启动，不能代替
下面的检查。推送之前在自己的机器上跑：

```bash
source "$HOME/.cargo/env"   # 如果 rustup 不在默认 PATH 里

cargo fmt --all --check --manifest-path core/Cargo.toml
cargo clippy --workspace --all-targets --manifest-path core/Cargo.toml -- -D warnings
cargo test --workspace --manifest-path core/Cargo.toml
cargo deny check --manifest-path core/Cargo.toml
./scripts/check-licenses.sh

cd app && flutter analyze && flutter test
```

改了 Dart 格式的，用 `dart format`。只改文档、不改引擎或应用行为的提交，
至少确认没有把密钥或生成目录加进来。

在你的改动碰到媒体后端或应用打包时，再跑下面这些（需要对应的模型、素材或构建）：

```bash
# 链接 libav 的媒体后端（默认关闭），含与 CLI 后端的一致性测试
cargo test -p vb-media -p vb-pipeline --features vb-pipeline/libav --manifest-path core/Cargo.toml
# 通过 llama.cpp 跑一个真实的 GGUF 视觉模型
VB_VLM_MODEL=<model.gguf> VB_VLM_MMPROJ=<mmproj.gguf> VB_VLM_IMAGE=<frame.jpg> \
  cargo test --release -p vb-llama a_real_model -- --nocapture
# whisper.cpp 转写一段有人声的素材
VB_WHISPER_MODEL=<ggml.bin> VB_WHISPER_CLIP=<clip.mp4> \
  cargo test --release -p vb-whisper a_real_model -- --nocapture
# 真实的 macOS 应用与引擎（需要 LGPL ffmpeg：scripts/build-ffmpeg-macos.sh）
cd app && flutter test integration_test -d macos \
  --dart-define=VB_IT_FOOTAGE=<folder> --dart-define=VB_IT_MUSIC=<music.wav>
```

### 提交

写清楚做了什么、为什么。看现有历史：一句完整的话，而不是含糊的 `update`。

每一个提交都要有签署行。`git commit -s` 会加上：

```
Signed-off-by: 你的名字 <you@example.com>
```

这表示你确认贡献是你有权提交的，并且你接受 `CLA.md`。

### 拉取请求

目标分支是 **`macos-app`**。填写拉取请求模板：

- 勾选 CLA 复选框；
- 确认每个提交都有 `Signed-off-by`。

缺任何一项都不会合并。说明你跑了哪些本地检查。维护者可以不合并你的补丁，
改由自己重新实现。

### 许可证

对公众，你的贡献随项目以 AGPL-3.0-or-later 提供（`LICENSE`、`NOTICE`）。
对维护者，你按 `CLA.md` 另行授权。你不因此失去自己的版权。

不要加入别人的 copyleft 代码、CC-BY-NC 的模型（例如 CoTracker3、MMAudio），
或你无权提交的材料。依赖许可由 `core/deny.toml` 和
`scripts/check-licenses.sh` 把关。

---

## English

Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>

Video Butler is offered to the public under **AGPL-3.0-or-later**.
Contributors keep their copyright and must accept [`CLA.md`](CLA.md):
a perpetual, worldwide, irrevocable, royalty-free copyright license and
patent license to zhengkuang, including the right to sublicense and to
relicense the contribution under other terms, including proprietary and
commercial licenses. A pull request without that grant will not be
merged. Maintainers may also reimplement an idea instead of merging.

Commercial licensing, closed-source use, private deployment, or
sponsorship does not go through a pull request. Write to
**217010072@link.cuhk.edu.cn**.

### Get the code

The repository may still be private. When you can see it:

```bash
git clone https://github.com/charlesbuling/VideoButler.git
cd VideoButler
git checkout macos-app
```

Fork on GitHub, then clone your fork. Branch from `macos-app`. Do not
commit on `macos-app` itself. The Windows app lives in
`charlesbuling/VideoBulter-windows` (branch `windows`) and is not part
of the 0.2.0 release of this repository.

### Branch names

Use a short lowercase name that says what the branch is for, such as
`fix/draft-overlap` or `feat/caption-locale`. Avoid `update` and `wip`.

### Local checks

There is no automatic CI. `.github/workflows/ci.yml` starts only by hand
and does not replace the checks below. Run them before you push:

```bash
source "$HOME/.cargo/env"   # if rustup is not on PATH

cargo fmt --all --check --manifest-path core/Cargo.toml
cargo clippy --workspace --all-targets --manifest-path core/Cargo.toml -- -D warnings
cargo test --workspace --manifest-path core/Cargo.toml
cargo deny check --manifest-path core/Cargo.toml
./scripts/check-licenses.sh

cd app && flutter analyze && flutter test
```

If you touched Dart formatting, run `dart format`. A documentation-only
change should at least not add secrets or generated directories.

When your change touches a media backend or packaging, also run these
(they need the matching models, footage or builds):

```bash
# The linked-libav media backend (off by default), with its parity suite against the CLI backend
cargo test -p vb-media -p vb-pipeline --features vb-pipeline/libav --manifest-path core/Cargo.toml
# A real GGUF vision model through llama.cpp
VB_VLM_MODEL=<model.gguf> VB_VLM_MMPROJ=<mmproj.gguf> VB_VLM_IMAGE=<frame.jpg> \
  cargo test --release -p vb-llama a_real_model -- --nocapture
# whisper.cpp on a clip with speech
VB_WHISPER_MODEL=<ggml.bin> VB_WHISPER_CLIP=<clip.mp4> \
  cargo test --release -p vb-whisper a_real_model -- --nocapture
# The real macOS app and engine (needs the LGPL ffmpeg from scripts/build-ffmpeg-macos.sh)
cd app && flutter test integration_test -d macos \
  --dart-define=VB_IT_FOOTAGE=<folder> --dart-define=VB_IT_MUSIC=<music.wav>
```

### Commits

Say what changed and why. Existing history uses a full sentence, not a
vague `update`.

Every commit needs a sign-off. `git commit -s` adds:

```
Signed-off-by: Your Name <you@example.com>
```

That certifies you have the right to submit the contribution and that
you accept `CLA.md`.

### Pull requests

Open the pull request against **`macos-app`**. Fill in the template:

- tick the CLA checkbox;
- confirm every commit has `Signed-off-by`.

A request missing either one will not be merged. Say which local checks
you ran. Maintainers may reimplement an idea instead of merging your
patch.

### License

To the public, your contribution is offered with the project under
AGPL-3.0-or-later (`LICENSE`, `NOTICE`). To the Maintainer, you also
grant the rights in `CLA.md`. You do not lose your copyright.

Do not add someone else's copyleft, a CC-BY-NC model (CoTracker3 and
MMAudio are barred), or material you cannot submit. Dependency licenses
are checked by `core/deny.toml` and `scripts/check-licenses.sh`.
