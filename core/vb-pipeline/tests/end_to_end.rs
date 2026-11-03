// Copyright (C) 2026 zhengkuang <217010072@link.cuhk.edu.cn>
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

//! End-to-end tests against real media.
//!
//! These run the whole chain -- ffmpeg, perception, narrative, grounding,
//! export -- rather than any single layer, which is the only way to catch a
//! contract that two layers interpret differently.
//!
//! They need `ffmpeg` and `test-media/sample.mp4`. When either is absent the
//! tests skip rather than fail, so a checkout without the fixture is still
//! green; CI generates the fixture so they always run there.

use std::path::{Path, PathBuf};
use vb_contracts::Micros;
use vb_contracts::editing_dsl::SlotKind;
use vb_infer::{MockAsr, MockTracker, MockVlm};
use vb_media::{FfmpegCli, MediaBackend};
use vb_narrative::ground;
use vb_pipeline::Pipeline;

fn sample() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
    if path.exists() && FfmpegCli::default().is_available() {
        Some(path)
    } else {
        eprintln!("skipping: needs ffmpeg and test-media/sample.mp4");
        None
    }
}

fn project_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vb-e2e-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_real_video_produces_a_valid_draft_with_no_network_and_no_weights() {
    let Some(input) = sample() else { return };
    let dir = project_dir("full");

    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);

    assert!(
        !pipeline.requires_network(),
        "the default configuration must run offline"
    );

    let result = pipeline
        .run(&input, &dir, None, None)
        .expect("the pipeline should complete on a well-formed video");

    // Both contracts hold.
    vb_contracts::validate::validate_footage_graph(&result.graph).expect("graph must validate");
    vb_contracts::validate::validate_editing_dsl(&result.dsl).expect("DSL must validate");

    // The orchestrator must never emit something its own gate rejects.
    assert!(
        result.grounding.is_clean(),
        "grounding rejected the orchestrator's own output: {:?}",
        result.grounding.rejected
    );

    assert!(!result.graph.shots.is_empty(), "no shots were detected");
    assert!(!result.dsl.slots.is_empty(), "the cut is empty");
    assert!(result.draft_path.exists(), "no draft was written");
    assert!(result.graph_path.exists());
    assert!(result.dsl_path.exists());

    // Mock adapters ran, so this must not be presented as a finished cut.
    assert!(
        !result.warnings.is_empty(),
        "a run using mock perception must say so"
    );
}

#[test]
fn every_exported_source_path_is_absolute() {
    // A relative path becomes a file:// URL that resolves only from the
    // directory the pipeline ran in, so every draft would break elsewhere.
    let Some(input) = sample() else { return };
    let dir = project_dir("paths");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

    // Deliberately pass a relative path, which is what a shell user types.
    let relative = pathdiff(&input);
    let result = Pipeline::new(&media, &asr, &vlm, &tracker)
        .run(&relative, &dir, None, None)
        .expect("pipeline should complete");

    for source in &result.graph.sources {
        assert!(
            Path::new(&source.path).is_absolute(),
            "source path {} is relative",
            source.path
        );
    }
}

/// Express the sample path relative to the current directory when possible.
fn pathdiff(path: &Path) -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf())
}

#[test]
fn a_hand_edited_timecode_outside_its_shot_is_rejected() {
    // The gate's whole purpose, exercised against real footage: take a sound
    // cut, move one in-point outside the shot it claims to come from, and
    // confirm the slot is dropped rather than quietly clamped back into range.
    let Some(input) = sample() else { return };
    let dir = project_dir("grounding");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

    let result = Pipeline::new(&media, &asr, &vlm, &tracker)
        .run(&input, &dir, None, None)
        .expect("pipeline should complete");

    let mut tampered = result.dsl.clone();
    let victim = tampered
        .slots
        .iter_mut()
        .find(|s| s.kind == SlotKind::Clip)
        .expect("the cut should contain at least one clip");
    let victim_id = victim.id.clone();

    // Push the range far past the end of the source.
    let source_ref = victim.source_ref.as_mut().unwrap();
    source_ref.in_us = Micros::from_secs(9_000);
    source_ref.out_us = Micros::from_secs(9_006);

    let before = tampered.slots.len();
    let report = ground(&mut tampered, &result.graph);

    assert_eq!(
        report.rejected.len(),
        1,
        "the tampered slot was not rejected"
    );
    assert_eq!(report.rejected[0].slot_id, victim_id);
    assert_eq!(tampered.slots.len(), before - 1);
    assert!(
        !tampered.slots.iter().any(|s| s.id == victim_id),
        "a rejected slot reached the renderer"
    );
    // Rejections are recorded, not silent.
    assert!(
        tampered
            .grounding
            .iter()
            .any(|g| g.slot_id == victim_id && g.rejection_reason.is_some())
    );
}

