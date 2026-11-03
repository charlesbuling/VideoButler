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

//! Dance: which moments are dancing, by how many, and how two of them join.
//!
//! Everything here reads the body poses perception sampled into the Footage
//! Graph (`Segment::poses`). A pose is compared as a shape -- keypoints
//! relative to the middle of the hips, in torso lengths, so a dancer close
//! to the lens and one across a square compare by what their bodies do --
//! and as a placement: where the body stands in the frame and how big it
//! is. A cut that keeps both, and the direction the limbs are moving,
//! carries the movement across the join and changes only the scene, which
//! is what a dance edit is.

use vb_contracts::Micros;
use vb_contracts::footage_graph::{PersonPose, PoseSample};

/// Below this a keypoint is the model guessing.
const SURE: f64 = 0.3;
/// Fewer shared keypoints than this and two poses cannot be compared.
const SHARED: usize = 6;
/// What a pair of frames costs when their dancers cannot be matched at all:
/// a different number of people, or nobody to compare.
pub const MISMATCH: f64 = 3.0;

/// COCO keypoint indices.
const L_SHOULDER: usize = 5;
const R_SHOULDER: usize = 6;
const L_HIP: usize = 11;
const R_HIP: usize = 12;

/// A person with where they stand taken out.
struct Shape {
    points: Vec<Option<(f64, f64)>>,
}

fn midpoint(p: &PersonPose, a: usize, b: usize) -> Option<(f64, f64)> {
    let (ka, kb) = (p.keypoints.get(a)?, p.keypoints.get(b)?);
    match (ka[2] >= SURE, kb[2] >= SURE) {
        (true, true) => Some(((ka[0] + kb[0]) / 2.0, (ka[1] + kb[1]) / 2.0)),
        (true, false) => Some((ka[0], ka[1])),
        (false, true) => Some((kb[0], kb[1])),
        (false, false) => None,
    }
}

fn shape(p: &PersonPose) -> Shape {
    let (cx, cy) = p.centre();
    let hips = midpoint(p, L_HIP, R_HIP).unwrap_or((cx, cy));
    let torso = match (
        midpoint(p, L_SHOULDER, R_SHOULDER),
        midpoint(p, L_HIP, R_HIP),
    ) {
        (Some(s), Some(h)) => ((s.0 - h.0).powi(2) + (s.1 - h.1).powi(2)).sqrt(),
        _ => p.height() / 3.0,
    }
    .max(p.height() / 8.0)
    .max(1e-3);
    Shape {
        points: p
            .keypoints
            .iter()
            .map(|k| (k[2] >= SURE).then(|| ((k[0] - hips.0) / torso, (k[1] - hips.1) / torso)))
            .collect(),
    }
}

/// How differently two people hold themselves: the root-mean-square
/// distance between their shared keypoints, in torso lengths.
fn shape_distance(a: &PersonPose, b: &PersonPose) -> Option<f64> {
    let (sa, sb) = (shape(a), shape(b));
    let mut sum = 0.0;
    let mut n = 0usize;
    for (pa, pb) in sa.points.iter().zip(&sb.points) {
        if let (Some(pa), Some(pb)) = (pa, pb) {
            sum += (pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2);
            n += 1;
        }
    }
    (n >= SHARED).then(|| (sum / n as f64).sqrt())
}

/// How differently two people sit in their frames: how far apart, as a
/// fraction of the frame, and how different in size.
fn placement_distance(a: &PersonPose, b: &PersonPose) -> f64 {
    let (ca, cb) = (a.centre(), b.centre());
    let moved = ((ca.0 - cb.0).powi(2) + (ca.1 - cb.1).powi(2)).sqrt();
    let resized = (a.height().max(1e-3) / b.height().max(1e-3)).ln().abs();
    2.0 * moved + resized
}

/// How far apart two frames' dancers are: shape, and where they stand.
/// Matched left to right; a different number of people is [`MISMATCH`].
pub fn pose_distance(a: &PoseSample, b: &PoseSample) -> f64 {
    let (pa, pb) = (a.subjects(), b.subjects());
    if pa.is_empty() || pa.len() != pb.len() {
        return MISMATCH;
    }
    let mut total = 0.0;
    for (x, y) in pa.iter().zip(&pb) {
        total += match shape_distance(x, y) {
            Some(d) => d + 0.5 * placement_distance(x, y),
            None => MISMATCH,
        };
    }
    total / pa.len() as f64
}

