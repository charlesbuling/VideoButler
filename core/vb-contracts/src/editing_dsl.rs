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

//! Output Contract: `editing-dsl/0.1`.
//!
//! A decision manifest, never a flattened render. This document *is* the
//! non-destructive guarantee: every cut, subtitle, transition and music choice
//! is a reversible record, so the user can change any of it without re-running
//! the pipeline.
//!
//! # No absolute timeline positions
//!
//! Nothing here records where a slot lands on the finished timeline. Slots
//! declare order and duration; text and audio anchor to `{slot_id, offset}`.
//! Absolute time is *derived*, by [`EditingDsl::resolve_timeline`], which walks
//! the spine subtracting transition overlap.
//!
//! This is deliberate. The previous implementation stored absolute positions
//! and they drifted, because a transition makes two slots overlap and a title
//! card inserts time that the sum of preceding durations does not predict.
//! Storing only relative position means the drift has nowhere to come from.
//! Adding a `timeline_start_us` field here would reintroduce the bug.

use crate::hash::Blake3Hash;
use crate::time::Micros;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: &str = "editing-dsl/0.1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditingDsl {
    pub schema_version: String,
    pub generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footage_graph_ref: Option<FootageGraphRef>,
    pub meta: Meta,
    pub slots: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Tracks::is_empty")]
    pub tracks: Tracks,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<Transition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grounding: Vec<GroundingRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<DslProvenance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FootageGraphRef {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_hash: Option<Blake3Hash>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_duration_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_num: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_den: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotKind {
    Clip,
    TitleCard,
    Gap,
}

/// One position on the spine.
///
/// A slot knows its own duration and its source footage. It does not know, and
/// must not store, where it lands on the finished timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: String,
    pub order: u32,
    pub kind: SlotKind,
    pub duration_us: Micros,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Why the orchestrator chose this. Shown in the UI so the user can judge
    /// the cut, and carried into the feedback loop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shot_id: Option<String>,
    pub in_us: Micros,
    pub out_us: Micros,
}

impl SourceRef {
    pub fn duration(&self) -> Micros {
        self.out_us.saturating_sub(self.in_us)
    }
}

/// The only way anything expresses a time position: relative to a slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub slot_id: String,
    /// May be negative, to lead into the slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_us: Option<i64>,
}

impl Anchor {
    pub fn at(slot_id: impl Into<String>) -> Self {
        Anchor {
            slot_id: slot_id.into(),
            offset_us: None,
        }
    }

    pub fn offset(&self) -> i64 {
        self.offset_us.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracks {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video: Vec<VideoItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<TextItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<AudioItem>,
}

impl Tracks {
    pub fn is_empty(&self) -> bool {
        self.video.is_empty() && self.text.is_empty() && self.audio.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoItem {
    pub slot_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
    /// Where the picture sits in the frame, keyed at moments of the slot.
    /// Absent is the picture fitted to the frame, as the editor places it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    /// What fills the frame round a picture that does not cover it. Absent
    /// is black.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
}

impl VideoItem {
    /// An item for `slot_id` that changes nothing about its picture.
    pub fn plain(slot_id: impl Into<String>) -> Self {
        VideoItem {
            slot_id: slot_id.into(),
            crop: None,
            filters: Vec::new(),
            transform: None,
            background: None,
        }
    }

    /// Whether this item says anything a renderer must act on.
    pub fn is_plain(&self) -> bool {
        self.crop.is_none()
            && self.filters.is_empty()
            && self.transform.is_none()
            && self.background.is_none()
    }
}

/// What fills the frame round a picture that does not cover it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    pub kind: BackgroundKind,
    /// `#RRGGBB` or `#RRGGBBAA`, for [`BackgroundKind::Color`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundKind {
    Black,
    Color,
    /// The picture itself, enlarged to cover the frame and blurred.
    Blur,
}

/// How a clip's picture is enlarged, moved and turned in the frame: a
/// decision like any other in the manifest, never baked into pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    /// In order of offset; the picture eases in a straight line from one to
    /// the next, and one keyframe holds for the whole slot.
    pub keyframes: Vec<TransformKeyframe>,
}

/// The picture's placement at one moment of a slot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformKeyframe {
    /// From the slot's own start, on the finished piece's clock -- relative,
    /// like every time in this document.
    pub offset_us: Micros,
    /// 1 is the picture fitted to the frame.
    pub scale: f64,
    /// How far the picture's centre moves right, as a share of the frame's
    /// width.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: f64,
    /// How far the picture's centre moves down, as a share of the frame's
    /// height.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y: f64,
    /// Clockwise, in degrees, about the picture's centre.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation_deg: f64,
}

impl TransformKeyframe {
    /// The picture as the editor places it on its own.
    pub fn fitted(offset_us: Micros) -> Self {
        TransformKeyframe {
            offset_us,
            scale: 1.0,
            x: 0.0,
            y: 0.0,
            rotation_deg: 0.0,
        }
    }