#[test]
fn a_second_run_reuses_the_cached_analysis() {
    // Incremental reuse is the load-bearing performance lever, so it is worth
    // asserting rather than assuming.
    let Some(input) = sample() else { return };
    let dir = project_dir("cache");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);

    let first = pipeline.run(&input, &dir, None, None).expect("first run");
    assert!(
        first.reused_sources == 0,
        "the first run cannot be a cache hit"
    );

    let second = pipeline.run(&input, &dir, None, None).expect("second run");
    assert!(
        second.reused_sources == second.total_sources,
        "an unchanged source and settings should hit the cache"
    );
    assert_eq!(
        first.graph.shots.len(),
        second.graph.shots.len(),
        "the reused analysis differs from the one it replaced"
    );
}

#[test]
fn changing_a_setting_misses_the_cache() {
    let Some(input) = sample() else { return };
    let dir = project_dir("cache-miss");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

    let mut pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);
    pipeline.run(&input, &dir, None, None).expect("first run");

    // A different scene threshold is a different analysis, so reusing the old
    // one would silently ignore the user's change.
    pipeline.config.scene_threshold = 0.9;
    let second = pipeline.run(&input, &dir, None, None).expect("second run");
    assert!(
        second.reused_sources == 0,
        "a changed setting must not reuse the previous analysis"
    );
}

#[test]
fn the_draft_timeline_accounts_for_transition_overlap() {
    // The recorded bug, checked end to end: with transitions present, the
    // resolved duration must be shorter than the sum of the slot durations.
    let Some(input) = sample() else { return };
    let dir = project_dir("overlap");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

    let result = Pipeline::new(&media, &asr, &vlm, &tracker)
        .run(&input, &dir, Some("rhythm.calm-observational"), None)
        .expect("pipeline should complete");

    let overlapping: u64 = result
        .dsl
        .transitions
        .iter()
        .filter(|t| t.kind != vb_contracts::editing_dsl::TransitionType::Cut)
        .map(|t| t.duration_us.0)
        .sum();

    if overlapping == 0 {
        return; // this template produced straight cuts; nothing to check
    }

    let naive_sum: u64 = result.dsl.slots.iter().map(|s| s.duration_us.0).sum();
    let resolved = result.dsl.resolve_timeline().total_duration_us.0;
    assert!(
        resolved < naive_sum,
        "resolved duration {resolved} should be shorter than the naive sum {naive_sum}"
    );
}

/// The linked-libav backend, driving the same chain the CLI backend does.
#[cfg(feature = "libav")]
mod libav {
    use super::*;
    use vb_contracts::footage_graph::Stage;
    use vb_media::LibAv;

    #[test]
    fn the_whole_chain_runs_in_process_and_agrees_with_the_cli() {
        let Some(input) = sample() else { return };
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

        let cli = FfmpegCli::default();
        let by_cli = Pipeline::new(&cli, &asr, &vlm, &tracker)
            .run(&input, &project_dir("libav-cli"), None, None)
            .expect("the CLI backend should complete");
        let by_lib = Pipeline::new(&LibAv, &asr, &vlm, &tracker)
            .run(&input, &project_dir("libav-lib"), None, None)
            .expect("the libav backend should complete");

        vb_contracts::validate::validate_footage_graph(&by_lib.graph).expect("graph must validate");
        vb_contracts::validate::validate_editing_dsl(&by_lib.dsl).expect("DSL must validate");
        assert!(by_lib.grounding.is_clean());
        assert!(by_lib.draft_path.exists());

        // The same shots, at the same times: the backends are interchangeable.
        let bounds = |r: &vb_pipeline::RunResult| {
            r.graph
                .shots
                .iter()
                .map(|s| (s.start_us, s.end_us))
                .collect::<Vec<_>>()
        };
        assert_eq!(bounds(&by_lib), bounds(&by_cli));

        // And provenance says which one ran, down to the ffmpeg release.
        for stage in [Stage::Probe, Stage::ShotDetect, Stage::Keyframes] {
            assert_eq!(by_lib.graph.stage_adapter(stage), Some("LibAv"));
            assert_eq!(by_cli.graph.stage_adapter(stage), Some("FfmpegCli"));
        }
        let probe = by_lib
            .graph
            .provenance
            .iter()
            .find(|p| p.stage == Stage::Probe)
            .unwrap();
        assert_eq!(probe.tool.as_deref(), Some("ffmpeg"));
        assert_eq!(probe.tool_version, LibAv.version());
    }

