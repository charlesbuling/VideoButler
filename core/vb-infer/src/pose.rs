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

//! The body-pose seam: who is in the frame and how each is standing.
//!
//! Dance needs it twice. Whether a moment is dancing at all, and by how
//! many, is read from the people and how their limbs move; and a dance cut
//! that carries the movement across the join -- the dancer's arm still
//! rising as the scene changes -- is found by comparing the pose at the end
//! of one clip with the pose at the start of the next.

use crate::depth::RgbImage;
use crate::{Adapter, InferError};

/// COCO's seventeen body keypoints, in the order every pose model here
/// reports them.
pub const KEYPOINTS: usize = 17;

/// One person, in the pixels of the image that was read.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyPose {
    /// Left, top, right, bottom.
    pub bbox: [f32; 4],
    pub score: f32,
    /// x, y and confidence for each of the seventeen keypoints.
    pub keypoints: [[f32; 3]; KEYPOINTS],
}

pub trait PoseEstimator: Adapter + Send + Sync {
    /// Everyone the model is reasonably sure of, most confident first.
    fn poses(&self, image: &RgbImage) -> Result<Vec<BodyPose>, InferError>;
}

/// Sees nobody, and says it is a mock.
#[derive(Debug, Default, Clone)]
pub struct MockPose;

impl Adapter for MockPose {
    fn name(&self) -> &'static str {
        "MockPose"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl PoseEstimator for MockPose {
    fn poses(&self, _image: &RgbImage) -> Result<Vec<BodyPose>, InferError> {
        Ok(Vec::new())
    }
}
