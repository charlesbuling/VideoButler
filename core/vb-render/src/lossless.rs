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

//! A cut exported without being re-encoded.
//!
//! When a cut is only cuts -- clips of footage end to end at their own
//! speed, nothing drawn over them and nothing added under them -- its video
//! can leave by copying each clip's compressed packets, as LosslessCut does
//! it: seconds for any length, and the footage exactly as filmed. A copy
//! can only start where a picture is whole, so each clip starts at the
//! keyframe at or before its in point, a little earlier than the edit says,
//! and a clip whose out point falls inside a group of B-frames runs on a few
//! frames to where its frames are whole; the plan says how much.
//!
//! Anything that changes pixels or sound -- a transition, text, a title
//! card, a photo, speed, placement, colour, music, a level -- needs the
//! renderer. So does footage that cannot share one track: two cameras, two
//! sizes, two turns. [`plan`] says which, in terms a person can act on.

use std::path::{Path, PathBuf};
use vb_contracts::editing_dsl::{AudioKind, SlotKind, TransitionType};
use vb_contracts::{EditingDsl, FootageGraph, Micros};
use vb_media::{CopyPart, MediaBackend};

/// How far before an in point a keyframe is looked for, first, then again.
const LOOK_BACK: [Micros; 2] = [Micros(10_000_000), Micros(60_000_000)];

/// Why a cut cannot leave without being re-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    /// This build cannot join files by copying.
    Unsupported,
    Empty,
    /// A title card or a gap: nothing to copy.
    NotFootage(String),
    Photo(String),
    Speed(String),
    /// Placement, crop or colour changed.
    Picture(String),
    Transition,
    Text,
    Music,
    /// Some clips' sound changed in level, or some muted and some not.
    Sound,
    /// The frame's shape is not the footage's.
    Canvas,
    /// The clips come from footage that cannot share one track: another
    /// camera, size, turn or codec.
    Mixed,
    /// No keyframe was found before this clip's in point.
    NoKeyframe(String),
    Missing(String),
}

/// One clip's copy.
#[derive(Debug, Clone, PartialEq)]
pub struct LosslessPart {
    pub slot_id: String,
    pub part: CopyPart,
    /// How much longer than the clip it is: back to its keyframe at the
    /// start, on to whole frames at the end.
    pub extra: Micros,
}

/// How a cut leaves without re-encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct LosslessPlan {
    pub parts: Vec<LosslessPart>,
    /// Whether the clips' own sound goes with them; none when every clip is
    /// muted.
    pub audio: bool,
    /// Every clip's extra, together.
    pub extra: Micros,
    /// How long the copy is.
    pub total: Micros,
}