    #[test]
    fn switching_the_media_backend_misses_the_cache() {
        // A graph built by one backend and served under the other would carry
        // a provenance record naming a backend that never ran.
        let Some(input) = sample() else { return };
        let dir = project_dir("libav-cache");
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);

        let cli = FfmpegCli::default();
        Pipeline::new(&cli, &asr, &vlm, &tracker)
            .run(&input, &dir, None, None)
            .expect("first run");

        let second = Pipeline::new(&LibAv, &asr, &vlm, &tracker)
            .run(&input, &dir, None, None)
            .expect("second run");
        assert!(
            second.reused_sources == 0,
            "a different backend must not reuse the previous backend's analysis"
        );
        assert_eq!(second.graph.stage_adapter(Stage::Probe), Some("LibAv"));
    }
}

/// A project is several clips, not one.
#[test]
fn a_project_of_several_clips_becomes_one_cut_across_them() {
    let Some(input) = sample() else { return };
    let dir = project_dir("multi");
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let media = FfmpegCli::default();

    // Three copies of the fixture stand in for three clips: different files,
    // so different sources, which is what the merge and the selection care
    // about. Their content is identical, so any difference in the result is
    // the pipeline's doing and not the footage's.
    let clips_dir = dir.join("clips");
    std::fs::create_dir_all(&clips_dir).unwrap();
    let clips: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            let dest = clips_dir.join(format!("{name}.mp4"));
            std::fs::copy(&input, &dest).unwrap();
            dest
        })
        .collect();

    let result = Pipeline::new(&media, &asr, &vlm, &tracker)
        .run_many(&clips, &dir, None, None)
        .expect("a project of three clips should complete");

    vb_contracts::validate::validate_footage_graph(&result.graph).expect("graph must validate");
    vb_contracts::validate::validate_editing_dsl(&result.dsl).expect("DSL must validate");
    assert!(
        result.grounding.is_clean(),
        "{:?}",
        result.grounding.rejected
    );

    assert_eq!(result.graph.sources.len(), 3, "one source per clip");
    assert_eq!(result.total_sources, 3);

    // Ids are local to each clip's own analysis and renumbered on merge; a
    // collision here would have two clips sharing a shot.
    let shot_ids: std::collections::HashSet<&str> =
        result.graph.shots.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(shot_ids.len(), result.graph.shots.len(), "shot ids collide");
    let keyframes: Vec<&str> = result
        .graph
        .shots
        .iter()
        .flat_map(|s| s.keyframes.iter().map(|k| k.id.as_str()))
        .collect();
    assert_eq!(
        keyframes
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        keyframes.len(),
        "keyframe ids collide"
    );

    // Every clip the user put in the project appears in the cut.
    let cut_from: std::collections::HashSet<&str> = result
        .dsl
        .slots
        .iter()
        .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
        .collect();
    assert_eq!(cut_from.len(), 3, "a clip was left out: {cut_from:?}");

    // Every slot resolves to a shot of the source it names, which is what the
    // exporter needs to point a draft at the right file.
    for slot in &result.dsl.slots {
        let Some(source_ref) = &slot.source_ref else {
            continue;
        };
        let Some(shot_id) = &source_ref.shot_id else {
            continue;
        };
        let shot = result
            .graph
            .shots
            .iter()
            .find(|s| &s.id == shot_id)
            .expect("slot names a shot that is not in the graph");
        assert_eq!(
            shot.source_id, source_ref.source_id,
            "slot {} claims the wrong source",
            slot.id
        );
    }
}

