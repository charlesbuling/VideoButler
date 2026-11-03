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

//! What every cut the editor hands back satisfies, made true after each
//! operation ([`tidy`]) and then checked ([`check`]).

use crate::{MIN_TEXT, MIN_TRANSITION};
use std::collections::HashSet;
use vb_contracts::editing_dsl::{AudioKind, SlotKind, TransitionType};
use vb_contracts::{EditingDsl, FootageGraph, Micros};

/// A clip's length at its speed: its footage divided by how fast it plays,
/// rounded to the microsecond -- what the grounding gate checks.
pub(crate) fn clip_length(in_us: Micros, out_us: Micros, speed: Option<f64>) -> Micros {
    let speed = speed.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    Micros((out_us.saturating_sub(in_us).0 as f64 / speed).round() as u64)
}

/// Make the invariants true: slots numbered in order, each clip as long as
/// its footage at its speed and named after the shot that holds it,
/// transitions only between neighbours and no longer than half of either,
/// track items only on slots that exist and inside them.
pub fn tidy(dsl: &mut EditingDsl, graph: &FootageGraph) {
    tidy_spine(dsl, graph);
    tidy_tracks(dsl);
}

/// The spine alone: slots and the transitions between them, so that the
/// timeline it resolves to is the one the cut will have.
pub(crate) fn tidy_spine(dsl: &mut EditingDsl, graph: &FootageGraph) {
    dsl.slots.sort_by_key(|s| s.order);
    for (i, slot) in dsl.slots.iter_mut().enumerate() {
        slot.order = i as u32;
        if slot.speed.is_some_and(|s| (s - 1.0).abs() < 1e-9) {
            slot.speed = None;
        }
        if let Some(src) = slot.source_ref.as_mut() {
            slot.duration_us = clip_length(src.in_us, src.out_us, slot.speed);
            // The shot that holds the whole range, or none: the gate checks
            // a named shot's bounds, and a trim may leave the one it was cut
            // from.
            let holds = |shot: &vb_contracts::Shot| {
                shot.source_id == src.source_id
                    && shot.contains(src.in_us)
                    && src.out_us <= shot.end_us
            };
            let keep = src
                .shot_id
                .as_deref()
                .and_then(|id| graph.shot(id))
                .is_some_and(holds);
            if !keep {
                src.shot_id = graph.shots.iter().find(|s| holds(s)).map(|s| s.id.clone());
            }
        }
    }

    // Transitions: one per neighbouring pair, in the order of the spine.
    let order: Vec<(String, Micros)> = dsl
        .slots
        .iter()
        .map(|s| (s.id.clone(), s.duration_us))
        .collect();
    let mut kept = Vec::new();
    let mut seen = HashSet::new();
    for pair in order.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let Some(t) = dsl
            .transitions
            .iter()
            .find(|t| t.from_slot_id == a.0 && t.to_slot_id == b.0)
        else {
            continue;
        };
        if !seen.insert((a.0.clone(), b.0.clone())) {
            continue;
        }
        let mut t = t.clone();
        if t.kind == TransitionType::Cut {
            continue;
        }
        let cap = Micros(a.1.0.min(b.1.0) / 2);
        t.duration_us = Micros(t.duration_us.0.min(cap.0));
        if t.duration_us < MIN_TRANSITION {
            continue;
        }
        kept.push(t);
    }
    dsl.transitions = kept;
}