/// Whether `dsl` can leave by copying, and how; else every reason it
/// cannot. Reads keyframes and stream fingerprints through `media`.
pub fn plan(
    dsl: &EditingDsl,
    graph: &FootageGraph,
    media: &dyn MediaBackend,
) -> Result<LosslessPlan, Vec<Blocker>> {
    let mut blockers = Vec::new();
    if !media.can_copy() {
        blockers.push(Blocker::Unsupported);
    }
    let slots = dsl.ordered_slots();
    if slots.is_empty() {
        return Err(vec![Blocker::Empty]);
    }
    if dsl
        .transitions
        .iter()
        .any(|t| t.kind != TransitionType::Cut && t.duration_us > Micros::ZERO)
    {
        blockers.push(Blocker::Transition);
    }
    if !dsl.tracks.text.is_empty() {
        blockers.push(Blocker::Text);
    }
    if dsl.tracks.audio.iter().any(|a| a.kind != AudioKind::Source) {
        blockers.push(Blocker::Music);
    }

    // Each clip: footage, at its own speed, its picture as filmed.
    let mut clips = Vec::new();
    for slot in &slots {
        if slot.kind != SlotKind::Clip {
            blockers.push(Blocker::NotFootage(slot.id.clone()));
            continue;
        }
        let Some(r) = &slot.source_ref else {
            blockers.push(Blocker::NotFootage(slot.id.clone()));
            continue;
        };
        let Some(source) = graph.source(&r.source_id) else {
            blockers.push(Blocker::Missing(slot.id.clone()));
            continue;
        };
        if source.is_still() {
            blockers.push(Blocker::Photo(slot.id.clone()));
            continue;
        }
        if slot.speed.is_some_and(|s| (s - 1.0).abs() > 1e-9) {
            blockers.push(Blocker::Speed(slot.id.clone()));
        }
        if let Some(item) = dsl.tracks.video.iter().find(|v| v.slot_id == slot.id)
            && (item.crop.is_some() || item.transform.is_some() || !item.filters.is_empty())
        {
            blockers.push(Blocker::Picture(slot.id.clone()));
        }
        clips.push((
            slot.id.clone(),
            PathBuf::from(&source.path),
            r.in_us,
            r.out_us,
            source,
        ));
    }

    // The frame is the footage's own, whose shape the copy keeps.
    if let (Some(aspect), Some((_, _, _, _, first))) = (
        dsl.meta
            .aspect_ratio
            .as_deref()
            .and_then(crate::parse_aspect),
        clips.first(),
    ) && let Some((w, h)) = first.display_size()
    {
        let want = aspect.0 as f64 / aspect.1 as f64;
        let have = w as f64 / h.max(1) as f64;
        if (want - have).abs() > want * 0.01 {
            blockers.push(Blocker::Canvas);
        }
    }

    // Sound: every clip's as filmed, or none of it.
    let sound = |slot_id: &str| {
        dsl.tracks
            .audio
            .iter()
            .find(|a| a.kind == AudioKind::Source && a.anchor.slot_id == slot_id)
    };
    let muted: Vec<bool> = clips
        .iter()
        .map(|(id, ..)| sound(id).is_some_and(|a| a.muted == Some(true)))
        .collect();
    let changed = clips.iter().any(|(id, ..)| {
        sound(id)
            .is_some_and(|a| a.muted != Some(true) && a.gain_db.is_some_and(|g| g.abs() > 1e-6))
    });
    let all_muted = !muted.is_empty() && muted.iter().all(|m| *m);
    if changed || (muted.iter().any(|m| *m) && !all_muted) {
        blockers.push(Blocker::Sound);
    }

    // One track: every clip's footage alike.
    if blockers.iter().all(|b| *b != Blocker::Unsupported) {
        let mut prints: Vec<String> = Vec::new();
        for (id, path, ..) in &clips {
            if !path.exists() {
                blockers.push(Blocker::Missing(id.clone()));
                continue;
            }
            match media.copy_fingerprint(path) {
                Ok(print) => prints.push(if all_muted {
                    // With no sound kept, only the pictures need to match.
                    print.split('|').next().unwrap_or_default().to_string()
                } else {
                    print
                }),
                Err(_) => blockers.push(Blocker::Missing(id.clone())),
            }
        }
        prints.dedup();
        if prints.len() > 1 {
            blockers.push(Blocker::Mixed);
        }
    }
    if !blockers.is_empty() {
        return Err(blockers);
    }

    let mut parts = Vec::with_capacity(clips.len());
    let mut total = Micros::ZERO;
    for (id, path, in_us, out_us, _) in clips {
        let Some(from) = keyframe_before(media, &path, in_us) else {
            blockers.push(Blocker::NoKeyframe(id));
            continue;
        };
        // Where the copy really ends; the out point when it cannot be read,
        // which only the length said here would get wrong.
        let end = media
            .copy_end(&path, from, out_us)
            .unwrap_or(out_us)
            .max(out_us);
        total = total + end.saturating_sub(from);
        parts.push(LosslessPart {
            slot_id: id,
            part: CopyPart {
                path,
                from,
                to: out_us,
            },
            extra: in_us.saturating_sub(from) + end.saturating_sub(out_us),
        });
    }
    if !blockers.is_empty() {
        return Err(blockers);
    }
    let extra = Micros(parts.iter().map(|p| p.extra.0).sum());
    Ok(LosslessPlan {
        parts,
        audio: !all_muted,
        extra,
        total,
    })
}

/// The keyframe at or just before `at`: where a copy of a clip starting at
/// `at` can start.
pub fn keyframe_before(media: &dyn MediaBackend, path: &Path, at: Micros) -> Option<Micros> {
    for back in LOOK_BACK {
        let from = at.saturating_sub(back);
        let found = media.keyframes(path, from, at).ok()?;
        if let Some(k) = found.into_iter().filter(|k| *k <= at).max() {
            return Some(k);
        }
        if from == Micros::ZERO {
            break;
        }
    }
    None
}

/// Write the plan's copy to `dest` (an `.mp4`), whole or not at all: it is
/// written beside `dest` under another name and moved there once complete.
pub fn export(
    plan: &LosslessPlan,
    media: &dyn MediaBackend,
    dest: &Path,
    progress: &mut dyn FnMut(f64) -> bool,
) -> Result<crate::RenderReport, crate::RenderError> {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy.mp4".into());
    let partial = dest.with_file_name(format!(".{name}.partial.mp4"));
    let _ = std::fs::remove_file(&partial);
    let parts: Vec<CopyPart> = plan.parts.iter().map(|p| p.part.clone()).collect();
    let mut stopped = false;
    let copied = media.copy_parts(&parts, &partial, plan.audio, &mut |share| {
        let more = progress(share);
        stopped |= !more;
        more
    });
    if stopped {
        let _ = std::fs::remove_file(&partial);
        return Err(crate::RenderError::Cancelled);
    }
    copied.map_err(crate::RenderError::Media)?;
    std::fs::rename(&partial, dest).map_err(|e| crate::RenderError::Io {
        path: dest.display().to_string(),
        source: e,
    })?;
    let info = media.probe(dest).map_err(crate::RenderError::Media)?;
    let (width, height) = info.display_size();
    Ok(crate::RenderReport {
        path: dest.to_path_buf(),
        frames: 0,
        duration: info.duration,
        width,
        height,
        warnings: Vec::new(),
    })
}