#[test]
fn adding_a_clip_re_analyses_only_that_clip() {
    let Some(input) = sample() else { return };
    let dir = project_dir("multi-cache");
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let media = FfmpegCli::default();
    let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);

    let clips_dir = dir.join("clips");
    std::fs::create_dir_all(&clips_dir).unwrap();
    let first = clips_dir.join("a.mp4");
    std::fs::copy(&input, &first).unwrap();

    let one = pipeline
        .run_many(std::slice::from_ref(&first), &dir, None, None)
        .unwrap();
    assert_eq!(one.reused_sources, 0, "the first run cannot be a cache hit");

    // A second clip joins the project, with content of its own: the cache is
    // keyed on what a file contains, so a byte-identical copy is the same
    // analysis and would hit the cache legitimately.
    let second = clips_dir.join("b.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("smptebars=size=320x180:rate=30:duration=3")
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "30"])
        .arg(&second)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !made {
        eprintln!("skipping: could not synthesise a second clip");
        return;
    }
    let two = pipeline
        .run_many(&[first, second], &dir, None, None)
        .unwrap();
    assert_eq!(two.total_sources, 2);
    assert_eq!(
        two.reused_sources, 1,
        "the clip that did not change should have come from the cache"
    );
}

// ---------------------------------------------------------------------------
// A run as the app asks for it: a style, music, progress, and a way to stop.
// ---------------------------------------------------------------------------

mod styled {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use vb_contracts::editing_dsl::AudioKind;
    use vb_contracts::footage_graph::BeatOrigin;
    use vb_narrative::{Mood, Style, Topic};
    use vb_pipeline::{Observer, PipelineError, Progress, RunOptions, RunStage, Workspace};

    fn music() -> Option<PathBuf> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/beat-120.wav");
        path.exists().then_some(path)
    }

    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Vec<(RunStage, f64)>>,
    }

    impl Observer for Recorder {
        fn progress(&self, progress: &Progress) {
            self.seen
                .lock()
                .unwrap()
                .push((progress.stage, progress.percent));
        }
    }

    #[test]
    fn a_styled_run_on_the_beat_of_chosen_music_puts_every_cut_on_a_beat() {
        let (Some(input), Some(music)) = (sample(), music()) else {
            return;
        };
        let dir = project_dir("styled-beat");
        let media = FfmpegCli::default();
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
        let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);

        let options = RunOptions {
            style: Style {
                mood: Some(Mood::Happy),
                topic: Some(Topic::Travel),
                beat_sync: Some(true),
            },
            music: Some(music.clone()),
            title: Some("Kyoto".into()),
            ..RunOptions::default()
        };
        let recorder = Recorder::default();
        let result = pipeline
            .run_project(&[input], &Workspace::beside(&dir), &options, &recorder)
            .expect("a styled run completes");

        assert_eq!(result.template_id, "style.happy.travel.beat");
        assert_eq!(result.dsl.meta.title, "Kyoto");

        // The tempo came from the music, and was measured, not assumed.
        let grid = result
            .graph
            .priors
            .as_ref()
            .and_then(|p| p.beat_grid.as_ref())
            .expect("a beat grid");
        assert_eq!(grid.origin, Some(BeatOrigin::Music));
        assert!((grid.bpm - 120.0).abs() < 1.5, "measured {}", grid.bpm);
        let beat = grid.beat_us().unwrap().0;

        // Every slot starts on a beat of it.
        let timeline = result.dsl.resolve_timeline();
        for slot in &timeline.slots {
            assert_eq!(
                slot.start_us.0 % beat,
                0,
                "{} is off the beat",
                slot.slot_id
            );
        }

        // The music is laid under the cut, starting on its first beat, and
        // never outruns its own file.
        let bgm = result
            .dsl
            .tracks
            .audio
            .iter()
            .find(|a| a.kind == AudioKind::Bgm)
            .expect("the chosen music is in the cut");
        assert_eq!(
            bgm.asset_path.as_deref(),
            Some(music.to_string_lossy().as_ref())
        );
        assert_eq!(bgm.asset_in_us, grid.first_beat_us);
        assert!(bgm.duration_us.unwrap() <= Micros::from_secs(12));

        vb_contracts::validate::validate_footage_graph(&result.graph).unwrap();
        vb_contracts::validate::validate_editing_dsl(&result.dsl).unwrap();

        // Progress went through every stage in order, forwards only, to 100.
        let seen = recorder.seen.lock().unwrap();
        let stages: Vec<RunStage> = seen.iter().map(|(s, _)| *s).collect();
        for stage in [
            RunStage::Prepare,
            RunStage::Analyse,
            RunStage::Music,
            RunStage::Compose,
            RunStage::Export,
            RunStage::Done,
        ] {
            assert!(stages.contains(&stage), "never reported {stage:?}");
        }
        let percents: Vec<f64> = seen.iter().map(|(_, p)| *p).collect();
        assert!(
            percents.windows(2).all(|w| w[1] >= w[0] - 1e-9),
            "progress went backwards: {percents:?}"
        );
        assert_eq!(*percents.last().unwrap(), 100.0);
    }

    #[test]
    fn a_cancelled_run_stops_and_writes_no_draft() {
        let Some(input) = sample() else { return };
        let dir = project_dir("cancelled");
        let media = FfmpegCli::default();
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
        let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);

        /// Asks to stop as soon as analysis has begun.
        struct StopEarly(AtomicBool);
        impl Observer for StopEarly {
            fn progress(&self, progress: &Progress) {
                if progress.stage == RunStage::Analyse {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            fn cancelled(&self) -> bool {
                self.0.load(Ordering::Relaxed)
            }
        }

        let err = pipeline
            .run_project(
                &[input],
                &Workspace::beside(&dir),
                &RunOptions::default(),
                &StopEarly(AtomicBool::new(false)),
            )
            .unwrap_err();
        assert!(
            matches!(
                err,
                PipelineError::Cancelled
                    | PipelineError::Perception(vb_perception::PerceptionError::Cancelled)
            ),
            "got {err}"
        );
        assert!(
            !dir.join("sample.otio").exists(),
            "a cancelled run must not leave a draft that looks finished"
        );
    }

    #[test]
    fn without_music_a_beat_cut_says_its_grid_is_a_chosen_tempo() {
        let Some(input) = sample() else { return };
        let dir = project_dir("fixed-grid");
        let media = FfmpegCli::default();
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
        let pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);
        let options = RunOptions {
            style: Style {
                mood: Some(Mood::Humor),
                topic: None,
                beat_sync: Some(true),
            },
            ..RunOptions::default()
        };
        let result = pipeline
            .run_project(
                &[input],
                &Workspace::beside(&dir),
                &options,
                &vb_pipeline::Silent,
            )
            .expect("the run completes");

        // The fixture has no sound, so there was nothing to listen to.
        let provenance = result.dsl.provenance.as_ref().unwrap();
        assert!(
            provenance
                .priors_applied
                .iter()
                .any(|p| p.starts_with("beatGrid.fixed")),
            "{:?}",
            provenance.priors_applied
        );
        assert!(result.dsl.tracks.audio.is_empty());
    }
}

