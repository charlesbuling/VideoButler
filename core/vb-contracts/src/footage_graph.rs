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

//! Input Contract: `footage-graph/0.1`.
//!
//! The perception stack writes this; the narrative layer reads it and nothing
//! else. Perception upgrades -- including Phase 2's 主体衔接 (subject
//! continuity) -- may add fields here, but may never change the shape the
//! narrative layer sees.

use crate::hash::Blake3Hash;
use crate::time::Micros;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "footage-graph/0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FootageGraph {
    pub schema_version: String,
    pub generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub sources: Vec<Source>,
    pub shots: Vec<Shot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Entity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transcript: Vec<TranscriptSegment>,
    /// Segments grouped by the view they show, across every source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Scene>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priors: Option<MinedPriors>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<ProvenanceEntry>,
}

impl FootageGraph {
    pub fn new(project_id: impl Into<String>) -> Self {
        FootageGraph {
            schema_version: SCHEMA_VERSION.to_string(),
            generated_at: crate::now_rfc3339(),
            project_id: Some(project_id.into()),
            sources: Vec::new(),
            shots: Vec::new(),
            entities: Vec::new(),
            transcript: Vec::new(),
            scenes: Vec::new(),
            priors: None,
            provenance: Vec::new(),
        }
    }

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn shot(&self, id: &str) -> Option<&Shot> {
        self.shots.iter().find(|s| s.id == id)
    }

    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// Shots covering a source, in timeline order.
    pub fn shots_of(&self, source_id: &str) -> impl Iterator<Item = &Shot> {
        self.shots.iter().filter(move |s| s.source_id == source_id)
    }

