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

//! What a run is doing, and whether to stop it.
//!
//! A run over a folder of clips takes minutes. Without this the only honest
//! thing a front end could show is a spinner, and a spinner cannot tell a
//! long analysis from a hung one. So the pipeline reports the stage it is in,
//! how far through the whole run it is, and which clip it is on -- and asks,
//! between units of work, whether the user has changed their mind.
//!
//! The percentage is an estimate and is described as one. What it must never
//! do is go backwards or stall at 99: both read as a bug even when the run is
//! healthy.

use vb_contracts::Stage;

/// The stages of a run, in the order they happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStage {
    /// Expanding folders, hashing files, deciding what needs analysing.
    Prepare,
    /// Looking at the footage: shots, keyframes, scores, speech, subjects.
    Analyse,
    /// Listening to the music for its tempo.
    Music,
    /// Choosing the clips and composing the cut.
    Compose,
    /// Writing the draft files.
    Export,
    /// Fetching a model into the library. Not a stage of a run; a download
    /// is its own job and reports under its own name.
    Download,
    /// Writing a video from the built-in editor's cut. Its own job too.
    Render,
    /// Nothing left to do.
    Done,
}

impl RunStage {
    /// A short line for a user, in the present tense.
    pub fn label(self) -> &'static str {
        match self {
            RunStage::Prepare => "Gathering the footage",
            RunStage::Analyse => "Watching the footage",
            RunStage::Music => "Listening for the beat",
            RunStage::Compose => "Choosing the cut",
            RunStage::Export => "Writing the draft",
            RunStage::Download => "Downloading the model",
            RunStage::Render => "Exporting the video",
            RunStage::Done => "Done",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            RunStage::Prepare => "prepare",
            RunStage::Analyse => "analyse",
            RunStage::Music => "music",
            RunStage::Compose => "compose",
            RunStage::Export => "export",
            RunStage::Download => "download",
            RunStage::Render => "render",
            RunStage::Done => "done",
        }
    }

    /// Every stage a run passes through, for a front end that wants to show
    /// the whole list with the current one marked.
    pub const ALL: [RunStage; 5] = [
        RunStage::Prepare,
        RunStage::Analyse,
        RunStage::Music,
        RunStage::Compose,
        RunStage::Export,
    ];
}

/// Where a run has got to.
#[derive(Debug, Clone)]
pub struct Progress {
    pub stage: RunStage,
    /// 0 to 100, over the whole run.
    pub percent: f64,
    /// What is happening now, specifically: usually the file being read.
    /// English, for the CLI; a front end words it from `step` and `file`.
    pub detail: String,
    /// A stable id for what is happening, e.g. `analyse.keyframes` or
    /// `music.listen`, so a front end can say it in the user's language.
    pub step: String,
    /// The file being worked on, when there is one.
    pub file: Option<String>,
    /// Which clip of how many, when the stage works through clips. While
    /// several clips are analysed at once, how many are finished.
    pub source_index: usize,
    pub source_count: usize,
    /// How many clips are being analysed at this moment.
    pub active: usize,
}

/// Watches a run and can stop it.
pub trait Observer: Sync {
    fn progress(&self, _progress: &Progress) {}

    /// Checked between units of work. True stops the run with
    /// [`crate::PipelineError::Cancelled`].
    fn cancelled(&self) -> bool {
        false
    }
}

/// Reports nothing and never cancels.
pub struct Silent;

impl Observer for Silent {}