/// A composition judge that answers like PhotoFramer and counts its calls.
struct CountingJudge {
    calls: std::sync::atomic::AtomicUsize,
}

impl vb_infer::Adapter for CountingJudge {
    fn name(&self) -> &'static str {
        "CountingJudge"
    }
}

impl vb_infer::VlmAdapter for CountingJudge {
    fn describe(&self, request: &vb_infer::VlmRequest) -> Result<String, vb_infer::InferError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // A rating that differs by still, so the ratings are not all equal.
        let n = request.image_path.to_string_lossy().len() % 4;
        Ok(format!(
            "<think>Looked at it.</think> <answer>{{\"rating\": {}.5}}</answer>",
            1 + n
        ))
    }
}

#[test]
fn the_composition_judge_rates_the_shortlist_once_and_is_remembered() {
    let Some(input) = sample() else { return };
    let dir = project_dir("judge");
    let media = FfmpegCli::default();
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let judge = CountingJudge {
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let mut pipeline = Pipeline::new(&media, &asr, &vlm, &tracker);
    pipeline.judge = Some(&judge);

    let first = pipeline.run(&input, &dir, None, None).expect("first run");
    let asked = judge.calls.load(std::sync::atomic::Ordering::SeqCst);
    assert!(asked > 0, "the judge was never asked");
    vb_contracts::validate::validate_footage_graph(&first.graph).expect("graph must validate");
    vb_contracts::validate::validate_editing_dsl(&first.dsl).expect("DSL must validate");
    let rated = first
        .graph
        .shots
        .iter()
        .flat_map(|s| &s.segments)
        .filter(|s| s.composition.is_some())
        .count();
    assert_eq!(rated, asked, "every rating given is in the graph");
    // Provenance names the judge, as it names every adapter that ran.
    assert!(
        first
            .graph
            .provenance
            .iter()
            .any(|p| p.stage == vb_contracts::Stage::Judge
                && p.adapter.as_deref() == Some("CountingJudge")),
        "the judge left no provenance"
    );

    // The same footage again: every rating comes from the cache.
    let second = pipeline.run(&input, &dir, None, None).expect("second run");
    assert_eq!(
        judge.calls.load(std::sync::atomic::Ordering::SeqCst),
        asked,
        "a second run asked the judge again"
    );
    assert_eq!(first.dsl.slots.len(), second.dsl.slots.len());
}

#[test]
fn videos_and_photos_become_one_cut_and_one_draft() {
    let Some(input) = sample() else { return };
    let media_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media");
    let photos: Vec<PathBuf> = ["photo-rotated.jpg", "photo.png", "photo-grid.heic"]
        .iter()
        .map(|name| media_dir.join(name))
        .filter(|p| p.exists())
        .collect();
    if photos.len() < 2 {
        eprintln!("skipping: needs the photo fixtures from make-test-media.sh");
        return;
    }
    let dir = project_dir("mixed");
    let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
    let media = FfmpegCli::default();
    let mut inputs = vec![input.clone()];
    inputs.extend(photos.iter().cloned());

    let result = Pipeline::new(&media, &asr, &vlm, &tracker)
        .run_many(&inputs, &dir, None, None)
        .expect("a project of a video and photos should complete");

    vb_contracts::validate::validate_footage_graph(&result.graph).expect("graph must validate");
    vb_contracts::validate::validate_editing_dsl(&result.dsl).expect("DSL must validate");
    assert!(
        result.grounding.is_clean(),
        "{:?}",
        result.grounding.rejected
    );

    // Each photo is a source of its own kind, with one shot it may be held
    // for, and nothing claimed for the stages that did not run on it.
    let stills: Vec<&vb_contracts::Source> = result
        .graph
        .sources
        .iter()
        .filter(|s| s.is_still())
        .collect();
    assert_eq!(stills.len(), photos.len());
    for source in &stills {
        assert_eq!(source.duration_us, vb_perception::STILL_HOLD);
        assert_eq!(source.has_audio, Some(false));
        let shots: Vec<&vb_contracts::Shot> = result.graph.shots_of(&source.id).collect();
        assert_eq!(shots.len(), 1);
        let segment = &shots[0].segments[0];
        assert_eq!(
            segment.usable(),
            Some((Micros::ZERO, vb_perception::STILL_HOLD))
        );
        assert!(segment.quality.is_some());
        assert!(!segment.signature.is_empty());
        assert!(segment.scene_id.is_some(), "a photo joins a scene");
    }
    let rotated = stills
        .iter()
        .find(|s| s.path.ends_with("photo-rotated.jpg"))
        .unwrap();
    assert_eq!(rotated.display_size(), Some((600, 800)));
    assert_eq!(
        rotated.created_at.as_deref(),
        Some("2023-09-29T18:31:43+08:00")
    );
    // Its keyframe still is upright, for the models that read it.
    let shot = result.graph.shots_of(&rotated.id).next().unwrap();
    let still = shot.keyframes[0]
        .path
        .as_ref()
        .expect("a still was written");
    let still = media
        .probe(&dir.join("_vbwork").join(still))
        .expect("the still reads back");
    assert_eq!((still.width, still.height), (600, 800));

    // The cut uses both kinds, and a photo from its start, briefly.
    let is_still = |slot: &vb_contracts::Slot| {
        slot.source_ref
            .as_ref()
            .and_then(|r| result.graph.source(&r.source_id))
            .is_some_and(|s| s.is_still())
    };
    let clips: Vec<&vb_contracts::Slot> = result
        .dsl
        .slots
        .iter()
        .filter(|s| s.kind == SlotKind::Clip)
        .collect();
    assert!(clips.iter().any(|s| is_still(s)), "no photo in the cut");
    assert!(clips.iter().any(|s| !is_still(s)), "no video in the cut");
    for slot in clips.iter().filter(|s| is_still(s)) {
        assert_eq!(slot.source_ref.as_ref().unwrap().in_us, Micros::ZERO);
        assert!(slot.duration_us <= Micros::from_secs(3));
    }
    // The frame and the rate are the video's.
    assert_eq!(result.dsl.meta.aspect_ratio.as_deref(), Some("16:9"));
    assert_eq!(result.dsl.meta.fps_num, Some(30));

    // The portable draft marks the photos, and the editor's takes them as
    // photo materials.
    let otio = std::fs::read_to_string(&result.draft_path).unwrap();
    assert!(otio.contains("\"still\": true") || otio.contains("\"still\":true"));
    let draft = dir.join("jianying-draft");
    vb_export::JianyingWriter::default()
        .write_draft(&result.dsl, &result.graph, &draft)
        .expect("the 剪映 draft is written");
    let content: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(draft.join("draft_content.json")).unwrap())
            .unwrap();
    let kinds: Vec<&str> = content["materials"]["videos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["type"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"photo") && kinds.contains(&"video"),
        "{kinds:?}"
    );
    assert_eq!(content["canvas_config"]["width"], 1280);
}