    /// Did a real adapter run this stage, or a mock?
    ///
    /// The orchestrator and the UI both need this: a montage built from mock
    /// descriptions is a structural smoke test, not a usable cut, and it must
    /// never be presented as one.
    pub fn stage_adapter(&self, stage: Stage) -> Option<&str> {
        self.provenance
            .iter()
            .find(|p| p.stage == stage)
            .and_then(|p| p.adapter.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub path: String,
    pub content_hash: Blake3Hash,
    /// A moving picture or a photograph; absent means video.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_kind: Option<MediaKind>,
    /// How long a video runs. For an image, the longest it may be held on
    /// screen: a photograph has no running time of its own.
    pub duration_us: Micros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_num: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_den: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// The clockwise turn, in degrees, that shows the picture upright.
    /// `width` and `height` are as stored, before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<u32>,
    /// What the video is of, as the video butler classifies it: `people`,
    /// `scenery`, `food` and the rest of the schema's list. Empty when it
    /// was not classified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// What kind of picture a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Video,
    /// A photograph: one picture, shown for as long as the cut holds it.
    Image,
}

impl Source {
    /// Is this a photograph rather than a video?
    pub fn is_still(&self) -> bool {
        self.media_kind == Some(MediaKind::Image)
    }

    /// When the camera recorded it, as seconds since the epoch, when its
    /// capture time is known.
    pub fn captured_seconds(&self) -> Option<i64> {
        self.created_at
            .as_deref()
            .and_then(crate::time::capture_seconds)
    }

    /// Width and height as the picture is shown, after its rotation.
    pub fn display_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.width?, self.height?);
        Some(if self.rotation.unwrap_or(0) % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        })
    }

    pub fn frame_rate(&self) -> crate::time::FrameRate {
        match (self.fps_num, self.fps_den) {
            (Some(n), Some(d)) if n > 0 && d > 0 => crate::time::FrameRate::new(n, d),
            _ => crate::time::FrameRate::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub id: String,
    pub source_id: String,
    pub start_us: Micros,
    pub end_us: Micros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_change_confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<Keyframe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<ShotAudio>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The shot followed from start to end and divided where the view
    /// changes. Tiles the shot, in order. Empty when the analysis could not
    /// sweep the footage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// Measurements across the whole shot, about two a second, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<TimelineSample>,
}

impl Shot {
    pub fn duration(&self) -> Micros {
        self.end_us.saturating_sub(self.start_us)
    }

    /// Does a timecode fall inside this shot? The grounding gate leans on this.
    ///
    /// The range is half-open: the end timecode belongs to the next shot, so
    /// that adjacent shots tile a source without overlapping.
    pub fn contains(&self, at: Micros) -> bool {
        at >= self.start_us && at < self.end_us
    }

    /// The best keyframe by aggregate score, which is how the montage selector
    /// ranks shots without invoking a model.
    pub fn best_keyframe(&self) -> Option<&Keyframe> {
        self.keyframes.iter().max_by(|a, b| {
            a.aggregate_score()
                .partial_cmp(&b.aggregate_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    pub fn score(&self) -> f64 {
        self.best_keyframe().map_or(0.0, |k| k.aggregate_score())
    }
}

/// A stretch of a shot that shows one view.
///
/// A phone take is usually one shot and several of these: the camera finds
/// its subject, holds, pans to the lake, holds again. Choosing from segments
/// rather than shots is what lets a cut take the lake without the pan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub start_us: Micros,
    pub end_us: Micros,
    /// The usable part, less the start of recording, a settling camera or a
    /// whip pan. Both absent when none of it is usable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usable_start_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usable_end_us: Option<Micros>,
    /// The scene this shows, one of the graph's `scenes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraMotion>,
    /// The shot keyframe that best represents this segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyframe_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
    /// A colour-and-layout vector of the view, comparable across sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signature: Vec<f64>,
    /// How much of the picture a near-plane occluder covers at the keyframe,
    /// 0..1, read from depth with the subject excluded. Absent when no depth
    /// model ran -- which is not the same as a clear view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occlusion: Option<f64>,
    /// How well the keyframe is composed, 0..1, from the composition model.
    /// Only moments shortlisted for a cut are judged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<f64>,
    /// A sentence for the screen, in the viewer's language, saying what the
    /// segment shows, from a general vision-language model over its
    /// keyframe. Only moments a cut uses are captioned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<Description>,
    /// How many people the pose model found at the keyframe. Absent when no
    /// pose model ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub people: Option<u32>,
    /// Body poses through the usable part, sampled where a dance cut may
    /// join this segment to another. Only moments a dance cut may use are
    /// sampled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub poses: Vec<PoseSample>,
    /// The beat the people move to, read from their poses where a 主体衔接
    /// cut may follow them. Absent where no one was followed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhythm: Option<Rhythm>,
}

/// The beat people move to, and where they keep none: their tempo, every
/// beat they hit, the stretches any one of them stands idle more than one
/// and a half beats, those someone spends watching what they hold, and
/// those they move to no beat. Times are the source's own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rhythm {
    pub bpm: f64,
    /// How regularly the movement comes back to the beat, 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub beats_us: Vec<Micros>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub idle: Vec<Span>,
    /// Where someone holds a hand still in front of their chest for most of
    /// four bars: watching a phone with the steps on it, not dancing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watching: Vec<Span>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unrhythmic: Vec<Span>,
}

impl Rhythm {
    /// How long one beat lasts.
    pub fn beat(&self) -> Micros {
        Micros((60_000_000.0 / self.bpm.max(1e-6)).round() as u64)
    }

    /// How long its people are not dancing -- idle or watching -- each
    /// moment counted once.
    pub fn stopped(&self) -> Micros {
        let mut spans: Vec<(u64, u64)> = self
            .idle
            .iter()
            .chain(&self.watching)
            .map(|s| (s.start_us.0, s.end_us.0))
            .collect();
        spans.sort();
        let (mut total, mut reach) = (0, 0);
        for (a, b) in spans {
            if b > reach {
                total += b - a.max(reach);
                reach = b;
            }
        }
        Micros(total)
    }
}

/// A stretch of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start_us: Micros,
    pub end_us: Micros,
}

/// The people in one frame and how each is standing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoseSample {
    pub at_us: Micros,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonPose>,
}

/// Who counts as one of the people in a frame: a confident detection, tall
/// enough to be a subject rather than a passer-by in the background.
const SUBJECT_SCORE: f64 = 0.5;
const SUBJECT_HEIGHT: f64 = 0.2;
const SUBJECT_OF_TALLEST: f64 = 0.45;

impl PoseSample {
    /// The people the frame is of, left to right: confident, and at least a
    /// fifth of the frame tall and nearly half as tall as the tallest, so a
    /// crowd behind two dancers does not make it a crowd scene.
    pub fn subjects(&self) -> Vec<&PersonPose> {
        let tallest = self
            .people
            .iter()
            .filter(|p| p.score >= SUBJECT_SCORE)
            .map(PersonPose::height)
            .fold(0.0, f64::max);
        let mut out: Vec<&PersonPose> = self
            .people
            .iter()
            .filter(|p| {
                let tall = p.height();
                p.score >= SUBJECT_SCORE
                    && tall >= SUBJECT_HEIGHT.min(tallest) * 0.999
                    && tall >= SUBJECT_OF_TALLEST * tallest
            })
            .collect();
        out.sort_by(|a, b| a.centre().0.total_cmp(&b.centre().0));
        out
    }

    /// How many people the frame is of.
    pub fn count(&self) -> u32 {
        self.subjects().len() as u32
    }
}

impl PersonPose {
    pub fn height(&self) -> f64 {
        (self.bbox[3] - self.bbox[1]).max(0.0)
    }

    pub fn centre(&self) -> (f64, f64) {
        (
            (self.bbox[0] + self.bbox[2]) / 2.0,
            (self.bbox[1] + self.bbox[3]) / 2.0,
        )
    }
}

/// One person: where they are, and the seventeen COCO body keypoints.
///
/// Coordinates are fractions of the upright frame, 0 at the left and top, so
/// a pose from a portrait clip compares with one from a landscape clip by
/// where the body sits in the picture.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonPose {
    /// Left, top, right, bottom.
    #[serde(rename = "box")]
    pub bbox: [f64; 4],
    pub score: f64,
    /// Nose, eyes, ears, shoulders, elbows, wrists, hips, knees, ankles, in
    /// COCO order: x, y and how sure the model is of the point.
    pub keypoints: Vec<[f64; 3]>,
}

