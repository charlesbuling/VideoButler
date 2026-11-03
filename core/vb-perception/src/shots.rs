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

//! Turning scene-change timestamps into a shot list.
//!
//! This is the first step of the temporal compression funnel: hours of frames
//! become a few hundred shots, and only that compressed storyboard is ever
//! reasoned about. Frame-level work is classical CV by design; no model sees
//! the raw stream.

use vb_contracts::Micros;
use vb_contracts::footage_graph::Shot;
use vb_media::SceneCut;

/// Build a shot list from detected cuts.
///
/// Cuts mark boundaries, so N cuts produce up to N+1 shots spanning the whole
/// source. Shots shorter than `min_shot` are merged forward into their
/// neighbour: scene detection fires spuriously on a camera flash or a fast
/// pan, and a 100ms "shot" is a detector artefact rather than something a
/// viewer would recognise.
pub fn build_shots(
    source_id: &str,
    cuts: &[SceneCut],
    duration: Micros,
    min_shot: Micros,
) -> Vec<Shot> {
    if duration == Micros::ZERO {
        return Vec::new();
    }

    // Boundaries, deduplicated and in order. A cut at 0 is meaningless
    // because the source already starts there.
    let mut boundaries: Vec<(Micros, f64)> = cuts
        .iter()
        .filter(|c| c.at > Micros::ZERO && c.at < duration)
        .map(|c| (c.at, c.score))
        .collect();
    boundaries.sort_by_key(|(at, _)| *at);
    boundaries.dedup_by_key(|(at, _)| *at);

    let mut spans: Vec<(Micros, Micros, f64)> = Vec::new();
    let mut start = Micros::ZERO;
    let mut confidence = 1.0; // the opening shot is a boundary by definition

    for (at, score) in boundaries {
        spans.push((start, at, confidence));
        start = at;
        confidence = score;
    }
    spans.push((start, duration, confidence));

    // Merge anything too short into the previous span, so the result still
    // tiles the source exactly with no gaps.
    let mut merged: Vec<(Micros, Micros, f64)> = Vec::with_capacity(spans.len());
    for span in spans {
        let too_short = span.1.saturating_sub(span.0) < min_shot;
        match merged.last_mut() {
            Some(prev) if too_short => prev.1 = span.1,
            _ => merged.push(span),
        }
    }

    // A single short source merges into nothing; keep it rather than
    // returning an empty list for a file that plainly has content.
    if merged.len() > 1 {
        if let Some(last) = merged.last() {
            if last.1.saturating_sub(last.0) < min_shot {
                let last = merged.pop().expect("checked non-empty");
                if let Some(prev) = merged.last_mut() {
                    prev.1 = last.1;
                }
            }
        }
    }

    merged
        .into_iter()
        .enumerate()
        .map(|(i, (start, end, conf))| Shot {
            id: format!("shot-{i:04}"),
            source_id: source_id.to_string(),
            start_us: start,
            end_us: end,
            shot_change_confidence: Some(conf.clamp(0.0, 1.0)),
            keyframes: Vec::new(),
            description: None,
            entity_refs: Vec::new(),
            actions: Vec::new(),
            audio: None,
            tags: Vec::new(),
            segments: Vec::new(),
            timeline: Vec::new(),
        })
        .collect()
}

