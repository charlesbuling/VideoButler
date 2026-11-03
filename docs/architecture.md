# Architecture

How the pieces fit, and why they are arranged this way. For requirements see
`PRD.md`, for build and check commands `CONTRIBUTING.md`; for product and
roadmap decisions see the development documentation beside this file.

## The problem the structure solves

Three parts of this system change at completely different rates.

Platform and runtime code churns constantly: ffmpeg versions, inference
engines, five target operating systems. Perception churns with the research
field, and grows in Phase 2 by 主体衔接 (subject continuity): subjects,
actions, contacted objects and backgrounds followed in 2D through every clip.
The narrative layer, by contrast, is meant to be iterated on for years and
accumulate value.

Wiring those together directly would mean every runtime change rippling into
the part that is supposed to be stable. So the layers communicate through two
versioned JSON contracts, and the dependency graph is arranged so the stable
layer cannot reach the volatile ones even by accident.

## Dependency direction

```
contracts/          JSON Schema, versioned, language-neutral
      |
vb-contracts        Rust types, validators, the microsecond timebase
      |
      +-- vb-media    ffmpeg behind MediaBackend
      +-- vb-infer    LLM / VLM / ASR / tracking adapters
      |
vb-perception       -> writes the Footage Graph
      |
vb-narrative        priors + orchestrator -> writes the Editing DSL
      |
      +-- vb-render   DSL -> render plan
      +-- vb-export   DSL -> OTIO
      |
vb-pipeline         stage graph, cache, provenance
      |
      +-- vb-cli      the `vb` binary
      +-- vb-ffi      C ABI -> Flutter
```

`vb-narrative` depends on `vb-contracts` and `vb-infer` and nothing else. A
test in that crate reads its own manifest and fails if the set widens, because
a comment cannot fail a build.

The practical consequence: when narrative code needs something perception
knows, the answer is to add a field to the Footage Graph schema, never to add a
dependency.

## The two contracts

**Footage Graph** (`footage-graph/0.1`) is what perception produces: sources
with content hashes, shots with timecodes, keyframes with classical scores,
optional dense descriptions, persistent entity identities, action labels, a
timestamped transcript, mined statistics, and a provenance record per stage.

**Editing DSL** (`editing-dsl/0.1`) is what the narrative layer produces: an
ordered spine of slots, tracks anchored to those slots, transitions between
them, grounding records, and provenance.

The JSON Schema is the source of truth and the Rust types follow it. A
round-trip test parses each fixture, re-serialises it and re-validates, so the
two cannot drift apart silently.

## Three decisions that shape everything

### Time is integer microseconds

No floats anywhere in the contracts. Float seconds accumulate drift across a
few hundred cuts, and frame numbers are ambiguous at rates like 30000/1001.
Microseconds are CapCut's native unit and convert exactly to OTIO and FCPXML
frame counts through each source's rational frame rate.

The one place a float is allowed is at the ffmpeg boundary, where timestamps
arrive as decimal seconds. They are converted once, immediately.

Frame conversion rounds up rather than truncating. At 30000/1001 a frame
boundary falls on a fraction of a microsecond, and truncating puts the
timestamp inside the previous frame's interval, so a round trip silently loses
a frame.

### The DSL stores no absolute timeline positions

A slot declares its order and its duration. It does not know where it lands on
the finished timeline, and there is no field in which it could record that.
Text and audio anchor to a slot plus an offset. Absolute time is derived by one
function, `resolve_timeline`, which walks the spine subtracting transition
overlap.

This exists because the previous implementation stored absolute positions and
they drifted. A transition makes two adjacent slots overlap, and a title card
inserts time that the sum of preceding durations does not predict. Deriving
position instead of storing it means the drift has nowhere to come from.

The renderer and the exporter both call that one function. Two independent
derivations of time is exactly how the problem started.

### Every decision is grounded before it leaves

A model asked to plan a cut will reference footage that does not exist: a
timecode past the end of a clip, an invented shot identifier, a person who
never appears. Nothing downstream catches this. The renderer would seek past
the end of the file and emit black frames, and the user would see a broken edit
with no explanation.

