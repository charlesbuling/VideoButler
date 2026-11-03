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

//! The built-in editor's operations on an Editing DSL.
//!
//! The editor in the app is a view; the cut is the DSL, and every change a
//! person makes -- a split, a trim, a clip moved, its speed, its sound, a
//! transition, a title, the music, where the picture sits -- is one of the
//! operations here, applied by the engine. That keeps the two promises the
//! product rests on:
//!
//! - **Non-destructive.** An operation changes the decision manifest and
//!   nothing else. Nothing is rendered, no footage is touched, and every
//!   step can be undone or redone because each is a whole DSL.
//! - **No absolute timeline positions.** Positions the user points at -- the
//!   playhead, where a text is dropped -- arrive as absolute times and are
//!   turned at once into `{slotId, offsetUs}` against
//!   [`EditingDsl::resolve_timeline`], the one derivation of time there is.
//!   Nothing absolute is ever stored.
//!
//! Every operation takes a valid cut to a valid cut: clips stay inside their
//! footage at the length their speed gives, transitions only join neighbours
//! and never outlast half of either, text and sound stay anchored to slots
//! that exist. An operation that cannot be carried out leaves the cut as it
//! was and says why, in an [`EditError`] the front end words for the user.
//!
//! What follows a slot when the slot changes: its picture settings, its own
//! sound and the text anchored to it stay with its footage -- a caption
//! describes its clip, a subtitle the words spoken in it. Music keeps its
//! place on the finished timeline, as it does in a phone editor, however
//! the clips under it move.

mod ops;
mod retime;
mod tidy;

use serde::{Deserialize, Serialize};
use vb_contracts::editing_dsl::{Background, Crop, TextRole, TextStyle, TransitionType};
use vb_contracts::{EditingDsl, FootageGraph, Micros};

pub use ops::{default_text_style, music_positions};
pub use tidy::{check, tidy};

/// The shortest a clip may be: three frames at 30 a second.
pub const MIN_SLOT: Micros = Micros(100_000);
/// The shortest a text may stay on screen.
pub const MIN_TEXT: Micros = Micros(200_000);
/// The shortest transition worth the name.
pub const MIN_TRANSITION: Micros = Micros(100_000);
/// Speeds the editor offers, slowest and fastest.
pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 4.0;
/// How long a new text stays on screen when nothing says otherwise.
pub const DEFAULT_TEXT: Micros = Micros(3_000_000);

/// One change to a cut. Times named `atUs` are positions on the finished
/// timeline, which the operation turns into anchors at once; times named
/// `inUs`/`outUs` are moments of the footage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Op {
    /// Cut a clip in two at the playhead. `slotId` chooses between two
    /// clips a transition overlaps there.
    Split {
        #[serde(default)]
        slot_id: Option<String>,
        at_us: u64,
    },
    /// Move a clip's in point or out point within its footage.
    Trim {
        slot_id: String,
        #[serde(default)]
        in_us: Option<u64>,
        #[serde(default)]
        out_us: Option<u64>,
    },
    /// How long a title card, a gap or a photograph is held.
    SetDuration {
        slot_id: String,
        duration_us: u64,
    },
    Delete {
        slot_id: String,
    },
    /// Put a slot at `toIndex` in the spine.
    Move {
        slot_id: String,
        to_index: usize,
    },
    /// A copy of a clip, right after it.
    Duplicate {
        slot_id: String,
    },
    Speed {
        slot_id: String,
        speed: f64,
    },
    /// A clip's own sound: its level, and whether it is heard.
    Volume {
        slot_id: String,
        #[serde(default)]
        gain_db: Option<f64>,
        #[serde(default)]
        muted: Option<bool>,
    },
    /// Every clip's own sound off, or back on.
    MuteAll {
        muted: bool,
    },
    /// The transition at the cut after `fromSlotId`. `cut` removes it.
    Transition {
        from_slot_id: String,
        kind: TransitionType,
        #[serde(default)]
        duration_us: u64,
    },
    /// The same transition at every cut.
    TransitionAll {
        kind: TransitionType,
        #[serde(default)]
        duration_us: u64,
    },
    /// Where a clip's picture sits: enlarged, moved, turned. Held for the
    /// whole clip.
    Transform {
        slot_id: String,
        scale: f64,
        #[serde(default)]
        x: f64,
        #[serde(default)]
        y: f64,
        #[serde(default)]
        rotation_deg: f64,
    },
    /// The picture fitted to the frame again.
    ResetTransform {
        slot_id: String,
    },
    Crop {
        slot_id: String,
        #[serde(default)]
        crop: Option<Crop>,
    },
    /// Brightness (-1..1), contrast (0..2) and saturation (0..2) of a clip,
    /// or of every clip.
    Adjust {
        slot_id: String,
        brightness: f64,
        contrast: f64,
        saturation: f64,
        #[serde(default)]
        all: bool,
    },
    /// What fills the frame round a clip's picture; every clip when no
    /// slot is named. None is black.
    Background {
        #[serde(default)]
        slot_id: Option<String>,
        #[serde(default)]
        background: Option<Background>,
    },
    /// A clip of the project's footage, at `atIndex` in the spine (the end
    /// when absent).
    Insert {
        source_id: String,
        in_us: u64,
        out_us: u64,
        #[serde(default)]
        at_index: Option<usize>,
    },
    AddText {
        at_us: u64,
        content: String,
        #[serde(default)]
        duration_us: Option<u64>,
        #[serde(default)]
        role: Option<TextRole>,
        #[serde(default)]
        style: Option<TextStyle>,
    },
    UpdateText {
        id: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        style: Option<TextStyle>,
        #[serde(default)]
        duration_us: Option<u64>,
    },
    MoveText {
        id: String,
        at_us: u64,
    },
    DeleteText {
        id: String,
    },
    /// Lay music under the whole cut from its start, in place of any there
    /// is: once, or again and again when `loop`. `assetDurationUs` is the
    /// file's length, which the engine reads before applying this.
    SetMusic {
        path: String,
        asset_duration_us: u64,
        #[serde(default, rename = "loop")]
        looped: bool,
        #[serde(default)]
        gain_db: Option<f64>,
    },
    UpdateMusic {
        id: String,
        #[serde(default)]
        gain_db: Option<f64>,
        #[serde(default)]
        muted: Option<bool>,
        #[serde(default)]
        fade_in_us: Option<u64>,
        #[serde(default)]
        fade_out_us: Option<u64>,
        #[serde(default)]
        asset_in_us: Option<u64>,
        #[serde(default)]
        at_us: Option<u64>,
        #[serde(default)]
        duration_us: Option<u64>,
    },
    /// One piece of music, or all of it.
    DeleteMusic {
        #[serde(default)]
        id: Option<String>,
    },
    /// The frame's shape: `16:9`, `9:16`, `1:1`, `4:3`, `3:4`...
    Canvas {
        aspect_ratio: String,
    },
    Rename {
        title: String,
    },
}