/// Where to sample keyframes within a shot.
///
/// The count scales with shot length so a long take is not represented by one
/// frame, and samples avoid the outer eighth of the shot, where transition
/// blending and motion blur make frames unrepresentative.
pub fn keyframe_times(shot: &Shot, max_per_shot: usize, per_seconds: u64) -> Vec<Micros> {
    let len = shot.duration();
    if len == Micros::ZERO || max_per_shot == 0 {
        return Vec::new();
    }

    let by_length = (len.0 / (per_seconds.max(1) * 1_000_000)) as usize + 1;
    let count = by_length.clamp(1, max_per_shot);

    let inset = len.0 / 8;
    let usable_start = shot.start_us.0 + inset;
    let usable_len = len.0.saturating_sub(inset * 2);

    if usable_len == 0 {
        return vec![Micros(shot.start_us.0 + len.0 / 2)];
    }

    (0..count)
        .map(|i| {
            // Sample at the midpoint of each of `count` equal slices, so
            // samples are spread evenly and never land on a boundary.
            let offset = usable_len * (2 * i as u64 + 1) / (2 * count as u64);
            Micros(usable_start + offset)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(at_s: f64, score: f64) -> SceneCut {
        SceneCut {
            at: Micros::from_secs_f64(at_s),
            score,
        }
    }

    #[test]
    fn cuts_become_spans_that_tile_the_whole_source() {
        let shots = build_shots(
            "src",
            &[cut(10.0, 0.8), cut(25.0, 0.9)],
            Micros::from_secs(40),
            Micros::from_secs(1),
        );
        assert_eq!(shots.len(), 3);
        assert_eq!(shots[0].start_us, Micros::ZERO);
        assert_eq!(shots[2].end_us, Micros::from_secs(40));
        // No gaps: each shot begins where the last ended.
        for pair in shots.windows(2) {
            assert_eq!(pair[0].end_us, pair[1].start_us);
        }
    }

    #[test]
    fn a_source_with_no_cuts_is_one_shot() {
        let shots = build_shots("src", &[], Micros::from_secs(30), Micros::from_secs(1));
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].duration(), Micros::from_secs(30));
    }

    #[test]
    fn spurious_short_cuts_are_merged_away() {
        // A camera flash fires three detections in half a second. None of them
        // is a shot a viewer would recognise.
        let shots = build_shots(
            "src",
            &[cut(10.0, 0.9), cut(10.1, 0.9), cut(10.2, 0.9)],
            Micros::from_secs(30),
            Micros::from_secs(1),
        );
        assert_eq!(shots.len(), 2, "flash artefacts became real shots");
        for s in &shots {
            assert!(s.duration() >= Micros::from_secs(1));
        }
    }

    #[test]
    fn a_short_final_shot_merges_backwards() {
        let shots = build_shots(
            "src",
            &[cut(10.0, 0.9), cut(29.9, 0.9)],
            Micros::from_secs(30),
            Micros::from_secs(1),
        );
        assert_eq!(shots.len(), 2);
        assert_eq!(shots.last().unwrap().end_us, Micros::from_secs(30));
    }

    #[test]
    fn a_source_shorter_than_the_minimum_still_yields_one_shot() {
        // Otherwise a five-second clip would analyse to nothing at all.
        let shots = build_shots("src", &[], Micros::from_secs(5), Micros::from_secs(10));
        assert_eq!(shots.len(), 1);
    }

    #[test]
    fn cuts_at_or_past_the_boundaries_are_ignored() {
        let shots = build_shots(
            "src",
            &[cut(0.0, 0.9), cut(60.0, 0.9), cut(99.0, 0.9)],
            Micros::from_secs(60),
            Micros::from_millis(100),
        );
        assert_eq!(shots.len(), 1);
    }

    #[test]
    fn a_zero_length_source_yields_nothing() {
        assert!(build_shots("src", &[], Micros::ZERO, Micros::from_secs(1)).is_empty());
    }

    #[test]
    fn keyframe_counts_scale_with_shot_length() {
        let short = Shot {
            id: "a".into(),
            source_id: "s".into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(2),
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
        let long = Shot {
            end_us: Micros::from_secs(60),
            ..short.clone()
        };
        assert!(keyframe_times(&long, 8, 5).len() > keyframe_times(&short, 8, 5).len());
        assert!(keyframe_times(&long, 8, 5).len() <= 8);
    }

    #[test]
    fn keyframes_land_inside_their_shot_and_avoid_the_edges() {
        let shot = Shot {
            id: "a".into(),
            source_id: "s".into(),
            start_us: Micros::from_secs(10),
            end_us: Micros::from_secs(30),
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
        let times = keyframe_times(&shot, 5, 5);
        assert!(!times.is_empty());
        for t in times {
            assert!(shot.contains(t), "keyframe {t} escaped its shot");
            // Outside the first and last eighth, where blending lives.
            assert!(t > Micros::from_secs(11));
            assert!(t < Micros::from_secs(29));
        }
    }

    #[test]
    fn a_shot_of_no_length_produces_no_keyframes() {
        let shot = Shot {
            id: "a".into(),
            source_id: "s".into(),
            start_us: Micros::from_secs(5),
            end_us: Micros::from_secs(5),
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
        assert!(keyframe_times(&shot, 5, 5).is_empty());
    }
}
