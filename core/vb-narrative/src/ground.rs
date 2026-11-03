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

//! The grounding gate.
//!
//! An LLM asked to plan a cut will confidently reference footage that does not
//! exist: a timecode past the end of a clip, a shot id it invented, a person
//! who never appears. Nothing downstream can catch this. The renderer would
//! happily seek to a timestamp beyond the file and emit black frames, and the
//! user would see a broken edit with no explanation.
//!
//! So every instruction is checked against the Footage Graph before any DSL
//! leaves this layer. **Failures are rejected and recorded, never repaired.**
//! Quietly clamping a bad timecode into range would turn a detectable
//! hallucination into a silently wrong edit, which is worse: the system would
//! look like it was working.

use std::collections::HashSet;
use vb_contracts::editing_dsl::{
    EditingDsl, GroundingRecord, GroundingStatus, SlotKind, TransitionType,
};
use vb_contracts::{FootageGraph, Micros};

/// Tolerance when checking a slot's declared duration against its source
/// range. Roughly a frame and a half at 30fps, which absorbs rounding without
/// letting a real mismatch through.
const DURATION_TOLERANCE: Micros = Micros(50_000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub slot_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct GroundingReport {
    pub verified: Vec<String>,
    pub rejected: Vec<Rejection>,
    /// Transitions dropped because they referenced a rejected slot or were
    /// internally inconsistent.
    pub dropped_transitions: usize,
    /// Track items dropped because their anchor slot is gone.
    pub dropped_track_items: usize,
}

impl GroundingReport {
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty()
    }

    /// Fraction of slots that survived. A low value means the orchestrator is
    /// hallucinating and the run should not be presented as a usable cut.
    pub fn pass_rate(&self) -> f64 {
        let total = self.verified.len() + self.rejected.len();
        if total == 0 {
            return 1.0;
        }
        self.verified.len() as f64 / total as f64
    }
}

/// Check one slot against the graph. `None` means it is sound.
fn check_slot(slot: &vb_contracts::Slot, graph: &FootageGraph) -> Option<String> {
    if slot.duration_us == Micros::ZERO {
        return Some("slot has zero duration".into());
    }

    match slot.kind {
        // A title card or gap invents no footage, so there is nothing to
        // ground. Its duration was already checked above.
        SlotKind::TitleCard | SlotKind::Gap => {
            if slot.source_ref.is_some() && slot.kind == SlotKind::Gap {
                return Some("a gap must not reference source footage".into());
            }
            None
        }
        SlotKind::Clip => {
            let Some(src_ref) = &slot.source_ref else {
                return Some("clip slot has no sourceRef".into());
            };

            let Some(source) = graph.source(&src_ref.source_id) else {
                return Some(format!(
                    "sourceId {:?} does not exist in the footage graph",
                    src_ref.source_id
                ));
            };

            if src_ref.in_us >= src_ref.out_us {
                return Some(format!(
                    "in point {} is not before out point {}",
                    src_ref.in_us, src_ref.out_us
                ));
            }

            if src_ref.out_us > source.duration_us {
                return Some(format!(
                    "out point {} is past the end of the source ({})",
                    src_ref.out_us, source.duration_us
                ));
            }

            // A named shot must exist, belong to this source, and actually
            // contain the range. This is the check that catches an invented
            // shot id, the most common hallucination.
            if let Some(shot_id) = &src_ref.shot_id {
                let Some(shot) = graph.shot(shot_id) else {
                    return Some(format!(
                        "shotId {shot_id:?} does not exist in the footage graph"
                    ));
                };
                if shot.source_id != src_ref.source_id {
                    return Some(format!(
                        "shot {shot_id:?} belongs to source {:?}, not {:?}",
                        shot.source_id, src_ref.source_id
                    ));
                }
                if !shot.contains(src_ref.in_us) {
                    return Some(format!(
                        "in point {} lies outside shot {shot_id:?} ({} to {})",
                        src_ref.in_us, shot.start_us, shot.end_us
                    ));
                }
                if src_ref.out_us > shot.end_us {
                    return Some(format!(
                        "out point {} runs past the end of shot {shot_id:?} ({})",
                        src_ref.out_us, shot.end_us
                    ));
                }
            }

            // The declared duration must match the source range, adjusted for
            // speed. A mismatch means the renderer and the DSL disagree about
            // how long the clip is, which desynchronises everything after it.
            let speed = slot.speed.unwrap_or(1.0);
            if speed <= 0.0 || !speed.is_finite() {
                return Some(format!("speed {speed} is not a positive finite number"));
            }
            let expected = (src_ref.duration().0 as f64 / speed).round() as u64;
            let declared = slot.duration_us.0;
            if declared.abs_diff(expected) > DURATION_TOLERANCE.0 {
                return Some(format!(
                    "declared duration {} does not match the source range at speed {speed} \
                     (expected about {})",
                    slot.duration_us,
                    Micros(expected)
                ));
            }

            None
        }
    }
}

