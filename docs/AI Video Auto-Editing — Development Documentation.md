# AI Video Auto-Editing ("Video Butler") · Development Documentation

**Last updated:** 2026-10-13 — lossless export and clip-by-clip export from the built-in editor, after LosslessCut (§6.7); the macOS installer also comes as the app alone (about 100 MB) with a separate model pack, and is ready for Developer ID notarisation. 2026-10-10 — a growing-up memory no longer takes in strangers the face model filed with the child: faces recorded off a screen are not the user's people, and a child's memory keeps only the sightings their own frames do not contradict, in one stretch of time (§6.6). 2026-10-09 — **0.2.0: the built-in editor.** A cut no longer has to leave the app to be finished: it opens in an editor built after 剪映's (split, trim, reorder, speed, sound, transitions, position, adjustments, backgrounds, text, music, canvas), every change an operation on the Editing DSL with undo, and it leaves as a rendered video or, as before, as a 剪映/CapCut/OTIO project file (§6.7). 2026-10-07 — the video butler run on 2,555 real videos: home is where someone comes back to (a move no longer reads as a year of trips), transport and study rules corrected for Apple Vision's label tree, people listed from two videos, the app's keyframe stills made upright, WeChat's camera clips dated by their names, trips with no location found by their drone flights, growing up for children only, and groups of one face joined after each pass (§6.6). Earlier the same day — the **video butler (视频管家)** and its **memory map (回忆地图)**: one tap files every video on the device by face and by what it shows, finds trips, people growing up, years and groups, and cuts a project for each (§6.6); AuraFace integrated for face grouping. 2026-10-06 — 主体衔接 leaves out learning: someone watching a phone with the steps, anyone standing idle alone, steps on their own, takes mostly not danced (§VII). 2026-10-05 — 主体衔接's beat rules revised and implemented: the dancers' rhythm read and idle left out, the dance on the beat first (speed within 1.2×), music looped or silent, the length second (§VII). 2026-10-04 — Phase 2 revised: **主体衔接** (subject continuity) on 2D recognition replaces the 4D spatio-temporal model (§VII); its first stage, for people, implemented

This document is the **single source of truth** for the project's ongoing development. It covers implementation objectives, methods, architecture, and milestones. Background research, market analysis, and platform comparisons are not treated as a development basis and instead live in the companion *Optimization Report*.

---

## 0. Executive Summary

Video Butler is a **cross-platform, on-device** AI video auto-editing product, developed around a two-phase model strategy. The load-bearing decisions that shape everything below:

| Dimension | Decision |
|---|---|
| Target platforms | **macOS, Windows, iOS, Android, Linux** |
| UI / application shell | **Flutter** (native compilation, one codebase, all five targets) |
| On-device inference | **A built-in model host** that runs open and self-trained models (inference only): **llama.cpp** for VLMs/LLMs, **ONNX Runtime** for vision models and the project's own models, with per-platform hardware acceleration (§3.3) |
| Development environment | **MacBook Pro (M5, 24 GB unified memory, 1 TB SSD)** as primary dev + reference target; heavy training offloaded to cloud/dedicated GPU |
| Roadmap | **Two phases** — Phase 1: application framework on open source; Phase 2: **主体衔接** (subject continuity across cuts, from 2D recognition — no 4D modelling) + self-trained models + associated LLMs |
| Storage & editing | **Local video storage + fully non-destructive re-editing**: a built-in editor over the Editing DSL with rendered-video export (from 0.2.0), and portable project files (CapCut draft / OTIO / FCPXML) |

Flutter was chosen over an Electron-style shell because Electron cannot target mobile (iOS/Android) at all, it bundles a full Chromium + Node.js runtime (the heaviest memory/size baseline of the options considered), and calling native inference libraries (llama.cpp / ONNX Runtime / ffmpeg) from it requires a Node native-addon or an out-of-process local service. Flutter compiles to native code, calls C/C++/Rust inference libraries directly through FFI, keeps heavy compute off the UI thread, and covers all five targets from one codebase.

---

## I. Goal & Product Definition

### 1.1 Goal

Develop an AI-powered video-editing tool that runs **entirely on local, consumer-grade hardware** and, with a single action, imports local videos and photos (mixed freely from 0.1.2), analyzes character–object–environment interactions and narrative arcs, and automatically handles segment selection, sequencing, pacing, subtitles, and background music — producing a **draft project file** that the user can continue to refine. All source video stays on-device (local storage), and every automated decision remains **fully re-editable (non-destructive)**.

### 1.2 Acceptance Boundary (Target Definition)

🟥 What can be reliably delivered is **"qualified AI rough-cut editing plus human fine-tuning,"** ** ** "cinematic-level autonomous screenwriting and directing may become reliably after this project's own model is well developed(ultimate target)." Every milestone acceptance is evaluated against the former definition.

- **Product form:** a locally developed and locally run application, packaged and distributed for **macOS, Windows, iOS, Android, and Linux**, capable of **fully offline operation** once installed.
- **Deliverable anchor:** a cut the user finishes either in the app's own editor, exported as a video, or in a downstream editor from a project file that opens there directly (CapCut Pro where available; OpenTimelineIO / FCPXML elsewhere) — the user chooses per cut (from 0.2.0, §6.7).
- **Non-destructive guarantee:** the AI produces a *decision manifest* (the Editing DSL), never a flattened render, so any cut, transition, subtitle, or music choice can be reverted or adjusted without re-running the pipeline.

### 1.3 Sub-Competency Breakdown

The product is decomposed into six sub-competencies, each independently testable and independently upgradable behind a stable contract:

1. **Perception** — shot detection, keyframe extraction, dense visual description, ASR, entity/object tracking, action/interaction labeling.
2. **Narrative understanding** — turning perception output into a structured story representation (arcs, beats, highlights).
3. **Editing decision** — statistical priors + template library + reasoning orchestrator → the Editing DSL.
4. **Rendering & export** — DSL → portable project files, and the built-in editor's preview and finished video, rendered on the device (§6.7).
5. **On-device inference runtime** — the cross-platform model host (llama.cpp for VLMs/LLMs, ONNX Runtime for vision and self-trained models, + per-platform accelerators) that runs every model.
6. **主体衔接 — subject continuity** (Phase 2) — each clip's action subject, action, contacted objects and background recognised in 2D, and assembled into one action that carries across every cut in position, size, pose, direction and speed (§VII).

---

## II. Overall Technical Approach

We **do not build a base model.** Instead we build our own **"narrative-understanding middleware + editing-decision layer."** The engineering essence is the project's moat:

> **A multi-stage perception pipeline (the brain) + draft export (the hands) + a sustainably iterable narrative-layer platform (the moat).**

Two structural commitments extend that core: (a) the whole system runs on a **portable, cross-platform on-device runtime** rather than a single fixed hardware configuration, and (b) the perception "brain" carries a **Phase 2 upgrade to 主体衔接** — subjects, actions, contacted objects and backgrounds recognised in 2D and followed through each clip — so that the middleware's input contract graduates from flat labels to structured 2D motion by added fields, without rewriting the narrative layer.

---

## III. Target Platforms & Technology Stack

### 3.1 Platform Matrix

| Layer | macOS | Windows | Linux | iOS | Android |
|---|---|---|---|---|---|
| UI shell | Flutter | Flutter | Flutter | Flutter | Flutter |
| Inference engine | llama.cpp / MLC LLM | llama.cpp / MLC LLM | llama.cpp / MLC LLM | llama.cpp / MLC LLM | llama.cpp / MLC LLM |
| HW acceleration | Metal / MLX / Core ML | CUDA / Vulkan / DirectML | CUDA / Vulkan / ROCm | Metal / Core ML / ANE | Vulkan / OpenCL / NNAPI |
| Media I/O | ffmpeg | ffmpeg | ffmpeg | ffmpeg (mobile build) | ffmpeg (mobile build) |
| Draft/export target | CapCut / OTIO / FCPXML | CapCut / OTIO | OTIO / FCPXML | CapCut / OTIO | CapCut / OTIO |

### 3.2 UI / Application Shell: Flutter

Flutter is the application shell: one Dart codebase covers all five targets, compiles to native code (so heavy inference never blocks the UI thread), carries a much lighter memory/size baseline than an Electron-style stack, and binds to native inference libraries directly through FFI. Mature open-source references already ship a **single Flutter codebase across Android/iOS/Windows/macOS/Linux** with llama.cpp bound over FFI, so this is a proven architecture rather than a bet.

**Per-platform native options (for teams that later split the codebase):** where a platform needs maximum polish, the shell can be re-implemented natively while sharing the same inference layer — **SwiftUI** on iOS/macOS (zero-bridge access to MLX / Core ML / the Neural Engine), **Jetpack Compose (Kotlin)** on Android (JNI/NDK straight to the `.so`), and **Tauri** or **Qt/C++** on desktop. Qt is worth a serious look for the desktop editor specifically, because a video tool needs timeline, waveform, and scene-graph widgets and Qt keeps UI and the C++ inference engine in one language with zero FFI overhead. This "shared engine, separate UI" fallback is documented in §VII and the Optimization Report; the **default remains one Flutter codebase.**

### 3.3 On-Device Inference: the Built-in Model Host

The app carries its own **model host**: it loads and runs open models and, from Phase 2, the project's self-trained models, on-device and offline. It does **inference only** — training happens on the development side (§VII) and its results arrive as model files. The host has two engines, both MIT, behind the adapter traits (§V):

- **llama.cpp** for VLMs and LLMs in GGUF — the composition and cinematography models, the general VLM, and later the screenwriter LLM (§6.5). It is built into a dynamic library of its own and loaded at runtime, never linked into the engine: whisper.cpp is linked, and the two carry different versions of ggml under the same symbol names.
- **ONNX Runtime** for everything else — depth, embeddings, face, pose, and the project's own small models — with the CoreML, NNAPI/XNNPACK, DirectML and CUDA execution providers per platform.

Every model is an entry in one catalogue — file, SHA-256, size, licence, engine, device tier — so adding an open model or importing a self-trained one is a catalogue entry, not pipeline code. The engine is chosen for the model, not the other way round; llama.cpp is not required to host everything.

We **use existing engines; we do not write our own.** A usable engine is not the ~1,000-line teaching example — the parts that make it production-grade (hand-written SIMD quantization kernels, five or six GPU backends, mmap + KV-cache management, per-architecture model compatibility) represent hundreds of contributors over years. Our competitive edge is 主体衔接 and the narrative layer, not matmul kernels. For the VLM/LLM engine:

