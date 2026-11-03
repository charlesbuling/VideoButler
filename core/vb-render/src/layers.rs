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

//! What is on screen and heard when, derived from the DSL.
//!
//! Every position here comes from [`EditingDsl::resolve_timeline`] and
//! [`vb_contracts::Timeline::resolve_anchor`]: the renderer never sums
//! durations itself, so it cannot disagree with the exporters about where
//! anything is.

use crate::RenderError;
use std::path::PathBuf;
use vb_contracts::editing_dsl::{
    AudioKind, Ducking, SlotKind, TextItem, Transform, TransformKeyframe, TransitionType, VideoItem,
};
use vb_contracts::{EditingDsl, FootageGraph, Micros};

/// One slot on the finished timeline.
#[derive(Debug, Clone)]
pub(crate) struct Layer {
    pub slot_id: String,
    pub start: Micros,
    pub end: Micros,
    pub content: Content,
    /// Crop, adjustments, placement and background, when the cut set any.
    pub video: Option<VideoItem>,
    /// How this slot comes in over the one before, and for how long.
    pub incoming: Option<(TransitionType, Micros)>,
    /// How this slot gives way to the next, and for how long.
    pub outgoing: Option<(TransitionType, Micros)>,
    /// The clip's own sound: linear gain, and whether it is silenced.
    pub gain: f32,
    pub muted: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Content {
    Clip {
        path: PathBuf,
        /// A photograph: one picture, held.
        still: bool,
        /// The picture's size upright, as shown.
        size: (u32, u32),
        in_us: Micros,
        out_us: Micros,
        speed: f64,
        has_audio: bool,
    },
    /// A title card or a gap: black, with whatever text lies over it.
    Black,
}

impl Layer {
    /// The source moment on screen at `t` on the finished timeline.
    pub fn source_time(&self, t: Micros) -> Micros {
        match &self.content {
            Content::Clip {
                in_us,
                out_us,
                speed,
                ..
            } => {
                let into = t.saturating_sub(self.start).0 as f64 * speed;
                Micros((in_us.0 + into.round() as u64).min(out_us.0.max(in_us.0)))
            }
            Content::Black => Micros::ZERO,
        }
    }

    /// The placement at `t`: the keyframes eased, or fitted.
    pub fn placement_at(&self, t: Micros) -> TransformKeyframe {
        let offset = t.saturating_sub(self.start);
        self.video
            .as_ref()
            .and_then(|v| v.transform.as_ref())
            .and_then(|tr: &Transform| tr.at(offset))
            .unwrap_or_else(|| TransformKeyframe::fitted(offset))
    }

    /// The largest the picture is ever shown, as a multiple of fitted.
    pub fn max_scale(&self) -> f64 {
        self.video
            .as_ref()
            .and_then(|v| v.transform.as_ref())
            .map(|t| {
                t.keyframes
                    .iter()
                    .map(|k| k.scale)
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .fold(f64::MIN, f64::max)
            })
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0)
    }
}

/// A text item placed on the finished timeline.
#[derive(Debug, Clone)]
pub(crate) struct TextLayer {
    pub item: TextItem,
    pub start: Micros,
    pub end: Micros,
}

/// A piece of music placed on the finished timeline.
#[derive(Debug, Clone)]
pub(crate) struct MusicLayer {
    pub path: PathBuf,
    pub start: Micros,
    pub end: Micros,
    pub asset_in: Micros,
    pub gain: f32,
    pub fade_in: Micros,
    pub fade_out: Micros,
    pub ducking: Option<Ducking>,
}

/// The whole piece, laid out.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub layers: Vec<Layer>,
    pub texts: Vec<TextLayer>,
    pub music: Vec<MusicLayer>,
    pub total: Micros,
}

pub(crate) fn db_to_gain(db: f64) -> f32 {
    if !db.is_finite() {
        return 1.0;
    }
    10f64.powf(db.clamp(-96.0, 24.0) / 20.0) as f32
}