So every slot is checked against the Footage Graph before any DSL is emitted.
Failures are **rejected and recorded**, never repaired. Clamping a bad timecode
into range would convert a detectable hallucination into a silently wrong edit,
which is worse, because the system would look like it was working.

The gate also removes anything that referenced a rejected slot, and reports a
pass rate. A low pass rate means the orchestrator is hallucinating and the run
should not be offered as a usable cut.

## Adapter seams

Every external dependency enters through a trait: `MediaBackend`, `AsrAdapter`,
`VlmAdapter`, `InferenceAdapter`, `TrackerAdapter`, `DraftWriter`.

Two reasons, both concrete. Licensing: two dependencies in this field are
CC-BY-NC and cannot ship, so containing each one to a single implementation
file is what keeps a license problem from becoming a refactor. And iOS, which
forbids spawning subprocesses, so the ffmpeg CLI backend is desktop-only and
mobile uses a linked-libav backend behind the same trait.

`TrackerAdapter` is the seam for identity across clips. Phase 2's 主体衔接
follows people and things through each clip in 2D behind adapters of their
own and adds what they find to the Footage Graph; no interface the narrative
layer sees changes.

Every adapter has a mock, every adapter reports whether it is one, and
provenance records which implementation ran. This is not a testing
convenience. The pipeline must produce a complete result with no network and no
weights, and the output of such a run has to stay distinguishable from a real
one all the way to the user.

The real implementations each live in their own crate, so each one's build
cost and licence stays with it. `vb-whisper` compiles whisper.cpp and is behind
a feature; `vb-apple` compiles an Objective-C shim over Vision and is a no-op
elsewhere; `vb-cloud` is the only crate that may reach the network, and
cargo-deny bans an HTTP client anywhere else. That last rule is what makes the
offline guarantee structural: the narrative and perception crates cannot make
a request because nothing they depend on can.

Choosing a run's adapters happens in one function, `api::adapters_for`. The
local set is always complete, and the cloud is layered on top of it rather
than in place of it, so an endpoint that is missing or down costs a run its
cloud descriptions and nothing else.

## The compression funnel

Hours of frames become a few hundred segments. Each take is swept from start
to end, two small frames a second, and every frame is measured by classical
computer vision: Laplacian variance for sharpness, mean intensity for
exposure, a coarse-to-fine alignment against the previous frame for camera
movement and for what else moved, colourfulness and contrast, and a
colour-and-layout signature of the view. From those readings each shot is
divided where the view changes, each segment gets its usable part and one
keyframe at its best moment, and segments are grouped into scenes across
every clip by how alike they look and how close in time they were filmed.
Only a segment's keyframe is ever described by a vision model.

The ordering matters. Frame-level differentiation is cheap CV work and models
only see the compressed storyboard. One model call per segment rather than
per frame is the difference between minutes and days.

The orchestrator reads that storyboard: it takes the best moment of as many
scenes as the cut's length allows, in capture order, never two stretches of
one scene side by side, and places each in and out point where the readings
say the picture is steady and the action is at rest.

## Incremental work

Getting a cut out of two hours of 4K in an evening depends far more on not
redoing work than on raw model speed. Every stage output is keyed on
`blake3(source content + stage name + stage parameters)`, so changing one
setting re-runs that stage and what follows, and nothing else.

Keying on content rather than path means a renamed file still hits the cache
and an edited file with the same name correctly misses. Adapter identities are
part of the key too: swapping a mock for a real model must not reuse the mock's
output.

The cache writes to a temporary file and renames, so an interrupted run leaves
no half-written entry a later run would read as complete.

## Three front ends, one operation set

`vb-pipeline::api` is the single definition of what the application can do.
The CLI, the C ABI and the local HTTP server are transports over it, not
independent implementations, so a capability cannot exist on one and quietly
differ on another.

The CLI came first on purpose. It forces the core to be drivable with no window
and no platform SDK, which keeps the app a consumer of the engine rather than
its owner, and gives CI a way to exercise every contract boundary headlessly.

The FFI boundary does two things beyond marshalling. It catches panics, because
unwinding into Dart is undefined behaviour. And it returns a tagged JSON result
rather than null-on-error, so the caller learns why a run failed.