/// Everything anchored to the spine: pictures, text and sound.
fn tidy_tracks(dsl: &mut EditingDsl) {
    let slots: std::collections::HashMap<String, (SlotKind, Micros)> = dsl
        .slots
        .iter()
        .map(|s| (s.id.clone(), (s.kind, s.duration_us)))
        .collect();

    // One picture item per slot, and only one that says something.
    let mut pictured = HashSet::new();
    dsl.tracks
        .video
        .retain(|v| slots.contains_key(&v.slot_id) && pictured.insert(v.slot_id.clone()));
    for item in &mut dsl.tracks.video {
        let duration = slots[&item.slot_id].1;
        if let Some(t) = item.transform.as_mut() {
            for k in &mut t.keyframes {
                k.offset_us = Micros(k.offset_us.0.min(duration.0));
            }
            t.keyframes.sort_by_key(|k| k.offset_us);
            if t.keyframes.is_empty() || crate::retime::is_fitted(t) {
                item.transform = None;
            }
        }
        item.filters
            .retain(|f| f.name != "eq" || !eq_is_identity(f));
    }
    dsl.tracks.video.retain(|v| !v.is_plain());

    // Text: anchored inside a slot that exists, on screen long enough, and
    // not past the end of the piece.
    dsl.tracks
        .text
        .retain(|t| slots.contains_key(&t.anchor.slot_id));
    let timeline = dsl.resolve_timeline();
    let total = timeline.total_duration_us;
    for text in &mut dsl.tracks.text {
        let slot = slots[&text.anchor.slot_id].1;
        let offset = text
            .anchor
            .offset()
            .clamp(0, slot.0.saturating_sub(1) as i64);
        text.anchor.offset_us = (offset != 0).then_some(offset);
        if text.duration_us < MIN_TEXT {
            text.duration_us = MIN_TEXT;
        }
        if let Some(start) = timeline.resolve_anchor(&text.anchor) {
            let room = total.saturating_sub(start);
            if text.duration_us > room {
                text.duration_us = room.max(MIN_TEXT.min(room));
            }
        }
    }
    dsl.tracks.text.retain(|t| t.duration_us > Micros::ZERO);

    // A clip's own sound: at most one item, on a clip, saying something.
    let mut heard = HashSet::new();
    dsl.tracks.audio.retain(|a| {
        if a.kind != AudioKind::Source {
            return slots.contains_key(&a.anchor.slot_id);
        }
        let on_clip = slots
            .get(&a.anchor.slot_id)
            .is_some_and(|(kind, _)| *kind == SlotKind::Clip);
        let says_something = a.muted == Some(true) || a.gain_db.is_some_and(|g| g.abs() > 1e-6);
        on_clip && says_something && heard.insert(a.anchor.slot_id.clone())
    });
    for a in &mut dsl.tracks.audio {
        if a.kind == AudioKind::Source {
            a.anchor.offset_us = None;
            a.duration_us = None;
        }
    }
    // Music not past the end of the piece.
    for a in &mut dsl.tracks.audio {
        if a.kind == AudioKind::Source {
            continue;
        }
        if let Some(start) = timeline.resolve_anchor(&a.anchor) {
            let room = total.saturating_sub(start);
            a.duration_us = Some(a.duration_us.map_or(room, |d| d.min(room)));
        }
    }
    dsl.tracks
        .audio
        .retain(|a| a.kind == AudioKind::Source || a.duration_us.is_some_and(|d| d > Micros::ZERO));

    // The grounding gate writes these afresh for the cut as it now is.
    dsl.grounding.clear();
}

fn eq_is_identity(filter: &vb_contracts::editing_dsl::Filter) -> bool {
    let p = |name: &str, default: f64| {
        filter
            .params
            .get(name)
            .and_then(|v| v.as_f64())
            .unwrap_or(default)
    };
    p("brightness", 0.0).abs() < 1e-6
        && (p("contrast", 1.0) - 1.0).abs() < 1e-6
        && (p("saturation", 1.0) - 1.0).abs() < 1e-6
}

/// Whether a cut the editor is about to hand back is valid, and why not.
pub fn check(dsl: &EditingDsl, graph: &FootageGraph) -> Result<(), String> {
    if dsl.slots.is_empty() {
        return Err("a cut needs at least one slot".into());
    }
    let mut ids = HashSet::new();
    for (i, slot) in dsl.slots.iter().enumerate() {
        if !ids.insert(slot.id.as_str()) {
            return Err(format!("two slots are called {}", slot.id));
        }
        if slot.order != i as u32 {
            return Err(format!("slot {} is out of order", slot.id));
        }
        if slot.duration_us == Micros::ZERO {
            return Err(format!("slot {} has no length", slot.id));
        }
        match (&slot.kind, &slot.source_ref) {
            (SlotKind::Clip, Some(src)) => {
                let source = graph
                    .source(&src.source_id)
                    .ok_or_else(|| format!("slot {} names no footage", slot.id))?;
                if src.in_us >= src.out_us || src.out_us > source.duration_us {
                    return Err(format!("slot {} runs outside its footage", slot.id));
                }
            }
            (SlotKind::Clip, None) => return Err(format!("clip {} has no footage", slot.id)),
            (SlotKind::Gap, Some(_)) => return Err(format!("gap {} names footage", slot.id)),
            _ => {}
        }
    }
    let mut text_ids = HashSet::new();
    for text in &dsl.tracks.text {
        if !text_ids.insert(text.id.as_str()) {
            return Err(format!("two texts are called {}", text.id));
        }
        if !ids.contains(text.anchor.slot_id.as_str()) {
            return Err(format!("text {} is anchored to no slot", text.id));
        }
    }
    let mut audio_ids = HashSet::new();
    for a in &dsl.tracks.audio {
        if !audio_ids.insert(a.id.as_str()) {
            return Err(format!("two sounds are called {}", a.id));
        }
        if !ids.contains(a.anchor.slot_id.as_str()) {
            return Err(format!("sound {} is anchored to no slot", a.id));
        }
    }
    for t in &dsl.transitions {
        if !ids.contains(t.from_slot_id.as_str()) || !ids.contains(t.to_slot_id.as_str()) {
            return Err("a transition joins a slot that is gone".into());
        }
    }
    Ok(())
}