/// Which way each keypoint of the first dancer is moving between two
/// frames, in torso lengths a second, with the body's own travel taken out.
fn velocity(before: &PoseSample, after: &PoseSample) -> Option<Vec<Option<(f64, f64)>>> {
    let seconds = after.at_us.0.checked_sub(before.at_us.0)? as f64 / 1e6;
    if seconds <= 0.0 {
        return None;
    }
    let (a, b) = (before.subjects(), after.subjects());
    let (a, b) = (a.first()?, b.first()?);
    let (sa, sb) = (shape(a), shape(b));
    Some(
        sa.points
            .iter()
            .zip(&sb.points)
            .map(|(pa, pb)| match (pa, pb) {
                (Some(pa), Some(pb)) => Some(((pb.0 - pa.0) / seconds, (pb.1 - pa.1) / seconds)),
                _ => None,
            })
            .collect(),
    )
}

/// How differently the limbs are moving at two moments: the mean distance
/// between the two motions, in torso lengths a second. None when either
/// motion cannot be read.
pub fn velocity_distance(
    a: (&PoseSample, &PoseSample),
    b: (&PoseSample, &PoseSample),
) -> Option<f64> {
    let (va, vb) = (velocity(a.0, a.1)?, velocity(b.0, b.1)?);
    let mut sum = 0.0;
    let mut n = 0usize;
    for (x, y) in va.iter().zip(&vb) {
        if let (Some(x), Some(y)) = (x, y) {
            sum += ((x.0 - y.0).powi(2) + (x.1 - y.1).powi(2)).sqrt();
            n += 1;
        }
    }
    (n >= SHARED).then(|| sum / n as f64)
}

