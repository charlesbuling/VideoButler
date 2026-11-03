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

//! OpenTimelineIO export.
//!
//! OTIO is the primary target because it is the format that keeps the product
//! from being locked to one editor: it opens in professional tools, it is the
//! only option on Linux where CapCut does not exist, and it converts onward to
//! FCPXML.
//!
//! # One semantic difference worth knowing
//!
//! OTIO treats a transition as an overlay marker: the clips either side keep
//! their full durations, and the timeline is their sum. Our renderer instead
//! *overlaps* clips by the transition duration, so its output is shorter. Both
//! are correct in their own model, so the clips are written at full length,
//! per OTIO's semantics, and the renderer's resolved duration is recorded in
//! the timeline metadata. Nothing is lost and neither model is misrepresented.

use crate::{DraftWriter, ExportError, file_url};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use vb_contracts::editing_dsl::{SlotKind, TransitionType};
use vb_contracts::{EditingDsl, FootageGraph, FrameRate, Micros};

#[derive(Debug, Default, Clone)]
pub struct OtioWriter;

/// An OTIO `RationalTime`, in frames at the project rate.
///
/// Times are carried as exact frame counts rather than seconds, because that
/// is what editors cut on and what makes a round trip lossless.
fn rational_time(t: Micros, rate: FrameRate) -> Value {
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": rate.as_f64(),
        "value": t.to_frames(rate.num, rate.den) as f64
    })
}

fn time_range(start: Micros, duration: Micros, rate: FrameRate) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational_time(start, rate),
        "duration": rational_time(duration, rate)
    })
}

fn otio_transition_name(kind: TransitionType) -> &'static str {
    match kind {
        TransitionType::Cut => "Cut",
        TransitionType::Xfade | TransitionType::Dissolve => "SMPTE_Dissolve",
        TransitionType::FadeToBlack => "Fade_To_Black",
        TransitionType::WipeLeft => "SMPTE_Wipe_Left",
        TransitionType::WipeRight => "SMPTE_Wipe_Right",
    }
}

