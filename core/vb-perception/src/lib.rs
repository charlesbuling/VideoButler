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

//! The perception pipeline.
//!
//! Reads media, writes a Footage Graph, and knows nothing about what happens
//! next. Every model it calls sits behind an adapter, so the same pipeline
//! runs with real models, with mocks, or with any mixture of the two.
//!
//! # The compression funnel
//!
//! Hours of frames become a few hundred shots, each represented by a handful
//! of scored keyframes, of which only the best is ever described by a vision
//! model. Frame-level differentiation is classical CV; models only ever see
//! the compressed storyboard. Without this, a multi-hour source could not be
//! reasoned about at all.

pub mod beats;
pub mod categories;
pub mod director;
pub mod pose;
pub mod rhythm;
pub mod scenes;
pub mod shots;
pub mod timeline;

use std::path::Path;
use vb_contracts::footage_graph::{CameraMotion, Segment, TimelineSample};
use vb_contracts::footage_graph::{
    Description, Entity, Keyframe, KeyframeScores, ProvenanceEntry, Stage, TranscriptSegment,
};
use vb_contracts::{Blake3Hash, FootageGraph, MediaKind, Micros, Shot, Source};
use vb_infer::{
    AsrAdapter, DepthAdapter, FaceDetector, MockDepth, MockFaces, MockPeople, MockPose,
    PersonSegmenter, PoseEstimator, RgbImage, TrackerAdapter, VlmAdapter,
};
use vb_media::{MediaBackend, MediaError};

pub use beats::{BeatDetection, detect as detect_beats};
pub use scenes::attach_scenes;
pub use shots::{build_shots, keyframe_times};

/// Which analysis produced a graph, for cache keys. Change it whenever what
/// perception writes for the same footage changes.
pub const ANALYSIS_VERSION: &str = "sweep-3";

/// How much of the keyframe stage's progress the sweep itself accounts for;
/// the rest is extracting the stills.
const SWEEP_SHARE: f64 = 0.8;

/// How long a photograph may be held on screen: the one shot a photo is
/// given runs this long. A photo has no running time of its own, and a cut
/// holds one for a few seconds; this is the ceiling the grounding gate
/// checks a cut against, not a pace.
pub const STILL_HOLD: Micros = Micros(10_000_000);

#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    /// ffmpeg scene score above which a frame is treated as a cut.
    pub scene_threshold: f64,
    /// Shots shorter than this are merged into their neighbour.
    pub min_shot: Micros,
    pub max_keyframes_per_shot: usize,
    /// Roughly one keyframe per this many seconds of shot.
    pub keyframe_every_seconds: u64,
    /// Size frames are decoded at for scoring. Small on purpose: the metrics
    /// do not improve with resolution, and this is what keeps scoring cheap
    /// enough to run over hours of 4K.
    pub score_width: u32,
    pub score_height: u32,
    /// Write keyframe stills to disk. Needed for description; skippable when
    /// only scores are wanted.
    pub extract_stills: bool,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        PerceptionConfig {
            scene_threshold: 0.35,
            min_shot: Micros::from_millis(800),
            max_keyframes_per_shot: 5,
            keyframe_every_seconds: 4,
            score_width: 320,
            score_height: 180,
            extract_stills: true,
        }
    }
}

/// The frame size the director reads: the short side at [`LOOK_SHORT_SIDE`],
/// the shape kept, both sides even.
fn look_size((w, h): (u32, u32)) -> (u32, u32) {
    let scale = LOOK_SHORT_SIDE as f64 / w.min(h).max(1) as f64;
    let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
    (even(w as f64 * scale), even(h as f64 * scale))
}

/// Watches one analysis as it runs.
///
/// A run over a folder of clips takes minutes, and a progress bar that sits
/// still for all of them is indistinguishable from a hang. Perception says
/// which stage it is in and how far through it, and asks between units of work
/// whether to stop, so a user who picked the wrong folder is not made to wait
/// it out.
pub trait Observer: Sync {
    /// `fraction` is how far through `stage` this analysis is, 0 to 1.
    fn stage(&self, _stage: Stage, _fraction: f64) {}

    /// Checked between units of work. True stops the analysis with
    /// [`PerceptionError::Cancelled`].
    fn cancelled(&self) -> bool {
        false
    }
}

/// Reports nothing and never cancels.
pub struct Silent;

impl Observer for Silent {}

/// The models the director looks through.
///
/// Depth decides whether there is an occlusion reading: with a mock depth
/// model every segment is left without one. Faces and people only refine
/// what depth finds, so a mock there makes the reading stricter, never
/// blinder. Pose, when it is real, counts the people at each keyframe and
/// records how they stand -- which is how a dance cut knows which moments
/// are dancing, and by how many.
#[derive(Clone, Copy)]
pub struct DirectorModels<'a> {
    pub depth: &'a dyn DepthAdapter,
    pub faces: &'a dyn FaceDetector,
    pub people: &'a dyn PersonSegmenter,
    pub pose: &'a dyn PoseEstimator,
}

impl DirectorModels<'static> {
    /// No models: the director reads nothing, and its provenance says so.
    pub fn none() -> Self {
        DirectorModels {
            depth: &MockDepth,
            faces: &MockFaces,
            people: &MockPeople,
            pose: &MockPose,
        }
    }
}

impl DirectorModels<'_> {
    /// Will the director read anything?
    pub fn reads(&self) -> bool {
        !self.depth.is_mock() || !self.pose.is_mock()
    }

    /// Every model that looked, for provenance and the cache key. A pose
    /// model is named only when there is one, so a graph analysed without
    /// one keeps the key it always had.
    pub fn name(&self) -> String {
        let mut name = format!(
            "{}+{}+{}",
            self.depth.name(),
            self.faces.name(),
            self.people.name()
        );
        if !self.pose.is_mock() {
            name.push('+');
            name.push_str(self.pose.name());
        }
        name
    }
}

/// What the director read at one keyframe.
struct Look {
    occlusion: Option<f64>,
    people: Option<vb_contracts::footage_graph::PoseSample>,
}

