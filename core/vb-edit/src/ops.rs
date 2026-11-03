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

//! Each operation, as a change to the DSL.

use crate::retime::{retime, simplify};
use crate::tidy::clip_length;
use crate::{
    DEFAULT_TEXT, EditError, MAX_SPEED, MIN_SLOT, MIN_SPEED, MIN_TEXT, MIN_TRANSITION, Op,
    Selection,
};
use std::collections::HashMap;
use vb_contracts::editing_dsl::{
    Anchor, AudioItem, AudioKind, Filter, Slot, SlotKind, SourceRef, TextItem, TextRole, TextStyle,
    Timeline, Transform, TransformKeyframe, Transition, TransitionType, VideoItem,
};
use vb_contracts::{EditingDsl, FootageGraph, Micros};

/// Where each piece of music starts on the finished timeline, by id, so it
/// can be put back there after the clips under it change.
pub fn music_positions(dsl: &EditingDsl, timeline: &Timeline) -> HashMap<String, Micros> {
    dsl.tracks
        .audio
        .iter()
        .filter(|a| a.kind != AudioKind::Source)
        .filter_map(|a| Some((a.id.clone(), timeline.resolve_anchor(&a.anchor)?)))
        .collect()
}

/// The slot a moment of the finished timeline falls in: the earlier of two
/// a transition overlaps, or the last slot for the very end.
fn slot_at(timeline: &Timeline, at: Micros) -> Option<&vb_contracts::ResolvedSlot> {
    timeline
        .slots
        .iter()
        .find(|s| s.start_us <= at && at < s.end_us)
        .or_else(|| timeline.slots.last().filter(|s| at >= s.end_us))
}

/// An anchor for a moment of the finished timeline.
fn anchor_at(timeline: &Timeline, at: Micros) -> Option<Anchor> {
    let slot = slot_at(timeline, at)?;
    let offset = at
        .saturating_sub(slot.start_us)
        .0
        .min(slot.duration().0.saturating_sub(1));
    Some(Anchor {
        slot_id: slot.slot_id.clone(),
        offset_us: (offset > 0).then_some(offset as i64),
    })
}

/// Put each piece of music back where it was on the finished timeline,
/// anchored to whichever slot is there now. Music whose place is past the
/// end of the cut is gone with the footage that was there.
pub(crate) fn place_music(dsl: &mut EditingDsl, positions: &HashMap<String, Micros>) {
    let timeline = dsl.resolve_timeline();
    let total = timeline.total_duration_us;
    dsl.tracks.audio.retain_mut(|a| {
        if a.kind == AudioKind::Source {
            return true;
        }
        let Some(&at) = positions.get(&a.id) else {
            return true;
        };
        if at >= total {
            return false;
        }
        match anchor_at(&timeline, at) {
            Some(anchor) => {
                a.anchor = anchor;
                true
            }
            None => false,
        }
    });
}

fn index_of(dsl: &EditingDsl, slot_id: &str) -> Result<usize, EditError> {
    dsl.slots
        .iter()
        .position(|s| s.id == slot_id)
        .ok_or_else(|| EditError::NoSuchSlot(slot_id.to_string()))
}

/// An id no slot, text or sound has yet: `prefix` and a number.
fn fresh_id(dsl: &EditingDsl, prefix: &str) -> String {
    let taken = |id: &str| {
        dsl.slots.iter().any(|s| s.id == id)
            || dsl.tracks.text.iter().any(|t| t.id == id)
            || dsl.tracks.audio.iter().any(|a| a.id == id)
    };
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|id| !taken(id))
        .expect("some number is free")
}

fn video_item_mut<'a>(dsl: &'a mut EditingDsl, slot_id: &str) -> &'a mut VideoItem {
    if let Some(i) = dsl.tracks.video.iter().position(|v| v.slot_id == slot_id) {
        return &mut dsl.tracks.video[i];
    }
    dsl.tracks.video.push(VideoItem::plain(slot_id));
    dsl.tracks.video.last_mut().expect("just pushed")
}