impl Segment {
    /// The usable range, when there is one.
    pub fn usable(&self) -> Option<(Micros, Micros)> {
        match (self.usable_start_us, self.usable_end_us) {
            (Some(start), Some(end)) if end > start => Some((start, end)),
            _ => None,
        }
    }
}

/// How the camera moved over a segment's usable part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CameraMotion {
    /// Held still, on a tripod or a steady hand.
    Still,
    /// Moving smoothly: walking with the subject, a slow drift.
    Steady,
    /// Sweeping across the view.
    Panning,
    Shaky,
}

/// One measurement of a shot, taken as the analysis swept through it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSample {
    pub at_us: Micros,
    /// How usable the picture is here, 0 to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    /// How much the picture changes once the camera's own movement is taken
    /// out, 0 to 1. The low points are the rests between actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<f64>,
    /// How fast the camera moves here, as a share of the frame a second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<f64>,
}

/// Segments that show one place or view, from any source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_ids: Vec<String>,
}

/// How different two segments' views look, from their signatures: 0 for
/// identical, and past about 0.03 a different scene.
///
/// Mean absolute difference, with the colour opponents counted three times
/// over: brightness moves with exposure and time of day, while the colour
/// of a place is what stays put.
pub fn view_distance(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 1.0;
    }
    let grid = a.len().saturating_sub(8);
    let mut total = 0.0;
    let mut weight = 0.0;
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let w = if i >= grid {
            0.5
        } else if i % 3 == 0 {
            1.0
        } else {
            3.0
        };
        total += w * (x - y).abs();
        weight += w;
    }
    total / weight
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keyframe {
    pub id: String,
    pub at_us: Micros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scores: Option<KeyframeScores>,
}

impl Keyframe {
    pub fn aggregate_score(&self) -> f64 {
        self.scores.as_ref().map_or(0.0, KeyframeScores::aggregate)
    }
}

/// Classical-CV selection metrics. Computed without invoking any model, which
/// is the point: frame-level differentiation is cheap CV work, and the LLM only
/// ever sees the compressed result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyframeScores {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharpness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_jitter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_quality: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<f64>,
}

