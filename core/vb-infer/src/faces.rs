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

//! The face-detection seam: where the faces are in one picture.
//!
//! Detection only. Grouping faces across clips is the tracker's job
//! ([`crate::TrackerAdapter`]); this answers the director's questions about a
//! single frame -- is the near thing a person or a door, is the face cut off,
//! are the eyes on the lens.

use crate::depth::RgbImage;
use crate::{Adapter, InferError};

/// One face, in picture coordinates normalised to 0..1, origin top left.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub confidence: f64,
    /// Right eye, left eye, nose tip, right and left mouth corner, as the
    /// person faces the camera. Same coordinates as the box.
    pub landmarks: [[f64; 2]; 5],
}

impl FaceBox {
    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn area(&self) -> f64 {
        self.w * self.h
    }
}

pub trait FaceDetector: Adapter + Send + Sync {
    fn faces(&self, image: &RgbImage) -> Result<Vec<FaceBox>, InferError>;
}

/// Finds no faces.
///
/// Inventing one would be worse than missing one: the director treats a
/// face as a subject, and a fabricated subject would shield a real occluder.
#[derive(Debug, Default, Clone)]
pub struct MockFaces;

impl Adapter for MockFaces {
    fn name(&self) -> &'static str {
        "MockFaces"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl FaceDetector for MockFaces {
    fn faces(&self, _image: &RgbImage) -> Result<Vec<FaceBox>, InferError> {
        Ok(Vec::new())
    }
}