/// Relative cost of each perception stage within one clip.
///
/// The keyframe stage is the sweep, which decodes the whole file once and
/// writes a still per usable segment; shot detection also decodes the file
/// once. The director runs three models on a frame of every usable segment,
/// which on the reference Mac costs about as much as the sweep. The rest are
/// listed at what they cost when their models are real, so the bar does not
/// have to be re-tuned the day one lands.
fn stage_share(stage: Stage) -> f64 {
    match stage {
        Stage::Probe => 0.02,
        // Cuts are found in the same decode as the sweep, which reports as
        // the keyframe stage.
        Stage::ShotDetect => 0.0,
        Stage::Keyframes => 0.50,
        Stage::CvScore => 0.0, // scored inside the keyframe pass
        Stage::Director => 0.30,
        Stage::Asr => 0.08,
        Stage::Describe => 0.06,
        Stage::Track => 0.04,
        Stage::MinePriors => 0.0,
        // After the merge, reported as part of composing.
        Stage::Judge | Stage::Pose | Stage::Caption => 0.0,
    }
}

/// How much of one clip's analysis is done when `stage` is `fraction` through.
pub(crate) fn within_source(stage: Stage, fraction: f64) -> f64 {
    let mut done = 0.0;
    for earlier in Stage::ALL {
        if earlier == stage {
            break;
        }
        done += stage_share(earlier);
    }
    (done + stage_share(stage) * fraction.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// Several clips analysed at once, reported as one run.
///
/// Each clip's progress is weighted by its size, as when they ran one after
/// another; the run's percentage is the weighted sum, so it moves steadily
/// however the clips interleave, and never backwards.
pub(crate) struct Analysing<'a> {
    pub outer: &'a dyn Observer,
    /// Percent of the whole run at which analysis starts, and its share.
    pub base: f64,
    pub span: f64,
    sizes: Vec<f64>,
    total: f64,
    names: Vec<String>,
    state: std::sync::Mutex<AnalysingState>,
    /// Set when one clip fails, so the others stop rather than finish work
    /// the run will throw away.
    pub failed: std::sync::atomic::AtomicBool,
}

struct AnalysingState {
    done: Vec<f64>,
    finished: usize,
    active: usize,
    percent: f64,
}

impl<'a> Analysing<'a> {
    /// `sizes` and `names` are per input; `already` is how many are done
    /// before any work starts (served from the cache).
    pub fn new(
        outer: &'a dyn Observer,
        base: f64,
        span: f64,
        sizes: Vec<f64>,
        names: Vec<String>,
        already: &[usize],
    ) -> Self {
        let total = sizes.iter().sum::<f64>().max(1.0);
        let mut done = vec![0.0; sizes.len()];
        for &i in already {
            done[i] = 1.0;
        }
        Analysing {
            outer,
            base,
            span,
            sizes,
            total,
            names,
            state: std::sync::Mutex::new(AnalysingState {
                done,
                finished: already.len(),
                active: 0,
                percent: base,
            }),
            failed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn with_state(&self, index: usize, step: &str, change: impl FnOnce(&mut AnalysingState)) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        change(&mut state);
        let through: f64 = state
            .done
            .iter()
            .zip(&self.sizes)
            .map(|(d, s)| d * s)
            .sum::<f64>()
            / self.total;
        state.percent = state.percent.max(self.base + self.span * through.min(1.0));
        let name = self.names.get(index).cloned().unwrap_or_default();
        self.outer.progress(&Progress {
            stage: RunStage::Analyse,
            percent: state.percent,
            detail: format!("{name} · {step}"),
            step: format!("analyse.{step}"),
            file: Some(name),
            source_index: state.finished,
            source_count: self.sizes.len(),
            active: state.active,
        });
    }

    pub fn started(&self, index: usize) {
        self.with_state(index, "probe", |s| s.active += 1);
    }

    pub fn finished(&self, index: usize) {
        self.with_state(index, "done", |s| {
            s.done[index] = 1.0;
            s.finished += 1;
            s.active = s.active.saturating_sub(1);
        });
    }

    pub fn stopped(&self, index: usize) {
        if let Ok(mut s) = self.state.lock() {
            s.active = s.active.saturating_sub(1);
        }
        let _ = index;
    }

    /// Perception's view of one clip of the batch.
    pub fn one(&'a self, index: usize) -> OneOf<'a> {
        OneOf { all: self, index }
    }
}

/// One clip of an [`Analysing`] batch, as perception reports into it.
pub(crate) struct OneOf<'a> {
    all: &'a Analysing<'a>,
    index: usize,
}

impl vb_perception::Observer for OneOf<'_> {
    fn stage(&self, stage: Stage, fraction: f64) {
        let within = within_source(stage, fraction);
        self.all.with_state(self.index, stage.name(), |s| {
            s.done[self.index] = s.done[self.index].max(within);
        });
    }

    fn cancelled(&self) -> bool {
        self.all.outer.cancelled() || self.all.failed.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Vec<f64>>,
    }

    impl Observer for Recorder {
        fn progress(&self, progress: &Progress) {
            self.seen.lock().unwrap().push(progress.percent);
        }
    }

    #[test]
    fn progress_through_one_clip_only_moves_forward() {
        let mut last = -1.0;
        for stage in Stage::ALL {
            for step in [0.0, 0.5, 1.0] {
                let at = within_source(stage, step);
                assert!(
                    at >= last,
                    "{stage:?} at {step} went backwards: {at} < {last}"
                );
                last = at;
            }
        }
        assert!(last <= 1.0);
    }

    #[test]
    fn a_clips_progress_stays_within_the_analysis_slice() {
        let recorder = Recorder::default();
        let batch = Analysing::new(
            &recorder,
            20.0,
            20.0,
            vec![1.0; 4],
            vec!["a".into(), "b".into(), "c".into(), "d".into()],
            &[],
        );
        for stage in Stage::ALL {
            vb_perception::Observer::stage(&batch.one(1), stage, 0.5);
        }
        let seen = recorder.seen.lock().unwrap();
        assert!(!seen.is_empty());
        for percent in seen.iter() {
            // Clip two of four is a quarter of the slice.
            assert!(
                (20.0..=25.0).contains(percent),
                "{percent} is outside this clip's share"
            );
        }
    }

    #[test]
    fn clips_analysed_together_add_up_to_one_steady_bar() {
        let recorder = Recorder::default();
        let batch = Analysing::new(
            &recorder,
            2.0,
            78.0,
            vec![1.0, 3.0, 1.0],
            vec!["a".into(), "b".into(), "c".into()],
            &[2],
        );
        batch.started(0);
        batch.started(1);
        vb_perception::Observer::stage(&batch.one(1), Stage::Director, 0.5);
        vb_perception::Observer::stage(&batch.one(0), Stage::Keyframes, 0.2);
        batch.finished(0);
        batch.finished(1);
        let seen = recorder.seen.lock().unwrap();
        for pair in seen.windows(2) {
            assert!(pair[1] >= pair[0], "went backwards: {seen:?}");
        }
        // The cached clip counted from the start; all three done is the end.
        assert!(seen[0] >= 2.0 + 78.0 * 0.2 - 1e-9, "{seen:?}");
        assert!((seen.last().unwrap() - 80.0).abs() < 1e-9, "{seen:?}");
    }

    #[test]
    fn cancellation_and_a_failed_clip_both_stop_perception() {
        struct Stopped;
        impl Observer for Stopped {
            fn cancelled(&self) -> bool {
                true
            }
        }
        let batch = Analysing::new(&Stopped, 0.0, 1.0, vec![1.0], vec!["a".into()], &[]);
        assert!(vb_perception::Observer::cancelled(&batch.one(0)));

        let running = Recorder::default();
        let batch = Analysing::new(&running, 0.0, 1.0, vec![1.0; 2], vec!["a".into(); 2], &[]);
        assert!(!vb_perception::Observer::cancelled(&batch.one(0)));
        batch
            .failed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(vb_perception::Observer::cancelled(&batch.one(0)));
    }

    #[test]
    fn every_stage_has_a_label_and_an_id() {
        for stage in RunStage::ALL {
            assert!(!stage.label().is_empty());
            assert!(!stage.id().is_empty());
        }
    }
}
