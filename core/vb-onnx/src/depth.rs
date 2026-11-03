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

//! Depth Anything V2 Small, behind [`DepthAdapter`].
//!
//! Apache-2.0 at this size (Base and Large are CC-BY-NC and cannot ship).
//! It gives relative inverse depth -- larger is nearer -- which is exactly
//! what finding a near-plane occluder needs; metric depth would add nothing.

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_infer::{Adapter, DepthAdapter, DepthMap, InferError, RgbImage};

const NAME: &str = "DepthAnythingV2Small";

/// The model's patch size: both sides of its input must be a multiple.
const PATCH: u32 = 14;
/// The short side the model was trained at.
const SHORT_SIDE: u32 = 518;
/// ImageNet statistics, which the model's DINOv2 encoder expects.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

pub struct DepthAnything {
    model: PathBuf,
    /// Built on first use, so constructing the adapter costs nothing and a
    /// missing runtime surfaces as `Unavailable` from the first call.
    session: Mutex<Option<Session>>,
}

impl DepthAnything {
    pub fn new(model: impl Into<PathBuf>) -> DepthAnything {
        DepthAnything {
            model: model.into(),
            session: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }

    /// The input size for a picture: the short side at 518, the long side
    /// scaled to keep the shape, both rounded to the patch size.
    pub fn input_size(width: u32, height: u32) -> (u32, u32) {
        let round = |v: f64| ((v / PATCH as f64).round().max(1.0) as u32) * PATCH;
        let scale = SHORT_SIDE as f64 / width.min(height).max(1) as f64;
        (round(width as f64 * scale), round(height as f64 * scale))
    }
}

impl Adapter for DepthAnything {
    fn name(&self) -> &'static str {
        NAME
    }
}

impl DepthAdapter for DepthAnything {
    fn depth(&self, image: &RgbImage) -> Result<DepthMap, InferError> {
        if image.width == 0 || image.height == 0 {
            return Err(InferError::Failed {
                adapter: NAME,
                detail: "an empty picture".into(),
            });
        }
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(runtime::open_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");

        let (w, h) = Self::input_size(image.width, image.height);
        let pixels = normalised_chw(image, w, h);
        let input =
            Tensor::from_array(([1usize, 3, h as usize, w as usize], pixels)).map_err(|e| {
                InferError::Failed {
                    adapter: NAME,
                    detail: e.to_string(),
                }
            })?;
        let input_name = session.inputs()[0].name().to_string();
        let outputs = session
            .run(ort::inputs![input_name => input])
            .map_err(|e| InferError::Failed {
                adapter: NAME,
                detail: e.to_string(),
            })?;
        let (shape, values) =
            outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| InferError::BadOutput {
                    adapter: NAME,
                    detail: e.to_string(),
                })?;
        // [1, H, W] from this export; [1, 1, H, W] from others. The last two
        // dimensions are the map either way.
        let dims: Vec<i64> = shape.iter().copied().collect();
        let (oh, ow) = match dims.as_slice() {
            [.., oh, ow] => (*oh as u32, *ow as u32),
            _ => {
                return Err(InferError::BadOutput {
                    adapter: NAME,
                    detail: format!("unexpected output shape {dims:?}"),
                });
            }
        };
        if values.len() != (oh * ow) as usize {
            return Err(InferError::BadOutput {
                adapter: NAME,
                detail: format!("{} values for a {ow}x{oh} map", values.len()),
            });
        }
        Ok(DepthMap::normalised(ow, oh, values.to_vec()))
    }
}

/// Bilinear resize to `w` x `h`, then ImageNet normalisation, channels first.
fn normalised_chw(image: &RgbImage, w: u32, h: u32) -> Vec<f32> {
    let plane = (w * h) as usize;
    let mut out = vec![0f32; plane * 3];
    let sx = image.width as f32 / w as f32;
    let sy = image.height as f32 / h as f32;
    for y in 0..h {
        // Pixel centres, clamped to the picture.
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (image.height - 1) as f32);
        let y0 = fy.floor() as u32;
        let y1 = (y0 + 1).min(image.height - 1);
        let ty = fy - y0 as f32;
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (image.width - 1) as f32);
            let x0 = fx.floor() as u32;
            let x1 = (x0 + 1).min(image.width - 1);
            let tx = fx - x0 as f32;
            let (a, b, c, d) = (
                image.pixel(x0, y0),
                image.pixel(x1, y0),
                image.pixel(x0, y1),
                image.pixel(x1, y1),
            );
            let i = (y * w + x) as usize;
            for ch in 0..3 {
                let top = a[ch] as f32 * (1.0 - tx) + b[ch] as f32 * tx;
                let bottom = c[ch] as f32 * (1.0 - tx) + d[ch] as f32 * tx;
                let v = (top * (1.0 - ty) + bottom * ty) / 255.0;
                out[ch * plane + i] = (v - MEAN[ch]) / STD[ch];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_keeps_the_shape_on_the_patch_grid() {
        assert_eq!(DepthAnything::input_size(1920, 1080), (924, 518));
        assert_eq!(DepthAnything::input_size(1080, 1920), (518, 924));
        assert_eq!(DepthAnything::input_size(512, 512), (518, 518));
    }

    #[test]
    fn normalisation_is_channels_first() {
        let image = RgbImage {
            width: 1,
            height: 1,
            rgb: vec![255, 0, 128],
        };
        let out = normalised_chw(&image, 1, 1);
        assert!((out[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-5);
        assert!((out[1] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-5);
        assert!((out[2] - (128.0 / 255.0 - MEAN[2]) / STD[2]).abs() < 1e-5);
    }

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let adapter = DepthAnything::new("/nonexistent/depth.onnx");
        let image = RgbImage {
            width: 2,
            height: 2,
            rgb: vec![0; 12],
        };
        match adapter.depth(&image) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    /// With the model from the library and ONNX Runtime fetched:
    /// VB_DEPTH_MODEL=<depth-anything-v2-small.onnx> cargo test -p vb-onnx a_real_model -- --nocapture
    #[test]
    fn a_real_model_finds_the_near_side_of_a_ramp() {
        let Some(model) = std::env::var_os("VB_DEPTH_MODEL") else {
            eprintln!("skipped: VB_DEPTH_MODEL is not set");
            return;
        };
        // A floor receding upward: bright near the bottom edge, fading to a
        // flat sky. Whatever the model makes of it, the bottom rows should
        // read nearer than the top ones.
        let (w, h) = (320u32, 180u32);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                if y < h / 2 {
                    rgb.extend_from_slice(&[150, 190, 235]);
                } else {
                    let stripe = ((x / 16 + y / 8) % 2) as u8 * 60;
                    rgb.extend_from_slice(&[90 + stripe, 80 + stripe, 60]);
                }
            }
        }
        let image = RgbImage {
            width: w,
            height: h,
            rgb,
        };
        let adapter = DepthAnything::new(PathBuf::from(model));
        let started = std::time::Instant::now();
        let map = adapter.depth(&image).expect("the model runs");
        eprintln!(
            "{}x{} map in {:?}",
            map.width,
            map.height,
            started.elapsed()
        );
        let row = |y: u32| (0..map.width).map(|x| map.at(x, y)).sum::<f32>() / map.width as f32;
        let (top, bottom) = (row(map.height / 8), row(map.height - 1 - map.height / 8));
        assert!(
            bottom > top,
            "bottom {bottom} should be nearer than top {top}"
        );
    }
}