Operations are named rather than exported one by one. `api::call(method,
args)` dispatches, and each transport exposes that single entry point:
`vb_call`, `POST /api/call/<method>`, `vb call`. The operation set -- projects,
runs, the model library, settings, the album -- grows inside the engine
without touching a transport, which matters most for the native app, where
every exported symbol has to be rooted by hand against dead-stripping.

## Long work, and the state that outlives it

A run over a folder takes minutes and a model download longer, so neither is
a call that blocks. `jobs.rs` runs work on its own thread and returns an id;
the caller polls for a stage, a percentage and the clip in hand, and may ask it
to stop. Stopping is cooperative: perception checks between shots, the
pipeline between stages. A panic in a job fails that job and nothing else.

The percentage is an estimate, weighted by file size across clips and by
stage within one, and it never goes backwards, because a bar that retreats
reads as a bug even when the work is fine.

What outlives a run lives in the library, the platform's own application-data
folder: the analysis cache, the stills it points at, downloaded models, the
projects, the album and the settings. Sharing the cache there is what makes a
clip analysed for one project free for the next. A project is a small JSON
file of what the user chose; it owns its outputs and none of the analysis, so
deleting one never costs another project anything.

## The app, and the Xcode problem

Flutter cannot build a macOS bundle without full Xcode, which is a large
install and an Apple ID away. Waiting on it would have meant no desktop app.

Flutter's web target needs no Apple toolchain at all, so the same Dart UI
compiles to web and `vb serve` hosts it from the engine over loopback. This is
not a parallel implementation. The UI talks to an `Engine` interface with two
implementations, `dart:ffi` on native platforms and HTTP on web, and both call
the same `api` module. When Xcode is installed, `flutter build macos` produces
a native bundle from the identical source and only the compiled-in `Engine`
changes.

Two constraints make the server safe to run. It binds loopback only, and it
sends no CORS headers, so nothing on another origin can read a response or
start a run. Both matter, because these endpoints browse the filesystem and
execute a pipeline on the user's machine. Static file serving resolves through
`canonicalize` and refuses anything outside the web root, symlinks included.

File picking goes through the engine rather than a platform picker, because
the web build cannot see the filesystem. One picker works everywhere, and the
path it returns is exactly the path the engine will open.

### Media on mobile

`FfmpegCli` spawns a process, which iOS forbids and Android makes impractical.
`LibAv` is the other `MediaBackend`: it calls linked libav in-process through a
small C shim (`vb-media/csrc/vb_av.c`), so Rust never depends on libav's struct
layouts and one source builds against any ffmpeg from 6.0 on. It sits behind
the `libav` feature, and `vb_media::default_backend()` selects it whenever it
is compiled in.

The two backends are held to the same output. They run the same `select`
filter for scene detection, use the same timestamp origin and seek semantics,
and produce the same full-range greyscale; on the test fixture the resulting
Footage Graphs are identical down to every keyframe score. The backend's name
is in the cache key and in each media stage's provenance, alongside the ffmpeg
version.

Android builds link it by default. `scripts/build-ffmpeg-android.sh`
cross-compiles a minimal LGPL ffmpeg per ABI -- only the containers, codecs
and filters analysis uses -- and refuses to finish if the result is not LGPL
or its sonames are versioned. `scripts/build-android.sh` links the engine
against it and packages the libraries into the APK.

This has run: on an API 36 arm64 emulator the app picks a video out of shared
storage and writes a draft, and the graph matches the desktop one exactly.

Two Android rules shape how that works. The app may read the user's videos
only with `READ_MEDIA_VIDEO`, which `MainActivity` requests, and it may not
write a project folder beside the footage, because shared storage accepts only
media files. `MainActivity` therefore sets `VB_WORKDIR` to a directory the app
owns, and `api::run` writes there when the caller names no project directory --
the host decides where output may go, without the engine learning about
Android. The consequence is that a draft is currently somewhere the user
cannot reach; MediaStore or a share intent is the missing piece.

iOS needs its own ffmpeg build plus full Xcode.