/// What an operation made that the editor should select next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum Selection {
    Slot(String),
    Text(String),
    Music(String),
}

/// Why an operation was not carried out. The cut is unchanged.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("there is no slot {0}")]
    NoSuchSlot(String),
    #[error("there is no text {0}")]
    NoSuchText(String),
    #[error("there is no music {0}")]
    NoSuchMusic(String),
    #[error("there is no source {0} in the project's footage")]
    NoSuchSource(String),
    #[error("slot {0} is not a clip of footage")]
    NotAClip(String),
    #[error("slot {0} is a photograph, which has no speed")]
    PhotoHasNoSpeed(String),
    #[error("{0}")]
    OutsideFootage(String),
    #[error("a clip cannot be shorter than {0}")]
    TooShort(Micros),
    #[error("nothing to split at {0}: too close to the clip's ends")]
    NothingToSplit(Micros),
    #[error("no clip is at {0}")]
    NothingThere(Micros),
    #[error("speed {0} is outside 0.25x to 4x")]
    BadSpeed(f64),
    #[error("slot {0} has no slot after it to transition into")]
    NoNextSlot(String),
    #[error("the only slot of a cut cannot be deleted")]
    LastSlot,
    #[error("text cannot be empty")]
    EmptyText,
    #[error("{0}")]
    BadValue(String),
    /// The operation produced a cut that is not valid. A bug, never the
    /// user's doing; the cut is left as it was.
    #[error("the edit would break the cut: {0}")]
    Broken(String),
}

/// Apply `op` to `dsl`, against the graph its footage comes from. On error
/// the DSL is exactly as it was.
pub fn apply(
    dsl: &mut EditingDsl,
    graph: &FootageGraph,
    op: &Op,
) -> Result<Option<Selection>, EditError> {
    let mut work = dsl.clone();
    let before = work.resolve_timeline();
    let mut music = ops::music_positions(&work, &before);
    let selection = ops::run(&mut work, graph, op, &before, &mut music)?;
    // The spine first, so music goes back to its place on the timeline the
    // cut now has, before anything anchored to a slot that is gone is
    // dropped.
    tidy::tidy_spine(&mut work, graph);
    ops::place_music(&mut work, &music);
    tidy::tidy(&mut work, graph);
    tidy::check(&work, graph).map_err(EditError::Broken)?;
    *dsl = work;
    Ok(selection)
}

/// How much of `edited` differs from the cut the orchestrator made, by
/// footage time: 1 less the share of the footage shown that the original
/// shows too. 0 is the cut as made; deleting half of it, or adding as much
/// again of other footage, is 0.5. Order is not counted -- the footage is.
pub fn change_ratio(original: &EditingDsl, edited: &EditingDsl) -> f64 {
    use std::collections::HashMap;
    fn ranges(dsl: &EditingDsl) -> HashMap<String, Vec<(u64, u64)>> {
        let mut out: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
        for slot in &dsl.slots {
            if let Some(r) = &slot.source_ref
                && r.out_us > r.in_us
            {
                out.entry(r.source_id.clone())
                    .or_default()
                    .push((r.in_us.0, r.out_us.0));
            }
        }
        for list in out.values_mut() {
            list.sort();
            let mut merged: Vec<(u64, u64)> = Vec::new();
            for (a, b) in list.drain(..) {
                match merged.last_mut() {
                    Some(last) if a <= last.1 => last.1 = last.1.max(b),
                    _ => merged.push((a, b)),
                }
            }
            *list = merged;
        }
        out
    }
    let total = |r: &HashMap<String, Vec<(u64, u64)>>| -> u64 {
        r.values().flatten().map(|(a, b)| b - a).sum()
    };
    let (a, e) = (ranges(original), ranges(edited));
    let (ta, te) = (total(&a), total(&e));
    if ta == 0 && te == 0 {
        return 0.0;
    }
    let mut kept = 0u64;
    for (source, list) in &e {
        let Some(theirs) = a.get(source) else {
            continue;
        };
        for (x, y) in list {
            for (p, q) in theirs {
                let lo = (*x).max(*p);
                let hi = (*y).min(*q);
                if hi > lo {
                    kept += hi - lo;
                }
            }
        }
    }
    (1.0 - kept as f64 / ta.max(te) as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