/// The short side of the frame the director's models see.
const LOOK_SHORT_SIDE: u32 = 518;

/// The adapters and settings one analysis run uses.
pub struct Perception<'a> {
    pub media: &'a dyn MediaBackend,
    pub asr: &'a dyn AsrAdapter,
    pub vlm: &'a dyn VlmAdapter,
    pub tracker: &'a dyn TrackerAdapter,
    pub director: DirectorModels<'a>,
    pub config: PerceptionConfig,
}

impl<'a> Perception<'a> {
    pub fn new(
        media: &'a dyn MediaBackend,
        asr: &'a dyn AsrAdapter,
        vlm: &'a dyn VlmAdapter,
        tracker: &'a dyn TrackerAdapter,
    ) -> Self {
        Perception {
            media,
            asr,
            vlm,
            tracker,
            director: DirectorModels::none(),
            config: PerceptionConfig::default(),
        }
    }

    /// Did any stage of a run fall back to a mock?
    ///
    /// The caller needs this to tell the user that a cut was assembled without
    /// the system having listened to or looked at their footage.
    pub fn mocked_stages(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.asr.is_mock() {
            out.push(self.asr.name());
        }
        if self.vlm.is_mock() {
            out.push(self.vlm.name());
        }
        if self.tracker.is_mock() {
            out.push(self.tracker.name());
        }
        out
    }

    /// Analyse one media file into a Footage Graph.
    pub fn analyze(
        &self,
        path: &Path,
        project_id: &str,
        work_dir: &Path,
    ) -> Result<FootageGraph, PerceptionError> {
        self.analyze_observed(path, project_id, work_dir, &Silent)
    }

    /// [`Self::analyze`], reporting progress to `observer` and stopping when
    /// it asks.
    pub fn analyze_observed(
        &self,
        path: &Path,
        project_id: &str,
        work_dir: &Path,
        observer: &dyn Observer,
    ) -> Result<FootageGraph, PerceptionError> {
        self.analyze_hashed(path, None, project_id, work_dir, observer)
    }

