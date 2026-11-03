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

//! Where captions go on a cut.
//!
//! A caption says in a sentence what a moment shows, and it has to be read:
//! on a clip too short to read it, or straight after the last one, it is
//! noise. So not every clip gets one. The rule is decided here, from the cut
//! alone, so the pipeline asks the vision model about exactly the moments
//! that will carry a caption, and the orchestrator places exactly those.

use vb_contracts::editing_dsl::SlotKind;
use vb_contracts::{EditingDsl, Micros};

/// A clip shorter than this goes by before its caption can be read.
pub const MIN_CLIP: Micros = Micros(1_800_000);
/// Quiet between one caption and the next, so the screen is not always
/// talking.
pub const GAP: Micros = Micros(2_500_000);
/// The longest a caption stays up.
pub const LONGEST: Micros = Micros(4_000_000);
/// Kept clear of the cut at either end of its clip.
pub const MARGIN: Micros = Micros(200_000);

/// One caption's place: on this clip, from this far into it, for this long.
/// Relative to the clip, never to the finished timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionPlace {
    pub slot_id: String,
    pub offset_us: Micros,
    pub duration_us: Micros,
}

/// Which clips of a cut carry a caption, and when within each.
pub fn caption_plan(dsl: &EditingDsl) -> Vec<CaptionPlace> {
    let timeline = dsl.resolve_timeline();
    let mut out = Vec::new();
    let mut free_from = Micros::ZERO;
    for slot in dsl.ordered_slots() {
        if slot.kind != SlotKind::Clip || slot.duration_us < MIN_CLIP {
            continue;
        }
        let Some(placed) = timeline.slot(&slot.id) else {
            continue;
        };
        let start = placed.start_us + MARGIN;
        if start < free_from {
            continue;
        }
        let duration = Micros(
            slot.duration_us
                .0
                .saturating_sub(2 * MARGIN.0)
                .min(LONGEST.0),
        );
        out.push(CaptionPlace {
            slot_id: slot.id.clone(),
            offset_us: MARGIN,
            duration_us: duration,
        });
        free_from = start + duration + GAP;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::editing_dsl::{Slot, SourceRef};

    fn cut(lengths_ms: &[u64]) -> EditingDsl {
        let mut dsl = EditingDsl::new("t");
        for (i, ms) in lengths_ms.iter().enumerate() {
            dsl.slots.push(Slot {
                id: format!("s{i}"),
                order: i as u32,
                kind: SlotKind::Clip,
                duration_us: Micros::from_millis(*ms),
                source_ref: Some(SourceRef {
                    source_id: "src".into(),
                    shot_id: None,
                    in_us: Micros::ZERO,
                    out_us: Micros::from_millis(*ms),
                }),
                speed: None,
                reason: None,
                score: None,
            });
        }
        dsl
    }

    #[test]
    fn short_clips_and_clips_right_after_a_caption_get_none() {
        let plan = caption_plan(&cut(&[3_000, 1_000, 2_000, 3_000, 5_000]));
        let ids: Vec<&str> = plan.iter().map(|p| p.slot_id.as_str()).collect();
        // s0 captioned to 2.8 s; s1 is too short; s2 starts at 4.0 s, before
        // 2.8 + 2.5; s3 starts at 6.0 s and is clear.
        assert_eq!(ids, vec!["s0", "s3"]);
        assert_eq!(plan[0].offset_us, MARGIN);
        assert_eq!(plan[0].duration_us, Micros::from_millis(2_600));
    }

    #[test]
    fn a_long_clip_holds_its_caption_only_so_long() {
        let plan = caption_plan(&cut(&[20_000]));
        assert_eq!(plan[0].duration_us, LONGEST);
    }

    #[test]
    fn a_fast_cut_gets_no_captions_rather_than_unreadable_ones() {
        assert!(caption_plan(&cut(&[800; 20])).is_empty());
    }
}