impl KeyframeScores {
    /// Weighted roll-up. Uses the stored aggregate when the scorer computed
    /// one, otherwise derives it from whichever components are present so a
    /// partially-scored keyframe still ranks sensibly.
    ///
    /// `flow_jitter` is inverted: steadier frames should rank higher.
    pub fn aggregate(&self) -> f64 {
        if let Some(a) = self.aggregate {
            return a.clamp(0.0, 1.0);
        }
        let parts: [(Option<f64>, f64); 5] = [
            (self.sharpness, 0.40),
            (self.flow_jitter.map(|j| 1.0 - j), 0.25),
            (self.face_quality, 0.15),
            (self.composition, 0.10),
            (self.exposure, 0.10),
        ];
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (value, w) in parts {
            if let Some(v) = value {
                sum += v.clamp(0.0, 1.0) * w;
                weight += w;
            }
        }
        if weight == 0.0 { 0.0 } else { sum / weight }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    Person,
    Animal,
    Object,
    Place,
    Text,
    Other,
}

/// A persistent cross-shot identity. The same `id` means the same subject
/// everywhere it appears, which is what lets the narrative layer reason about
/// "the same person two shots later".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub id: String,
    pub kind: EntityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_us: Option<Micros>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shot_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker: Option<String>,
    /// A still of this subject, relative to the analysis work directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    /// An appearance embedding for grouping the same subject across clips and
    /// projects. A similarity measure, never an identity claim.
    ///
    /// `f64` because that is what a JSON number is: an `f32` here would come
    /// back from a round trip as a different number, so reading and writing
    /// a graph would quietly change it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signature: Vec<f64>,
}

/// How unalike two appearance signatures are: 0 for the same direction,
/// 1 for unrelated, 2 for opposite.
///
/// Compared by direction rather than magnitude, so the answer does not move
/// with exposure or image size. Signatures of different lengths come from
/// different models and are never comparable -- treating them as comparable
/// by truncation would group strangers.
pub fn signature_distance<T: Copy + Into<f64>>(a: &[T], b: &[T]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return f32::MAX;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b) {
        let (x, y): (f64, f64) = ((*x).into(), (*y).into());
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return f32::MAX;
    }
    (1.0 - dot / (norm_a.sqrt() * norm_b.sqrt())) as f32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub verb: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_entity_id: Option<String>,
    pub start_us: Micros,
    pub end_us: Micros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotAudio {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_speech: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music_energy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub source_id: String,
    pub start_us: Micros,
    pub end_us: Micros,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinedPriors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_length: Option<ShotLengthPrior>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beat_grid: Option<BeatGrid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_frequency: Option<std::collections::BTreeMap<String, f64>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotLengthPrior {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p10_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90_us: Option<Micros>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatGrid {
    pub bpm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_beat_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<BeatOrigin>,
    /// The music the grid was mined from, when the user chose some.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
    /// How long that music runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_duration_us: Option<Micros>,
}

impl BeatGrid {
    /// One beat, in microseconds.
    pub fn beat_us(&self) -> Option<Micros> {
        (self.bpm.is_finite() && self.bpm > 0.0)
            .then(|| Micros((60_000_000.0 / self.bpm).round() as u64))
    }
}

/// Where a beat grid came from.
///
/// A fixed grid is a pacing choice made because there was nothing to listen
/// to, and must never be reported as if it had been detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BeatOrigin {
    Music,
    Source,
    Fixed,
}

/// Pipeline stages, in the order they run. The discriminant order matters:
/// invalidating a stage invalidates everything after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Probe,
    ShotDetect,
    Keyframes,
    CvScore,
    Director,
    Asr,
    Describe,
    Track,
    MinePriors,
    /// The composition judge's second look at a cut's shortlist, after the
    /// sources are merged.
    Judge,
    /// Body poses through the moments a dance cut may join, after the
    /// sources are merged.
    Pose,
    /// A sentence about each moment a cut captions, after the cut is made.
    Caption,
}