    /// [`Self::analyze_observed`] for a file whose content hash the caller
    /// already has: reading a 9 GB clip a second time only to hash it again
    /// costs more than some whole stages.
    pub fn analyze_hashed(
        &self,
        path: &Path,
        known_hash: Option<Blake3Hash>,
        project_id: &str,
        work_dir: &Path,
        observer: &dyn Observer,
    ) -> Result<FootageGraph, PerceptionError> {
        let checkpoint = |stage: Stage, fraction: f64| -> Result<(), PerceptionError> {
            if observer.cancelled() {
                return Err(PerceptionError::Cancelled);
            }
            observer.stage(stage, fraction);
            Ok(())
        };
        let mut graph = FootageGraph::new(project_id);

        // Asked once, and recorded on every stage the backend runs, so graphs
        // built by different ffmpeg releases can be told apart.
        let media_version = self.media.version();

        // --- probe -------------------------------------------------------
        checkpoint(Stage::Probe, 0.0)?;
        let started = std::time::Instant::now();
        let info = self.media.probe(path)?;
        if info.still {
            return self.analyze_still(path, info, known_hash, project_id, work_dir, observer);
        }
        if info.duration == Micros::ZERO {
            return Err(PerceptionError::EmptyMedia {
                path: path.display().to_string(),
            });
        }
        let content_hash = match known_hash {
            Some(hash) => hash,
            None => Blake3Hash::of_file(path).map_err(|e| PerceptionError::Io {
                path: path.display().to_string(),
                source: e,
            })?,
        };
        let source_id = "src-0001".to_string();

        // Store the absolute path. A draft carries a file:// URL built from
        // this, and a relative path there produces a URL that resolves only
        // from the directory the pipeline happened to be run in -- which is
        // never the directory the editor opens the draft from.
        let absolute = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        graph.sources.push(Source {
            id: source_id.clone(),
            path: absolute.display().to_string(),
            content_hash,
            media_kind: Some(MediaKind::Video),
            duration_us: info.duration,
            fps_num: Some(info.fps_num),
            fps_den: Some(info.fps_den),
            width: Some(info.width),
            height: Some(info.height),
            video_codec: info.video_codec.clone(),
            audio_codec: info.audio_codec.clone(),
            has_audio: Some(info.has_audio),
            created_at: info.created_at.clone(),
            rotation: Some(info.rotation),
            categories: Vec::new(),
        });
        graph.provenance.push(self.media_provenance(
            Stage::Probe,
            &["probe"],
            started,
            &media_version,
        ));

        // --- shot detection and the sweep, from one decode -----------------
        //
        // Every frame goes through the scene filter, and a small frame every
        // half second is read for the sweep, which follows each take from
        // start to end. Where the sweep works, it divides each shot into
        // segments and chooses one keyframe per segment; where the backend
        // cannot sweep, keyframes are spread through each shot as before,
        // and scored one by one.
        checkpoint(Stage::ShotDetect, 0.0)?;
        let started = std::time::Instant::now();
        let (cuts, readings) = self.scan(path, info.duration, observer)?;
        tracing::info!(cuts = cuts.len(), "scene detection complete");
        graph.shots = build_shots(&source_id, &cuts, info.duration, self.config.min_shot);
        graph.provenance.push(self.media_provenance(
            Stage::ShotDetect,
            &[&format!("threshold={}", self.config.scene_threshold)],
            started,
            &media_version,
        ));

        // Stills live under the source's content hash. Two clips in one
        // project both produce a "shot-0000-kf-0", and a shared directory
        // would have the second overwrite the first.
        let keyframe_dir = work_dir
            .join("keyframes")
            .join(&graph.sources[0].content_hash.to_hex()[..16]);
        let shot_count = graph.shots.len();
        let swept = !readings.is_empty();
        for shot_index in 0..shot_count {
            let fraction = shot_index as f64 / shot_count as f64;
            checkpoint(
                Stage::Keyframes,
                SWEEP_SHARE + (1.0 - SWEEP_SHARE) * fraction,
            )?;
            let shot = &graph.shots[shot_index];
            let (start, end) = (shot.start_us, shot.end_us);
            let from = readings.partition_point(|r| r.at < start);
            let to = readings.partition_point(|r| r.at < end);
            if to > from {
                let division =
                    timeline::divide(&shot.id, start, end, info.duration, &readings[from..to]);
                self.keep_division(
                    &mut graph.shots[shot_index],
                    division,
                    &readings[from..to],
                    path,
                    &keyframe_dir,
                    work_dir,
                );
            } else {
                self.spread_keyframes(&mut graph.shots[shot_index], path, &keyframe_dir, work_dir);
            }
        }
        graph.provenance.push(self.media_provenance(
            Stage::Keyframes,
            &[if swept { "sweep" } else { "adaptive" }],
            started,
            &media_version,
        ));
        graph.provenance.push(self.media_provenance(
            Stage::CvScore,
            &[if swept {
                "sweep:laplacian+exposure+motion+colour"
            } else {
                "laplacian+exposure+difference"
            }],
            started,
            &media_version,
        ));

        // --- the director's eye --------------------------------------------
        //
        // One frame of each usable segment, at its keyframe, through depth,
        // faces and people: how much of the picture something in front of
        // the scene covers. The narrow end of the funnel again -- a frame a
        // segment, never the stream.
        checkpoint(Stage::Director, 0.0)?;
        let started = std::time::Instant::now();
        if self.director.reads() {
            let (w, h) = look_size(info.display_size());
            let total: usize = graph
                .shots
                .iter()
                .map(|s| s.segments.iter().filter(|g| g.usable().is_some()).count())
                .sum();
            let mut done = 0usize;
            for shot in &mut graph.shots {
                for n in 0..shot.segments.len() {
                    let Some((from, to)) = shot.segments[n].usable() else {
                        continue;
                    };
                    checkpoint(Stage::Director, done as f64 / total.max(1) as f64)?;
                    done += 1;
                    let at = shot.segments[n]
                        .keyframe_id
                        .as_ref()
                        .and_then(|id| shot.keyframes.iter().find(|k| &k.id == id))
                        .map(|k| k.at_us)
                        .unwrap_or(Micros((from.0 + to.0) / 2));
                    match self.look(path, at, w, h) {
                        Ok(look) => {
                            if let Some(occlusion) = look.occlusion {
                                shot.segments[n].occlusion =
                                    Some((occlusion * 1000.0).round() / 1000.0);
                            }
                            if let Some(people) = look.people {
                                shot.segments[n].people = Some(pose::count(&people));
                                shot.segments[n].poses = vec![people];
                            }
                        }
                        // One frame the models could not read costs that
                        // segment its reading, not the analysis.
                        Err(e) => tracing::warn!(
                            segment = %shot.segments[n].id,
                            error = %e,
                            "the director could not read this segment"
                        ),
                    }
                }
            }
        }
        graph.provenance.push(self.provenance(
            Stage::Director,
            &self.director.name(),
            &[&format!("short={LOOK_SHORT_SIDE}")],
            started,
        ));

        // --- speech ------------------------------------------------------
        checkpoint(Stage::Asr, 0.0)?;
        let started = std::time::Instant::now();
        match self.asr.transcribe(path) {
            Ok(utterances) => {
                graph.transcript = utterances
                    .into_iter()
                    .enumerate()
                    .map(|(i, u)| TranscriptSegment {
                        id: format!("tr-{i:04}"),
                        source_id: source_id.clone(),
                        start_us: u.start,
                        end_us: u.end,
                        text: u.text,
                        speaker: u.speaker,
                        language: u.language,
                        confidence: u.confidence,
                    })
                    .collect();
            }
            Err(e) => {
                // Losing speech degrades the cut; it does not invalidate it.
                tracing::warn!(error = %e, "transcription failed, continuing without speech");
            }
        }
        graph
            .provenance
            .push(self.provenance(Stage::Asr, self.asr.name(), &[], started));

        // --- description --------------------------------------------------
        // Only the single best keyframe per shot is described. This is the
        // narrow end of the funnel: one model call per shot rather than per
        // frame is the difference between minutes and days.
        checkpoint(Stage::Describe, 0.0)?;
        let started = std::time::Instant::now();
        for index in 0..shot_count {
            checkpoint(Stage::Describe, index as f64 / shot_count as f64)?;
            let shot = &mut graph.shots[index];
            if shot.segments.is_empty() {
                let Some(still) = shot.best_keyframe().and_then(|k| k.path.clone()) else {
                    continue;
                };
                shot.description = self.describe(&work_dir.join(&still), &shot.id);
                continue;
            }
            // One description per usable segment, of its own keyframe: a take
            // that goes from the street to the lake is described as both.
            for n in 0..shot.segments.len() {
                let segment = &shot.segments[n];
                if segment.usable().is_none() {
                    continue;
                }
                let Some(still) = segment
                    .keyframe_id
                    .as_ref()
                    .and_then(|id| shot.keyframes.iter().find(|k| &k.id == id))
                    .and_then(|k| k.path.clone())
                else {
                    continue;
                };
                shot.segments[n].description = self.describe(&work_dir.join(&still), &shot.id);
            }
            // The shot as a whole is described by its best segment.
            shot.description = shot
                .segments
                .iter()
                .filter(|s| s.description.is_some())
                .max_by(|a, b| {
                    a.quality
                        .unwrap_or(0.0)
                        .total_cmp(&b.quality.unwrap_or(0.0))
                })
                .and_then(|s| s.description.clone());
        }
        graph
            .provenance
            .push(self.provenance(Stage::Describe, self.vlm.name(), &[], started));

        // --- identity tracking --------------------------------------------
        checkpoint(Stage::Track, 0.0)?;
        let started = std::time::Instant::now();
        let windows: Vec<(String, vb_infer::tracker::TrackWindow)> = graph
            .shots
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    vb_infer::tracker::TrackWindow {
                        start: s.start_us,
                        end: s.end_us,
                        stills: s
                            .keyframes
                            .iter()
                            .filter_map(|k| k.path.as_ref().map(|p| work_dir.join(p)))
                            .collect(),
                    },
                )
            })
            .collect();
        match self.tracker.track(path, &windows) {
            Ok(entities) => {
                for e in &entities {
                    for shot_id in &e.shot_ids {
                        if let Some(shot) = graph.shots.iter_mut().find(|s| &s.id == shot_id) {
                            shot.entity_refs.push(e.id.clone());
                        }
                    }
                }
                graph.entities = entities
                    .into_iter()
                    .map(|e| Entity {
                        id: e.id,
                        kind: e.kind,
                        label: e.label,
                        first_seen_us: Some(e.first_seen),
                        last_seen_us: Some(e.last_seen),
                        shot_ids: e.shot_ids,
                        confidence: e.confidence,
                        tracker: Some(self.tracker.name().to_string()),
                        thumbnail_path: e.thumbnail.map(|p| relative_to(&p, work_dir)),
                        signature: e.signature.into_iter().map(f64::from).collect(),
                    })
                    .collect();
            }
            Err(e) => tracing::warn!(error = %e, "tracking failed, continuing without entities"),
        }
        graph
            .provenance
            .push(self.provenance(Stage::Track, self.tracker.name(), &[], started));
        observer.stage(Stage::Track, 1.0);

        Ok(graph)
    }

    /// Analyse a photograph: one picture, read once.
    ///
    /// It becomes a source with one shot of [`STILL_HOLD`] and one segment
    /// spanning it, all usable, the camera still: the same picture at every
    /// moment. Its reading is the sweep's, taken of the one picture at the
    /// sweep's size, so its quality and its colour-and-layout signature are
    /// comparable with every video's -- which is how a photo joins the scene
    /// of the videos filmed around it. Then the director, a description and
    /// the faces, as for a video's keyframe. There is nothing to listen to
    /// and nothing to cut, so speech and shot detection do not run, and
    /// provenance does not say they did.
    fn analyze_still(
        &self,
        path: &Path,
        info: vb_media::MediaInfo,
        known_hash: Option<Blake3Hash>,
        project_id: &str,
        work_dir: &Path,
        observer: &dyn Observer,
    ) -> Result<FootageGraph, PerceptionError> {
        let checkpoint = |stage: Stage, fraction: f64| -> Result<(), PerceptionError> {
            if observer.cancelled() {
                return Err(PerceptionError::Cancelled);
            }
            observer.stage(stage, fraction);
            Ok(())
        };
        let media_version = self.media.version();
        let started = std::time::Instant::now();
        let content_hash = match known_hash {
            Some(hash) => hash,
            None => Blake3Hash::of_file(path).map_err(|e| PerceptionError::Io {
                path: path.display().to_string(),
                source: e,
            })?,
        };
        let source_id = "src-0001".to_string();
        let absolute = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut graph = FootageGraph::new(project_id);
        graph.sources.push(Source {
            id: source_id.clone(),
            path: absolute.display().to_string(),
            content_hash,
            media_kind: Some(MediaKind::Image),
            duration_us: STILL_HOLD,
            fps_num: None,
            fps_den: None,
            width: Some(info.width),
            height: Some(info.height),
            video_codec: info.video_codec.clone(),
            audio_codec: None,
            has_audio: Some(false),
            created_at: info.created_at.clone(),
            rotation: Some(info.rotation),
            categories: Vec::new(),
        });
        graph.provenance.push(self.media_provenance(
            Stage::Probe,
            &["probe", "still"],
            started,
            &media_version,
        ));

        // --- the one reading ----------------------------------------------
        checkpoint(Stage::Keyframes, 0.0)?;
        let started = std::time::Instant::now();
        let frame = self.media.rgb_frame(
            path,
            Micros::ZERO,
            timeline::SWEEP_WIDTH,
            timeline::SWEEP_HEIGHT,
        )?;
        let reading = timeline::Reader::default().read(&frame);
        let quality = (reading.quality() * 1000.0).round() / 1000.0;
        let shot_id = "shot-0000".to_string();
        let keyframe_id = format!("{shot_id}-kf-0");
        let keyframe_dir = work_dir
            .join("keyframes")
            .join(&graph.sources[0].content_hash.to_hex()[..16]);
        let still = if self.config.extract_stills {
            self.still(
                path,
                Micros::ZERO,
                &keyframe_dir.join(format!("{keyframe_id}.jpg")),
                work_dir,
            )
        } else {
            None
        };
        let mut scores = KeyframeScores {
            sharpness: Some(reading.sharpness),
            flow_jitter: Some(0.0),
            face_quality: None,
            composition: Some(reading.composition()),
            exposure: Some(reading.exposure),
            aggregate: None,
        };
        scores.aggregate = Some(reading.quality());
        let mut shot = Shot {
            id: shot_id.clone(),
            source_id: source_id.clone(),
            start_us: Micros::ZERO,
            end_us: STILL_HOLD,
            shot_change_confidence: None,
            keyframes: vec![Keyframe {
                id: keyframe_id.clone(),
                at_us: Micros::ZERO,
                path: still.clone(),
                scores: Some(scores),
            }],
            description: None,
            entity_refs: Vec::new(),
            actions: Vec::new(),
            audio: None,
            tags: Vec::new(),
            segments: vec![Segment {
                id: format!("{shot_id}-seg-0"),
                start_us: Micros::ZERO,
                end_us: STILL_HOLD,
                usable_start_us: Some(Micros::ZERO),
                usable_end_us: Some(STILL_HOLD),
                quality: Some(quality),
                camera: Some(CameraMotion::Still),
                keyframe_id: Some(keyframe_id),
                signature: reading
                    .signature
                    .iter()
                    .map(|v| (v * 1000.0).round() / 1000.0)
                    .collect(),
                ..Default::default()
            }],
            // One measurement, which holds for as long as the photo is shown.
            timeline: vec![TimelineSample {
                at_us: Micros::ZERO,
                quality: Some(quality),
                motion: Some(0.0),
                camera: Some(0.0),
            }],
        };
        for stage in [Stage::Keyframes, Stage::CvScore] {
            graph.provenance.push(self.media_provenance(
                stage,
                &["still", "sweep:laplacian+exposure+colour"],
                started,
                &media_version,
            ));
        }

        // --- the director's eye --------------------------------------------
        checkpoint(Stage::Director, 0.0)?;
        let started = std::time::Instant::now();
        if self.director.reads() {
            let (w, h) = look_size(info.display_size());
            match self.look(path, Micros::ZERO, w, h) {
                Ok(look) => {
                    let segment = &mut shot.segments[0];
                    if let Some(occlusion) = look.occlusion {
                        segment.occlusion = Some((occlusion * 1000.0).round() / 1000.0);
                    }
                    if let Some(people) = look.people {
                        segment.people = Some(pose::count(&people));
                        segment.poses = vec![people];
                    }
                }
                Err(e) => tracing::warn!(error = %e, "the director could not read this photo"),
            }
        }
        graph.provenance.push(self.provenance(
            Stage::Director,
            &self.director.name(),
            &[&format!("short={LOOK_SHORT_SIDE}")],
            started,
        ));

        // --- description --------------------------------------------------
        checkpoint(Stage::Describe, 0.0)?;
        let started = std::time::Instant::now();
        if let Some(still) = &still {
            let description = self.describe(&work_dir.join(still), &shot.id);
            shot.segments[0].description = description.clone();
            shot.description = description;
        }
        graph
            .provenance
            .push(self.provenance(Stage::Describe, self.vlm.name(), &[], started));
        graph.shots.push(shot);

        // --- identity tracking --------------------------------------------
        checkpoint(Stage::Track, 0.0)?;
        let started = std::time::Instant::now();
        let windows = vec![(
            shot_id.clone(),
            vb_infer::tracker::TrackWindow {
                start: Micros::ZERO,
                end: STILL_HOLD,
                stills: still.iter().map(|p| work_dir.join(p)).collect(),
            },
        )];
        match self.tracker.track(path, &windows) {
            Ok(entities) => {
                for e in &entities {
                    if e.shot_ids.contains(&shot_id) {
                        graph.shots[0].entity_refs.push(e.id.clone());
                    }
                }
                graph.entities = entities
                    .into_iter()
                    .map(|e| Entity {
                        id: e.id,
                        kind: e.kind,
                        label: e.label,
                        first_seen_us: Some(e.first_seen),
                        last_seen_us: Some(e.last_seen),
                        shot_ids: e.shot_ids,
                        confidence: e.confidence,
                        tracker: Some(self.tracker.name().to_string()),
                        thumbnail_path: e.thumbnail.map(|p| relative_to(&p, work_dir)),
                        signature: e.signature.into_iter().map(f64::from).collect(),
                    })
                    .collect();
            }
            Err(e) => tracing::warn!(error = %e, "tracking failed, continuing without entities"),
        }
        graph
            .provenance
            .push(self.provenance(Stage::Track, self.tracker.name(), &[], started));
        observer.stage(Stage::Track, 1.0);
        Ok(graph)
    }

    /// Find the cuts and sweep the whole file into readings, from one decode
    /// where the backend can. No readings when the backend cannot sweep.
    ///
    /// A backend that cannot sweep, or a file it fails on part way, falls
    /// back to keyframes picked in advance: the analysis is poorer, not
    /// broken. Cancelling stops the decode itself.
    fn scan(
        &self,
        path: &Path,
        duration: Micros,
        observer: &dyn Observer,
    ) -> Result<(Vec<vb_media::SceneCut>, Vec<timeline::Reading>), PerceptionError> {
        let mut reader = timeline::Reader::default();
        let mut readings = Vec::new();
        let result = self.media.scan(
            path,
            self.config.scene_threshold,
            timeline::SWEEP_EVERY,
            timeline::SWEEP_WIDTH,
            timeline::SWEEP_HEIGHT,
            &mut |frame| {
                if observer.cancelled() {
                    return false;
                }
                if duration > Micros::ZERO {
                    let through = frame.at.0 as f64 / duration.0 as f64;
                    observer.stage(Stage::Keyframes, SWEEP_SHARE * through.min(1.0));
                }
                readings.push(reader.read(&frame));
                true
            },
        );
        if observer.cancelled() {
            return Err(PerceptionError::Cancelled);
        }
        match result {
            Ok(scan) => match scan.frames {
                Ok(_) => Ok((scan.cuts, readings)),
                Err(MediaError::Unsupported { .. }) => Ok((scan.cuts, Vec::new())),
                Err(e) => {
                    tracing::warn!(error = %e, "the sweep failed; spreading keyframes instead");
                    Ok((scan.cuts, Vec::new()))
                }
            },
            // The one decode failed part way. Cuts alone may still come out
            // of the file; the sweep's partial readings are not kept.
            Err(e) => {
                tracing::warn!(error = %e, "the scan failed; finding cuts alone");
                let cuts = self
                    .media
                    .detect_scenes(path, self.config.scene_threshold)?;
                Ok((cuts, Vec::new()))
            }
        }
    }

    /// Keep a shot's division: its segments, its readings, and one keyframe
    /// per segment at the segment's best moment, with a still for each one
    /// worth describing.
    fn keep_division(
        &self,
        shot: &mut vb_contracts::Shot,
        division: timeline::Division,
        readings: &[timeline::Reading],
        path: &Path,
        keyframe_dir: &Path,
        work_dir: &Path,
    ) {
        let mut keyframes = Vec::with_capacity(division.segments.len());
        let mut segments = division.segments;
        for (n, segment) in segments.iter_mut().enumerate() {
            let Some(reading) = division
                .representative
                .get(n)
                .and_then(|i| readings.get(*i))
            else {
                continue;
            };
            let id = format!("{}-kf-{n}", shot.id);
            // Only a usable segment is ever described or shown, so only its
            // still is worth the decode.
            let still = if self.config.extract_stills && segment.usable().is_some() {
                self.still(
                    path,
                    reading.at,
                    &keyframe_dir.join(format!("{id}.jpg")),
                    work_dir,
                )
            } else {
                None
            };
            let mut scores = KeyframeScores {
                sharpness: Some(reading.sharpness),
                flow_jitter: Some((reading.camera_speed() / 0.45).clamp(0.0, 1.0)),
                face_quality: None,
                composition: Some(reading.composition()),
                exposure: Some(reading.exposure),
                aggregate: None,
            };
            scores.aggregate = Some(reading.quality());
            segment.keyframe_id = Some(id.clone());
            keyframes.push(Keyframe {
                id,
                at_us: reading.at,
                path: still,
                scores: Some(scores),
            });
        }
        shot.keyframes = keyframes;
        shot.segments = segments;
        shot.timeline = division.timeline;
    }

    /// Keyframes spread through a shot and scored one at a time: how a shot
    /// is analysed when there is no sweep to divide it by.
    fn spread_keyframes(
        &self,
        shot: &mut vb_contracts::Shot,
        path: &Path,
        keyframe_dir: &Path,
        work_dir: &Path,
    ) {
        let times = keyframe_times(
            shot,
            self.config.max_keyframes_per_shot,
            self.config.keyframe_every_seconds,
        );
        shot.keyframes = times
            .iter()
            .enumerate()
            .map(|(i, at)| {
                let id = format!("{}-kf-{i}", shot.id);
                let still = if self.config.extract_stills {
                    self.still(path, *at, &keyframe_dir.join(format!("{id}.jpg")), work_dir)
                } else {
                    None
                };
                Keyframe {
                    id,
                    at_us: *at,
                    path: still,
                    scores: self.score_frame(path, *at),
                }
            })
            .collect();
    }

    /// Extract a still, returning its path relative to the work directory.
    ///
    /// A still that will not extract is not fatal: the keyframe keeps its
    /// scores and simply cannot be described.
    fn still(&self, path: &Path, at: Micros, dest: &Path, work_dir: &Path) -> Option<String> {
        match self.media.extract_still(path, at, dest) {
            Ok(p) => Some(relative_to(&p, work_dir)),
            Err(e) => {
                tracing::warn!(still = %dest.display(), error = %e, "still extraction failed");
                None
            }
        }
    }

    /// Describe one still, or `None` when the model saw nothing or failed.
    ///
    /// A mock describes nothing: its placeholder says so in words, and a
    /// scene named after "no vision model loaded" is how a mock's output
    /// would reach a user as if something had looked.
    fn describe(&self, still: &Path, shot_id: &str) -> Option<Description> {
        if self.vlm.is_mock() {
            return None;
        }
        match self.vlm.describe(&vb_infer::VlmRequest::describe(still)) {
            // Nothing recognised is recorded as no description, not as an
            // empty one: an empty string reads downstream as "looked, and
            // there is nothing to say", which is a different claim.
            Ok(text) if text.trim().is_empty() => None,
            Ok(text) => Some(Description {
                text,
                model: Some(self.vlm.name().to_string()),
                confidence: None,
            }),
            Err(e) => {
                tracing::warn!(shot = %shot_id, error = %e, "description failed");
                None
            }
        }
    }

    /// Classical scores for one frame.
    ///
    /// Returns `None` rather than zeros when the frame cannot be decoded, so a
    /// failed read is distinguishable from a genuinely bad frame.
    fn score_frame(&self, path: &Path, at: Micros) -> Option<KeyframeScores> {
        let frame = self
            .media
            .gray_frame(path, at, self.config.score_width, self.config.score_height)
            .ok()?;

        let sharpness = vb_media::laplacian_sharpness(&frame);
        let exposure = vb_media::exposure_quality(&frame);

        // Motion is estimated by comparing with a frame a fifth of a second
        // later. A large difference at a keyframe means camera shake or motion
        // blur, both of which make it a poor representative of its shot.
        let flow_jitter = self
            .media
            .gray_frame(
                path,
                at + Micros::from_millis(200),
                self.config.score_width,
                self.config.score_height,
            )
            .ok()
            .map(|next| vb_media::frame_difference(&frame, &next));

        let mut scores = KeyframeScores {
            sharpness: Some(sharpness),
            flow_jitter,
            face_quality: None,
            composition: None,
            exposure: Some(exposure),
            aggregate: None,
        };
        scores.aggregate = Some(scores.aggregate());
        Some(scores)
    }

    /// Provenance for a stage the media backend ran. The adapter field names
    /// the backend; the tool fields name the ffmpeg release underneath it.
    /// Read one frame the way the director does.
    fn look(&self, path: &Path, at: Micros, w: u32, h: u32) -> Result<Look, String> {
        let frame = self
            .media
            .rgb_frame(path, at, w, h)
            .map_err(|e| e.to_string())?;
        let image = RgbImage {
            width: frame.width,
            height: frame.height,
            rgb: frame.rgb,
        };
        let occlusion = if self.director.depth.is_mock() {
            None
        } else {
            let depth = self
                .director
                .depth
                .depth(&image)
                .map_err(|e| e.to_string())?;
            let faces = self.director.faces.faces(&image).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "face detection failed; reading without faces");
                Vec::new()
            });
            let people = if self.director.people.is_mock() {
                None
            } else {
                self.director
                    .people
                    .people(&image)
                    .map_err(|e| tracing::warn!(error = %e, "person segmentation failed"))
                    .ok()
            };
            Some(director::occlusion(&depth, &faces, people.as_ref()))
        };
        let people = if self.director.pose.is_mock() {
            None
        } else {
            match self.director.pose.poses(&image) {
                Ok(found) => Some(pose::sample(at, &found, image.width, image.height)),
                Err(e) => {
                    tracing::warn!(error = %e, "pose estimation failed");
                    None
                }
            }
        };
        Ok(Look { occlusion, people })
    }

    fn media_provenance(
        &self,
        stage: Stage,
        params: &[&str],
        started: std::time::Instant,
        version: &Option<String>,
    ) -> ProvenanceEntry {
        ProvenanceEntry {
            tool: Some("ffmpeg".to_string()),
            tool_version: version.clone(),
            ..self.provenance(stage, self.media.name(), params, started)
        }
    }

    fn provenance(
        &self,
        stage: Stage,
        adapter: &str,
        params: &[&str],
        started: std::time::Instant,
    ) -> ProvenanceEntry {
        let joined = params.join(",");
        ProvenanceEntry {
            stage,
            tool: None,
            tool_version: None,
            adapter: Some(adapter.to_string()),
            params_hash: Blake3Hash::of_parts(&[stage.name().as_bytes(), joined.as_bytes()]),
            completed_at: Some(vb_contracts::now_rfc3339()),
            duration_ms: Some(started.elapsed().as_millis() as u64),
        }
    }
}

