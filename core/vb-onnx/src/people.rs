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

//! PP-HumanSeg, behind [`PersonSegmenter`].
//!
//! PaddlePaddle's portrait segmentation, Apache-2.0, 6 MB, from OpenCV's
//! model zoo as ONNX. Coarse at 192 x 192, which is plenty for deciding
//! whether a region filling a fifth of the frame is a person.

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_infer::{Adapter, InferError, PersonMask, PersonSegmenter, RgbImage};

const NAME: &str = "PPHumanSeg";
const SIDE: u32 = 192;

pub struct PpHumanSeg {
    model: PathBuf,
    session: Mutex<Option<Session>>,
}

impl PpHumanSeg {
    pub fn new(model: impl Into<PathBuf>) -> PpHumanSeg {
        PpHumanSeg {
            model: model.into(),
            session: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }
}

impl Adapter for PpHumanSeg {
    fn name(&self) -> &'static str {
        NAME
    }
}

impl PersonSegmenter for PpHumanSeg {
    fn people(&self, image: &RgbImage) -> Result<PersonMask, InferError> {
        if image.width == 0 || image.height == 0 {
            return Ok(PersonMask {
                width: 0,
                height: 0,
                values: Vec::new(),
            });
        }
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(runtime::open_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");

        // Squashed to the square, as the model was trained: RGB, scaled to
        // -1..1, channels first.
        let plane = (SIDE * SIDE) as usize;
        let mut input = vec![0f32; plane * 3];
        for y in 0..SIDE {
            let sy = (((y as f32 + 0.5) * image.height as f32 / SIDE as f32) as u32)
                .min(image.height - 1);
            for x in 0..SIDE {
                let sx = (((x as f32 + 0.5) * image.width as f32 / SIDE as f32) as u32)
                    .min(image.width - 1);
                let px = image.pixel(sx, sy);
                let i = (y * SIDE + x) as usize;
                for (ch, &v) in px.iter().enumerate() {
                    input[ch * plane + i] = (v as f32 / 255.0 - 0.5) / 0.5;
                }
            }
        }
        let failed = |e: ort::Error| InferError::Failed {
            adapter: NAME,
            detail: e.to_string(),
        };
        let tensor = Tensor::from_array(([1usize, 3, SIDE as usize, SIDE as usize], input))
            .map_err(failed)?;
        let input_name = session.inputs()[0].name().to_string();
        let outputs = session
            .run(ort::inputs![input_name => tensor])
            .map_err(failed)?;
        let (_, data) =
            outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| InferError::BadOutput {
                    adapter: NAME,
                    detail: e.to_string(),
                })?;
        if data.len() != plane * 2 {
            return Err(InferError::BadOutput {
                adapter: NAME,
                detail: format!("{} values for two {SIDE}x{SIDE} planes", data.len()),
            });
        }
        // Two planes, background and person. OpenCV's export ends in a
        // softmax already, so the pair sums to one; another export might
        // stop at the logits, which get one here. Applying it twice would
        // pull every pixel towards a half.
        let probabilities = (0..plane)
            .step_by(97)
            .all(|i| (data[i] + data[plane + i] - 1.0).abs() < 0.01);
        let values = (0..plane)
            .map(|i| {
                let (bg, person) = (data[i], data[plane + i]);
                if probabilities {
                    person.clamp(0.0, 1.0)
                } else {
                    let m = bg.max(person);
                    let (eb, ep) = ((bg - m).exp(), (person - m).exp());
                    ep / (eb + ep)
                }
            })
            .collect();
        Ok(PersonMask {
            width: SIDE,
            height: SIDE,
            values,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let adapter = PpHumanSeg::new("/nonexistent/pphumanseg.onnx");
        let image = RgbImage {
            width: 4,
            height: 4,
            rgb: vec![0; 48],
        };
        match adapter.people(&image) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_blank_wall_is_nobody() {
        let Some(model) = std::env::var_os("VB_PERSON_MODEL") else {
            eprintln!("skipped: VB_PERSON_MODEL is not set");
            return;
        };
        let image = RgbImage {
            width: 320,
            height: 180,
            rgb: vec![200; 320 * 180 * 3],
        };
        let mask = PpHumanSeg::new(PathBuf::from(model))
            .people(&image)
            .unwrap();
        let mean = mask.values.iter().sum::<f32>() / mask.values.len() as f32;
        assert!(mean < 0.2, "mean person probability {mean} on a blank wall");
    }
}
