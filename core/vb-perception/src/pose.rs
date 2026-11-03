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

//! Body poses as the Footage Graph records them.
//!
//! The pose model reports people in the pixels of the frame it read; the
//! graph keeps them as fractions of the upright frame, so a pose from a
//! portrait clip compares with one from a landscape clip by where the body
//! sits in the picture, and rounded, because a thousandth of the frame is
//! finer than any keypoint is sure of.

use vb_contracts::Micros;
use vb_contracts::footage_graph::{PersonPose, PoseSample};
use vb_infer::BodyPose;

fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// The people the model found at `at`, in the graph's terms.
pub fn sample(at: Micros, found: &[BodyPose], width: u32, height: u32) -> PoseSample {
    let (w, h) = (width.max(1) as f64, height.max(1) as f64);
    PoseSample {
        at_us: at,
        people: found
            .iter()
            .map(|p| PersonPose {
                bbox: [
                    round(p.bbox[0] as f64 / w),
                    round(p.bbox[1] as f64 / h),
                    round(p.bbox[2] as f64 / w),
                    round(p.bbox[3] as f64 / h),
                ],
                score: round(p.score as f64),
                keypoints: p
                    .keypoints
                    .iter()
                    .map(|k| {
                        [
                            round(k[0] as f64 / w),
                            round(k[1] as f64 / h),
                            round(k[2] as f64),
                        ]
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// How many people a frame shows: see [`PoseSample::subjects`].
pub fn count(sample: &PoseSample) -> u32 {
    sample.count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(top: f32, bottom: f32, score: f32) -> BodyPose {
        BodyPose {
            bbox: [100.0, top, 200.0, bottom],
            score,
            keypoints: [[150.0, top, 0.9]; 17],
        }
    }

    #[test]
    fn the_graph_keeps_fractions_of_the_frame() {
        let s = sample(Micros::from_secs(3), &[person(0.0, 360.0, 0.97)], 640, 360);
        assert_eq!(s.at_us, Micros::from_secs(3));
        assert_eq!(s.people[0].bbox, [0.156, 0.0, 0.313, 1.0]);
        assert_eq!(s.people[0].keypoints.len(), 17);
        assert_eq!(s.people[0].keypoints[0], [0.234, 0.0, 0.9]);
    }

    #[test]
    fn a_passer_by_behind_the_dancers_is_not_counted() {
        let s = sample(
            Micros::ZERO,
            &[
                person(40.0, 340.0, 0.95),
                person(60.0, 330.0, 0.9),
                // Small and far behind them.
                person(150.0, 190.0, 0.8),
                // Unsure.
                person(50.0, 300.0, 0.35),
            ],
            640,
            360,
        );
        assert_eq!(count(&s), 2);
    }
}