impl OtioWriter {
    /// Build the OTIO document as JSON.
    ///
    /// Separated from writing it so tests can inspect the structure without
    /// touching the filesystem.
    pub fn build(&self, dsl: &EditingDsl, graph: &FootageGraph) -> Result<Value, ExportError> {
        let rate = match (dsl.meta.fps_num, dsl.meta.fps_den) {
            (Some(n), Some(d)) if n > 0 && d > 0 => FrameRate::new(n, d),
            _ => graph
                .sources
                .first()
                .map(|s| s.frame_rate())
                .unwrap_or_default(),
        };

        let ordered = dsl.ordered_slots();
        let mut video_children: Vec<Value> = Vec::with_capacity(ordered.len() * 2);

        for (i, slot) in ordered.iter().enumerate() {
            // A transition sits between the two clips it joins, which is how
            // OTIO represents it.
            if i > 0 {
                let previous = ordered[i - 1];
                if let Some(t) = dsl
                    .transitions
                    .iter()
                    .find(|t| t.from_slot_id == previous.id && t.to_slot_id == slot.id)
                {
                    if t.kind != TransitionType::Cut && t.duration_us > Micros::ZERO {
                        let half = Micros(t.duration_us.0 / 2);
                        video_children.push(json!({
                            "OTIO_SCHEMA": "Transition.1",
                            "name": format!("{} to {}", previous.id, slot.id),
                            "transition_type": otio_transition_name(t.kind),
                            "in_offset": rational_time(half, rate),
                            "out_offset": rational_time(half, rate),
                            "metadata": { "videoButler": { "durationUs": t.duration_us.0 } }
                        }));
                    }
                }
            }

            match slot.kind {
                SlotKind::Clip => {
                    let src_ref =
                        slot.source_ref
                            .as_ref()
                            .ok_or_else(|| ExportError::DanglingSource {
                                slot_id: slot.id.clone(),
                                source_id: "<none>".into(),
                            })?;
                    let source = graph.source(&src_ref.source_id).ok_or_else(|| {
                        ExportError::DanglingSource {
                            slot_id: slot.id.clone(),
                            source_id: src_ref.source_id.clone(),
                        }
                    })?;

                    // A clip played faster or slower: OTIO's own time warp,
                    // the clip as long on the track as it plays, its media
                    // run through at the warp's rate.
                    let speed = slot
                        .speed
                        .filter(|v| v.is_finite() && *v > 0.0 && (*v - 1.0).abs() > 1e-9);
                    let (length, effects) = match speed {
                        Some(speed) => (
                            slot.duration_us,
                            json!([{
                                "OTIO_SCHEMA": "LinearTimeWarp.1",
                                "name": "speed",
                                "effect_name": "LinearTimeWarp",
                                "time_scalar": speed,
                                "metadata": {}
                            }]),
                        ),
                        None => (src_ref.duration(), json!([])),
                    };
                    // Where the picture sits, when the cut moved it: OTIO
                    // has no transform of its own, so it travels as data.
                    let transform = dsl
                        .tracks
                        .video
                        .iter()
                        .find(|v| v.slot_id == slot.id)
                        .and_then(|v| v.transform.as_ref())
                        .and_then(|t| serde_json::to_value(t).ok());
                    video_children.push(json!({
                        "OTIO_SCHEMA": "Clip.1",
                        "name": slot.source_ref.as_ref()
                            .and_then(|r| r.shot_id.clone())
                            .unwrap_or_else(|| slot.id.clone()),
                        "source_range": time_range(src_ref.in_us, length, rate),
                        "effects": effects,
                        "media_reference": {
                            "OTIO_SCHEMA": "ExternalReference.1",
                            "target_url": file_url(Path::new(&source.path)),
                            "available_range": time_range(Micros::ZERO, source.duration_us, rate)
                        },
                        // The reason the orchestrator chose this clip travels
                        // with the draft, so a human refining the cut can see
                        // what the decision was based on.
                        "metadata": {
                            "videoButler": {
                                "slotId": slot.id,
                                "reason": slot.reason,
                                "score": slot.score,
                                "speed": slot.speed,
                                "transform": transform,
                                // A photograph: its available range is how
                                // long it may be held, not a running time.
                                "still": source.is_still()
                            }
                        }
                    }));
                }
                SlotKind::TitleCard | SlotKind::Gap => {
                    let title = dsl
                        .tracks
                        .text
                        .iter()
                        .find(|t| t.anchor.slot_id == slot.id)
                        .map(|t| t.content.clone());
                    video_children.push(json!({
                        "OTIO_SCHEMA": "Gap.1",
                        "name": title.clone().unwrap_or_else(|| slot.id.clone()),
                        "source_range": time_range(Micros::ZERO, slot.duration_us, rate),
                        "metadata": {
                            "videoButler": {
                                "slotId": slot.id,
                                "kind": if slot.kind == SlotKind::TitleCard { "titleCard" } else { "gap" },
                                "text": title
                            }
                        }
                    }));
                }
            }
        }

        // Text goes on its own track, resolved to absolute timeline
        // positions. This is the one place absolute time appears, and it comes
        // from resolve_timeline rather than from anything stored in the DSL.
        let timeline = dsl.resolve_timeline();
        let mut text_children: Vec<Value> = Vec::new();
        let mut cursor = Micros::ZERO;
        let mut subtitles: Vec<_> = dsl
            .tracks
            .text
            .iter()
            .filter_map(|t| timeline.resolve_anchor(&t.anchor).map(|at| (at, t)))
            .collect();
        subtitles.sort_by_key(|(at, _)| *at);

        for (at, item) in subtitles {
            if at > cursor {
                text_children.push(json!({
                    "OTIO_SCHEMA": "Gap.1",
                    "name": "",
                    "source_range": time_range(Micros::ZERO, at - cursor, rate)
                }));
            }
            text_children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": item.content,
                "source_range": time_range(Micros::ZERO, item.duration_us, rate),
                "metadata": {
                    "videoButler": {
                        "role": item.role,
                        "text": item.content,
                        "transcriptSegmentId": item.transcript_segment_id
                    }
                }
            }));
            cursor = at + item.duration_us;
        }

        let mut tracks = vec![json!({
            "OTIO_SCHEMA": "Track.1",
            "name": "Video",
            "kind": "Video",
            "children": video_children
        })];
        if !text_children.is_empty() {
            tracks.push(json!({
                "OTIO_SCHEMA": "Track.1",
                "name": "Text",
                "kind": "Video",
                "children": text_children
            }));
        }

        Ok(json!({
            "OTIO_SCHEMA": "Timeline.1",
            "name": dsl.meta.title,
            "global_start_time": rational_time(Micros::ZERO, rate),
            "tracks": {
                "OTIO_SCHEMA": "Stack.1",
                "name": "tracks",
                "children": tracks
            },
            "metadata": {
                "videoButler": {
                    "schemaVersion": dsl.schema_version,
                    "templateId": dsl.meta.template_id,
                    "variantId": dsl.meta.variant_id,
                    // The renderer overlaps clips by the transition duration,
                    // so its output is shorter than the OTIO track sum.
                    "resolvedDurationUs": timeline.total_duration_us.0,
                    "provenance": dsl.provenance
                }
            }
        }))
    }
}