impl Stage {
    pub const ALL: [Stage; 12] = [
        Stage::Probe,
        Stage::ShotDetect,
        Stage::Keyframes,
        Stage::CvScore,
        Stage::Director,
        Stage::Asr,
        Stage::Describe,
        Stage::Track,
        Stage::MinePriors,
        Stage::Judge,
        Stage::Pose,
        Stage::Caption,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Probe => "probe",
            Stage::ShotDetect => "shotDetect",
            Stage::Keyframes => "keyframes",
            Stage::CvScore => "cvScore",
            Stage::Director => "director",
            Stage::Asr => "asr",
            Stage::Describe => "describe",
            Stage::Track => "track",
            Stage::MinePriors => "minePriors",
            Stage::Judge => "judge",
            Stage::Pose => "pose",
            Stage::Caption => "caption",
        }
    }

    /// Stages invalidated when this one is, itself included.
    pub fn downstream(self) -> impl Iterator<Item = Stage> {
        Stage::ALL.into_iter().filter(move |s| *s >= self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceEntry {
    pub stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    /// Which adapter implementation ran. Mock output must never be mistaken
    /// for real output, so this is written for every stage, always.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    pub params_hash: Blake3Hash,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_both_idle_and_watching_is_stopped_once() {
        let span = |a: u64, b: u64| Span {
            start_us: Micros(a),
            end_us: Micros(b),
        };
        let rhythm = Rhythm {
            bpm: 100.0,
            idle: vec![span(0, 4), span(10, 12)],
            watching: vec![span(2, 6), span(11, 20)],
            ..Rhythm::default()
        };
        // 0-6 and 10-20.
        assert_eq!(rhythm.stopped(), Micros(16));
    }

    #[test]
    fn shot_containment_is_half_open() {
        // Adjacent shots must tile a source without a timecode belonging to
        // both, or the grounding gate would accept an ambiguous cut point.
        let shot = Shot {
            id: "s1".into(),
            source_id: "src1".into(),
            start_us: Micros::from_secs(10),
            end_us: Micros::from_secs(20),
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: vec![],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: Vec::new(),
            timeline: Vec::new(),
        };
        assert!(shot.contains(Micros::from_secs(10)));
        assert!(shot.contains(Micros::from_secs(19)));
        assert!(!shot.contains(Micros::from_secs(20)));
        assert!(!shot.contains(Micros::from_secs(9)));
    }

    #[test]
    fn partial_scores_still_aggregate() {
        let scores = KeyframeScores {
            sharpness: Some(0.8),
            ..Default::default()
        };
        // Only one component present, so the roll-up is that component, not a
        // value dragged toward zero by absent ones.
        assert!((scores.aggregate() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn flow_jitter_is_inverted_in_the_roll_up() {
        let steady = KeyframeScores {
            flow_jitter: Some(0.0),
            ..Default::default()
        };
        let shaky = KeyframeScores {
            flow_jitter: Some(1.0),
            ..Default::default()
        };
        assert!(steady.aggregate() > shaky.aggregate());
    }

    #[test]
    fn signatures_compare_by_direction_and_refuse_mismatched_lengths() {
        let a = [0.1f32, 0.2, 0.3];
        assert!(signature_distance(&a, &a) < 1e-6);
        // Twice as long, same direction: the same subject photographed
        // brighter must not read as a different one.
        let brighter: Vec<f32> = a.iter().map(|x| x * 2.0).collect();
        assert!(signature_distance(&a, &brighter) < 1e-6);
        // Different models produce different lengths, which are not
        // comparable at all.
        assert_eq!(signature_distance(&a, &[0.1f32, 0.2]), f32::MAX);
        assert_eq!(signature_distance(&[0.0f32, 0.0, 0.0], &a), f32::MAX);
    }

    #[test]
    fn downstream_invalidation_includes_self_and_later_stages() {
        let d: Vec<_> = Stage::Keyframes.downstream().collect();
        assert!(d.contains(&Stage::Keyframes));
        assert!(d.contains(&Stage::CvScore));
        assert!(d.contains(&Stage::MinePriors));
        assert!(!d.contains(&Stage::Probe));
        assert!(!d.contains(&Stage::ShotDetect));
    }
}
