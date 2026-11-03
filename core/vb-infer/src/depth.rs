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

//! The depth seam: how far each part of a picture is from the camera.
//!
//! Composition is three-dimensional. A car door or a sleeve filling the near
//! plane over half the frame is foreground occlusion, and no reading of
//! sharpness or colour tells it apart from texture -- both are detailed and
//! both are in focus. A depth map says it directly, so the director reads
//! layers from here rather than from a description of the frame.

use crate::{Adapter, InferError};

/// A picture as 8-bit RGB, row-major, three bytes a pixel, upright.
#[derive(Debug, Clone)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl RgbImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 3) as usize;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

/// Relative nearness over the picture, row-major.
///
/// `values` run 0.0 (the farthest thing in this picture) to 1.0 (the
/// nearest). They are relative to the picture, not metres: monocular depth
/// is only defined up to scale and shift, so two maps compare by shape, not
/// by value.
#[derive(Debug, Clone)]
pub struct DepthMap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl DepthMap {
    pub fn at(&self, x: u32, y: u32) -> f32 {
        self.values[(y * self.width + x) as usize]
    }

    /// Rescale raw model output so the farthest point is 0 and the nearest 1.
    /// A picture with no depth at all (a flat wall) comes back all zero.
    pub fn normalised(width: u32, height: u32, mut values: Vec<f32>) -> DepthMap {
        let (lo, hi) = values
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let span = hi - lo;
        for v in &mut values {
            *v = if span > f32::EPSILON {
                (*v - lo) / span
            } else {
                0.0
            };
        }
        DepthMap {
            width,
            height,
            values,
        }
    }
}

pub trait DepthAdapter: Adapter + Send + Sync {
    fn depth(&self, image: &RgbImage) -> Result<DepthMap, InferError>;
}

/// Sees every picture as flat.
///
/// A flat map has no near plane, so nothing is ever judged occluded: the
/// mock can only fail to find a problem, never invent one.
#[derive(Debug, Default, Clone)]
pub struct MockDepth;

impl Adapter for MockDepth {
    fn name(&self) -> &'static str {
        "MockDepth"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl DepthAdapter for MockDepth {
    fn depth(&self, image: &RgbImage) -> Result<DepthMap, InferError> {
        Ok(DepthMap {
            width: image.width,
            height: image.height,
            values: vec![0.0; (image.width * image.height) as usize],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalising_spans_the_nearest_and_the_farthest() {
        let map = DepthMap::normalised(2, 2, vec![2.0, 4.0, 6.0, 10.0]);
        assert_eq!(map.values, vec![0.0, 0.25, 0.5, 1.0]);
        assert_eq!(map.at(1, 1), 1.0);
    }

    #[test]
    fn a_flat_picture_has_no_near_plane() {
        let map = DepthMap::normalised(2, 1, vec![3.0, 3.0]);
        assert_eq!(map.values, vec![0.0, 0.0]);
    }

    #[test]
    fn the_mock_is_flat_and_says_so() {
        let image = RgbImage {
            width: 4,
            height: 2,
            rgb: vec![0; 24],
        };
        let map = MockDepth.depth(&image).unwrap();
        assert!(map.values.iter().all(|&v| v == 0.0));
        assert!(MockDepth.is_mock());
    }
}