- **llama.cpp (MIT)** — primary. Broadest hardware coverage (Metal/CUDA/Vulkan and, crucially for us, first-class **Windows and Linux**), GGUF as the de-facto quantization format, a stable C API ideal for FFI. Order-of-magnitude on-device throughput on a flagship phone is ~10–30 tokens/s.
- **MLC LLM (Apache 2.0)** — alternative. TVM-compiled kernels ("compile once, run everywhere," including WebGPU); typically higher throughput on a given device at the cost of a heavier per-platform build. Benchmark both on target hardware before locking in.
- Both host the **same quantized model weights**, so the engine choice is an implementation detail behind the adapter interface (§V), not a product decision.

### 3.4 Compute Tiering

Compute budgets are tiered by **device class**, with a clean split between what runs on-device and what is deferred to cloud/offline GPU.

| Tier | Reference hardware | On-device workload |
|---|---|---|
| **A — Workstation/desktop** | MacBook Pro M5 24 GB; PC ≥12 GB VRAM | Full local pipeline; 7B–8B VLM/LLM quantized resident; Whisper large-v3 / high-accuracy ASR |
| **B — Flagship mobile** | 12–16 GB iPhone / Android flagship | 7B–8B Q4 resident (the sweet spot), real-time; lightweight perception; compact on-device ASR |
| **C — Mid/low-end or legacy** | ≤8 GB devices, older machines | Small VLM (e.g. Qwen3-VL-2B / SmolVLM2 class; FastVLM's weights are Apple research-licensed and cannot ship) + compact ASR; heavy steps deferred |
| **Cloud / offline (optional)** | Dedicated NVIDIA GPU or cloud | Model training/distillation (Phase 2), 13B–32B models when genuinely required — never 主体衔接 itself, which runs on the device |

Design rule: **the on-device path must produce a complete (if lower-fidelity) result with no network.** Cloud is an accelerator, never a dependency — this preserves the "runs entirely on local hardware" and "offline operation" acceptance criteria.

---

## IV. Development & Runtime Environment (Finalized)

🟥 **Primary development & reference environment: MacBook Pro (M5, 24 GB unified memory, 1 TB SSD).** Apple Silicon's unified memory lets a 7B–8B quantized model and the perception models share memory without host↔device copies; MLX / Metal / Core ML provide the acceleration path, and the same M-series machine doubles as the "consumer-grade" target we validate against.

- **Cross-compilation:** Flutter builds macOS/iOS locally on the M5; Android is built on the Mac; Windows and Linux on their own machines, since Flutter builds a desktop app only on its own OS. There is no CI (decided 2026-10-17): each target's script runs on a developer's machine, and the checks run there before every push.
- **CUDA-only work moves off the dev machine.** A WSL2 + NVIDIA workflow is retained **only** for the tasks that require it — model training and distillation (Phase 2) — on a dedicated NVIDIA box or cloud GPU, not on the M5.
- **Downstream editor integration:** where CapCut Pro is present (Windows/macOS/iOS/Android), the app writes drafts into CapCut's actual draft root (auto-detected via `root_meta_info.json`); on platforms without CapCut (Linux, or pro workflows) it emits **OpenTimelineIO / FCPXML**. Both are non-destructive project files.
- **Compute conclusion:** for the "≈2 hours of 4K footage per day, finished the same evening" scenario, the M5 handles orchestration, ASR, VLM description, and DSL compilation locally with incremental processing and on-disk result reuse; a single dedicated NVIDIA GPU (RTX 5090-class) remains sufficient for Phase 2's training. The key lever is unchanged: **incremental processing + persisting intermediate results to disk.**

---

## V. Narrative Layer Development Platform Architecture (Core Deliverable)

The perception and rendering layers are primarily **integration** work; the narrative layer requires long-term, continuous iteration, so it is designed as a distinct, pluggable platform with clearly defined boundaries. This architecture is deliberately stable — platform and runtime changes should never touch it, which is exactly the point of the contracts.

### Four Contracts / Subsystems

1. **Input Contract — Footage Graph.** A standard JSON schema (`footage-graph/0.1`) containing the shot list, keyframe descriptions, entity IDs, ASR transcripts, timestamps, and action/interaction labels. Upgrading the perception stack — including Phase 2's 主体衔接 — must not change the narrative layer, only enrich the graph.
2. **Subsystem A — Prior Knowledge Base (PriorStore).** Statistical pattern mining plus a template registry. Each template is a YAML/JSON config (parameters + optional prompt snippet), managed independently and added/edited/retired without touching core inference code.
3. **Subsystem B — Reasoning Orchestrator.** Runs the decision loop *Footage Graph + Priors + Templates → Editing DSL*, including agentic look-up and grounding verification. **The LLM backend sits behind a unified adapter interface and is replaceable** — this adapter is the seam that lets llama.cpp, MLC LLM, or a cloud API be swapped per device tier.
4. **Output Contract — Editing DSL.** A structured editing-command format (`editing-dsl/0.1`) aligned with the downstream draft format (in/out points, sequence, duration, transitions, effects, audio). This is the standard interface handed from the narrative layer to the rendering layer, and the guarantee of non-destructive re-editing.

### Feedback Loop (independent branch; does not block online generation)

Collect "N AI variants vs. the user's final selection/fine-tune" data in a standard format and use it **offline** for parameter fine-tuning, template iteration, and highlight-detector training. The core signal is the **modification rate** (`changeRatio`) of the AI rough cut against the human refinement.

---

## VI. Key Implementation Plan (Perception → Decision → Export)

### 6.1 Temporal Compression Funnel (consecutive similar frames → LLM-readable shot list)

**Core principle:** frame-level differentiation is handled by **classical computer vision**; the LLM only ever sees the compressed, structured storyboard, never the raw frame stream.

- **Two-pass retrieval:** a cheap model produces a rough description at **low frame rate**; the LLM plans and selects candidate clips; a **second, high-frame-rate pass** re-reads the original footage by timecode with a stronger model for fine-grained understanding.
- **Preferred selection metric:** automatic scoring with classical metrics (Laplacian variance for sharpness, optical-flow jitter, eye-openness/expression, composition) **without** invoking an LLM.

### 6.2 Countermeasures for the Six Major Bottlenecks in Narrative Comprehension (engineering firepower concentration point)

These are the six comprehension failures that most degrade rough-cut quality, with the concrete countermeasure the pipeline applies to each. They define where engineering effort is concentrated.

| # | Bottleneck | Countermeasure |
|---|---|---|
| 1 | **Cross-shot entity identity** — "is this the same person/object two shots later?" | Persistent entity IDs in the Footage Graph via segmentation-tracking (SAM 2, which Phase 2's 主体衔接 also uses to follow things through a clip, §VII); IDs carry through the whole timeline. |
| 2 | **Long-video temporal context** — a VLM cannot hold hours of footage | The Temporal Compression Funnel (§6.1): only the compressed storyboard reaches the LLM; raw frames never do. |
| 3 | **Action/interaction semantics** — "who did what to whom" | Dedicated action/interaction labels (MediaPipe / lightweight detectors now; in Phase 2, 主体衔接's subject–action–contacted-object records, measured from 2D tracks, §VII) rather than relying on the LLM to infer motion from stills. |
| 4 | **Causal / narrative arc** — beats, setups, payoffs | Statistical Prior Layer + template matching (§6.3) supply arc structure the perception layer cannot; the orchestrator grounds beats against timestamps. |
| 5 | **Multimodal alignment** — dialogue/audio vs. picture | ASR transcripts with timestamps aligned to shots in the Footage Graph; beat-aligned synchronization as a prior parameter. |
| 6 | **Hallucination / grounding** — the LLM inventing content not in the footage | Grounding verification in the Reasoning Orchestrator: every DSL instruction must resolve to a real timecode/entity in the graph or it is rejected. |

### 6.3 Editing Decision Layer: Three Parallel Stages (no strict two-stage sequence)

From day one, three layers are built in parallel:

1. **Statistical Prior Layer** — quantifiable patterns from contiguous segments (shot-length distribution, transition frequency, beat-aligned synchronization) as prior parameters.
2. **Template Library Layer** — modular composition (rhythm framework + emotional tone + content-specific modules), multi-tag blending, triggered by perception-layer signals for template matching.
3. **Preference-Feedback Loop** — each iteration generates 2–3 style variants for A/B testing; minor perturbation in template-parameter space or evolutionary search; the "AI rough cut → manual refinement" diff is the most critical signal source. The core metric, `changeRatio`, decreases with use, thereby increasing fit over time.

### 6.4 Exporting Project Files

- **Primary path:** compile the DSL to a **CapCut Pro draft** saved in the root directory CapCut actually reads (auto-detected from `root_meta_info.json`); it becomes visible after CapCut restarts. An **in-house plaintext draft generator** integrates transitions, cropping, subtitle styles, filters, and BGM tracks directly into the draft, with the earlier `cutcli` path kept as a fallback.
- **Portable / pro path:** **OpenTimelineIO (OTIO) → FCPXML** for professional users and for platforms where CapCut is unavailable (notably Linux). OTIO is the cross-platform interchange that keeps the product from being locked to a single editor.
- **Dependency isolation:** early open-source dependencies (e.g. `cutcli`) sit behind an adaptation layer so they can be forked or replaced without touching the DSL contract.

### 6.5 Director and Screenwriter Modules

User footage varies without limit — selfies, travel, dance, portrait and landscape mixed — so editing decisions are made by two named modules. Both map onto the Four Contracts rather than sitting beside them.

- **Director** — judges each moment as a picture: composition, light, colour, expression, and whether the shot means anything at all. It has two halves, because the narrative layer never sees pixels. The half that **looks** lives in perception and writes its scores into the Footage Graph (additive fields only). The half that **chooses** — orientation policy, which moment of a scene to use — lives in the Orchestrator.
- **Screenwriter** — decides order. It owns the no-consecutive-repeat rule (a scene may recur in a cut, never back to back) and a registry of **ordering strategies**: each strategy is a pluggable implementation registered in the PriorStore like a template, all emit the same result, and all pass the same constraint check. New orderings ship after release as new registry entries, without touching the Orchestrator.

**Division of labour.** Hard constraints — no consecutive repeat, the delivered length, the beat grid, grounding — are always rules: a model proposes, the rule checks, and a violation is rejected and recorded, exactly like the grounding gate. Measurable quantities stay classical CV (§6.1). Judgements that need to see the picture use **open models off the shelf in Phase 1**; **self-trained models replace them in Phase 2 only**.

| Module | Item | Phase 1 (open models, no training) | Phase 2 (self-trained) |
|---|---|---|---|
| Director | Sharpness, exposure, shake, colour | Classical measurement (implemented) | — |
| Director | Expression — eyes open, smile, mid-word | MediaPipe Face Landmarker blendshapes + rules | — |
| Director | Junk shots — camera coming up or going down, whip pans; a selfie not yet ready (face growing or moving fast, eyes on the screen, face cut off) | Motion rules (implemented) + face rules | Classifier trained on Phase-1 labels and user edits |
| Director | Composition | Composition-specialised model + depth layering + cinematography model (below) | Distilled composition head |
| Director | Light — type and condition | Cinematography model | Distilled with composition |
| Director | Orientation | Rule: pick the target orientation (the output aspect, else the footage majority); the other orientation ranks below, or is cropped through crop suggestions | — |
| Screenwriter | No consecutive repeat | Rule on scene id + visual similarity (implemented), applied to every strategy | Stays a rule |
| Screenwriter | Order by capture time | Rule (implemented) | — |
| Screenwriter | Order by dance continuity | 2D pose at each clip's head and tail; transition cost = pose + velocity distance; order = minimum-cost path under the beat grid | Generalised into 主体衔接 (§VII); its join cost later learned by the self-supervised continuity model |
| Screenwriter | Later strategies — story arc, emotion curve, by person, by place | Local VLM/LLM over the compressed storyboard, through the grounding gate and the constraint check | Distilled task-specific LLM (§VII) |

**Composition: a specialised model, and depth.** A general VLM scores composition poorly: on CADB, Qwen2.5-VL-32B reaches SRCC 0.42 where PhotoFramer's 7B composition model, fine-tuned on CADB, GAIC, KU-PCP and AVA, reaches 0.76. That model already carries the classical composition frameworks — KU-PCP's nine classes: rule of thirds, centre, symmetric, horizontal, vertical, diagonal, curved, triangle, pattern. A picture is three-dimensional and does not reduce to a sentence, so composition is not judged from a description: **monocular depth** measures the layers directly. A car door, a sleeve or a dashboard filling the near plane over a large part of the frame is foreground occlusion — the case classical signals could not separate from texture. **ShotVL-7B**, trained on shots from 200+ acclaimed films, adds shot size, framing, camera angle, lens, lighting type and lighting condition. **Cost:** these models score only the candidates the classical funnel shortlists (about three times the moments the cut needs), not every segment — §6.1's second pass. Tier C skips the 7B models and keeps depth and the classical scores.

**Dance transitions.** A low transition cost gives a hard cut on the beat — a match cut on the movement; a high one gets a covering transition placed on the beat. Only 剪映's dissolve has a known resource id today; any further transition must be read from drafts 剪映 itself writes before it can be emitted.

**Subjects for the screenwriter.** Ordering by who and what appears reads the Footage Graph's entity and action fields, which perception fills (face grouping, objects, actions). Nothing in the screenwriter looks at pixels.

### 6.6 The Video Butler (视频管家) and the Memory Map (回忆地图)

Decided 2026-10-07: the album page becomes the **video butler**, and the product gains a one-tap hand-over of all the footage on the device ("一键托管"). It is the same pipeline turned around: instead of the user choosing footage for a cut, the butler reads every video, files it, finds the stories in the collection, and makes each one a project — non-destructive and offline like every other cut.

**What one tap does.** The butler walks the folders videos and photos live in (on a Mac: Movies, Pictures, Desktop, Documents, Downloads; on Android: DCIM, Movies, Pictures, Download — the user can change them, and a disk plugged in is added whole in one tap, since a picker opened at home could not reach an external drive a user had their videos on), skipping app bundles, photo-library packages, build output and editors' drafts, and each file once however many links point at it. A caller that already holds the list of files — a file server that has indexed its disk, the ButlerNAS case — hands it over in place of the walk (`hosting.start` with `files`, decided 2026-10-17): what is not on the list leaves the catalogue, except what came from a kept folder that cannot be read now, as with a walk. Each video it has not seen is looked at in **three frames** (a fifth, a half and four fifths of the way in), and each photo once (decided 2026-10-17: photos are footage to the butler as they have been to a project since 0.1.2): scene labels with their confidences, and the faces in them. A picture under 480 pixels on its short side is an icon, a sticker or a thumbnail, not a photo someone took, and is passed over; a screenshot, known by its name, is a picture of a screen as a screen recording is a video of one — whoever is on it is never filed as one of the user's people, and it is never a live-stream clip. What was *seen* is kept in a catalogue in the library, keyed by path, size and modification time, so a second pass looks only at what is new, and a category the user removes, or a change to the rules, re-files the collection without decoding a frame. With hosting on, the app looks again each time it opens; a disk kept and not plugged in keeps what was filed from it, and the page says it is not connected, and why a disk macOS has not let the app read shows nothing. A 230-video sample library (a trip, a set of 4K family clips, a drone album) took 79 s on the reference Mac through the CLI backend.

**Categories.** Two kinds, as asked:

- *Faces*, person by person: the album. Faces are found by YuNet and grouped by **AuraFace** (this table's face embedder, §VII), across every video; Apple Vision's feature prints remain the fallback where AuraFace is not installed, and they are not identity — on a sample set they filed two people as one person and the whole library under four. Only faces large enough to be a video's subject (at least 0.4% of the picture, about a fifteenth of its width and height) are filed: passers-by are counted in a frame, never listed as people. Nor are faces in a video recorded off a screen — a screen recorder's file name, or Vision reading most of its frames as a screenshot: a live stream's host, someone in a film or on a call is someone on a screen, not one of the user's people (decided 2026-10-10). A live-stream host recorded off a phone screen had been filed with a baby filmed years later — 0.615 from one of the faces the baby's group is kept by, under the 0.62 a face is matched at — and opened the baby's growing-up memory; 29 groups on the 2,555 videos were only ever faces on screens. Such faces are still counted, so a live-stream clip is still filed as one, and those filed before are taken out of the album on the next pass, without decoding. The butler lists a person once they are the subject of two videos — a day of photos counting as one, since a burst of twenty photos of a stranger at a viewpoint is one sighting, not twenty — or the user named them: of 723 faces on 2,555 real videos 613 were in one video only, mostly strangers in saved clips, drawn faces and blurs; the album keeps them all. Groups are made one face at a time, and two can each gather sightings that end up nearer each other than either was to its first, which nothing went back to join: after each pass, groups whose kept faces come within 0.48 — far stricter than the 0.62 a face is matched at — and that are never the subjects of one video together, nor both named by the user, are joined. On 2,555 real videos the four pairs closer than 0.47 were each one person (one in four groups, another in two), while from 0.50 up two children in one frame, two different adults, and a baby and a stranger on a screen came as close; so the matching threshold was not loosened. The user names a group and can merge two groups that are one person.
- *Smart categories*: study, live-stream clips, children, electronics, transport, food, gatherings, scenery, animals (`vb_perception::categories`). Rules over the labels — a category is a set of labels and a confidence a frame must reach — not a model trained to sort videos. *Study* (books, pages, handwriting, whiteboards in the picture) and *live-stream clips* (a screen recording, by its name or by Vision reading its frames as screenshots, with someone on it) have no label of their own; the app marks them as unchecked. Apple Vision's label tree needed two corrections on real footage (2,555 videos, below): it files doors, windows, stairs and escalators under *conveyance*, which had put 145 corridors and doorways among 350 transport videos, and every screenshot under *document* at the screenshot's own confidence, which had made 221 of 257 study videos a phone's screen recorded. Transport no longer reads *conveyance*; study reads no screen recording and no bare *document* — a real page comes with *printed page*, *handwriting* or *book* — and holds 18 videos there: forms, handwriting, a recorded lecture. Live-stream clips there are short videos recorded off the phone or saved from Douyin, with someone on screen.

Every category opens onto its videos: played in place, chosen all or some, and made into a new project. A category can be removed — no video is filed under it from then on — and restored.

**Memories**, found from the catalogue's numbers (`vb-pipeline::memories`): when (the camera's clock; a file's date is never trusted for a memory — but a camera app's file name can be the clock: WeChat's own camera writes no capture time and names a clip `wx_camera_<milliseconds since 1970>` as it saves it, 0.3 to 16 s after the creation time on the 25 that had both, so that name is read as one; a clip saved out of a chat, `mmexport…`, or a 剪映 export, never is), where (the GPS fix the camera wrote, located offline against the map below), and who is a subject.

| Kind | Rule |
|---|---|
| Trip (旅行记忆) | Home is where someone lives and comes back to, and people move. A clip is filmed where someone lived when its place was filmed on three days or more spanning two months or more within three months either side of it (a year where little was filmed; else the place lived in on the most days overall). Those clips, in order, fall into runs at one place, and a run of six weeks or less between two runs at another place, lived in at least as much around then, is a visit — a weekend back in the city lived in before is a trip there, while a year of footage from it is not a string of trips. Clips filmed away within 36 h of each other are one trip; clips with no location (drones, action cameras) join it for up to four days after its last located clip; home clips within a day of leaving or returning are its departure and return. At least three clips away on two days or more away — departure and return do not count — and at most 45 days. Named after the city 70% of it was filmed near, else the province (China) or country — never the country of home — else its two main regions: "YYYY年M月到<地方>的旅行回忆", "YYYY年M月到<地区A>、<地区B>的旅行回忆". A trip nothing locates — a phone backup can lose every GPS fix, which Android zeroes in a copy made without the location permission — is a run of clips with no location at all, each within 36 h of the one before, at most 45 days, with a drone (what DJI Fly saves, `dji_fly_…`) flown on two days or more of it: on one sample backup a week away was 202 clips with no location and drone footage on seven days, while the one other drone flight in years was a single afternoon. It has no place: 「YYYY年M月的旅行回忆」, no pin, no lit region. |
| Growing up (成长记忆) | A child who is the subject of five videos or more, over three calendar months or more, at least 90 days apart. People only: one pet cannot be told from another. Nothing on the device estimates age, so a child is someone whose frames, those they are alone in, the labeller reads as a baby, child or teen more often than as an adult — on 2,555 real videos the three children so in 152 of 169 frames, 179 of 180 and 61 of 66, and none of eleven adults in more than a fifth of theirs, who had been fourteen 「TA的成长记忆」. The memory is made of the sightings the child's own frames do not contradict — a sighting alone in frames that read as an adult and as no child at all is someone else's (a parent at the edge of a baby's frame still leaves a baby word) — in the one stretch of time with no gap of more than a year (decided 2026-10-10): faces are grouped, not known, and on the 2,555 videos a baby filmed over about a year had also been filed with four unrelated people — two on screens, two at gatherings years earlier; these left the memory, from 196 videos to 190, and nothing else did. |
| Year (年度记忆) | Every video of a calendar year by the local clock (the zone of where it was filmed, or the one a camera's file name implies), at least six on three days. Cut by time, or by kind — people, then scenery, then food, then the rest, each from January to December — which the user chooses per year. |
| Group (人群记忆) | The same two or more subjects together in three videos or more on two days or more; a group always seen with one more person is that larger group. |

Each memory becomes a project (`project.memory`), made from at most **40 of its videos** spread over its time — a year in proportion to its sections — and cut in the background, one after another, with the heat guard. Photos fill what the videos cannot: a memory of forty videos or more is cut from videos as before photos were filed, and one of fewer takes every video and photos spread over its time for the rest — the cut holds a photo three seconds at most and never lets photos crowd out the videos, so more would be analysed for nothing. A memory whose footage has not changed is not cut again; one whose project the user deleted is not made again unless asked. The new topics `annual` (template `content.annual-review`) and `together` carry them; ordering by kind is a screenwriter strategy in the orchestrator (`extra.order: sections`), reading the Footage Graph's new optional `source.categories` — perception enriches the graph, the narrative layer orders it.

**The memory map.** Every memory with a location is a signal labelled with its year and month on a world the user pans and zooms, the regions it was filmed in lit in its kind's colour; a layer panel top right shows one kind, with the places of its videos. It looks like a monitor, after **World Monitor** (decided 2026-10-08): a night sea, dark teal land with lit coasts, a faint grid, glowing signals that pulse a few beats when shown, monospace tags and readouts (where the view is, how close), dark whatever the app's theme. Only the look is borrowed — World Monitor is AGPL-3.0 and none of its code is in the app. On a phone held upright the map opens on where the memories are, since the whole world is a thin band there. Drawn from **Natural Earth**'s 1:10m outlines (public domain), China's point-of-view countries and China's provinces, simplified into `vb-geo` and compiled in: the engine names a trip from the same outlines the app lights, and nothing is fetched. ⚠️ A map shown in an app published in mainland China needs a 审图号 (map review approval); this data has none. Before release there, the map must pass review or be replaced by the standard map service's (§XII).

*Measured 2026-10-17 with photos:* two days of a sample trip, 395 photos and 24 videos handed over as a list, filed in 61 s on the reference Mac (CLI backend, Apple Vision labels, AuraFace). The photos joined the people the videos had found — one person of the trip in 3 videos and 13 photos — and 382 of them joined the trip by time, named after its region from the videos' GPS, since none of the photos carries a fix. One 0.83 s clip was passed over as shorter than a second.

*Measured 2026-10-07 on a sample library (230 videos):* a ten-day trip found and named after its region and month (「YYYY年M月到<地方>的旅行回忆」), 183 videos, the 59 drone clips with no GPS joined by time; two years; three pairs' group memories; one adult of the trip given a growing-up memory across three years. With AuraFace the four people of the trip were each grouped (24, 18, 17, 15 videos); masks with sunglasses split one person into two groups, and a newborn and the toddler a year on did not group — the merge in the album is for both. A decorated ceiling passed YuNet at 0.75 as a face once.

*Measured 2026-10-07 on a sample library of 2,555 videos on an external drive:* about ten years of phone backups, WeChat, DJI, screen recordings and saved clips, and a move between two cities — 7 minutes for the pass on the app's backend (LibAv), 16 s for a pass with nothing new. It found five trips where the first rule had found eleven: a year lived in one city had read as eight trips there, and two day trips counted the evening at home before them as a second day; the fifth is a week-long trip, which that phone backup holds with every GPS fix zeroed — Android's redaction of location for an app without the permission, kept in the copy — found by its drone flights and named 「YYYY年M月的旅行回忆」. Seven years, three children growing up (there had been fourteen people, eleven of them adults, all titled 「TA的成长记忆」) and nine groups; two people of the trip, who had been four groups and two, are one each. WeChat's camera dated by its file names took the videos with no camera clock from 817 to 377 — saved clips, 剪映 exports, screen recordings — which are in no memory. The pass also found two faults in the app's media backend, both fixed: keyframe stills of clips filmed on their side reached every model on their side, with blacks 16 levels up, and VideoToolbox's refusal of some valid H.264 (a Meizu's temporal layers, some WeChat clips) cost those videos every frame. With upright stills the videos with a subject went from 677 to 825, and the largest person from 54 videos to 119.

### 6.7 The Built-in Editor (0.2.0)

Decided 2026-10-09, and the version moves to **0.2.0** with it. Exporting to 剪映 had too many limits to be the only way to finish a cut: it reads footage only under `~/Movies` (its sandbox), rewrites a draft encrypted once it is opened, takes transitions only by resource ids read from its own drafts, and on a phone takes a cut as loose clips with no audio, text or transitions. So the product gains its **own editor, modelled on 剪映's**, and from 0.2.0 every analysed cut offers two ways on: **edit it in the app** and export the finished video, or **export a project file** (剪映 / CapCut draft, OTIO) as before. An edited cut can still leave as a project file.

**One cut, one manifest.** The editor is a view over the Editing DSL; the cut stays a decision manifest from first to last, and nothing is rendered until a video is asked for. Every change is one operation of `vb-edit`, applied by the engine: split at the playhead, trim either end, a card's or photograph's length, delete, move, duplicate, speed 0.25–4×, a clip's volume and mute, every clip's sound off, a transition per join or at every join (cut, dissolve, dip to black, wipe left or right), position (scale, move, turn), crop, brightness / contrast / saturation, the background round a picture that does not fill the frame (black, a colour, or the picture blurred), a stretch of the project's own footage inserted, text (words, size, colour, bold, outline, box, placed by alignment or dragged in the preview, length), music (laid under the cut once or looped, volume, mute, fades, replaced, removed), the canvas's ratio and the title. Each takes a valid cut to a valid cut, checked after every operation; one that cannot be carried out leaves the cut as it was and says why, in the user's language. Positions the user points at — the playhead, where a text is dropped — become `{slotId, offsetUs}` anchors at once against `resolve_timeline`; nothing absolute is ever stored, so the drift rule of the contracts holds in the editor too. A clip's own sound and the text anchored to it follow the clip; music keeps its place on the finished timeline as clips move under it, as in a phone editor.

**Sessions, undo, and newer cuts.** The engine keeps one session per project (`vb-pipeline::editor`), written to disk after every change under the project's `edit/`: the AI's cut it started from, the cut as it is now, a copy of the analysis (a new run of the project rewrites its own), and which run it began from. Undo and redo are whole cuts, 200 steps deep. Cutting the project again never overwrites a person's edit: the editor says a newer cut exists and offers to start from it, keeping the old edit beside it. The operations are `editor.open`, `state`, `apply`, `undo`, `redo`, `revert`, `frame`, `strip`, `footage`, `export` and `close`, through the one dispatcher like every other.

**The preview.** With the playhead stopped, the editor shows the engine's own frame: the renderer's compositor at that moment, exactly what the exported video holds. Playing, each clip plays through the platform's player (AVFoundation, ExoPlayer) from its moment of footage at its speed, opened two seconds before it is due, and laid out by the renderer's rules carried into Dart — fitted then scaled, moved and turned; the same brightness/contrast/saturation matrix; text in points of a frame whose shorter side is 540; the same four blends — with the music under it and its fades. The web build, which cannot play a local file, steps through the engine's frames.

**The finished video.** `vb-render` composes every frame in Rust — placement, adjustments, backgrounds, transitions, and text shaped and rasterised by `vb-text` (cosmic-text over the system's own fonts, Chinese first; MIT/Apache) — mixes the sound (each clip's at its speed with its pitch kept, through the same `atempo` description on both media backends; music with its fades; ducking), and hands both to the media backend's encoder: H.264 or HEVC from the platform's encoder (VideoToolbox on Apple; never x264/x265, which are GPL) and AAC from libav, in an MP4 with its index at the front, BT.709 limited range and labelled so. No part of the edit is an ffmpeg filtergraph, so no path ever enters a filter string and no GPL filter is needed (`drawtext` would need FreeType or libass, and `eq` is GPL). The LGPL ffmpeg gains the AAC encoder, the MP4/MOV muxers, five audio filters (`abuffer`, `abuffersink`, `atempo`, `aresample`, `aformat`) and, on Apple, the VideoToolbox encoders. The in-process backend (`LibAv`, the app's) reads frames and sound in order and writes the piece in process; the parity suite holds it to the CLI backend — frames within one time-base tick and 1.5 levels, sound sample-identical at 0.5×, 1× and 2×. A render is a job with progress and cancel, paced by the heat guard; a cancelled one leaves nothing behind.

**Lossless export (decided 2026-10-13, after LosslessCut).** A cut that is only cuts — clips of untouched footage of one kind, end to end, at their own speed, nothing drawn over them, nothing added under them, the canvas the footage's shape — leaves by copying each clip's compressed packets: seconds for any length, and the footage exactly as filmed. A copy can only start where a picture is whole, so each clip starts at the keyframe at or before its in point; and a clip whose out point falls inside a group of B-frames runs on a few frames to that group's reference frame, every frame shown, rather than freezing over the frames between — the export sheet says how much both add before anything is written; whatever stands in the way (a transition, text, a title card, a photo, speed, placement, colour, music, a changed level, footage from two cameras) is listed in words a person can act on. Neither media backend gives the concat demuxer in and out points, which end a part when any stream reaches the out point — phones store sound ahead of picture, and real footage lost a third of a second of picture at every seam. The linked backend copies the packets itself; the CLI backend cuts each part to a file and joins the whole files, as LosslessCut does; and the parity suite holds their keyframes, frames and sound to the same times. It is also the one video export a build with no encoder has — Android, Windows, Linux — and on Android it writes to Movies/VideoButler. Beside it, each clip can leave as a file of its own, copied when it is footage as filmed and rendered otherwise. LosslessCut is GPL-2.0: its approach was studied, none of its code is used. Smart cut — re-encoding only up to the first keyframe — was left out: the re-encoded part's codec header differs from the camera's, which LosslessCut itself calls experimental.

**The feedback signal.** A cut a person changed carries `provenance.editor`, `editedAt` and `changeRatio` — how much of the finished piece's footage differs from the orchestrator's, by footage time — computed by the engine on every save. This is §6.3's core metric, now measured where the editing happens instead of recovered from another editor's draft.

*Measured 2026-10-09 on the reference Mac, through the app's own backend (LibAv, VideoToolbox):* twelve sample clips cut to 29 s, then edited — a dissolve at all nine joins, a caption, one clip at half speed — and exported at 1080p30 H.264 + AAC: 27.5 s of video in **7.5 s**, about 3.7× real time, 22 MB; the frames looked at mid-dissolve, under the caption and in the slowed clip. In the real app, end to end (the integration test): the run's cut opened in the editor, split, captioned, played through the platform's player, exported at 720p and read back at the cut's length.

**Not yet.** A rendered video on Android (its MediaCodec encoder through libav, or Media3) and Windows/Linux (Media Foundation; OpenH264); keyframed animation beyond one held placement; effect, filter and sticker libraries; a second video track (picture in picture); the music's waveform; subtitles from speech inside the editor; snapping to the beat.

---

## VII. Long-Term Roadmap: Two Phases

### Phase 1 — Application Framework on Open Source

**Objective:** deliver a working, cross-platform product by **integrating existing open-source components**, not by training new models.

- **Shell & runtime:** Flutter + llama.cpp/MLC LLM (§III), one codebase across five platforms.
- **Perception (off-the-shelf):** shot detection + classical CV scoring; **Whisper / whisper.cpp** (or a compact on-device ASR) for transcripts; a **VLM dense-description adapter** (OpenAI-compatible; Qwen-VL / small VLM / cloud) for keyframe descriptions; **SAM 2** for segmentation/identity; **MediaPipe / lightweight detectors** for faces, hands, objects.
- **Narrative & export:** the full Four-Contracts platform (§V) and the export layer (§6.4), already substantially implemented (see §IX).
- **Exit criteria:** the §1.2 acceptance boundary met on all five platforms, fully offline on Tier-A/B hardware.

🟥 **Phase 1 trains nothing.** Every model is an open model used as released, chosen by measured effect; the licence is a gate, not a preference (§XI).

**Phase 1 model selection** (licences read from the model repositories, 2026-09-22):

| Role | Model | Engine | Weights licence |
|---|---|---|---|
| Composition score + classical composition class — **integrated** as the second pass over a cut's shortlist; a local model until the converted GGUF is hosted | PhotoFramer composition model (Qwen2.5-VL-7B, GRPO), converted to GGUF Q4_K_M | llama.cpp | MIT |
| Cinematography: shot size, framing, angle, lens, lighting | ShotVL-7B (Qwen2.5-VL-7B) | llama.cpp | Apache 2.0 |
| General VLM: scene naming in the user's language, descriptions, later orderings — **integrated** for captions, 8B at Q4_K_M with its F16 projector as Qwen published them | Qwen3-VL 8B / 4B / 2B by tier | llama.cpp | Apache 2.0 |
| Depth layering: occlusion, subject distance — **integrated** | Depth Anything V2 Small (DA3 Small/Base to evaluate) | ONNX Runtime | Apache 2.0 |
| Scene similarity embedding; **integrated for scene labels where there is no Apple Vision** (Android, later Windows and Linux): zero-shot against a fixed table of places, subjects and activities, the image half at fp16 in the app, the table's sentences embedded once on the development side. This table gives scene naming to the general VLM, which on a phone's CPU takes about a minute an image; *a deviation to confirm or reverse* | SigLIP 2 (base, patch 16, 224 px) | ONNX Runtime | Apache 2.0 |
| Salient subject mask: placement, cut-off subject | BiRefNet (lite) | ONNX Runtime | MIT |
| Face detection and landmarks — **integrated** | YuNet (OpenCV) — in place of MediaPipe's detector, which is published only as TFLite | ONNX Runtime | MIT |
| Person mask: a subject whose face is hidden by a mask, sunglasses or a hood — **integrated** | PP-HumanSeg (OpenCV model zoo) | ONNX Runtime | Apache 2.0 |
| Expression blendshapes | MediaPipe Face Landmarker | ONNX Runtime (converted) or LiteRT | Apache 2.0 |
| Face embedding for grouping — **integrated** for the album and the video butler (§6.6), aligned on YuNet's five landmarks | AuraFace v1 — `glintr100.onnx` only | ONNX Runtime | Apache 2.0 |
| 2D human pose: dance continuity — **integrated as RTMO-s**, the family's one-stage multi-person model, because top-down RTMPose needs a person detector this table does not name; *a deviation to confirm or reverse* | RTMPose / RTMW (rtmlib's ONNX exports) | ONNX Runtime | Apache 2.0 |
| Motion feature: continuity, to evaluate | V-JEPA 2 | ONNX Runtime | MIT |
| ASR | whisper.cpp (implemented) | whisper.cpp | MIT |

Where two candidates compete, the one that does better on real footage the user has judged is kept; a new model enters the catalogue only after that comparison.

**Measured so far (2026-09-22, a 126-clip sample set):** depth + YuNet + PP-HumanSeg find the foreground occlusion the classical signals could not — every door, dashboard, sleeve and arm in the previous cut, with about 85% of flagged segments truly unusable — and now weigh each segment in the Orchestrator (§6.5). PhotoFramer was converted to GGUF (Q4_K_M, 4.5 GB, plus a 1.35 GB projector) and run through llama.cpp at about 10 s a frame: it tells usable frames from junk well (AUC 0.92 over 21 usable and 14 unusable frames of that set) but misses small intrusions such as an arm at the edge, which depth catches. It now runs as §6.1's second pass: after a first cut, the clips it used and the best segment of each strongest scene — three times as many as the cut holds — are rated, each image capped at 1,024 tokens (about five seconds a frame on the reference Mac), and the cut is made again with the ratings.

**Measured 2026-09-23 (the same set, and a 131-minute set of 4K HEVC clips):**

- *Speed.* The 131-minute 4K set had taken 15.6 hours: every clip decoded twice in software (scenes, then the sweep), one clip at a time, and the app napped by macOS once its window was covered. VideoToolbox decoding (bit-exact, repacked to the software layout, so every reading is unchanged), one decode for scenes and sweep, three clips at once, and a process activity against App Nap brought one 86-second 4K clip from about two minutes to 25 seconds, and 126 clips of 1080p to about five minutes.
- *Captions.* Qwen3-VL 8B writes one sentence per captioned clip in the user's language, about four seconds each — a plain sentence of who is doing what, and where — replacing speech-to-subtitle, which on this footage produced noise such as "纪利闆兵:难得疯了" in mixed scripts.
- *Dance.* On the sample set, motion alone picked selfies, a horse ride and a drive as "dancing" and missed a seated hand dance. A stretch counts as dancing when its dancers are seen whole (shoulders, elbows, wrists, a hip) in 60% of its poses, their number holds in 80%, and their arms move at least 0.35 torso lengths a second; that picked the seated hand dance and the outdoor duets, and rejected the rest. The cut groups solos and duets, and the in points chosen for the joins put matching poses either side of most cuts (pose differences 0.15–0.47 in torso lengths). The takes' own audio was tried for aligning them and failed: the song they danced to is not in the recordings.

**Measured 2026-09-24 (scene labels off Apple platforms):** SigLIP 2 was checked against 92 frames of the two sample sets, each judged by looking. A place it names is right 85% of the time and it names one for 42% of frames; people and things 82% and 49%; activities 63% and 24%, the weak kind (a masked selfie read as skiing). Its int8 export gave different answers on Android's ONNX Runtime than on the Mac's — a horse ride labelled nothing, a grassland a desert — so the app carries the fp16 export, which matches full precision on both: about 60 ms a still on the reference Mac, 0.4 s on an arm64 Android emulator. Face grouping has no replacement off Apple platforms yet.

### Phase 2 — 主体衔接 (Subject Continuity) + Self-Trained Models + Associated LLMs

🟥 **Revised 2026-10-04: Phase 2 builds no 4D spatio-temporal model.** The previous plan — a subject-centric 4D representation (each subject's 3D appearance plus its motion along the time axis), reconstructed offline — is withdrawn together with its 3D steps: 2D→3D pose lifting, the dynamic scene graph, the worldline database and the feed-forward 4D routes. What the product needed from it is one capability, 主体衔接, and that is reached on 2D pictures alone.

**Objective: 主体衔接.** At every cut the action subject keeps its place in the frame, its size and its pose, and its movement carries on in the same direction at the same speed: the shot changes, the action plays straight through. Perception recognises only what a 2D picture shows — subject, action, object, background — and subjects and objects may each be people or things, one or several. It is a cut style, chosen as the dance style is today; footage in which no local全套动作 is found is reported as such, never cut as if it held one. It runs on the device and offline, like the director.

**Terms**

| Term | Meaning |
|---|---|
| 动作主体 — action subject | Who or what performs the action: a dancer, a couple, a child on a swing, a dog, a kite. People or things, one or several. |
| 直接接触客体 — contacted object | A person or thing the subject touches directly while acting: a partner's hand, the ball being kicked, the swing. Near but untouched is not an object of the action. |
| 背景 — background | Everything else. It is recognised so that a join can change it — the scene changes, the action does not — and it is left out of every continuity measurement. |
| 动作 — action | What the subject does, read from its 2D movement: body pose over time for people; position, size and orientation over time for things. |
| **local全套动作** | One clip's complete action of a subject: the continuous stretch in which the subject is followed and acting, keeping only the frames that show the subject, and only the data of the subject and its contacted objects. A subject that stops and starts again in one clip has one per stretch. |
| **global全套动作** | The whole action, assembled from every clip's local全套动作: each piece shows a part no other piece shows and continues the piece before it in direction and speed. The finished cut plays it complete, from start to end. |

**Method.** Steps 1–2 are perception and write the Footage Graph; steps 3–7 are the narrative layer and write the DSL.

1. **Recognise, in 2D.** In each sampled frame of a candidate stretch: the subjects, the objects they touch and the background, each subject and object followed through the clip under one id. People come from what the director already runs — RTMO-s poses, PP-HumanSeg masks, YuNet faces; things need masks of their own (models below). Contact is measured, not assumed: a hand, a foot or a mask edge meets the object in the picture *and* at the same depth (Depth Anything, already run), so a ball behind a player is not a ball at the foot.
2. **Extract each local全套动作.** Per clip and subject: where the action begins and ends — at the rests the sweep already finds, or where the subject enters or leaves the frame — and at every sample the box, mask area and orientation of the subject and its contacted objects, the keypoints of people, and their velocities. Place and size are fractions of the frame and movement is in subject lengths a second (torso lengths for people, as the dance cut measures), so a near take and a far take of one action compare. Several subjects are matched one to one: a duet joins a duet. A take that performs the action more than once — a dance danced twice through — holds one local全套动作 a performance: it is split where it matches itself a phrase later (at least 8 s, over most of the performance, at most 0.6 of its typical pose gap).
3. **Compare every pair, in both directions.** (a) *Alignment*: whether two local全套动作 show the same action and at what offset — subsequence matching of the normalised motion, on the keypoints that move (a body still from the waist down otherwise makes any two arm movements look alike) — which is what says which part of one repeats part of the other. (b) *Continuation*: at each candidate join, how far place, size, pose, direction and speed jump from the outgoing piece to the incoming one, once the incoming clip is framed (step 6).
4. **Assemble the global全套动作, whatever order the footage was read in.** Takes of one action start at different points of it — one holds moves 1–4, another 3–7, a third 6–9 — while files are imported, read and finished in an order that has nothing to do with that: copy order, and three clips analysed at once, largest first. So nothing is chained in reading order. Every local全套动作 is placed on the action's own time axis by its alignments to the others — the layout step of sequence assembly, where each take is a read of the whole and the overlaps place the reads. On real dance, one pair's alignment score does not tell a true match from a chance one — two takes of different moves line up nearly as well as two of the same — so placement is by consensus: the take most aligned with the rest starts the axis, and each next take goes where most of those already placed put it, joining only when more agree than not (two performances in one clip count once) and at least two do, or one alignment stands out on its own. Pieces that do not overlap (a throw in one place, the catch in another) are joined by continuation alone. The result is the order of the pieces and, between each pair, the window the switch may fall in: their overlap, or their boundary. No moment of the action is shown twice; capture time only breaks ties.
5. **Cut on the beat.** Each join is placed on a beat inside its window, every clip a whole number of beats as in every beat cut, and all the in and out points are chosen together for the least total jump — the dance cut's Viterbi (§6.5), widened from a fixed order to the layout's windows. Where the people dance, each clip starts on one of their own beats, which the piece's speed puts on the cut's (*Beat, music and length*, below).
6. **Frame each clip.** Crop, scale and rotate so that at each join the subject's size, place and orientation match the clip before. The framing is keyframed at offsets within the slot, so a clip can match the one before at its in point and the one after at its out point. Two bounds are physical. The picture is only ever enlarged — shrinking it would leave an empty border — so sizes are matched by cropping into the clip whose subject is smaller, only as far as its resolution allows against the output's (a quarter past it: 1.25× for a 1080p clip in a 1080p cut). Size comes first: the picture is enlarged to the other subject's size and no further, and moved as far as the frame's edge allows at that size; a place it cannot reach stays in the join's cost rather than being bought with a larger subject. And rotating needs further enlargement to keep the frame filled, so it is held to small angles. A join that needs more than the bounds allow does not count as continuous.
7. **Check and report.** The cut plays the global全套动作 from start to end, except where the people stand idle or its length forces clips to be shortened (*Beat, music and length*, below). Every join's remaining jump — place, size, pose, direction, speed — is recorded. A join above threshold is not presented as continuous: its clip's reason says so, as the dance cut's does today, and the result counts the joins that carry the action and those that do not. Thresholds are set on real footage by looking at the joins, like every threshold in this project.

**How it fits the contracts.** No new contract, and the narrative layer still sees no pixels.

- **Footage Graph:** subject and object tracks, contacts and the local全套动作 are added, optional fields of `footage-graph/0.1` — no new version. Only candidate stretches are read densely, cached by content and parameters, as the dance cut's poses are today (for 主体衔接, ten a second). For people one field was added, `segment.rhythm`: the tempo the people move to, how clearly, the times of their beats, and where they stand idle — measured from the poses perception already writes (`segment.poses`), which are the subjects' tracks. The local全套动作 are derived from those in the narrative layer — geometry on the graph's numbers, like the rest. Things will need their tracks added.
- **Narrative layer:** alignment, continuation, assembly, beat placement and framing are geometry on the graph's numbers. The Screenwriter owns the order, as a registered ordering strategy (§6.5); the Director's choosing half owns the framing.
- **Editing DSL:** the framing is a decision in the manifest, never baked into pixels, so 主体衔接 stays non-destructive and the user can reset any clip's framing in the editor. The DSL's `crop` rectangle — which nothing writes or reads yet — cannot express scale, rotation or keyframes, so an optional keyframed transform is added: scale, offset and rotation at offsets within the slot, never at absolute timeline positions. The renderer applies it in its filter graph; the 剪映 writer emits it as the clip's own scale, position and rotation keyframes, copied from a draft 剪映 writes, not guessed; OTIO carries it in metadata. *Deviation, to close:* 剪映 writes a draft back encrypted once it has opened it, and no draft on the development machine has a speed change or keyframes, so the speed material and keyframe shapes follow the open-source pyJianYingDraft's reading of 剪映's drafts; they must be checked on 剪映's screen before they count as copied.
- **Rules:** inside a 主体衔接 cut the no-consecutive-repeat rule is met by construction — no moment of the action is shown twice — so two pieces of one scene may follow each other where the second continues the first; outside the action the rule stands. At a join, speed is matched by choosing the moments: no clip is retimed on its own to match another.
- **Beat, music and length** (decided 2026-10-05, replacing the rules of 2026-10-04, which put the length first, never looped the music and sped up to 1.5×). When a 主体衔接 cut follows the beat — the user turned 卡点 on — the dance comes first and the length second:
  1. *Read the rhythm.* Before anything is chosen, every take with people in it is read for the beat they move to (`segment.rhythm`): the tempo of their limbs' movement, how clearly it stands out, and the moment of each beat, from the poses at ten a second.
  2. *Leave out what is not dancing* (widened 2026-10-06: learning the steps is not dancing). Removed from the global全套动作 — kept in its take, so the take still aligns with the others whole, and never shown:
     - *idle*: anyone — the people together, or any one of them alone — standing still past the pose model's own jitter for more than one and a half of their beats: waiting, a pose held past the music, a dancer standing while the other dances;
     - *watching*: someone keeping a hand held still in front of their chest for more than 60% of four bars, as a person does with a phone that shows the steps. On the sample set's performances no four bars came above 0.47; in a take of two people learning the dance it covered 94% of the take;
     - *a step on its own*: dancing shorter than a bar with no dancing either side.

     A take whose people dance less than a quarter of their action is not a dance — learning the steps, waiting about — and is left out whole (in the sample set the learning take danced 4% of its action; no performance less than 34%). A move no other take repeats, danced for less than two bars, carries no phrase and is not part of the dance either (the dancers turning to walk off after one). Whether movement keeps a beat is recorded but removes nothing: in the sample set one real dance read no clear beat, and random movement can read as clearly as a dance.
  3. *Put the dance on the beat.* The cut's beat is the music's or, with no music, the dancers' own. Each piece plays at the speed that puts its dancers' beat on the cut's — their tempo, or half or double it, brought to the cut's within 1.2× faster or slower — and each clip starts on one of its take's beats. A piece further from the cut's beat than that plays at its own speed, its cuts on the beat and its steps not, and says so. No clip is ever sped up or slowed down by more than 1.2×.
  4. *Music.* Music never shortens the cut and never speeds it up. Shorter than the cut, it plays again from its first beat when the user chose to loop it; otherwise the rest of the cut has no music. With no length chosen the cut's length is unknown until the footage is analysed, so the preflight cannot ask: the loop switch says what each way does.
  5. *Then the length.* The room is the length the user chose or, when none is chosen, the global全套动作's own length at the speeds of step 3. Longer than the room: every clip is shortened, each by the same share, in whole beats, with the in and out points chosen again for the least total jump (step 5) — never sped up off the beat it was put on. At the shortest clips the template allows and still too long, the cut ends where the room does, as every cut does. Shorter than a chosen length, the cut ends with the action rather than being padded.

  Off the beat, nothing is idle by rhythm and a room too small is met by speed first: every clip by one shared factor, at most 1.2× — shared so that each join keeps the speeds it was matched at — then shortened as in step 5. The result says which of these happened: each piece's tempo and speed and whether its steps are on the beat, how much idle was left out, and how much of the action was not shown. The speed is the slot's existing `speed`, which the grounding gate checks (declared duration = source range ÷ speed); the 剪映 writer writes it as a speed material, OTIO as a linear time warp, and the render plan carries it.
- **The dance cut is its first case:** people as subjects, dancing as the action, pose, placement and movement already matched at each join (§6.5). 主体衔接 generalises it to things, contacted objects, overlapping takes and framing.

**Delivery order**, each on macOS first: (1) people — alignment, layout, beat placement, and the DSL's framing transform and shared speed through the renderer and 剪映, on the poses the dance cut already reads; (2) things — object masks and contact; (3) the other platforms, as the model host reaches them.

**Status (2026-10-04): (1) is implemented** — topic `continuity` (主体衔接) in the app and the CLI, `vb_narrative::continuity`, the DSL's `videoItem.transform`, slot speed and framing in the 剪映 and OTIO writers and the render plan; the renderer itself still only plans. *Measured on a sample set's 13 dance clips* (`probe_continuity`, frames looked at): 12 local全套动作 (one take danced twice, split at 25 s); 10 performances from 9 takes, filmed in several different places, placed on one 37.5 s axis, and frames at one moment of that axis show the same move in every place; a 100 s take and one other stayed pieces of their own (the long one is the dancers learning the moves from a phone, which the movement measure takes for an action — a weakness to fix). The cut moves between places every four beats; 6 of 8 joins carry the movement (cost below 0.6), the subject's size matched to within a few per cent, its place where the frame's edge allows. Automatic length cut it at 2:40; a chosen 60 s played every clip at 1.5×, shortened each by one share and delivered exactly 60 s.

**Status (2026-10-05): the beat rules are implemented** — `segment.rhythm` in the Footage Graph (tempo, clarity, beats, idle, unrhythmic), read by `vb_perception::rhythm` from poses at ten a second; idle left out; each piece at the speed that puts its dancers' beat on the cut's, within 1.2× either way; every clip starting on one of its take's beats; music looped or silent after it ends; the length after all of that. The 1.5× results above are superseded. *Measured on the same 13 clips* (`rhythm_probe`, `probe_continuity`): the dances read 86–102 BPM at clarity 0.71–0.89, one dance no tempo, the learning take 79.8 BPM with 8.7 s idle. With no music the cut follows the dancers' 92 BPM: the main piece at 1.00×, the learning take at 1.15×, a 3.6 s piece at 1.02×; 14.2 s of idle left out; all 17 clips start on a beat of their take; automatic length 1:58, 95% of the action shown (whole beats and starts moved onto beats take the rest). A first version stopped walking a piece at the first idle stretch less than a beat ahead and cut the 99 s take to 6 s, 51 s in all. With a 100 BPM song two pieces went onto its beat (1.09×, 1.11×) and the learning take could not within 1.2×, which its note says; a chosen 60 s was delivered as 99 of the song's beats (59.4 s), every clip shortened by one share. A 17 s song played once with the rest silent, or again from its first beat when looping was chosen, and never shortened the cut. Still open: the learning take reads as a clear 80 BPM dance — idle covers only its pauses — so it plays as an action; beat times are good to about ±0.08 s at ten samples a second; and the 剪映 speed and keyframe format must still be checked on 剪映's screen (a beat-synced `continuity` draft).

**Status (2026-10-06): learning is not dancing** — `segment.rhythm` also records `watching`, and reads `idle` person by person; the narrative layer skips both and steps on their own, leaves out whole a take danced less than a quarter of its action, and on the beat drops a lone move shorter than two bars. *Measured on the same 13 clips* (`rhythm_probe`, `probe_continuity`, frames looked at): the learning take (99 s) and the 6 s opening of another take are left out whole; the first 9 s of one take, where one dancer holds the phone and the other tries the moves, and the 5 s of another where one waits while the other dances, are skipped; the dancers walking off after a dance is dropped; no performance has a moment read as watching. With no music the cut is now the dance and nothing else: 29.3 s at the dancers' 92 BPM, 9 clips from 6 takes, every one starting on a dancer's beat, 5 of 8 joins carrying the movement — where it had been 1:58, 76 s of it the learning take. Two faults found on the way and fixed: a clip that ran past the end of its action into the footage after it did not see the stops there, and a walk along a piece stopped at the first idle stretch less than a beat ahead.

**Recognition models.** Candidates only. Each enters the catalogue after it beats its alternative on footage the user has judged, and only through §XI's licence gate — MIT, Apache-2.0 or BSD-3-Clause, which the catalogue's test enforces.

| Step | Function | Candidate | Note |
|---|---|---|---|
| People | Boxes, 17 keypoints, person mask, faces | RTMO-s · PP-HumanSeg · YuNet — **integrated** | Already run at each keyframe; read densely only for candidate stretches |
| Things: find | The subject when it is not a person | BiRefNet lite (MIT), from the Phase 1 table | Its salient mask gives the box that starts the tracker |
| Things: follow | A mask per subject and object through the clip, one id each | SAM 2.1 tiny / small (Apache 2.0) | Box-prompted; its video propagation is the tracker. Its speed through ONNX Runtime on a phone is to be measured |
| Things: name | What a subject or object is | SigLIP 2 (**integrated**) or Apple Vision, over the masked crop | Labels, as today |
| Motion of things | Trajectories where there are no keypoints — a turning object, a rope | TAPIR / BootsTAPIR / LocoTrack (Apache 2.0) | Only if box and mask motion proves too coarse; never CoTracker3 |
| Contact | Touching, not merely overlapping | Depth Anything V2 Small (**integrated**) + a rule | Nothing to train |
| Alignment | The same action, and the offset between takes | Classical subsequence matching (dynamic time warping) on the normalised motion; V-JEPA 2 features (MIT) to evaluate | — |

SAM 3 / 3.1 would find and follow things by name, but its licence is none of the three (§XI) and the catalogue refuses it as things stand; it becomes a candidate only after counsel and an explicit change to that rule. Detectors under AGPL or GPL — Ultralytics YOLO, YOLO-World — cannot ship.

**Self-trained models.** All model training happens in Phase 2, on the development side (a dedicated NVIDIA box or cloud, never the user's device). Trained models are exported to ONNX or GGUF and shipped through the same catalogue as the open models (§3.3), so importing one replaces a Phase-1 model without touching the pipeline. Candidates: a director head for composition and light, distilled from the Phase-1 models and refined on user edits (`changeRatio`); the junk-shot classifier; a self-supervised continuity model for 主体衔接 over the same 2D tracks (positives are the true continuation of a randomly split action clip, so no hand labels), which replaces the hand-set join cost once it beats it on joins the user has judged; and the screenwriter LLM below. Training data centres on the **composition of shots by historically renowned directors and of viral short-form videos**. ⚠️ Both are copyrighted, and the short-video platforms' terms restrict collection: settle the data rights with counsel before collecting (§X, §XII).

**Associated LLMs:** small, **task-specialized** models — not a general assistant. Produced by **knowledge distillation** from a capable teacher and **INT4/INT8 quantization + structured pruning** for the editing-decision tasks, so they fit the on-device tiers (§3.4). The narrative-layer adapter (§V) lets these proprietary models drop in behind the same interface Phase 1 used for open models.

**Architectural split:** only training runs **offline/cloud**. 主体衔接 and every model it uses run **on the device**, offline, sized to the tiers like every other model (§3.4); nothing in it needs the network.

---

## VIII. End-to-End Implementation Path

This ties the three tracks — **product**, **engineering architecture**, and **model development** — into one sequenced plan. The tracks run in parallel but gate on the milestones below.

### 8.1 Product Development Track

Define the cross-platform UX (single-action import → review N variants → refine → export), the per-platform packaging/distribution story (App Store / Play / notarized macOS / MSIX / AppImage), the local-storage and non-destructive re-edit model, and the downstream-editor integrations. Product exit criterion mirrors §1.2 on every platform.

### 8.2 Engineering Architecture Track

Stand up the Flutter shell + FFI inference layer; port the existing pipeline (ASR, VLM adapter, DSL compiler, PriorStore, renderer) onto the cross-platform runtime; keep the Four Contracts as hard module boundaries; give each of the five targets a build script; isolate every third-party dependency behind an adapter (so a license or upstream change is contained).

### 8.3 Model Development Track

Phase 1 uses open models as-is behind adapters. Phase 2 builds 主体衔接 on 2D recognition, on the device — the enriched Footage Graph fields (subject and object tracks, contacts, local全套动作) and the DSL's keyframed framing — trains the self-supervised continuity model on the development side, and distills/quantizes the specialized LLMs. Every new model must land behind an existing contract without changing the narrative layer.

### 8.4 Sequenced Milestones

| Stage | Track focus | Deliverable | Est. |
|---|---|---|---|
| **0 — Foundation** | Eng | Core pipeline complete & stable (see §IX) | done |
| **1 — Re-platform** | Product + Eng | Flutter shell + llama.cpp/MLC FFI; existing pipeline running cross-platform; macOS + Windows first, then Linux, then iOS/Android | core, ~3–4 mo (individual/small team) |
| **2 — Cross-platform GA** | Product + Eng | All five platforms meet §1.2, fully offline; OTIO/CapCut export; packaging/distribution | — |
| **3 — 主体衔接 for people (Phase 2)** | Model + Eng | People's local全套动作 from their poses; alignment, a layout independent of reading order, joins on the beat; the DSL's keyframed framing and shared speed through the renderer and 剪映; macOS first | — |
| **4 — 主体衔接 for things; Phase 2 models** | Model + Eng | Object masks (SAM 2.1) and contact; every platform the model host runs on; the self-supervised continuity model and the distilled LLMs through the catalogue | — |

Stages 0–2 are the **core, mandatory workflow** (~3–4 months at an individual/small-team pace); acceptance is "a passable AI rough cut plus manual refinement." Stages 3–4 are the differentiating, longer-horizon investment.

---

## IX. Development Milestones — Completed to Date

These are **done** and carried forward; Stage 1 re-platforms them onto Flutter without reworking their logic.

- ✅ **CapCut draft export loop fixed** — the draft directory now points at CapCut's real draft root.
- ✅ **Whisper ASR integrated** (`0.1.2`) — built-in ffmpeg + `ggml-small`; transcripts segmented and exported as CapCut subtitles; CPU backend today, CUDA/accelerated backend planned for long clips.
- ✅ **Footage Graph schema finalized** (`0.1.3`, `footage-graph/0.1`) — shot, timecode, keyframes, transcriptions, description/entity/action-label slots; adaptive keyframe extraction; VLM dense-description adapter (OpenAI-compatible; Qwen-VL / Gemini configurable).
- ✅ **Editing DSL v0.1** (`0.1.4`, `editing-dsl/0.1`) — video/text/audio tracks + transition slots; all draft payloads generated by DSL compilation; end-to-end tested.
- ✅ **Prior Knowledge Base v0** (`0.1.5`) — template registry (`prior-store/*.json`) across rhythm/emotion/content, 9 built-in templates + a plug-and-play custom directory; statistical prior mining (`mineShotPriors`); automatic template matching; mixed-clip montage selection (`selectClipsForMontage`: cross-media blending, deduplication, beat-aligned grid).
- ✅ **Built-in editor** (`0.2.0` of this codebase, 2026-10-09; not the earlier prototype's 0.2.0 below) — every analysed cut can be finished in the app or exported as a project file: `vb-edit`'s operations on the DSL with undo, engine-side sessions kept on disk, a preview that is the renderer's own frame when stopped and the platform's player when playing, and a finished video rendered in Rust and encoded through the media backend (VideoToolbox H.264/HEVC + AAC in MP4) on both the CLI and the linked-libav backend (§6.7).
- ✅ **Rendering & export layer** (`0.2.0`, P3) — single-pass ffmpeg `filter_complex` (crop suggestions / xfade transitions / ASS subtitles / eq / BGM sidechain ducking / title cards; >40 chain-item segments + concat) + in-house plaintext draft generator (transitions/cropping/subtitles/filters/BGM integrated; `cutcli` as fallback) + three-way export UI. On-device end-to-end: **38.7 s** first output, **27.1 s** final clip on a 1,660 s source. Key lesson: **DSL timeline ≠ render timeline** (title-card/transition overlap causes drift); time elements are anchored to manifest slots.

---

## X. Known Technical Constraints & Pitfalls (continuously updated)

- **ffmpeg / lavfi filename escaping** — single quotes in filenames are unsupported; on Windows the backslash must be rewritten as forward-slash-plus-colon, and filenames containing single quotes cannot be reliably escaped at any level. Correct behavior: raise an error and fall back to fixed-length segmentation.
- **CapCut draft visibility** — by default `cutcli` writes drafts to a temp dir the editor does not scan; set `CUT_DRAFTS_DIR` (or `cutcli config set-dir`) to CapCut's real draft root. Drafts written while CapCut is running only appear after a restart.
- **`cutcli ≥ 1.3.13`** — `videos add` accepts only `http(s)` URLs; the app must start a temporary `localhost` service for fragment files.
- **Diff matching** — when homologous adjacent segment boundaries abut, run both passes (precise in/out first, then overlapping fallback); deduplicate against the original fragments (upper bound `changeRatio` = 100%).
- **MMAudio license** — CC-BY-NC, **non-commercial only**, 8-second limit; ship as an **optional add-on** with a commercial-safe default, or replace before any paid release (see §XI).
- **Copyright** — statistical priors or templates that incorporate copyrighted film/TV clips carry commercial risk; plan data sources in advance.
- **Packaging** — package installs may be blocked by security policy in some environments; use cached/mirrored sources. Under Flutter this means the platform toolchains plus a pinned/mirrored pub cache — see build docs.
- **Cross-platform / on-device** — iOS strictly limits background execution (a full overnight compute job is impractical); Android is more permissive with a foreground service + wake-lock but typically needs charging + screen-locked. Sustained inference generates heat and battery drain — budget for it and prefer resident-in-memory quantized models on flagships over disk-streaming tricks.

---

## XI. Open-Source Dependencies & Licenses

Verified against upstream as of 2026-09. **This is engineering guidance, not legal advice — have counsel review the ⚠️/🚫 items before any commercial release.**

**This project's licence (decided 2026-10-14, SPDX `AGPL-3.0-or-later`):** AGPL-3.0-or-later (`LICENSE`), Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn> (the GitHub account `charlesbuling`, which owns the repositories), not yet published — the repositories stay private until the owner decides otherwise. The AGPL binds those who receive the code, not its copyright holder, so a commercial or app-store build stays the owner's to make under a separate commercial licence. To keep it so, dependencies stay permissive, no third party's copyleft code enters — World Monitor's AGPL-3.0 included, and LosslessCut's GPL-2.0-only cannot be combined with AGPL-3.0 at all — and outside contributions are accepted only under `CLA.md`, which keeps the contributor's copyright and grants zhengkuang the right to relicense, including under commercial terms.

**Model policy:** models are chosen for effect first, and the licence is a gate: shipped weights must permit commercial use. Non-commercial weights may be compared on the development side but never ship, and a fine-tune inherits its base model's licence whatever its own card says.

| Component | Role | License | Commercial |
|---|---|---|---|
| llama.cpp | Inference engine (primary) | MIT | ✅ |
| MLC LLM | Inference engine (alt) | Apache 2.0 | ✅ |
| ExecuTorch | Inference engine (mobile alt) | BSD-style | ✅ |
| Flutter | UI shell | BSD-3-Clause | ✅ |
| Tauri / Qt (desktop alt) | UI shell | MIT-Apache / LGPLv3·GPLv3·commercial | ✅ / ⚠️ Qt: dynamic-link LGPLv3 for closed source; some modules GPLv3-only; static linking needs a commercial license |
| Kotlin / Jetpack Compose, Swift | Native shells | Apache 2.0 | ✅ |
| Whisper / whisper.cpp | ASR | MIT | ✅ |
| SAM 2 | Segmentation/identity | Apache 2.0 (code + weights) | ✅ |
| SAM 3 / SAM 3.1 / SAM 3D | Concept segmentation and tracking; 3D (no longer planned — Phase 2 is 2D) | Custom "SAM License" | ⚠️ commercial allowed **with restrictions** (no military/nuclear/espionage use; no reverse engineering; redistribution needs license + attribution; export-control terms) — read in full |
| **CoTracker3** | Point tracking | **CC-BY-NC 4.0** | 🚫 **non-commercial — do not ship.** Replace with **TAPIR / BootsTAPIR / LocoTrack (Apache 2.0)** |
| MediaPipe / LiteRT | Face/hand/pose, mobile inference | Apache 2.0 | ✅ |
| MMAudio | Audio generation | CC-BY-NC | 🚫 non-commercial (see §X) |
| ONNX Runtime / `ort` (Rust binding) | Model host engine | MIT / Apache 2.0 | ✅ |
| PhotoFramer composition model | Composition score + class | MIT (base Qwen2.5-VL-7B, Apache 2.0) | ✅ ⚠️ trained partly on research-use data (AVA) |
| ShotVL-7B | Cinematography | Apache 2.0 (base Qwen2.5-VL-7B) | ✅ ⚠️ trained on ShotQA, which is CC-BY-NC-ND |
| Qwen3-VL 2B / 4B / 8B | General VLM | Apache 2.0 | ✅ |
| Depth Anything V2 Small; DA3 Small / Base | Depth | Apache 2.0 | ✅ |
| SigLIP 2 · BiRefNet · AuraFace v1 (`glintr100.onnx`) · RTMPose / rtmlib · V-JEPA 2 | Embedding · subject mask · face embedding · pose · motion | Apache 2.0 · MIT · Apache 2.0 · Apache 2.0 · MIT | ✅ |
| RTMO-s (`Xenova/RTMO-s`, ONNX export of MMPose's RTMO) | Multi-person 2D pose | Apache 2.0 | ✅ |
| YuNet (`face_detection_yunet_2023mar.onnx`) | Face detection | MIT | ✅ ⚠️ trained on WIDER FACE, which is research-use |
| PP-HumanSeg (`human_segmentation_pphumanseg_2023mar.onnx`) | Person mask | Apache 2.0 | ✅ |
| Depth Anything V2 Base / Large; DA3 Large / Giant | Depth | CC-BY-NC 4.0 | 🚫 |
| FastVLM | Small VLM | Apple research licence (`apple-amlr`) | 🚫 |
| Qwen2.5-VL-3B and its fine-tunes (incl. ShotVL-3B) | VLM | Qwen research licence | 🚫 |
| InsightFace pretrained packs, incl. the `scrfd` / `2d106det` / `1k3d68` / `genderage` files shipped beside AuraFace | Face detection / recognition | Non-commercial research only | 🚫 |
| ffmpeg | Media I/O | LGPL/GPL (build-dependent) | ⚠️ use LGPL build; avoid GPL-only components unless you can comply. The editor's export uses only LGPL parts: the platform's H.264/HEVC encoder (VideoToolbox), libav's own AAC encoder and MP4 muxer, `atempo`/`aresample`/`aformat`, and packets copied for lossless export; never x264/x265, `drawtext` or `eq` |
| LosslessCut | Reference for lossless export (keyframe cuts, parts joined) | GPL-2.0-only | 🚫 as code: ideas only, none of it in the app |
| cosmic-text (with fontdb, harfrust, swash) | Text shaped and rasterised into an exported video (`vb-text`), with the system's fonts | MIT OR Apache-2.0 | ✅ |
| Natural Earth 1:10m (countries, China's point of view; provinces; populated places) | The memory map's outlines and place names, compiled into `vb-geo` | Public domain | ✅ ⚠️ not a reviewed map: publishing it in mainland China needs a 审图号 |

> **Two items block a paid release today: CoTracker3 and MMAudio.** Both are CC-BY-NC. The tracker has clean Apache-2.0 drop-in replacements; MMAudio must be gated as an optional add-on or replaced. Everything else is either permissive or manageable with the noted care.

---

## XII. Risks & Mitigation (development perspective)

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **License contamination** (CoTracker3 / MMAudio / Qt-GPL) reaching a shipped build | Med | High | Adapter isolation per dependency; swap CoTracker3 → TAPIR early; gate MMAudio; LGPL ffmpeg/Qt builds; licence scan before every push (`cargo deny`, `check-licenses.sh`) |
| **Prebuilt ONNX Runtime needs macOS 13.3–14**, the app supports 12 | — | — | **Resolved:** ONNX Runtime is built from source at the app's deployment target (`scripts/build-onnxruntime-macos.sh`); the engine still loads it before a run and reports framing as not checked where it will not load |
| **On-device performance** below real-time on Tier-B/C | Med | High | Quantize to FP32/FP16; distilled specialized models; device-tier fallbacks; benchmark llama.cpp vs MLC on real hardware before commit |
| **Cross-platform divergence** (5 targets diverge, QA/maintenance cost) | Med | Med | One Flutter codebase as default; hard module contracts; each target built and checked on its own machine before a release; reserve "shared engine, separate UI" only if UX truly diverges |
| **Phase-2 risk** (主体衔接 finds too few continuous joins in real footage, or following things in 2D is too heavy for phones) | Med | High | People first, on the dance cut's pose matching that already works; thresholds set against joins the user has judged; models added only when they beat the classical measure; every Phase 1 cut keeps working without it, and a join that does not carry the action is reported, never presented as continuous |
| **Downstream-editor lock-in / breakage** (CapCut draft format changes) | Med | Med | OTIO/FCPXML as the portable interchange; `cutcli`/draft writers behind the adaptation layer; keep forks |
| **Background-execution limits** (esp. iOS) break "overnight" heavy jobs | High (iOS) | Med | Prefer resident quantized models over disk-streaming; move heavy jobs to cloud/offline; foreground-service + charging constraint on Android |
| **Copyright exposure** in priors/templates and Phase-2 training data (renowned directors' films, viral short videos) | Med | High | Plan clean data sources and settle rights with counsel before collecting; respect platform terms; avoid embedding copyrighted clips in shipped priors or models |
| **Model/API drift** (upstream open models change) | Med | Low | Unified adapter interface; pinned versions; the narrative layer is insulated by the Four Contracts |
| **Map review (审图号)** — a world or China map shown in an app published in mainland China must be reviewed; the memory map's Natural Earth outlines are not | High (CN release) | High | China's point-of-view layer already; before a CN release, pass review or draw from the standard map service's reviewed maps; the map is one data file in `vb-geo`, so it can be swapped without touching the app |

---

*This document consolidates the product definition, cross-platform architecture, on-device inference strategy, narrative-layer platform, and two-phase roadmap into a single reference for ongoing development. Companion deliverable: the **Video Butler — Technical Optimization Report** (alternative approaches and areas for further improvement).*