/// Store keyframe paths relative to the work directory, so a project folder
/// stays portable between machines.
fn relative_to(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Combine per-file analyses into one graph for a project.
///
/// Each file is analysed and cached on its own, keyed by its content, so
/// adding a clip to a project re-analyses that clip alone. Ids inside a part
/// are therefore local to it, and are renumbered here: sources in the order
/// given, shots and transcript segments running across the whole project.
///
/// A keyframe's stored path is deliberately left alone. The still on disk is
/// named by the analysis that produced it, not by the id it ends up with, and
/// rewriting the id must not break the pointer to the file.
pub fn merge_graphs(project_id: &str, parts: Vec<FootageGraph>) -> FootageGraph {
    let mut graph = FootageGraph::new(project_id);
    let mut shot_index = 0usize;
    let mut segment_index = 0usize;

    for (i, mut part) in parts.into_iter().enumerate() {
        let source_id = format!("src-{:04}", i + 1);

        // Entity ids are local to a part too, and shots point at them by id,
        // so the rename has to happen before the shots are rewritten.
        let mut renamed: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for entity in part.entities.iter_mut() {
            let id = format!("{source_id}-{}", entity.id);
            renamed.insert(std::mem::replace(&mut entity.id, id.clone()), id);
        }

        for source in part.sources.drain(..) {
            graph.sources.push(Source {
                id: source_id.clone(),
                ..source
            });
        }

        let mut shot_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for mut shot in part.shots.drain(..) {
            let id = format!("shot-{shot_index:04}");
            shot_index += 1;
            shot_ids.insert(shot.id.clone(), id.clone());
            let mut keyframe_ids: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (j, keyframe) in shot.keyframes.iter_mut().enumerate() {
                let new_id = format!("{id}-kf-{j}");
                keyframe_ids.insert(std::mem::replace(&mut keyframe.id, new_id.clone()), new_id);
            }
            // Segments are named after their shot and point at its keyframes,
            // so both follow the renumbering.
            for (k, segment) in shot.segments.iter_mut().enumerate() {
                segment.id = format!("{id}-seg-{k}");
                if let Some(renamed) = segment
                    .keyframe_id
                    .as_ref()
                    .and_then(|old| keyframe_ids.get(old))
                {
                    segment.keyframe_id = Some(renamed.clone());
                }
            }
            for entity_ref in shot.entity_refs.iter_mut() {
                if let Some(renamed) = renamed.get(entity_ref) {
                    entity_ref.clone_from(renamed);
                }
            }
            shot.id = id;
            shot.source_id = source_id.clone();
            graph.shots.push(shot);
        }

        for mut segment in part.transcript.drain(..) {
            segment.id = format!("tr-{segment_index:04}");
            segment_index += 1;
            segment.source_id = source_id.clone();
            graph.transcript.push(segment);
        }

        for mut entity in part.entities.drain(..) {
            // The shot ids an entity points at have just been renumbered, so
            // it follows them: a stale id would resolve to another clip's
            // shot, and none at all left the album not knowing which clip a
            // person was in.
            entity.shot_ids = entity
                .shot_ids
                .iter()
                .filter_map(|old| shot_ids.get(old).cloned())
                .collect();
            graph.entities.push(entity);
        }

        graph.provenance.append(&mut part.provenance);
    }

    graph
}

/// Mine statistics and attach them to the graph.
///
/// Separate from `analyze` because it needs no media access and can be re-run
/// cheaply after a manual edit to the shot list.
pub fn attach_priors(graph: &mut FootageGraph) {
    graph.priors = Some(mine(graph));
}

fn mine(graph: &FootageGraph) -> vb_contracts::footage_graph::MinedPriors {
    // The mining itself lives in the narrative layer, which owns priors.
    // Duplicating the statistics here would let the two drift apart, so
    // perception only computes what the graph schema needs directly.
    // A photo's shot is how long it may be held, not a length anyone filmed.
    let mut lengths: Vec<u64> = graph
        .shots
        .iter()
        .filter(|s| !graph.source(&s.source_id).is_some_and(Source::is_still))
        .map(|s| s.duration().0)
        .collect();
    lengths.sort_unstable();
    if lengths.is_empty() {
        return Default::default();
    }
    let sum: u128 = lengths.iter().map(|l| *l as u128).sum();
    vb_contracts::footage_graph::MinedPriors {
        shot_length: Some(vb_contracts::footage_graph::ShotLengthPrior {
            mean_us: Some(Micros((sum / lengths.len() as u128) as u64)),
            median_us: Some(Micros(lengths[lengths.len() / 2])),
            p10_us: Some(Micros(lengths[lengths.len() / 10])),
            p90_us: Some(Micros(lengths[lengths.len() * 9 / 10])),
        }),
        beat_grid: None,
        transition_frequency: None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PerceptionError {
    #[error(transparent)]
    Media(#[from] MediaError),

    #[error("{path} has no duration; it may be corrupt or not a media file")]
    EmptyMedia { path: String },

    #[error("the analysis was cancelled")]
    Cancelled,

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vb_infer::{MockAsr, MockTracker, MockVlm};
    use vb_media::backend::{GrayFrame, MediaInfo, SceneCut};

    /// A backend that returns fixed data, so the pipeline can be exercised
    /// without ffmpeg or any media file present.
    struct FakeMedia {
        duration: Micros,
        cuts: Vec<SceneCut>,
    }

    impl MediaBackend for FakeMedia {
        fn name(&self) -> &'static str {
            "FakeMedia"
        }
        fn probe(&self, _p: &Path) -> Result<MediaInfo, MediaError> {
            Ok(MediaInfo {
                duration: self.duration,
                fps_num: 30,
                fps_den: 1,
                width: 1920,
                height: 1080,
                video_codec: Some("h264".into()),
                audio_codec: None,
                has_audio: false,
                rotation: 0,
                created_at: None,
                still: false,
                location: None,
            })
        }
        fn detect_scenes(&self, _p: &Path, _t: f64) -> Result<Vec<SceneCut>, MediaError> {
            Ok(self.cuts.clone())
        }
        fn gray_frame(
            &self,
            _p: &Path,
            at: Micros,
            w: u32,
            h: u32,
        ) -> Result<GrayFrame, MediaError> {
            // Vary with timestamp so different keyframes score differently.
            let base = (at.0 / 1_000_000) as u8;
            let pixels = (0..(w * h))
                .map(|i| base.wrapping_add((i % 97) as u8))
                .collect();
            Ok(GrayFrame {
                width: w,
                height: h,
                pixels,
            })
        }
        fn extract_still(
            &self,
            _p: &Path,
            _at: Micros,
            dest: &Path,
        ) -> Result<PathBuf, MediaError> {
            Ok(dest.to_path_buf())
        }
    }

    fn run(duration_s: u64, cuts: Vec<SceneCut>) -> FootageGraph {
        let media = FakeMedia {
            duration: Micros::from_secs(duration_s),
            cuts,
        };
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
        let mut p = Perception::new(&media, &asr, &vlm, &tracker);
        p.config.extract_stills = false;
        // A real file is needed only for content hashing.
        let tmp = std::env::temp_dir().join("vb-perception-test-input.bin");
        std::fs::write(&tmp, b"fake media bytes").unwrap();
        p.analyze(&tmp, "test", &std::env::temp_dir()).unwrap()
    }

    #[test]
    fn analysis_produces_a_graph_that_satisfies_the_contract() {
        let g = run(
            60,
            vec![
                SceneCut {
                    at: Micros::from_secs(20),
                    score: 0.8,
                },
                SceneCut {
                    at: Micros::from_secs(40),
                    score: 0.9,
                },
            ],
        );
        vb_contracts::validate::validate_footage_graph(&g)
            .expect("perception must emit a valid footage graph");
        assert_eq!(g.shots.len(), 3);
        assert_eq!(g.sources.len(), 1);
    }

    #[test]
    fn every_shot_gets_scored_keyframes() {
        let g = run(60, vec![]);
        for shot in &g.shots {
            assert!(
                !shot.keyframes.is_empty(),
                "shot {} has no keyframes",
                shot.id
            );
            for kf in &shot.keyframes {
                let s = kf.scores.as_ref().expect("keyframe must be scored");
                assert!(s.aggregate.is_some());
                assert!(shot.contains(kf.at_us));
            }
        }
    }

    #[test]
    fn provenance_names_the_mock_adapters_that_ran() {
        // Without this the output would be indistinguishable from a real run.
        let g = run(30, vec![]);
        assert_eq!(g.stage_adapter(Stage::Asr), Some("MockAsr"));
        assert_eq!(g.stage_adapter(Stage::Describe), Some("MockVlm"));
        assert_eq!(g.stage_adapter(Stage::Track), Some("MockTracker"));
        for stage in Stage::ALL.iter().take(7) {
            assert!(
                g.provenance.iter().any(|p| p.stage == *stage),
                "stage {stage:?} left no provenance record"
            );
        }
    }

    #[test]
    fn mocked_stages_are_reported() {
        let media = FakeMedia {
            duration: Micros::from_secs(10),
            cuts: vec![],
        };
        let (asr, vlm, tracker) = (MockAsr, MockVlm, MockTracker);
        let p = Perception::new(&media, &asr, &vlm, &tracker);
        assert_eq!(p.mocked_stages().len(), 3);
    }

    #[test]
    fn the_source_path_is_absolute_so_draft_urls_resolve() {
        // A relative path here becomes a file:// URL that only resolves from
        // the directory the pipeline ran in, breaking every exported draft.
        let g = run(10, vec![]);
        let path = &g.sources[0].path;
        assert!(
            Path::new(path).is_absolute(),
            "source path {path} is relative; exported drafts would not resolve"
        );
    }

    #[test]
    fn a_merged_graph_keeps_each_subject_on_its_own_clips_shots() {
        let part = |shots: usize| {
            let mut g = run(20, vec![]);
            g.shots = (0..shots)
                .map(|i| vb_contracts::Shot {
                    id: format!("shot-{i:04}"),
                    ..g.shots[0].clone()
                })
                .collect();
            g.entities.push(Entity {
                id: "person-001".into(),
                kind: vb_contracts::footage_graph::EntityKind::Person,
                label: None,
                first_seen_us: None,
                last_seen_us: None,
                shot_ids: vec!["shot-0001".into()],
                confidence: None,
                tracker: None,
                thumbnail_path: None,
                signature: vec![],
            });
            g
        };
        let merged = merge_graphs("p", vec![part(2), part(3)]);
        // The second clip's shot-0001 is the merged graph's fourth shot.
        assert_eq!(merged.entities[0].shot_ids, vec!["shot-0001".to_string()]);
        assert_eq!(merged.entities[1].shot_ids, vec!["shot-0003".to_string()]);
        let shot = merged.shot("shot-0003").unwrap();
        assert_eq!(shot.source_id, "src-0002");
    }

    #[test]
    fn mined_priors_describe_the_shot_lengths() {
        let mut g = run(
            60,
            vec![SceneCut {
                at: Micros::from_secs(30),
                score: 0.9,
            }],
        );
        attach_priors(&mut g);
        assert!(g.priors.unwrap().shot_length.is_some());
    }
}