#[test]
fn the_butler_files_photos_beside_videos_from_a_walk_or_from_a_list() {
    let Some(video) = sample() else { return };
    let media_dir = video.parent().unwrap().to_path_buf();
    let fixtures = [
        video.clone(),
        media_dir.join("photo-rotated.jpg"),
        media_dir.join("photo.png"),
    ];
    if fixtures.iter().any(|f| !f.exists()) {
        eprintln!("skipping: needs the photo fixtures from make-test-media.sh");
        return;
    }
    let root = project_dir("butler");
    let footage = root.join("footage");
    std::fs::create_dir_all(&footage).unwrap();
    for fixture in &fixtures {
        std::fs::copy(fixture, footage.join(fixture.file_name().unwrap())).unwrap();
    }
    // An icon is not a photo someone took.
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=64x64",
        ])
        .args(["-frames:v", "1"])
        .arg(footage.join("icon.png"))
        .status()
        .expect("ffmpeg runs");
    assert!(made.success());

    let library = vb_pipeline::Library::at(root.join("library"));
    let mut settings = library.settings();
    settings.hosting.roots = vec![footage.to_string_lossy().to_string()];
    library.save_settings(&settings).unwrap();
    let (media, vlm, tracker) = (FfmpegCli::default(), MockVlm, MockTracker);
    let eyes = vb_pipeline::hosting::Eyes {
        media: &media,
        labels: &vlm,
        faces: &tracker,
    };
    let quiet = |_: f64, _: &str, _: usize, _: usize| {};

    // A walk through the folder finds the video and the pictures.
    let pass = vb_pipeline::hosting::pass(&library, &eyes, None, &vb_pipeline::Silent, &quiet)
        .expect("a pass over the folder");
    assert_eq!(pass.found, 4);
    let catalogue = vb_pipeline::hosting::load(&library);
    let entry = |name: &str| {
        catalogue
            .entries
            .iter()
            .find(|e| e.name() == name)
            .unwrap_or_else(|| panic!("{name} was not filed"))
    };
    let photo = entry("photo-rotated.jpg");
    assert!(photo.is_photo());
    assert!(photo.error.is_none(), "{:?}", photo.error);
    assert_eq!(photo.duration_us, 0);
    assert_eq!((photo.width, photo.height), (600, 800), "upright");
    assert_eq!(
        photo.captured_at.as_deref(),
        Some("2023-09-29T18:31:43+08:00"),
        "the camera's clock, from its EXIF"
    );
    assert!(!entry("sample.mp4").is_photo());
    assert_eq!(
        entry("icon.png").error.as_deref(),
        Some("too small to be a photo")
    );
    let page = vb_pipeline::hosting::overview(&library, vb_pipeline::Lang::En);
    assert_eq!(
        (page["videos"].as_u64(), page["photos"].as_u64()),
        (Some(1), Some(2))
    );

    // The list a file server already holds, in place of the walk: nothing
    // on it is looked at again, and what is not on it leaves.
    let listed = [footage.join("sample.mp4"), footage.join("photo.png")];
    let pass =
        vb_pipeline::hosting::pass(&library, &eyes, Some(&listed), &vb_pipeline::Silent, &quiet)
            .expect("a pass over the list");
    assert_eq!(
        (pass.found, pass.kept, pass.looked, pass.gone),
        (2, 2, 0, 2)
    );
    let mut names: Vec<String> = vb_pipeline::hosting::load(&library)
        .entries
        .iter()
        .map(|e| e.name())
        .collect();
    names.sort();
    assert_eq!(names, ["photo.png", "sample.mp4"]);
    let _ = std::fs::remove_dir_all(&root);
}