fn source_sound_mut<'a>(dsl: &'a mut EditingDsl, slot_id: &str) -> &'a mut AudioItem {
    if let Some(i) = dsl
        .tracks
        .audio
        .iter()
        .position(|a| a.kind == AudioKind::Source && a.anchor.slot_id == slot_id)
    {
        return &mut dsl.tracks.audio[i];
    }
    let id = fresh_id(dsl, &format!("aud-src-{slot_id}-"));
    dsl.tracks.audio.push(AudioItem {
        id,
        anchor: Anchor::at(slot_id),
        kind: AudioKind::Source,
        duration_us: None,
        asset_path: None,
        asset_in_us: None,
        gain_db: None,
        muted: None,
        fade_in_us: None,
        fade_out_us: None,
        ducking: None,
    });
    dsl.tracks.audio.last_mut().expect("just pushed")
}

fn is_photo(graph: &FootageGraph, slot: &Slot) -> bool {
    slot.source_ref
        .as_ref()
        .and_then(|r| graph.source(&r.source_id))
        .is_some_and(|s| s.is_still())
}

/// The style a new text takes: large, white with a dark edge, in the middle
/// of the frame -- where a phone editor puts new text.
pub fn default_text_style() -> TextStyle {
    TextStyle {
        font_size_pt: Some(36.0),
        primary_color: Some("#FFFFFF".into()),
        outline_color: Some("#000000".into()),
        outline_width: Some(1.5),
        alignment: Some("middleCenter".into()),
        ..TextStyle::default()
    }
}

/// Texts anchored to `slot_id`, moved onto a clock that maps an old offset
/// `o` to `o * scale + shift`, inside a slot now `duration` long. A text
/// whose whole span is now outside the slot goes with the footage it was
/// over; one partly outside is kept to what is left.
fn retime_texts(dsl: &mut EditingDsl, slot_id: &str, scale: f64, shift: i64, duration: Micros) {
    let d = duration.0 as i64;
    dsl.tracks.text.retain_mut(|t| {
        if t.anchor.slot_id != slot_id {
            return true;
        }
        let start = (t.anchor.offset() as f64 * scale).round() as i64 + shift;
        let length = ((t.duration_us.0 as f64 * scale).round() as i64).max(MIN_TEXT.0 as i64);
        let end = start + length;
        if end <= 0 || start >= d {
            return false;
        }
        let (start, length) = if start < 0 { (0, end) } else { (start, length) };
        t.anchor.offset_us = (start > 0).then_some(start);
        t.duration_us = Micros(length.max(MIN_TEXT.0 as i64) as u64);
        true
    });
}

fn retime_picture(dsl: &mut EditingDsl, slot_id: &str, scale: f64, shift: i64, duration: Micros) {
    if let Some(item) = dsl.tracks.video.iter_mut().find(|v| v.slot_id == slot_id)
        && let Some(t) = &item.transform
    {
        item.transform = retime(t, scale, shift, duration);
    }
}

