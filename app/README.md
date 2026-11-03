# Video Butler — Flutter app

One Dart UI over the Rust engine, running on macOS today and Android alongside
it. The same source targets Windows, Linux and iOS as those are enabled.

## Running it

**macOS**, no Xcode needed:

```bash
./scripts/run-macos.sh
```

That builds the UI for web, builds the engine, and serves both from
`http://127.0.0.1:8787`.

**Android**:

```bash
./scripts/build-android.sh          # or: ./scripts/build-android.sh release
```

That cross-compiles the engine for three ABIs into `android/app/src/main/jniLibs`
and assembles an APK.

**Native macOS**, once full Xcode is installed:

```bash
./scripts/build-macos-native.sh
```

## Why macOS runs as a local web app

Flutter cannot build a macOS bundle without full Xcode, which is a ~10 GB
install. Its web target needs no Apple toolchain at all, so the same Dart UI
compiles to web and `vb serve` hosts it from the engine over loopback.

Nothing about the UI diverges. When Xcode is installed,
`scripts/build-macos-native.sh` produces a real `.app` from the identical
source, and the only thing that changes is which `Engine` implementation is
compiled in.

If you would rather install Xcode without the App Store, `xcodes` is already
installed:

```bash
xcodes install --latest       # needs an Apple ID
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -runFirstLaunch
```

## The engine seam

The UI talks to `Engine` and never to a transport. Two implementations:

| Platform | Implementation | How it reaches the core |
|---|---|---|
| Android, native macOS, Windows, Linux, iOS | `FfiEngine` | `dart:ffi` into the Rust library |
| Web | `HttpEngine` | HTTP to `vb serve` on loopback |

Both call the same Rust `api` module, so behaviour cannot drift between them.
`engine_factory.dart` picks one with a conditional import, so the web bundle
never contains FFI code and the native binary never contains an HTTP client it
would not use.

The FFI path needs no bridge package and no code generation, because the C ABI
was designed for it: every entry point takes and returns C strings holding
JSON. Two rules hold on the Dart side. Panics never cross the boundary, which
Rust guarantees. And every returned string is freed with `vb_free_string`,
because that memory came from Rust's allocator.

Long analyses run in an `Isolate`, so the UI never blocks. That is the whole
reason the engine is native rather than written in Dart.

## File picking goes through the engine

`Engine.browse` lists directories, rather than a platform file picker. The web
build cannot see the filesystem at all, so routing this through the engine
means one picker works everywhere and the path it returns is exactly the path
the engine will open.

## What works, and what does not

The app shows real engine state, lists the real templates, runs the real
pipeline, and reports what it found: every shot detected including the ones
left out, the reason behind each chosen clip, anything grounding rejected, and
the files written.

It is honest about its limits, and must stay that way. Speech, vision and
tracking are mock adapters, so the app labels them and says plainly that a
result is a structurally complete draft rather than a finished cut.

**On Android the engine cannot analyse video yet.** The media backend shells
out to `ffmpeg`, and Android does not let an app spawn a process. The app
builds, runs, and talks to the real engine, and the engine status card says
exactly this. Fixing it means implementing the linked-libav `MediaBackend`,
which is the same work iOS will need.

## Layout

```
lib/
  main.dart
  src/
    models.dart            what the engine returns
    theme.dart             one seed colour, light and dark
    engine/
      engine.dart          the interface the UI talks to
      ffi_engine.dart      dart:ffi implementation
      http_engine.dart     HTTP implementation for web
      engine_factory.dart  conditional import picks one
    ui/
      home_page.dart       source, template, run
      file_browser.dart    engine-backed picker
      result_view.dart     shots, the cut, rejections, outputs
      widgets.dart         cards, chips, stats, score bars
test/
  models_test.dart         parsing, including forward compatibility
  engine_contract_test.dart
```