    pub fn is_fitted(&self) -> bool {
        (self.scale - 1.0).abs() < 1e-9
            && self.x.abs() < 1e-9
            && self.y.abs() < 1e-9
            && self.rotation_deg.abs() < 1e-9
    }

    /// The same placement, as close as two keyframes can be.
    pub fn same_placement(&self, other: &Self) -> bool {
        (self.scale - other.scale).abs() < 1e-9
            && (self.x - other.x).abs() < 1e-9
            && (self.y - other.y).abs() < 1e-9
            && (self.rotation_deg - other.rotation_deg).abs() < 1e-9
    }
}

impl Transform {
    /// The placement at `offset` into the slot: the keyframe there, or the
    /// straight line between the two either side, or the nearest end.
    pub fn at(&self, offset: Micros) -> Option<TransformKeyframe> {
        let first = self.keyframes.first()?;
        if offset <= first.offset_us {
            return Some(TransformKeyframe {
                offset_us: offset,
                ..*first
            });
        }
        for pair in self.keyframes.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if offset <= b.offset_us {
                let span = b.offset_us.0.saturating_sub(a.offset_us.0).max(1) as f64;
                let f = (offset.0.saturating_sub(a.offset_us.0)) as f64 / span;
                let mix = |p: f64, q: f64| p + (q - p) * f;
                return Some(TransformKeyframe {
                    offset_us: offset,
                    scale: mix(a.scale, b.scale),
                    x: mix(a.x, b.x),
                    y: mix(a.y, b.y),
                    rotation_deg: mix(a.rotation_deg, b.rotation_deg),
                });
            }
        }
        let last = self.keyframes.last()?;
        Some(TransformKeyframe {
            offset_us: offset,
            ..*last
        })
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Normalised crop rectangle, so it survives a resolution change.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextRole {
    Subtitle,
    Title,
    LowerThird,
    Caption,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextItem {
    pub id: String,
    pub anchor: Anchor,
    pub duration_us: Micros,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<TextRole>,
    /// Set when this text came from ASR, so a subtitle traces back to the
    /// utterance it transcribes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_segment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_v_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// A box behind the text, `#RRGGBB` or `#RRGGBBAA`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    /// Where the text's centre sits across the frame, 0..1, once the user
    /// placed it; with `y` it overrides the place `alignment` gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Where the text's centre sits down the frame, 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioKind {
    Source,
    Bgm,
    Sfx,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioItem {
    pub id: String,
    pub anchor: Anchor,
    pub kind: AudioKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_path: Option<String>,
    /// Where in the asset playback starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_in_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    /// Silent, with `gain_db` kept so that unmuting restores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_in_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_out_us: Option<Micros>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ducking: Option<Ducking>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ducking {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_gain_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_db: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitionType {
    Cut,
    Xfade,
    FadeToBlack,
    WipeLeft,
    WipeRight,
    Dissolve,
}

/// Joins two adjacent slots. `duration_us` is the overlap the renderer
/// subtracts when deriving absolute time -- the exact quantity that caused
/// the drift the old implementation fought.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub from_slot_id: String,
    pub to_slot_id: String,
    #[serde(rename = "type")]
    pub kind: TransitionType,
    pub duration_us: Micros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroundingStatus {
    Verified,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingRecord {
    pub slot_id: String,
    pub status: GroundingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_shot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_entity_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DslProvenance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_adapter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priors_applied: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params_hash: Option<Blake3Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// The built-in editor and its version, once a person changed the cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
    /// How much of the finished piece the person changed from the cut the
    /// orchestrator made, by footage time: 0 the cut as made, 1 none kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_ratio: Option<f64>,
}

// ---------------------------------------------------------------------------
// Timeline resolution
// ---------------------------------------------------------------------------

/// Where a slot actually lands once transition overlap is accounted for.
///
/// Produced by [`EditingDsl::resolve_timeline`]. Renderers and exporters
/// consume this; they must never compute absolute time themselves, or the two
/// implementations will disagree and drift will be back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSlot {
    pub slot_id: String,
    /// Absolute start on the finished timeline.
    pub start_us: Micros,
    /// Absolute end on the finished timeline.
    pub end_us: Micros,
    /// Overlap with the preceding slot, already subtracted from `start_us`.
    pub incoming_overlap_us: Micros,
}

impl ResolvedSlot {
    pub fn duration(&self) -> Micros {
        self.end_us.saturating_sub(self.start_us)
    }
}

/// The resolved timeline: every slot's absolute placement, plus the total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub slots: Vec<ResolvedSlot>,
    pub total_duration_us: Micros,
}

impl Timeline {
    pub fn slot(&self, slot_id: &str) -> Option<&ResolvedSlot> {
        self.slots.iter().find(|s| s.slot_id == slot_id)
    }

    /// Resolve an anchor to an absolute timeline position.
    ///
    /// Returns `None` for an anchor naming a slot that does not exist, which
    /// is a malformed DSL rather than something to paper over. A negative
    /// offset that would place the item before zero clamps to zero.
    pub fn resolve_anchor(&self, anchor: &Anchor) -> Option<Micros> {
        let slot = self.slot(&anchor.slot_id)?;
        let base = slot.start_us.0 as i64;
        Some(Micros(base.saturating_add(anchor.offset()).max(0) as u64))
    }
}

impl EditingDsl {
    pub fn new(title: impl Into<String>) -> Self {
        EditingDsl {
            schema_version: SCHEMA_VERSION.to_string(),
            generated_at: crate::now_rfc3339(),
            footage_graph_ref: None,
            meta: Meta {
                title: title.into(),
                target_duration_us: None,
                aspect_ratio: None,
                fps_num: None,
                fps_den: None,
                template_id: None,
                variant_id: None,
                locale: None,
            },
            slots: Vec::new(),
            tracks: Tracks::default(),
            transitions: Vec::new(),
            grounding: Vec::new(),
            provenance: None,
        }
    }

    /// Slots in spine order.
    pub fn ordered_slots(&self) -> Vec<&Slot> {
        let mut slots: Vec<&Slot> = self.slots.iter().collect();
        slots.sort_by_key(|s| s.order);
        slots
    }

    pub fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// The transition joining two specific adjacent slots, if any.
    fn transition_between(&self, from: &str, to: &str) -> Option<&Transition> {
        self.transitions
            .iter()
            .find(|t| t.from_slot_id == from && t.to_slot_id == to)
    }

    /// Derive absolute timeline positions from the spine.
    ///
    /// **This is the only place absolute time is computed.** Walk the slots in
    /// order; each one starts where the previous ended, pulled back by the
    /// overlap of the transition joining them. A cut has zero overlap, so
    /// slots abut. An overlap is clamped to half of the shorter adjacent slot,
    /// because a transition longer than that would consume a neighbour
    /// entirely and reorder the spine.
    pub fn resolve_timeline(&self) -> Timeline {
        let ordered = self.ordered_slots();
        let mut resolved: Vec<ResolvedSlot> = Vec::with_capacity(ordered.len());
        let mut cursor = Micros::ZERO;

        for (i, slot) in ordered.iter().enumerate() {
            let overlap = if i == 0 {
                Micros::ZERO
            } else {
                let prev = ordered[i - 1];
                match self.transition_between(&prev.id, &slot.id) {
                    Some(t) if t.kind != TransitionType::Cut => {
                        let cap = prev.duration_us.0.min(slot.duration_us.0) / 2;
                        Micros(t.duration_us.0.min(cap))
                    }
                    _ => Micros::ZERO,
                }
            };

            let start = cursor.saturating_sub(overlap);
            let end = start + slot.duration_us;
            resolved.push(ResolvedSlot {
                slot_id: slot.id.clone(),
                start_us: start,
                end_us: end,
                incoming_overlap_us: overlap,
            });
            cursor = end;
        }

        Timeline {
            slots: resolved,
            total_duration_us: cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: &str, order: u32, secs: u64) -> Slot {
        Slot {
            id: id.into(),
            order,
            kind: SlotKind::Clip,
            duration_us: Micros::from_secs(secs),
            source_ref: Some(SourceRef {
                source_id: "src1".into(),
                shot_id: None,
                in_us: Micros::ZERO,
                out_us: Micros::from_secs(secs),
            }),
            speed: None,
            reason: None,
            score: None,
        }
    }

    fn dsl_with(slots: Vec<Slot>, transitions: Vec<Transition>) -> EditingDsl {
        let mut d = EditingDsl::new("t");
        d.slots = slots;
        d.transitions = transitions;
        d
    }

    #[test]
    fn slots_abut_when_there_are_no_transitions() {
        let d = dsl_with(vec![slot("a", 0, 5), slot("b", 1, 3)], vec![]);
        let tl = d.resolve_timeline();
        assert_eq!(tl.slots[0].start_us, Micros::ZERO);
        assert_eq!(tl.slots[1].start_us, Micros::from_secs(5));
        assert_eq!(tl.total_duration_us, Micros::from_secs(8));
    }

    #[test]
    fn a_transition_shortens_the_total_by_its_overlap() {
        // The drift bug in one test: with a 1s crossfade the finished piece is
        // 7s, not the 8s a naive sum of durations would predict.
        let d = dsl_with(
            vec![slot("a", 0, 5), slot("b", 1, 3)],
            vec![Transition {
                from_slot_id: "a".into(),
                to_slot_id: "b".into(),
                kind: TransitionType::Xfade,
                duration_us: Micros::from_secs(1),
            }],
        );
        let tl = d.resolve_timeline();
        assert_eq!(tl.slots[1].start_us, Micros::from_secs(4));
        assert_eq!(tl.total_duration_us, Micros::from_secs(7));
    }

    #[test]
    fn overlap_is_capped_at_half_the_shorter_neighbour() {
        // A 10s transition between a 5s and a 2s slot would otherwise swallow
        // the short slot whole and push its start before its predecessor's.
        let d = dsl_with(
            vec![slot("a", 0, 5), slot("b", 1, 2)],
            vec![Transition {
                from_slot_id: "a".into(),
                to_slot_id: "b".into(),
                kind: TransitionType::Xfade,
                duration_us: Micros::from_secs(10),
            }],
        );
        let tl = d.resolve_timeline();
        assert_eq!(tl.slots[1].incoming_overlap_us, Micros::from_secs(1));
        assert_eq!(tl.slots[1].start_us, Micros::from_secs(4));
    }

    #[test]
    fn a_cut_transition_contributes_no_overlap() {
        let d = dsl_with(
            vec![slot("a", 0, 5), slot("b", 1, 3)],
            vec![Transition {
                from_slot_id: "a".into(),
                to_slot_id: "b".into(),
                kind: TransitionType::Cut,
                duration_us: Micros::ZERO,
            }],
        );
        assert_eq!(d.resolve_timeline().total_duration_us, Micros::from_secs(8));
    }

    #[test]
    fn a_title_card_between_clips_shifts_everything_after_it() {
        // Title cards are the other half of the drift: they insert real time
        // that the clip durations alone do not account for.
        let mut title = slot("t", 1, 2);
        title.kind = SlotKind::TitleCard;
        title.source_ref = None;
        let d = dsl_with(vec![slot("a", 0, 5), title, slot("b", 2, 3)], vec![]);
        let tl = d.resolve_timeline();
        assert_eq!(tl.slot("b").unwrap().start_us, Micros::from_secs(7));
        assert_eq!(tl.total_duration_us, Micros::from_secs(10));
    }

    #[test]
    fn slots_resolve_in_order_field_order_not_array_order() {
        // The array is unordered; `order` is the spine. A DSL that arrives
        // shuffled must still resolve correctly.
        let d = dsl_with(vec![slot("b", 1, 3), slot("a", 0, 5)], vec![]);
        let tl = d.resolve_timeline();
        assert_eq!(tl.slots[0].slot_id, "a");
        assert_eq!(tl.slots[1].slot_id, "b");
    }

    #[test]
    fn anchors_resolve_against_the_derived_timeline() {
        let d = dsl_with(vec![slot("a", 0, 5), slot("b", 1, 3)], vec![]);
        let tl = d.resolve_timeline();
        let anchor = Anchor {
            slot_id: "b".into(),
            offset_us: Some(500_000),
        };
        assert_eq!(tl.resolve_anchor(&anchor), Some(Micros::from_millis(5_500)));
    }

    #[test]
    fn a_negative_anchor_offset_clamps_at_zero() {
        let d = dsl_with(vec![slot("a", 0, 5)], vec![]);
        let tl = d.resolve_timeline();
        let anchor = Anchor {
            slot_id: "a".into(),
            offset_us: Some(-9_000_000),
        };
        assert_eq!(tl.resolve_anchor(&anchor), Some(Micros::ZERO));
    }

    #[test]
    fn an_anchor_to_a_missing_slot_does_not_resolve() {
        let d = dsl_with(vec![slot("a", 0, 5)], vec![]);
        let tl = d.resolve_timeline();
        assert_eq!(tl.resolve_anchor(&Anchor::at("nope")), None);
    }
}