pub(crate) fn run(
    dsl: &mut EditingDsl,
    graph: &FootageGraph,
    op: &Op,
    timeline: &Timeline,
    music: &mut HashMap<String, Micros>,
) -> Result<Option<Selection>, EditError> {
    match op {
        Op::Split { slot_id, at_us } => {
            split(dsl, graph, timeline, slot_id.as_deref(), Micros(*at_us))
        }
        Op::Trim {
            slot_id,
            in_us,
            out_us,
        } => trim(dsl, graph, slot_id, in_us.map(Micros), out_us.map(Micros)),
        Op::SetDuration {
            slot_id,
            duration_us,
        } => set_duration(dsl, graph, slot_id, Micros(*duration_us)),
        Op::Delete { slot_id } => {
            let i = index_of(dsl, slot_id)?;
            if dsl.slots.len() == 1 {
                return Err(EditError::LastSlot);
            }
            dsl.slots.remove(i);
            dsl.tracks.text.retain(|t| &t.anchor.slot_id != slot_id);
            dsl.tracks
                .audio
                .retain(|a| a.kind != AudioKind::Source || &a.anchor.slot_id != slot_id);
            // The neighbours either side are joined by a cut.
            let neighbour = dsl
                .slots
                .get(i)
                .or_else(|| dsl.slots.get(i.saturating_sub(1)))
                .map(|s| s.id.clone());
            Ok(neighbour.map(Selection::Slot))
        }
        Op::Move { slot_id, to_index } => {
            let from = index_of(dsl, slot_id)?;
            let slot = dsl.slots.remove(from);
            let to = (*to_index).min(dsl.slots.len());
            dsl.slots.insert(to, slot);
            for (i, s) in dsl.slots.iter_mut().enumerate() {
                s.order = i as u32;
            }
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::Duplicate { slot_id } => {
            let i = index_of(dsl, slot_id)?;
            let mut copy = dsl.slots[i].clone();
            copy.id = fresh_id(dsl, "s");
            let new_id = copy.id.clone();
            for s in dsl.slots.iter_mut().skip(i + 1) {
                s.order += 1;
            }
            copy.order = dsl.slots[i].order + 1;
            dsl.slots.insert(i + 1, copy);
            if let Some(item) = dsl
                .tracks
                .video
                .iter()
                .find(|v| &v.slot_id == slot_id)
                .cloned()
            {
                dsl.tracks.video.push(VideoItem {
                    slot_id: new_id.clone(),
                    ..item
                });
            }
            if let Some(sound) = dsl
                .tracks
                .audio
                .iter()
                .find(|a| a.kind == AudioKind::Source && &a.anchor.slot_id == slot_id)
                .cloned()
            {
                let id = fresh_id(dsl, &format!("aud-src-{new_id}-"));
                dsl.tracks.audio.push(AudioItem {
                    id,
                    anchor: Anchor::at(&new_id),
                    ..sound
                });
            }
            Ok(Some(Selection::Slot(new_id)))
        }
        Op::Speed { slot_id, speed } => {
            if !(speed.is_finite() && (MIN_SPEED..=MAX_SPEED).contains(speed)) {
                return Err(EditError::BadSpeed(*speed));
            }
            let i = index_of(dsl, slot_id)?;
            let slot = &dsl.slots[i];
            let Some(src) = &slot.source_ref else {
                return Err(EditError::NotAClip(slot_id.clone()));
            };
            if is_photo(graph, slot) {
                return Err(EditError::PhotoHasNoSpeed(slot_id.clone()));
            }
            let old = slot.speed.unwrap_or(1.0);
            let length = clip_length(src.in_us, src.out_us, Some(*speed));
            if length < MIN_SLOT {
                return Err(EditError::TooShort(MIN_SLOT));
            }
            dsl.slots[i].speed = Some(*speed);
            dsl.slots[i].duration_us = length;
            let scale = old / speed;
            retime_texts(dsl, slot_id, scale, 0, length);
            retime_picture(dsl, slot_id, scale, 0, length);
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::Volume {
            slot_id,
            gain_db,
            muted,
        } => {
            let i = index_of(dsl, slot_id)?;
            if dsl.slots[i].kind != SlotKind::Clip || is_photo(graph, &dsl.slots[i]) {
                return Err(EditError::NotAClip(slot_id.clone()));
            }
            if let Some(g) = gain_db
                && !(g.is_finite() && (-60.0..=20.0).contains(g))
            {
                return Err(EditError::BadValue(format!("gain {g} dB")));
            }
            let item = source_sound_mut(dsl, slot_id);
            if let Some(g) = gain_db {
                item.gain_db = Some(*g);
            }
            if let Some(m) = muted {
                item.muted = m.then_some(true);
            }
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::MuteAll { muted } => {
            let clips: Vec<String> = dsl
                .slots
                .iter()
                .filter(|s| s.kind == SlotKind::Clip && !is_photo(graph, s))
                .map(|s| s.id.clone())
                .collect();
            for id in clips {
                source_sound_mut(dsl, &id).muted = muted.then_some(true);
            }
            Ok(None)
        }
        Op::Transition {
            from_slot_id,
            kind,
            duration_us,
        } => {
            let i = index_of(dsl, from_slot_id)?;
            let Some(next) = dsl.slots.get(i + 1).map(|s| s.id.clone()) else {
                return Err(EditError::NoNextSlot(from_slot_id.clone()));
            };
            set_transition(dsl, from_slot_id, &next, *kind, Micros(*duration_us));
            Ok(Some(Selection::Slot(from_slot_id.clone())))
        }
        Op::TransitionAll { kind, duration_us } => {
            let pairs: Vec<(String, String)> = dsl
                .slots
                .windows(2)
                .map(|w| (w[0].id.clone(), w[1].id.clone()))
                .collect();
            for (a, b) in pairs {
                set_transition(dsl, &a, &b, *kind, Micros(*duration_us));
            }
            Ok(None)
        }
        Op::Transform {
            slot_id,
            scale,
            x,
            y,
            rotation_deg,
        } => {
            index_of(dsl, slot_id)?;
            let finite = [*scale, *x, *y, *rotation_deg]
                .iter()
                .all(|v| v.is_finite());
            if !finite || !(0.05..=20.0).contains(scale) || x.abs() > 4.0 || y.abs() > 4.0 {
                return Err(EditError::BadValue(format!(
                    "placement {scale} at ({x}, {y}) turned {rotation_deg}°"
                )));
            }
            let keyframe = TransformKeyframe {
                offset_us: Micros::ZERO,
                scale: *scale,
                x: *x,
                y: *y,
                rotation_deg: rotation_deg.rem_euclid(360.0),
            };
            let keyframe = TransformKeyframe {
                rotation_deg: if keyframe.rotation_deg > 180.0 {
                    keyframe.rotation_deg - 360.0
                } else {
                    keyframe.rotation_deg
                },
                ..keyframe
            };
            video_item_mut(dsl, slot_id).transform = Some(simplify(vec![keyframe]));
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::ResetTransform { slot_id } => {
            index_of(dsl, slot_id)?;
            if let Some(item) = dsl.tracks.video.iter_mut().find(|v| &v.slot_id == slot_id) {
                item.transform = None;
                item.crop = None;
            }
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::Crop { slot_id, crop } => {
            index_of(dsl, slot_id)?;
            if let Some(c) = crop {
                let ok = [c.x, c.y, c.w, c.h]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && c.w >= 0.05
                    && c.h >= 0.05
                    && c.x + c.w <= 1.0 + 1e-9
                    && c.y + c.h <= 1.0 + 1e-9;
                if !ok {
                    return Err(EditError::BadValue(format!("crop {c:?}")));
                }
            }
            let full = crop.is_some_and(|c| c.x == 0.0 && c.y == 0.0 && c.w == 1.0 && c.h == 1.0);
            video_item_mut(dsl, slot_id).crop = if full { None } else { *crop };
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::Adjust {
            slot_id,
            brightness,
            contrast,
            saturation,
            all,
        } => {
            index_of(dsl, slot_id)?;
            let ok = brightness.is_finite()
                && contrast.is_finite()
                && saturation.is_finite()
                && (-1.0..=1.0).contains(brightness)
                && (0.0..=2.0).contains(contrast)
                && (0.0..=2.0).contains(saturation);
            if !ok {
                return Err(EditError::BadValue(format!(
                    "brightness {brightness}, contrast {contrast}, saturation {saturation}"
                )));
            }
            let targets: Vec<String> = if *all {
                dsl.slots
                    .iter()
                    .filter(|s| s.kind == SlotKind::Clip)
                    .map(|s| s.id.clone())
                    .collect()
            } else {
                vec![slot_id.clone()]
            };
            let filter = Filter {
                name: "eq".into(),
                params: [
                    ("brightness".to_string(), serde_json::json!(brightness)),
                    ("contrast".to_string(), serde_json::json!(contrast)),
                    ("saturation".to_string(), serde_json::json!(saturation)),
                ]
                .into_iter()
                .collect(),
            };
            for id in targets {
                let item = video_item_mut(dsl, &id);
                item.filters.retain(|f| f.name != "eq");
                item.filters.push(filter.clone());
            }
            Ok(Some(Selection::Slot(slot_id.clone())))
        }
        Op::Background {
            slot_id,
            background,
        } => {
            if let Some(b) = background
                && b.kind == vb_contracts::editing_dsl::BackgroundKind::Color
                && !b
                    .color
                    .as_deref()
                    .is_some_and(|c| c.len() >= 7 && c.starts_with('#'))
            {
                return Err(EditError::BadValue(
                    "a colour background needs a colour".into(),
                ));
            }
            let background = background
                .clone()
                .filter(|b| b.kind != vb_contracts::editing_dsl::BackgroundKind::Black);
            let targets: Vec<String> = match slot_id {
                Some(id) => {
                    index_of(dsl, id)?;
                    vec![id.clone()]
                }
                None => dsl
                    .slots
                    .iter()
                    .filter(|s| s.kind == SlotKind::Clip)
                    .map(|s| s.id.clone())
                    .collect(),
            };
            for id in targets {
                video_item_mut(dsl, &id).background = background.clone();
            }
            Ok(slot_id.clone().map(Selection::Slot))
        }
        Op::Insert {
            source_id,
            in_us,
            out_us,
            at_index,
        } => {
            let source = graph
                .source(source_id)
                .ok_or_else(|| EditError::NoSuchSource(source_id.clone()))?;
            let (in_us, out_us) = (Micros(*in_us), Micros(*out_us));
            if out_us > source.duration_us || in_us >= out_us {
                return Err(EditError::OutsideFootage(format!(
                    "{in_us} to {out_us} is not inside {} ({})",
                    source.path, source.duration_us
                )));
            }
            if out_us.saturating_sub(in_us) < MIN_SLOT {
                return Err(EditError::TooShort(MIN_SLOT));
            }
            let id = fresh_id(dsl, "s");
            let at = at_index.unwrap_or(dsl.slots.len()).min(dsl.slots.len());
            for s in dsl.slots.iter_mut().skip(at) {
                s.order += 1;
            }
            dsl.slots.insert(
                at,
                Slot {
                    id: id.clone(),
                    order: at as u32,
                    kind: SlotKind::Clip,
                    duration_us: out_us.saturating_sub(in_us),
                    source_ref: Some(SourceRef {
                        source_id: source_id.clone(),
                        shot_id: None,
                        in_us,
                        out_us,
                    }),
                    speed: None,
                    reason: None,
                    score: None,
                },
            );
            Ok(Some(Selection::Slot(id)))
        }
        Op::AddText {
            at_us,
            content,
            duration_us,
            role,
            style,
        } => {
            if content.trim().is_empty() {
                return Err(EditError::EmptyText);
            }
            let anchor = anchor_at(timeline, Micros(*at_us))
                .ok_or(EditError::NothingThere(Micros(*at_us)))?;
            let id = fresh_id(dsl, "txt-");
            dsl.tracks.text.push(TextItem {
                id: id.clone(),
                anchor,
                duration_us: Micros(duration_us.unwrap_or(DEFAULT_TEXT.0).max(MIN_TEXT.0)),
                content: content.clone(),
                role: Some(role.unwrap_or(TextRole::Caption)),
                transcript_segment_id: None,
                style: Some(style.clone().unwrap_or_else(default_text_style)),
            });
            Ok(Some(Selection::Text(id)))
        }
        Op::UpdateText {
            id,
            content,
            style,
            duration_us,
        } => {
            let text = dsl
                .tracks
                .text
                .iter_mut()
                .find(|t| &t.id == id)
                .ok_or_else(|| EditError::NoSuchText(id.clone()))?;
            if let Some(c) = content {
                if c.trim().is_empty() {
                    return Err(EditError::EmptyText);
                }
                text.content = c.clone();
                // Retyped by hand, it no longer transcribes what was said.
                text.transcript_segment_id = None;
            }
            if let Some(s) = style {
                text.style = Some(s.clone());
            }
            if let Some(d) = duration_us {
                text.duration_us = Micros((*d).max(MIN_TEXT.0));
            }
            Ok(Some(Selection::Text(id.clone())))
        }
        Op::MoveText { id, at_us } => {
            let anchor = anchor_at(timeline, Micros(*at_us))
                .ok_or(EditError::NothingThere(Micros(*at_us)))?;
            let text = dsl
                .tracks
                .text
                .iter_mut()
                .find(|t| &t.id == id)
                .ok_or_else(|| EditError::NoSuchText(id.clone()))?;
            text.anchor = anchor;
            Ok(Some(Selection::Text(id.clone())))
        }
        Op::DeleteText { id } => {
            let before = dsl.tracks.text.len();
            dsl.tracks.text.retain(|t| &t.id != id);
            if dsl.tracks.text.len() == before {
                return Err(EditError::NoSuchText(id.clone()));
            }
            Ok(None)
        }
        Op::SetMusic {
            path,
            asset_duration_us,
            looped,
            gain_db,
        } => {
            if path.trim().is_empty() || *asset_duration_us == 0 {
                return Err(EditError::BadValue("the music has no length".into()));
            }
            dsl.tracks.audio.retain(|a| a.kind != AudioKind::Bgm);
            music.retain(|id, _| dsl.tracks.audio.iter().any(|a| &a.id == id));
            let total = timeline.total_duration_us;
            let pass = Micros(*asset_duration_us);
            let mut at = Micros::ZERO;
            let mut first = None;
            while at < total {
                let length = Micros(pass.0.min(total.0 - at.0));
                let last = at + length >= total || !looped;
                let id = fresh_id(dsl, "aud-music-");
                let anchor = anchor_at(timeline, at).ok_or(EditError::NothingThere(at))?;
                dsl.tracks.audio.push(AudioItem {
                    id: id.clone(),
                    anchor,
                    kind: AudioKind::Bgm,
                    duration_us: Some(length),
                    asset_path: Some(path.clone()),
                    asset_in_us: None,
                    gain_db: gain_db.filter(|g| g.abs() > 1e-6),
                    muted: None,
                    fade_in_us: None,
                    fade_out_us: last.then(|| Micros(length.0.min(1_500_000))),
                    ducking: None,
                });
                music.insert(id.clone(), at);
                first.get_or_insert(id);
                at = at + length;
                if !looped {
                    break;
                }
            }
            Ok(first.map(Selection::Music))
        }
        Op::UpdateMusic {
            id,
            gain_db,
            muted,
            fade_in_us,
            fade_out_us,
            asset_in_us,
            at_us,
            duration_us,
        } => {
            let total = timeline.total_duration_us;
            let item = dsl
                .tracks
                .audio
                .iter_mut()
                .find(|a| &a.id == id && a.kind != AudioKind::Source)
                .ok_or_else(|| EditError::NoSuchMusic(id.clone()))?;
            if let Some(g) = gain_db {
                if !(g.is_finite() && (-60.0..=20.0).contains(g)) {
                    return Err(EditError::BadValue(format!("gain {g} dB")));
                }
                item.gain_db = (g.abs() > 1e-6).then_some(*g);
            }
            if let Some(m) = muted {
                item.muted = m.then_some(true);
            }
            if let Some(f) = fade_in_us {
                item.fade_in_us = (*f > 0).then_some(Micros(*f));
            }
            if let Some(f) = fade_out_us {
                item.fade_out_us = (*f > 0).then_some(Micros(*f));
            }
            if let Some(a) = asset_in_us {
                item.asset_in_us = (*a > 0).then_some(Micros(*a));
            }
            if let Some(d) = duration_us {
                if *d < MIN_TEXT.0 {
                    return Err(EditError::TooShort(MIN_TEXT));
                }
                item.duration_us = Some(Micros(*d));
            }
            if let Some(at) = at_us {
                if Micros(*at) >= total {
                    return Err(EditError::NothingThere(Micros(*at)));
                }
                music.insert(id.clone(), Micros(*at));
            }
            Ok(Some(Selection::Music(id.clone())))
        }
        Op::DeleteMusic { id } => {
            match id {
                Some(id) => {
                    let before = dsl.tracks.audio.len();
                    dsl.tracks
                        .audio
                        .retain(|a| a.kind == AudioKind::Source || &a.id != id);
                    if dsl.tracks.audio.len() == before {
                        return Err(EditError::NoSuchMusic(id.clone()));
                    }
                }
                None => dsl.tracks.audio.retain(|a| a.kind == AudioKind::Source),
            }
            Ok(None)
        }
        Op::Canvas { aspect_ratio } => {
            let parsed = aspect_ratio
                .split_once(':')
                .and_then(|(w, h)| {
                    Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                })
                .filter(|(w, h)| (1..=64).contains(w) && (1..=64).contains(h));
            let Some((w, h)) = parsed else {
                return Err(EditError::BadValue(format!(
                    "aspect ratio {aspect_ratio:?}"
                )));
            };
            dsl.meta.aspect_ratio = Some(format!("{w}:{h}"));
            Ok(None)
        }
        Op::Rename { title } => {
            let title = title.trim();
            if title.is_empty() {
                return Err(EditError::EmptyText);
            }
            let old = std::mem::replace(&mut dsl.meta.title, title.to_string());
            // The title card says the title, so it follows a new one.
            for text in &mut dsl.tracks.text {
                if text.role == Some(TextRole::Title) && text.content == old {
                    text.content = title.to_string();
                }
            }
            Ok(None)
        }
    }
}

fn set_transition(
    dsl: &mut EditingDsl,
    from: &str,
    to: &str,
    kind: TransitionType,
    duration: Micros,
) {
    dsl.transitions
        .retain(|t| !(t.from_slot_id == from && t.to_slot_id == to));
    if kind == TransitionType::Cut || duration < MIN_TRANSITION {
        return;
    }
    dsl.transitions.push(Transition {
        from_slot_id: from.to_string(),
        to_slot_id: to.to_string(),
        kind,
        duration_us: duration,
    });
}

fn split(
    dsl: &mut EditingDsl,
    graph: &FootageGraph,
    timeline: &Timeline,
    slot_id: Option<&str>,
    at: Micros,
) -> Result<Option<Selection>, EditError> {
    let resolved = match slot_id {
        Some(id) => timeline
            .slot(id)
            .filter(|s| s.start_us <= at && at < s.end_us)
            .ok_or(EditError::NothingThere(at))?,
        None => timeline
            .slots
            .iter()
            .find(|s| s.start_us <= at && at < s.end_us)
            .ok_or(EditError::NothingThere(at))?,
    };
    let id = resolved.slot_id.clone();
    let offset = at.saturating_sub(resolved.start_us);
    let i = index_of(dsl, &id)?;
    let slot = dsl.slots[i].clone();
    let Some(src) = slot.source_ref.clone() else {
        return Err(EditError::NotAClip(id));
    };
    if offset < MIN_SLOT || slot.duration_us.saturating_sub(offset) < MIN_SLOT {
        return Err(EditError::NothingToSplit(at));
    }
    let speed = slot.speed.unwrap_or(1.0);
    let cut = src.in_us + Micros((offset.0 as f64 * speed).round() as u64);
    let second_id = fresh_id(dsl, "s");
    let mut first = slot.clone();
    first.source_ref = Some(SourceRef {
        out_us: cut,
        ..src.clone()
    });
    first.duration_us = clip_length(src.in_us, cut, slot.speed);
    let mut second = slot.clone();
    second.id = second_id.clone();
    second.order = slot.order + 1;
    second.source_ref = Some(SourceRef { in_us: cut, ..src });
    second.duration_us = clip_length(cut, second.source_ref.as_ref().unwrap().out_us, slot.speed);
    let first_len = first.duration_us;
    let second_len = second.duration_us;
    for s in dsl.slots.iter_mut().skip(i + 1) {
        s.order += 1;
    }
    dsl.slots[i] = first;
    dsl.slots.insert(i + 1, second);

    // The transition out of the clip is now out of its second half.
    for t in &mut dsl.transitions {
        if t.from_slot_id == id {
            t.from_slot_id = second_id.clone();
        }
    }
    // The picture settings and the clip's own sound hold for both halves;
    // a keyed placement is cut where the clip is.
    if let Some(item) = dsl.tracks.video.iter().find(|v| v.slot_id == id).cloned() {
        let shift = -(offset.0 as i64);
        let mut second_item = VideoItem {
            slot_id: second_id.clone(),
            ..item.clone()
        };
        second_item.transform = item
            .transform
            .as_ref()
            .and_then(|t: &Transform| retime(t, 1.0, shift, second_len));
        if let Some(first_item) = dsl.tracks.video.iter_mut().find(|v| v.slot_id == id) {
            first_item.transform = item
                .transform
                .as_ref()
                .and_then(|t| retime(t, 1.0, 0, first_len));
        }
        dsl.tracks.video.push(second_item);
    }
    if let Some(sound) = dsl
        .tracks
        .audio
        .iter()
        .find(|a| a.kind == AudioKind::Source && a.anchor.slot_id == id)
        .cloned()
    {
        let sound_id = fresh_id(dsl, &format!("aud-src-{second_id}-"));
        dsl.tracks.audio.push(AudioItem {
            id: sound_id,
            anchor: Anchor::at(&second_id),
            ..sound
        });
    }
    // Text that starts in the second half goes with it.
    for text in &mut dsl.tracks.text {
        if text.anchor.slot_id == id && text.anchor.offset() >= offset.0 as i64 {
            text.anchor = Anchor {
                slot_id: second_id.clone(),
                offset_us: Some(text.anchor.offset() - offset.0 as i64).filter(|o| *o > 0),
            };
        }
    }
    let _ = graph;
    Ok(Some(Selection::Slot(second_id)))
}

fn trim(
    dsl: &mut EditingDsl,
    graph: &FootageGraph,
    slot_id: &str,
    new_in: Option<Micros>,
    new_out: Option<Micros>,
) -> Result<Option<Selection>, EditError> {
    let i = index_of(dsl, slot_id)?;
    let slot = dsl.slots[i].clone();
    let Some(src) = slot.source_ref.clone() else {
        return Err(EditError::NotAClip(slot_id.to_string()));
    };
    let source = graph
        .source(&src.source_id)
        .ok_or_else(|| EditError::NoSuchSource(src.source_id.clone()))?;
    let in_us = new_in.unwrap_or(src.in_us);
    let out_us = new_out.unwrap_or(src.out_us);
    if out_us > source.duration_us || in_us >= out_us {
        return Err(EditError::OutsideFootage(format!(
            "{in_us} to {out_us} is not inside {} ({})",
            source.path, source.duration_us
        )));
    }
    let length = clip_length(in_us, out_us, slot.speed);
    if length < MIN_SLOT {
        return Err(EditError::TooShort(MIN_SLOT));
    }
    let speed = slot.speed.unwrap_or(1.0);
    // The finished clock moves by the footage trimmed off the front, at the
    // clip's speed.
    let moved = (in_us.0 as f64 - src.in_us.0 as f64) / speed;
    let shift = -(moved.round() as i64);
    dsl.slots[i].source_ref = Some(SourceRef {
        in_us,
        out_us,
        ..src
    });
    dsl.slots[i].duration_us = length;
    retime_texts(dsl, slot_id, 1.0, shift, length);
    retime_picture(dsl, slot_id, 1.0, shift, length);
    Ok(Some(Selection::Slot(slot_id.to_string())))
}

fn set_duration(
    dsl: &mut EditingDsl,
    graph: &FootageGraph,
    slot_id: &str,
    duration: Micros,
) -> Result<Option<Selection>, EditError> {
    if duration < MIN_SLOT {
        return Err(EditError::TooShort(MIN_SLOT));
    }
    let i = index_of(dsl, slot_id)?;
    let slot = dsl.slots[i].clone();
    match (&slot.kind, &slot.source_ref) {
        (SlotKind::TitleCard | SlotKind::Gap, _) => {
            let old = slot.duration_us;
            dsl.slots[i].duration_us = duration;
            // A title held longer keeps its title on screen as long.
            for text in &mut dsl.tracks.text {
                if text.anchor.slot_id == slot_id
                    && text.anchor.offset() == 0
                    && text.duration_us == old
                {
                    text.duration_us = duration;
                }
            }
            Ok(Some(Selection::Slot(slot_id.to_string())))
        }
        (SlotKind::Clip, Some(src)) if is_photo(graph, &slot) => {
            let out = src.in_us + duration;
            trim(dsl, graph, slot_id, None, Some(out))
        }
        _ => Err(EditError::NotAClip(slot_id.to_string())),
    }
}
