# Developing on Windows

This is the Windows developer guide for the public `charlesbuling/VideoButler` repository (AGPL-3.0-or-later). It covers what differs on a Windows machine from the macOS / Linux / mobile builds.

## Before cloning

```bash
git config --global core.autocrlf false   # .gitattributes keeps *.sh LF regardless
git config --global core.longpaths true   # Rust's target paths run past 260 characters
```

Git for Windows' own installer sets `core.autocrlf true` system-wide, and a
checkout made under it has every text file in CRLF. Fix it in place, with
nothing uncommitted:

```bash
git config core.autocrlf false && git rm -r --cached -q . && git reset --hard -q
```

## Local build artefacts (not in git)

`.gitignore` keeps these out of the public tree; build them on the machine:

| Path | How to make it |
|---|---|
| `build/ffmpeg-windows-x86_64/current/` | LGPL ffmpeg with Media Foundation H.264/HEVC encoders, D3D11 decoding, and x86 SIMD. `scripts/build-ffmpeg-cross.sh windows-x86_64` from an MSYS2 shell on Windows, or from a Mac/Linux with Zig. |
| `build/onnxruntime-windows/current/` | `scripts/fetch-onnxruntime-windows.sh` |
| `test-media/` | `scripts/make-test-media.sh` (HEIC stills need macOS `sips`; copy the fixture from a Mac if you only have Windows). |

The 0.2.0 Windows Release zip already contains the LGPL ffmpeg DLLs beside `video_butler.exe`; you only need the ffmpeg tree above when rebuilding the engine from source.

## Tools

- Git for Windows (Git Bash runs every script in `scripts/`)
- Rust stable with the MSVC toolchain (`x86_64-pc-windows-msvc`)
- Visual Studio 2022 Build Tools: "Desktop development with C++" (`cl.exe`
  for vb-media's C shim, CMake for whisper.cpp and llama.cpp, the Windows
  SDK) and the component "C++ Clang Compiler for Windows", whose libclang
  `whisper-rs-sys` and `llama-cpp-sys-2` need for bindgen. `scripts/env.sh`
  finds both through vswhere, so a plain Git Bash builds; an LLVM install
  named in `LIBCLANG_PATH` wins.
- Flutter stable, with Windows desktop enabled, and Windows' **Developer
  Mode** on (Settings > For developers): Flutter links plugins with
  symlinks, which Windows allows only there or to an administrator.
- `nuget.exe` on `PATH`: video_player_win's build fetches Microsoft's WIL
  headers (MIT) with it, and downloads nuget.exe 5.10.0 from
  dist.nuget.org itself when there is none -- which failed from mainland
  China. The same file, by its SHA-256, is `tools/NuGet.exe` in the
  `NuGet.CommandLine` 5.10.0 package on nuget.org.
- To build ffmpeg again, and only for that: MSYS2 (`pacman -S make
  diffutils binutils gcc nasm`) and Zig 0.16.0 on its `PATH`. Git Bash has
  no `make`.

From mainland China the usual hosts are slow or unreachable. What worked:
rustup and crates through `rsproxy.cn`, Flutter through
`storage.flutter-io.cn` and `pub.flutter-io.cn`, MSYS2 through
`mirrors.tuna.tsinghua.edu.cn`, Zig as the `ziglang` wheel from Tsinghua's
PyPI mirror; Visual Studio and ONNX Runtime came from Microsoft's CDN and
GitHub directly. Downloads there drop TLS connections mid-file: resume with
`curl -C - --retry-all-errors`. A pub mirror is written into
`app/pubspec.lock` in place of `https://pub.dev`, and a lock naming another
host is resolved afresh, newer versions and all; `build-windows.sh` points
the lock at the mirror while pub runs and back after. By hand: `flutter
pub get` once, then `--no-pub` on `analyze`, `test` and `build`, and put
the lock's `url:` lines back to `https://pub.dev` before committing.

## Build

```bash
./scripts/build-windows.sh
# -> app/build/windows/x64/runner/Release/video_butler.exe, with vb_ffi.dll,
#    vb_llama_engine.dll, ONNX Runtime and ffmpeg's DLLs beside it
```

The engine alone, against the carried ffmpeg, and its tests -- which need
ffmpeg's DLLs on `PATH` to start, and a CLI ffmpeg for the parity suite:

```bash
source scripts/env.sh
export VB_LIBAV_DIR="$(cd build/ffmpeg-windows-x86_64/current && pwd -W)"
cargo build --release -p vb-ffi --features libav,whisper \
  --target x86_64-pc-windows-msvc --manifest-path core/Cargo.toml
PATH="$PWD/build/ffmpeg-windows-x86_64/current/bin:$PATH" \
  cargo test -p vb-media --features libav --manifest-path core/Cargo.toml
```

A Flutter build whose first CMake configure failed (a plugin's download,
say) installs to `C:\Program Files\video_butler` from then on, and fails
there for want of rights: the template points the install at the build
only while the prefix is CMake's first default, and the failed run cached
it. Delete `app/build/windows/x64/CMakeCache.txt` and build again.

The real app, driven from code (`app/integration_test/app_test.dart`), on
the engine `build-windows.sh` built:

```bash
cd app && flutter test integration_test/app_test.dart -d windows --no-pub \
  --dart-define=VB_IT_FOOTAGE="$(cd ../test-media && pwd -W)" \
  --dart-define=VB_IT_MUSIC="$(cd ../test-media && pwd -W)/beat-120.wav" \
  --dart-define=VB_IT_PHOTOS=true
```

The carried ffmpeg is libraries only, no `ffmpeg.exe`. Anything that runs
the CLI backend (`FfmpegCli`) needs an ffmpeg on `PATH`; a GPL build is fine
for local tests and must never ship (see Licensing in ). The CLI
backend writes with `h264_mf` there too, and asks once whether Media
Foundation's encoder is the GPU's.

## Models

Not in the repository: the Mac's library holds 18 GB, past what GitHub
takes. The library on Windows is `%USERPROFILE%\AppData\Roaming\VideoButler`
(`VB_LIBRARY_DIR` overrides it), and the catalogue downloads each model with
its SHA-256 checked, e.g. `vb call models.download '{"id":"whisper-base-q5"}' --wait`.
ONNX Runtime is Microsoft's release build, fetched by
`scripts/fetch-onnxruntime-windows.sh` into `build/onnxruntime-windows`
and loaded by full path: System32 has an older `onnxruntime.dll` of Windows
ML's own. PhotoFramer is a local model (converted on the Mac, not
downloadable): copy its two files across by hand.

## Where Windows stands

Built and run on a Windows 10 machine (i5-7500, GTX 1660 SUPER) since
2026-10-14; see  for what was measured there.

## Keeping the Mac in step

```bash
git remote add mac https://github.com/charlesbuling/VideoButler.git
git fetch mac
git merge mac/macos-app      # the Mac's work into windows
```

The other way, the Mac adds this repository as a remote and merges
`windows`.