/// Where a slot's picture is placed must be a placement: keyed inside the
/// slot, in order, enlarged by a real amount, every number finite.
fn check_transform(slot: &vb_contracts::Slot, dsl: &EditingDsl) -> Option<String> {
    let transform = dsl
        .tracks
        .video
        .iter()
        .find(|v| v.slot_id == slot.id)?
        .transform
        .as_ref()?;
    if transform.keyframes.is_empty() {
        return Some("a transform with no keyframes".into());
    }
    let mut last = Micros::ZERO;
    for k in &transform.keyframes {
        if k.offset_us > slot.duration_us {
            return Some(format!(
                "a transform keyframe at {} lies past the slot's end ({})",
                k.offset_us, slot.duration_us
            ));
        }
        if k.offset_us < last {
            return Some("transform keyframes out of order".into());
        }
        last = k.offset_us;
        if !(k.scale.is_finite() && k.scale > 0.0)
            || !k.x.is_finite()
            || !k.y.is_finite()
            || !k.rotation_deg.is_finite()
        {
            return Some(format!(
                "a transform keyframe at {} is not a placement",
                k.offset_us
            ));
        }
    }
    None
}

/// Verify every decision in the DSL against the footage it claims to use.
///
/// Rejected slots are removed, along with any transition or track item that
/// referenced them, and a [`GroundingRecord`] is written for every slot so the
/// rejection is visible rather than silent.
pub fn ground(dsl: &mut EditingDsl, graph: &FootageGraph) -> GroundingReport {
    let mut report = GroundingReport::default();
    let mut records: Vec<GroundingRecord> = Vec::with_capacity(dsl.slots.len());
    let mut keep: HashSet<String> = HashSet::new();

    for slot in &dsl.slots {
        let verdict = check_slot(slot, graph).or_else(|| check_transform(slot, dsl));
        match verdict {
            None => {
                let resolved_shot = slot.source_ref.as_ref().and_then(|r| r.shot_id.clone());
                let entities = resolved_shot
                    .as_deref()
                    .and_then(|id| graph.shot(id))
                    .map(|s| s.entity_refs.clone())
                    .unwrap_or_default();

                records.push(GroundingRecord {
                    slot_id: slot.id.clone(),
                    status: GroundingStatus::Verified,
                    resolved_shot_id: resolved_shot,
                    resolved_entity_ids: entities,
                    rejection_reason: None,
                });
                report.verified.push(slot.id.clone());
                keep.insert(slot.id.clone());
            }
            Some(reason) => {
                tracing::warn!(slot = %slot.id, %reason, "grounding rejected a slot");
                records.push(GroundingRecord {
                    slot_id: slot.id.clone(),
                    status: GroundingStatus::Rejected,
                    resolved_shot_id: None,
                    resolved_entity_ids: Vec::new(),
                    rejection_reason: Some(reason.clone()),
                });
                report.rejected.push(Rejection {
                    slot_id: slot.id.clone(),
                    reason,
                });
            }
        }
    }

    dsl.slots.retain(|s| keep.contains(&s.id));

    // A transition to a slot that no longer exists would leave the renderer
    // deriving overlap against nothing.
    let before = dsl.transitions.len();
    dsl.transitions.retain(|t| {
        let endpoints_exist = keep.contains(&t.from_slot_id) && keep.contains(&t.to_slot_id);
        let duration_is_consistent = t.kind != TransitionType::Cut || t.duration_us == Micros::ZERO;
        endpoints_exist && duration_is_consistent
    });
    report.dropped_transitions = before - dsl.transitions.len();

    let before = dsl.tracks.video.len() + dsl.tracks.text.len() + dsl.tracks.audio.len();
    dsl.tracks.video.retain(|v| keep.contains(&v.slot_id));
    dsl.tracks.text.retain(|t| keep.contains(&t.anchor.slot_id));
    dsl.tracks
        .audio
        .retain(|a| keep.contains(&a.anchor.slot_id));
    let after = dsl.tracks.video.len() + dsl.tracks.text.len() + dsl.tracks.audio.len();
    report.dropped_track_items = before - after;

    dsl.grounding = records;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Blake3Hash;
    use vb_contracts::editing_dsl::{Anchor, Slot, SourceRef, TextItem, Transition};
    use vb_contracts::footage_graph::{Shot, Source};

    fn graph() -> FootageGraph {
        let mut g = FootageGraph::new("t");
        g.sources.push(Source {
            id: "src".into(),
            path: "/x.mp4".into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros::from_secs(60),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: None,
            audio_codec: None,
            has_audio: Some(true),
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        });
        g.shots.push(Shot {
            id: "shot-1".into(),
            source_id: "src".into(),
            start_us: Micros::from_secs(10),
            end_us: Micros::from_secs(20),
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: vec!["ent-1".into()],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: Vec::new(),
            timeline: Vec::new(),
        });
        g
    }

    fn clip(id: &str, in_s: u64, out_s: u64, shot: Option<&str>) -> Slot {
        Slot {
            id: id.into(),
            order: 0,
            kind: SlotKind::Clip,
            duration_us: Micros::from_secs(out_s - in_s),
            source_ref: Some(SourceRef {
                source_id: "src".into(),
                shot_id: shot.map(str::to_string),
                in_us: Micros::from_secs(in_s),
                out_us: Micros::from_secs(out_s),
            }),
            speed: None,
            reason: None,
            score: None,
        }
    }

    fn dsl_with(slots: Vec<Slot>) -> EditingDsl {
        let mut d = EditingDsl::new("t");
        d.slots = slots;
        d
    }

    #[test]
    fn a_placement_keyed_past_its_slot_is_rejected_not_clamped() {
        use vb_contracts::editing_dsl::{Transform, TransformKeyframe, VideoItem};
        let framed = |offset_s: u64, scale: f64| VideoItem {
            slot_id: "a".into(),
            crop: None,
            filters: Vec::new(),
            transform: Some(Transform {
                keyframes: vec![
                    TransformKeyframe {
                        scale,
                        ..TransformKeyframe::fitted(Micros::ZERO)
                    },
                    TransformKeyframe::fitted(Micros::from_secs(offset_s)),
                ],
            }),
            background: None,
        };
        // Keyed within its six seconds: fine.
        let mut d = dsl_with(vec![clip("a", 12, 18, Some("shot-1"))]);
        d.tracks.video.push(framed(6, 1.2));
        assert!(ground(&mut d, &graph()).is_clean());
        // Keyed at nine: a placement the slot never reaches.
        let mut d = dsl_with(vec![clip("a", 12, 18, Some("shot-1"))]);
        d.tracks.video.push(framed(9, 1.2));
        let r = ground(&mut d, &graph());
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reason.contains("past the slot's end"));
        // Enlarged by nothing at all.
        let mut d = dsl_with(vec![clip("a", 12, 18, Some("shot-1"))]);
        d.tracks.video.push(framed(6, 0.0));
        assert_eq!(ground(&mut d, &graph()).rejected.len(), 1);
    }

    #[test]
    fn a_sound_clip_is_verified_and_carries_its_entities() {
        let mut d = dsl_with(vec![clip("a", 12, 18, Some("shot-1"))]);
        let r = ground(&mut d, &graph());
        assert!(r.is_clean());
        assert_eq!(d.slots.len(), 1);
        assert_eq!(d.grounding[0].status, GroundingStatus::Verified);
        assert_eq!(d.grounding[0].resolved_entity_ids, vec!["ent-1"]);
    }

    #[test]
    fn an_invented_shot_id_is_rejected() {
        // The most common hallucination: a shot that was never in the graph.
        let mut d = dsl_with(vec![clip("a", 12, 18, Some("shot-99"))]);
        let r = ground(&mut d, &graph());
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reason.contains("does not exist"));
        assert!(
            d.slots.is_empty(),
            "a rejected slot must not reach the renderer"
        );
    }

    #[test]
    fn a_timecode_outside_its_shot_is_rejected_not_clamped() {
        // Shot 1 runs 10s to 20s; this asks for 5s to 8s inside it.
        let mut d = dsl_with(vec![clip("a", 5, 8, Some("shot-1"))]);
        let r = ground(&mut d, &graph());
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reason.contains("outside shot"));
        // The point of the gate: the slot is gone, not silently moved into range.
        assert!(d.slots.is_empty());
    }

    #[test]
    fn a_clip_running_past_the_end_of_the_source_is_rejected() {
        let mut d = dsl_with(vec![clip("a", 55, 70, None)]);
        let r = ground(&mut d, &graph());
        assert_eq!(r.rejected.len(), 1);
        assert!(r.rejected[0].reason.contains("past the end"));
    }

    #[test]
    fn an_unknown_source_is_rejected() {
        let mut slot = clip("a", 1, 5, None);
        slot.source_ref.as_mut().unwrap().source_id = "nope".into();
        let mut d = dsl_with(vec![slot]);
        assert_eq!(ground(&mut d, &graph()).rejected.len(), 1);
    }

    #[test]
    fn an_inverted_range_is_rejected() {
        let mut slot = clip("a", 10, 15, None);
        let sr = slot.source_ref.as_mut().unwrap();
        sr.in_us = Micros::from_secs(15);
        sr.out_us = Micros::from_secs(10);
        let mut d = dsl_with(vec![slot]);
        let r = ground(&mut d, &graph());
        assert!(r.rejected[0].reason.contains("not before"));
    }

    #[test]
    fn a_duration_that_disagrees_with_its_source_range_is_rejected() {
        // The renderer would cut 6s of footage while the timeline reserved 20s,
        // desynchronising everything after it.
        let mut slot = clip("a", 12, 18, Some("shot-1"));
        slot.duration_us = Micros::from_secs(20);
        let mut d = dsl_with(vec![slot]);
        let r = ground(&mut d, &graph());
        assert!(r.rejected[0].reason.contains("does not match"));
    }

    #[test]
    fn speed_is_accounted_for_when_checking_duration() {
        // 6s of footage at 2x occupies 3s of timeline, which is correct.
        let mut slot = clip("a", 12, 18, Some("shot-1"));
        slot.speed = Some(2.0);
        slot.duration_us = Micros::from_secs(3);
        let mut d = dsl_with(vec![slot]);
        assert!(ground(&mut d, &graph()).is_clean());
    }

    #[test]
    fn a_title_card_grounds_without_any_footage() {
        let mut d = dsl_with(vec![Slot {
            id: "t".into(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: Micros::from_secs(2),
            source_ref: None,
            speed: None,
            reason: None,
            score: None,
        }]);
        assert!(ground(&mut d, &graph()).is_clean());
    }

    #[test]
    fn a_zero_length_slot_is_rejected() {
        let mut slot = clip("a", 12, 18, None);
        slot.duration_us = Micros::ZERO;
        let mut d = dsl_with(vec![slot]);
        assert_eq!(ground(&mut d, &graph()).rejected.len(), 1);
    }

    #[test]
    fn rejecting_a_slot_also_removes_what_referenced_it() {
        // Orphaned transitions and subtitles would otherwise point at nothing.
        let mut d = dsl_with(vec![
            clip("good", 12, 18, Some("shot-1")),
            clip("bad", 12, 18, Some("shot-99")),
        ]);
        d.transitions.push(Transition {
            from_slot_id: "good".into(),
            to_slot_id: "bad".into(),
            kind: TransitionType::Xfade,
            duration_us: Micros::from_millis(500),
        });
        d.tracks.text.push(TextItem {
            id: "sub".into(),
            anchor: Anchor::at("bad"),
            duration_us: Micros::from_secs(2),
            content: "orphan".into(),
            role: None,
            transcript_segment_id: None,
            style: None,
        });

        let r = ground(&mut d, &graph());
        assert_eq!(r.rejected.len(), 1);
        assert_eq!(r.dropped_transitions, 1);
        assert_eq!(r.dropped_track_items, 1);
        assert!(d.transitions.is_empty());
        assert!(d.tracks.text.is_empty());
    }

    #[test]
    fn a_cut_transition_claiming_a_duration_is_dropped() {
        let mut d = dsl_with(vec![
            clip("a", 12, 18, Some("shot-1")),
            clip("b", 12, 18, Some("shot-1")),
        ]);
        d.transitions.push(Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Cut,
            duration_us: Micros::from_secs(1),
        });
        let r = ground(&mut d, &graph());
        assert_eq!(r.dropped_transitions, 1);
    }

    #[test]
    fn the_pass_rate_reports_how_much_was_hallucinated() {
        let mut d = dsl_with(vec![
            clip("a", 12, 18, Some("shot-1")),
            clip("b", 12, 18, Some("shot-99")),
            clip("c", 12, 18, Some("shot-98")),
        ]);
        let r = ground(&mut d, &graph());
        assert!((r.pass_rate() - 1.0 / 3.0).abs() < 1e-9);
    }
}
