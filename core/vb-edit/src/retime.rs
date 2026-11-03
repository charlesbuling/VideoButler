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

//! Keeping a slot's keyed placement with its footage when the slot's own
//! clock changes: cut in two, trimmed at either end, sped up or slowed.
//!
//! Keyframe offsets are on the finished piece's clock, from the slot's
//! start. An edit that changes that clock maps each moment of the old slot
//! to the new one by a straight line -- a shift for a trim, a stretch for a
//! speed -- and the placement in the new slot is the old one at the moment
//! that maps there: the motion stays with the picture.

use vb_contracts::Micros;
use vb_contracts::editing_dsl::{Transform, TransformKeyframe};

/// `transform` on a slot whose clock maps an old offset `o` to the new
/// `o * scale + shift`, cut to the new slot's `duration`.
pub(crate) fn retime(
    transform: &Transform,
    scale: f64,
    shift: i64,
    duration: Micros,
) -> Option<Transform> {
    if transform.keyframes.is_empty() {
        return None;
    }
    let to_new = |o: Micros| (o.0 as f64 * scale).round() as i64 + shift;
    let to_old = |n: i64| {
        let o = ((n - shift) as f64 / scale).round();
        Micros(o.max(0.0) as u64)
    };
    let d = duration.0 as i64;
    let at = |n: i64| {
        transform
            .at(to_old(n))
            .map(|k| TransformKeyframe {
                offset_us: Micros(n.clamp(0, d) as u64),
                ..k
            })
            .expect("a transform with keyframes has a placement everywhere")
    };
    let mut keyframes = vec![at(0)];
    for k in &transform.keyframes {
        let n = to_new(k.offset_us);
        if n > 0 && n < d {
            keyframes.push(TransformKeyframe {
                offset_us: Micros(n as u64),
                ..*k
            });
        }
    }
    keyframes.push(at(d));
    Some(simplify(keyframes))
}

/// Drop keyframes that repeat the placement either side of them; one that
/// never changes is one keyframe.
pub(crate) fn simplify(mut keyframes: Vec<TransformKeyframe>) -> Transform {
    keyframes.sort_by_key(|k| k.offset_us);
    keyframes.dedup_by(|b, a| a.offset_us == b.offset_us);
    if keyframes.iter().all(|k| k.same_placement(&keyframes[0])) {
        keyframes.truncate(1);
        keyframes[0].offset_us = Micros::ZERO;
        return Transform { keyframes };
    }
    let mut out: Vec<TransformKeyframe> = Vec::with_capacity(keyframes.len());
    for (i, k) in keyframes.iter().enumerate() {
        let prev_same = i > 0 && keyframes[i - 1].same_placement(k);
        let next_same = keyframes.get(i + 1).is_some_and(|n| n.same_placement(k));
        // A keyframe in the middle of a hold says nothing the two ends do
        // not.
        if prev_same && next_same {
            continue;
        }
        out.push(*k);
    }
    Transform { keyframes: out }
}

/// Whether a transform leaves the picture fitted the whole time.
pub(crate) fn is_fitted(transform: &Transform) -> bool {
    transform.keyframes.iter().all(TransformKeyframe::is_fitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kf(offset_ms: u64, scale: f64) -> TransformKeyframe {
        TransformKeyframe {
            scale,
            ..TransformKeyframe::fitted(Micros::from_millis(offset_ms))
        }
    }

    #[test]
    fn a_trim_at_the_start_keeps_the_motion_on_its_picture() {
        // 1.0 to 2.0 over four seconds; two seconds trimmed off the front
        // leaves 1.5 to 2.0 over the two left.
        let t = Transform {
            keyframes: vec![kf(0, 1.0), kf(4000, 2.0)],
        };
        let r = retime(&t, 1.0, -2_000_000, Micros::from_secs(2)).unwrap();
        assert_eq!(r.keyframes.len(), 2);
        assert!((r.keyframes[0].scale - 1.5).abs() < 1e-9);
        assert_eq!(r.keyframes[0].offset_us, Micros::ZERO);
        assert!((r.keyframes[1].scale - 2.0).abs() < 1e-9);
        assert_eq!(r.keyframes[1].offset_us, Micros::from_secs(2));
    }

    #[test]
    fn twice_the_speed_halves_the_offsets() {
        let t = Transform {
            keyframes: vec![kf(0, 1.0), kf(4000, 2.0)],
        };
        let r = retime(&t, 0.5, 0, Micros::from_secs(2)).unwrap();
        assert_eq!(r.keyframes[1].offset_us, Micros::from_secs(2));
        assert!((r.keyframes[1].scale - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_hold_stays_one_keyframe() {
        let t = Transform {
            keyframes: vec![kf(0, 1.3)],
        };
        let r = retime(&t, 1.0, -1_000_000, Micros::from_secs(1)).unwrap();
        assert_eq!(r.keyframes.len(), 1);
        assert!((r.keyframes[0].scale - 1.3).abs() < 1e-9);
    }
}
