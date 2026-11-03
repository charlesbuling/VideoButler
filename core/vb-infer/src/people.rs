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

//! The person-segmentation seam: which pixels are people.
//!
//! The director needs it to tell a subject from an occluder when both are
//! right against the lens. A face settles it when one is found, but the
//! people in a travel video wear masks, sunglasses and hoods, and a face
//! detector misses most of those; a person mask does not.

use crate::depth::RgbImage;
use crate::{Adapter, InferError};

/// How likely each pixel is to be a person, 0..1, row-major.
#[derive(Debug, Clone)]
pub struct PersonMask {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl PersonMask {
    /// The value at a point given in 0..1 picture coordinates.
    pub fn at_normalised(&self, x: f64, y: f64) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let px = ((x * self.width as f64) as u32).min(self.width - 1);
        let py = ((y * self.height as f64) as u32).min(self.height - 1);
        self.values[(py * self.width + px) as usize]
    }
}

pub trait PersonSegmenter: Adapter + Send + Sync {
    fn people(&self, image: &RgbImage) -> Result<PersonMask, InferError>;
}

/// Sees nobody.
#[derive(Debug, Default, Clone)]
pub struct MockPeople;

impl Adapter for MockPeople {
    fn name(&self) -> &'static str {
        "MockPeople"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl PersonSegmenter for MockPeople {
    fn people(&self, _image: &RgbImage) -> Result<PersonMask, InferError> {
        Ok(PersonMask {
            width: 1,
            height: 1,
            values: vec![0.0],
        })
    }
}