/// Lay out a DSL against the graph its clips come from.
pub(crate) fn layout(dsl: &EditingDsl, graph: &FootageGraph) -> Result<Layout, RenderError> {
    let timeline = dsl.resolve_timeline();
    let mut layers = Vec::with_capacity(timeline.slots.len());
    for (i, resolved) in timeline.slots.iter().enumerate() {
        let slot = dsl
            .slot(&resolved.slot_id)
            .ok_or_else(|| RenderError::MissingSlot(resolved.slot_id.clone()))?;
        let content = match (&slot.kind, &slot.source_ref) {
            (SlotKind::Clip, Some(src)) => {
                let source =
                    graph
                        .source(&src.source_id)
                        .ok_or_else(|| RenderError::MissingSource {
                            slot_id: slot.id.clone(),
                            source_id: src.source_id.clone(),
                        })?;
                let size = source
                    .display_size()
                    .filter(|(w, h)| *w > 0 && *h > 0)
                    .ok_or_else(|| RenderError::UnknownSize(source.path.clone()))?;
                Content::Clip {
                    path: PathBuf::from(&source.path),
                    still: source.is_still(),
                    size,
                    in_us: src.in_us,
                    out_us: src.out_us,
                    speed: slot
                        .speed
                        .filter(|v| v.is_finite() && *v > 0.0)
                        .unwrap_or(1.0),
                    has_audio: source.has_audio.unwrap_or(true) && !source.is_still(),
                }
            }
            (SlotKind::Clip, None) => {
                return Err(RenderError::MissingSource {
                    slot_id: slot.id.clone(),
                    source_id: "<none>".into(),
                });
            }
            _ => Content::Black,
        };
        let transition_kind = |from: &str, to: &str| {
            dsl.transitions
                .iter()
                .find(|t| t.from_slot_id == from && t.to_slot_id == to)
                .map(|t| t.kind)
        };
        let incoming = (resolved.incoming_overlap_us > Micros::ZERO)
            .then(|| {
                let prev = &timeline.slots[i - 1].slot_id;
                transition_kind(prev, &slot.id).map(|k| (k, resolved.incoming_overlap_us))
            })
            .flatten();
        let outgoing = timeline
            .slots
            .get(i + 1)
            .filter(|next| next.incoming_overlap_us > Micros::ZERO)
            .and_then(|next| {
                transition_kind(&slot.id, &next.slot_id).map(|k| (k, next.incoming_overlap_us))
            });
        let sound = dsl
            .tracks
            .audio
            .iter()
            .find(|a| a.kind == AudioKind::Source && a.anchor.slot_id == slot.id);
        layers.push(Layer {
            slot_id: slot.id.clone(),
            start: resolved.start_us,
            end: resolved.end_us,
            content,
            video: dsl
                .tracks
                .video
                .iter()
                .find(|v| v.slot_id == slot.id)
                .cloned(),
            incoming,
            outgoing,
            gain: sound.and_then(|a| a.gain_db).map_or(1.0, db_to_gain),
            muted: sound.and_then(|a| a.muted).unwrap_or(false),
        });
    }

    let total = timeline.total_duration_us;
    let texts = dsl
        .tracks
        .text
        .iter()
        .filter_map(|item| {
            let start = timeline.resolve_anchor(&item.anchor)?;
            let end = Micros((start + item.duration_us).0.min(total.0));
            (end > start).then(|| TextLayer {
                item: item.clone(),
                start,
                end,
            })
        })
        .collect();
    let music = dsl
        .tracks
        .audio
        .iter()
        .filter(|a| a.kind != AudioKind::Source)
        .filter_map(|a| {
            let path = a.asset_path.as_ref().filter(|p| !p.is_empty())?;
            let start = timeline.resolve_anchor(&a.anchor)?;
            let end = a
                .duration_us
                .map_or(total, |d| Micros((start + d).0.min(total.0)));
            (end > start && !a.muted.unwrap_or(false)).then(|| MusicLayer {
                path: PathBuf::from(path),
                start,
                end,
                asset_in: a.asset_in_us.unwrap_or(Micros::ZERO),
                gain: a.gain_db.map_or(1.0, db_to_gain),
                fade_in: a.fade_in_us.unwrap_or(Micros::ZERO),
                fade_out: a.fade_out_us.unwrap_or(Micros::ZERO),
                ducking: a.ducking.filter(|d| d.enabled.unwrap_or(false)),
            })
        })
        .collect();

    Ok(Layout {
        layers,
        texts,
        music,
        total,
    })
}