/// How much a stretch's dancers move their arms, in torso lengths a second,
/// the body's own travel taken out: every dancer's elbows and wrists, the
/// median over the steps between samples, so one bad detection does not
/// make a stroll into a dance. Arms rather than legs, because a dance
/// filmed seated -- a hand dance in a row of seats -- is still a dance.
pub fn energy(samples: &[PoseSample]) -> Option<f64> {
    const ARMS: [usize; 4] = [7, 8, 9, 10];
    let mut steps: Vec<f64> = Vec::new();
    for w in samples.windows(2) {
        let seconds = w[1].at_us.0.saturating_sub(w[0].at_us.0) as f64 / 1e6;
        let (a, b) = (w[0].subjects(), w[1].subjects());
        if seconds <= 0.0 || a.is_empty() || a.len() != b.len() {
            continue;
        }
        for (x, y) in a.iter().zip(&b) {
            let (sx, sy) = (shape(x), shape(y));
            let moving: Vec<f64> = ARMS
                .iter()
                .filter_map(|&k| match (sx.points[k], sy.points[k]) {
                    (Some(p), Some(q)) => {
                        Some(((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt() / seconds)
                    }
                    _ => None,
                })
                .collect();
            if moving.len() >= 3 {
                steps.push(moving.iter().sum::<f64>() / moving.len() as f64);
            }
        }
    }
    if steps.len() < 2 {
        return None;
    }
    steps.sort_by(f64::total_cmp);
    Some(steps[steps.len() / 2])
}

/// The share of a stretch's samples in which everyone in it is seen whole
/// enough to dance: shoulders, elbows, wrists and hips in sight. A selfie
/// shows a face and a shoulder, a ride shows a horse's neck and somebody's
/// legs, a drive shows two hands on a wheel; none of them is a dance
/// however much it moves.
pub fn visible(samples: &[PoseSample]) -> f64 {
    const BODY: [(usize, usize); 4] = [(5, 6), (7, 8), (9, 10), (11, 12)];
    if samples.is_empty() {
        return 0.0;
    }
    let whole = samples
        .iter()
        .filter(|s| {
            let people = s.subjects();
            !people.is_empty()
                && people.iter().all(|p| {
                    BODY.iter().all(|&(l, r)| {
                        let seen = |k: usize| p.keypoints.get(k).is_some_and(|k| k[2] >= SURE);
                        if l == 11 {
                            // Seated, one hip is often hidden by the seat.
                            seen(l) || seen(r)
                        } else {
                            seen(l) && seen(r)
                        }
                    })
                })
        })
        .count();
    whole as f64 / samples.len() as f64
}

/// The share of a stretch's samples showing its usual number of people: a
/// dance has its dancers throughout, a street has passers-by coming and
/// going.
pub fn steady(samples: &[PoseSample]) -> f64 {
    let Some(n) = people(samples) else {
        return 0.0;
    };
    samples.iter().filter(|s| s.count() == n).count() as f64 / samples.len().max(1) as f64
}

/// Whole enough, often enough, to be dancing.
const VISIBLE: f64 = 0.6;
/// The same people, often enough.
const STEADY: f64 = 0.8;
/// Arms moving at least this much, in torso lengths a second (the median
/// over the stretch).
pub const MOVING: f64 = 0.35;

/// Is this stretch dancing? Its arm energy when it is: the dancers seen
/// whole, the same number of them throughout, their arms moving. Set on a
/// 126-clip trip against the clips a person picked out as dances -- a hand
/// dance in plane seats, duets by a lake and on a hill -- and the ones the
/// motion alone had taken for dances: selfies, a ride, a drive, a crowd.
pub fn dancing(samples: &[PoseSample]) -> Option<f64> {
    if visible(samples) < VISIBLE || steady(samples) < STEADY {
        return None;
    }
    energy(samples).filter(|e| *e >= MOVING)
}

/// How many people a stretch shows: the most common count across its
/// samples, ties to the larger.
pub fn people(samples: &[PoseSample]) -> Option<u32> {
    let mut counts = std::collections::BTreeMap::<u32, usize>::new();
    for s in samples {
        *counts.entry(s.count()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(n, _)| n)
}

/// The sample nearest `t`, when one is within `within` of it.
pub fn nearest(samples: &[PoseSample], t: Micros, within: Micros) -> Option<usize> {
    let (i, s) = samples
        .iter()
        .enumerate()
        .min_by_key(|(_, s)| s.at_us.0.abs_diff(t.0))?;
    (s.at_us.0.abs_diff(t.0) <= within.0).then_some(i)
}

/// What cutting from one stretch at `out_at` to another at `in_at` costs:
/// how far the pose, the placement and the movement jump. Low is a match
/// cut on the movement. [`MISMATCH`] when either end has no pose to read.
pub fn join_cost(
    out_samples: &[PoseSample],
    out_at: Micros,
    in_samples: &[PoseSample],
    in_at: Micros,
) -> f64 {
    let within = Micros(300_000);
    let (Some(a), Some(b)) = (
        nearest(out_samples, out_at, within),
        nearest(in_samples, in_at, within),
    ) else {
        return MISMATCH;
    };
    let pose = pose_distance(&out_samples[a], &in_samples[b]);
    if pose >= MISMATCH {
        return MISMATCH;
    }
    // The movement: into the outgoing frame, and out of the incoming one.
    let motion = match (a.checked_sub(1), in_samples.get(b + 1)) {
        (Some(before), Some(after)) => velocity_distance(
            (&out_samples[before], &out_samples[a]),
            (&in_samples[b], after),
        ),
        _ => None,
    };
    pose + 0.15 * motion.unwrap_or(2.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A person standing at `x` (0..1) with arms at `arm` (0 down, 1 up),
    /// `size` of the frame tall.
    pub(crate) fn body(x: f64, size: f64, arm: f64) -> PersonPose {
        let top = 0.5 - size / 2.0;
        let y = |f: f64| top + f * size;
        let w = size / 4.0;
        let mut k = vec![[x, y(0.05), 0.9]; 17];
        k[5] = [x - w / 2.0, y(0.2), 0.9];
        k[6] = [x + w / 2.0, y(0.2), 0.9];
        k[7] = [x - w * 0.7, y(0.35 - 0.3 * arm), 0.9];
        k[8] = [x + w * 0.7, y(0.35 - 0.3 * arm), 0.9];
        k[9] = [x - w * 0.8, y(0.5 - 0.5 * arm), 0.9];
        k[10] = [x + w * 0.8, y(0.5 - 0.5 * arm), 0.9];
        k[11] = [x - w / 3.0, y(0.55), 0.9];
        k[12] = [x + w / 3.0, y(0.55), 0.9];
        k[13] = [x - w / 3.0, y(0.75), 0.9];
        k[14] = [x + w / 3.0, y(0.75), 0.9];
        k[15] = [x - w / 3.0, y(0.98), 0.9];
        k[16] = [x + w / 3.0, y(0.98), 0.9];
        PersonPose {
            bbox: [x - w, top, x + w, top + size],
            score: 0.95,
            keypoints: k,
        }
    }

    fn at(ms: u64, people: Vec<PersonPose>) -> PoseSample {
        PoseSample {
            at_us: Micros::from_millis(ms),
            people,
        }
    }

    #[test]
    fn the_same_pose_matches_and_a_different_one_does_not() {
        let up = at(0, vec![body(0.5, 0.8, 1.0)]);
        let up_again = at(0, vec![body(0.5, 0.8, 1.0)]);
        let down = at(0, vec![body(0.5, 0.8, 0.0)]);
        assert!(pose_distance(&up, &up_again) < 1e-9);
        assert!(pose_distance(&up, &down) > 0.3);
    }

    #[test]
    fn the_shape_is_compared_wherever_the_dancer_stands_but_placement_counts() {
        let centre = at(0, vec![body(0.5, 0.8, 0.5)]);
        let aside = at(0, vec![body(0.2, 0.4, 0.5)]);
        let d = pose_distance(&centre, &aside);
        // Same shape, so only the placement term: well short of a mismatch.
        assert!(d > 0.1 && d < 1.5, "{d}");
    }

    #[test]
    fn a_duet_does_not_match_a_solo() {
        let solo = at(0, vec![body(0.5, 0.8, 0.5)]);
        let duet = at(0, vec![body(0.3, 0.8, 0.5), body(0.7, 0.8, 0.5)]);
        assert_eq!(pose_distance(&solo, &duet), MISMATCH);
        assert_eq!(people(&[duet.clone(), duet, solo]), Some(2));
    }

    #[test]
    fn waving_arms_are_energetic_and_standing_still_is_not() {
        let waving: Vec<PoseSample> = (0..8)
            .map(|i| at(i * 250, vec![body(0.5, 0.8, (i % 2) as f64)]))
            .collect();
        let still: Vec<PoseSample> = (0..8)
            .map(|i| at(i * 250, vec![body(0.5, 0.8, 0.3)]))
            .collect();
        let (moving, standing) = (energy(&waving).unwrap(), energy(&still).unwrap());
        assert!(moving > 1.0, "{moving}");
        assert!(standing < 0.01, "{standing}");
        assert!(dancing(&waving).is_some());
        assert!(dancing(&still).is_none());
    }

    #[test]
    fn a_body_seen_only_in_part_is_not_dancing_however_it_moves() {
        // A selfie: the arms wave, and nothing below the shoulders is seen.
        let selfie: Vec<PoseSample> = (0..8)
            .map(|i| {
                let mut p = body(0.5, 0.8, (i % 2) as f64);
                for k in [11, 12, 13, 14, 15, 16] {
                    p.keypoints[k][2] = 0.05;
                }
                at(i * 250, vec![p])
            })
            .collect();
        assert!(energy(&selfie).unwrap() > 1.0);
        assert_eq!(visible(&selfie), 0.0);
        assert!(dancing(&selfie).is_none());
    }

    #[test]
    fn passers_by_coming_and_going_are_not_a_dance() {
        let street: Vec<PoseSample> = (0..8)
            .map(|i| {
                let people = (0..=(i % 3))
                    .map(|n| body(0.2 + 0.3 * n as f64, 0.8, (i % 2) as f64))
                    .collect();
                at(i * 250, people)
            })
            .collect();
        assert!(steady(&street) < 0.8);
        assert!(dancing(&street).is_none());
    }

    #[test]
    fn a_join_that_keeps_the_movement_costs_less_than_one_that_breaks_it() {
        // One clip raising the arms, another raising them too, a third
        // lowering them, all passing the same pose at the join.
        let raise = |start: u64| -> Vec<PoseSample> {
            (0..5)
                .map(|i| at(start + i * 250, vec![body(0.5, 0.8, i as f64 / 4.0)]))
                .collect()
        };
        let lower: Vec<PoseSample> = (0..5)
            .map(|i| at(i * 250, vec![body(0.5, 0.8, 1.0 - i as f64 / 4.0)]))
            .collect();
        let a = raise(0);
        let b = raise(10_000);
        // Out of a at 0.5 s (arms half up, rising) into b at 10.5 s (the
        // same, still rising) or into lower at 0.5 s (half up, falling).
        let keeps = join_cost(
            &a,
            Micros::from_millis(500),
            &b,
            Micros::from_millis(10_500),
        );
        let breaks = join_cost(
            &a,
            Micros::from_millis(500),
            &lower,
            Micros::from_millis(500),
        );
        assert!(keeps < breaks, "{keeps} vs {breaks}");
        assert!(keeps < 0.05, "{keeps}");
    }
}