impl DraftWriter for OtioWriter {
    fn name(&self) -> &'static str {
        "OpenTimelineIO"
    }

    fn extension(&self) -> &'static str {
        "otio"
    }

    fn write(
        &self,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        dest: &Path,
    ) -> Result<PathBuf, ExportError> {
        let document = self.build(dsl, graph)?;
        let text = serde_json::to_string_pretty(&document).map_err(|e| ExportError::Serialize {
            format: "OTIO",
            source: e,
        })?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ExportError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        std::fs::write(dest, text).map_err(|e| ExportError::Io {
            path: dest.display().to_string(),
            source: e,
        })?;
        Ok(dest.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Blake3Hash;
    use vb_contracts::editing_dsl::{Anchor, Slot, SourceRef, TextItem, Transition};
    use vb_contracts::footage_graph::Source;

    fn graph() -> FootageGraph {
        let mut g = FootageGraph::new("t");
        g.sources.push(Source {
            id: "src".into(),
            path: "/tmp/a b.mp4".into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros::from_secs(60),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: None,
            audio_codec: None,
            has_audio: Some(false),
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        });
        g
    }

    fn clip(id: &str, order: u32, in_s: u64, out_s: u64) -> Slot {
        Slot {
            id: id.into(),
            order,
            kind: SlotKind::Clip,
            duration_us: Micros::from_secs(out_s - in_s),
            source_ref: Some(SourceRef {
                source_id: "src".into(),
                shot_id: Some(format!("shot-{id}")),
                in_us: Micros::from_secs(in_s),
                out_us: Micros::from_secs(out_s),
            }),
            speed: None,
            reason: Some("because".into()),
            score: Some(0.8),
        }
    }

    fn dsl() -> EditingDsl {
        let mut d = EditingDsl::new("Test cut");
        d.meta.fps_num = Some(30);
        d.meta.fps_den = Some(1);
        d.slots = vec![clip("a", 0, 10, 14), clip("b", 1, 20, 26)];
        d
    }

    fn children(doc: &Value, track: usize) -> &Vec<Value> {
        doc["tracks"]["children"][track]["children"]
            .as_array()
            .unwrap()
    }

    #[test]
    fn clips_become_otio_clips_with_frame_accurate_ranges() {
        let doc = OtioWriter.build(&dsl(), &graph()).unwrap();
        let kids = children(&doc, 0);
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0]["OTIO_SCHEMA"], "Clip.1");
        // 10s at 30fps is frame 300; 4s is 120 frames.
        assert_eq!(kids[0]["source_range"]["start_time"]["value"], 300.0);
        assert_eq!(kids[0]["source_range"]["duration"]["value"], 120.0);
    }

    #[test]
    fn a_sped_up_clip_is_a_time_warp_as_long_as_it_plays() {
        // Six seconds of source in four on screen.
        let mut d = dsl();
        d.slots[1].duration_us = Micros::from_secs(4);
        d.slots[1].speed = Some(1.5);
        let doc = OtioWriter.build(&d, &graph()).unwrap();
        let clip = &children(&doc, 0)[1];
        assert_eq!(clip["source_range"]["start_time"]["value"], 600.0);
        assert_eq!(clip["source_range"]["duration"]["value"], 120.0);
        assert_eq!(clip["effects"][0]["OTIO_SCHEMA"], "LinearTimeWarp.1");
        assert_eq!(clip["effects"][0]["time_scalar"], 1.5);
        // A clip at its own speed has no effect at all.
        assert_eq!(children(&doc, 0)[0]["effects"], serde_json::json!([]));
    }

    #[test]
    fn the_media_reference_url_is_escaped() {
        let doc = OtioWriter.build(&dsl(), &graph()).unwrap();
        let url = children(&doc, 0)[0]["media_reference"]["target_url"]
            .as_str()
            .unwrap();
        assert_eq!(url, "file:///tmp/a%20b.mp4");
    }

    #[test]
    fn the_reason_for_each_cut_travels_with_the_draft() {
        // A human refining the cut should be able to see what it was based on.
        let doc = OtioWriter.build(&dsl(), &graph()).unwrap();
        assert_eq!(
            children(&doc, 0)[0]["metadata"]["videoButler"]["reason"],
            "because"
        );
    }

    #[test]
    fn a_transition_is_written_between_the_clips_it_joins() {
        let mut d = dsl();
        d.transitions.push(Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Xfade,
            duration_us: Micros::from_secs(1),
        });
        let doc = OtioWriter.build(&d, &graph()).unwrap();
        let kids = children(&doc, 0);
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[1]["OTIO_SCHEMA"], "Transition.1");
        assert_eq!(kids[1]["transition_type"], "SMPTE_Dissolve");
        // Half of one second either side, at 30fps.
        assert_eq!(kids[1]["in_offset"]["value"], 15.0);
    }

    #[test]
    fn a_cut_produces_no_transition_element() {
        let mut d = dsl();
        d.transitions.push(Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Cut,
            duration_us: Micros::ZERO,
        });
        let doc = OtioWriter.build(&d, &graph()).unwrap();
        assert_eq!(children(&doc, 0).len(), 2);
    }

    #[test]
    fn a_title_card_becomes_a_named_gap() {
        let mut d = dsl();
        d.slots.push(Slot {
            id: "title".into(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: Micros::from_secs(2),
            source_ref: None,
            speed: None,
            reason: None,
            score: None,
        });
        d.slots[0].order = 1;
        d.slots[1].order = 2;
        d.tracks.text.push(TextItem {
            id: "txt".into(),
            anchor: Anchor::at("title"),
            duration_us: Micros::from_secs(2),
            content: "Morning trail".into(),
            role: None,
            transcript_segment_id: None,
            style: None,
        });

        let doc = OtioWriter.build(&d, &graph()).unwrap();
        let first = &children(&doc, 0)[0];
        assert_eq!(first["OTIO_SCHEMA"], "Gap.1");
        assert_eq!(first["name"], "Morning trail");
        assert_eq!(first["metadata"]["videoButler"]["kind"], "titleCard");
    }

    #[test]
    fn the_renderers_shorter_duration_is_recorded_rather_than_lost() {
        // OTIO sums clip durations; our renderer overlaps them. Recording both
        // keeps the difference visible instead of making one model wrong.
        let mut d = dsl();
        d.transitions.push(Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Xfade,
            duration_us: Micros::from_secs(1),
        });
        let doc = OtioWriter.build(&d, &graph()).unwrap();
        let resolved = doc["metadata"]["videoButler"]["resolvedDurationUs"]
            .as_u64()
            .unwrap();
        assert_eq!(resolved, Micros::from_secs(9).0, "4s + 6s minus 1s overlap");
    }

    #[test]
    fn a_dangling_source_reference_is_an_error_not_a_broken_draft() {
        let mut d = dsl();
        d.slots[0].source_ref.as_mut().unwrap().source_id = "missing".into();
        let err = OtioWriter.build(&d, &graph()).unwrap_err();
        assert!(matches!(err, ExportError::DanglingSource { .. }));
    }

    #[test]
    fn text_gets_its_own_track_at_resolved_positions() {
        let mut d = dsl();
        d.tracks.text.push(TextItem {
            id: "sub".into(),
            anchor: Anchor {
                slot_id: "b".into(),
                offset_us: Some(500_000),
            },
            duration_us: Micros::from_secs(2),
            content: "hello".into(),
            role: None,
            transcript_segment_id: Some("tr-1".into()),
            style: None,
        });
        let doc = OtioWriter.build(&d, &graph()).unwrap();
        let subs = children(&doc, 1);
        // Slot b starts at 4s, plus a 0.5s offset, so a 4.5s leading gap.
        assert_eq!(subs[0]["source_range"]["duration"]["value"], 135.0);
        assert_eq!(subs[1]["name"], "hello");
    }

    #[test]
    fn the_document_is_serialisable_json() {
        let doc = OtioWriter.build(&dsl(), &graph()).unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["OTIO_SCHEMA"], "Timeline.1");
    }
}
