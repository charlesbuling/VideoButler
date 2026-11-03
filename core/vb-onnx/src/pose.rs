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

//! RTMO-s, behind [`PoseEstimator`].
//!
//! The one-stage multi-person model of OpenMMLab's RTMPose family,
//! Apache-2.0: one pass gives every person's box and seventeen COCO body
//! keypoints, with no person detector to run first. 40 MB as ONNX.
//!
//! It reads a 640 x 640 picture: the frame scaled to fit, laid at the top
//! left, the rest filled with grey 114, in BGR and 0..255 -- how MMPose
//! trained it and how rtmlib feeds it.

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_infer::pose::KEYPOINTS;
use vb_infer::{Adapter, BodyPose, InferError, PoseEstimator, RgbImage};

const NAME: &str = "RTMO-s";
const SIDE: u32 = 640;
const PAD: f32 = 114.0;
/// Below this the model is guessing at a person.
const MIN_SCORE: f32 = 0.3;

pub struct RtmoPose {
    model: PathBuf,
    session: Mutex<Option<Session>>,
}

impl RtmoPose {
    pub fn new(model: impl Into<PathBuf>) -> RtmoPose {
        RtmoPose {
            model: model.into(),
            session: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }
}

impl Adapter for RtmoPose {
    fn name(&self) -> &'static str {
        NAME
    }
}

/// The picture as the model reads it, and the scale that got it there.
fn letterboxed(image: &RgbImage) -> (Vec<f32>, f32) {
    let scale = (SIDE as f32 / image.width as f32).min(SIDE as f32 / image.height as f32);
    let w = ((image.width as f32 * scale).round() as u32).clamp(1, SIDE);
    let h = ((image.height as f32 * scale).round() as u32).clamp(1, SIDE);
    let plane = (SIDE * SIDE) as usize;
    let mut out = vec![PAD; plane * 3];
    for y in 0..h {
        // Bilinear, sampling at pixel centres, as OpenCV's resize does.
        let fy = ((y as f32 + 0.5) / scale - 0.5).clamp(0.0, (image.height - 1) as f32);
        let (y0, wy) = (fy.floor() as u32, fy - fy.floor());
        let y1 = (y0 + 1).min(image.height - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / scale - 0.5).clamp(0.0, (image.width - 1) as f32);
            let (x0, wx) = (fx.floor() as u32, fx - fx.floor());
            let x1 = (x0 + 1).min(image.width - 1);
            let (a, b, c, d) = (
                image.pixel(x0, y0),
                image.pixel(x1, y0),
                image.pixel(x0, y1),
                image.pixel(x1, y1),
            );
            let i = (y * SIDE + x) as usize;
            for ch in 0..3 {
                let top = a[ch] as f32 * (1.0 - wx) + b[ch] as f32 * wx;
                let bottom = c[ch] as f32 * (1.0 - wx) + d[ch] as f32 * wx;
                // RGB in, BGR out: channel 0 of the tensor is blue.
                out[(2 - ch) * plane + i] = top * (1.0 - wy) + bottom * wy;
            }
        }
    }
    (out, scale)
}

impl PoseEstimator for RtmoPose {
    fn poses(&self, image: &RgbImage) -> Result<Vec<BodyPose>, InferError> {
        if image.width == 0 || image.height == 0 {
            return Ok(Vec::new());
        }
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(runtime::open_fixed_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");

        let (pixels, scale) = letterboxed(image);
        let failed = |e: ort::Error| InferError::Failed {
            adapter: NAME,
            detail: e.to_string(),
        };
        let tensor = Tensor::from_array(([1usize, 3, SIDE as usize, SIDE as usize], pixels))
            .map_err(failed)?;
        let input_name = session.inputs()[0].name().to_string();
        let outputs = session
            .run(ort::inputs![input_name => tensor])
            .map_err(failed)?;
        let bad = |detail: String| InferError::BadOutput {
            adapter: NAME,
            detail,
        };
        // dets: [1, N, 5] as x1 y1 x2 y2 score; keypoints: [1, N, 17, 3].
        let (dets_shape, dets) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| bad(e.to_string()))?;
        let (_, points) = outputs[1]
            .try_extract_tensor::<f32>()
            .map_err(|e| bad(e.to_string()))?;
        let n = dets_shape.get(1).copied().unwrap_or(0).max(0) as usize;
        if dets.len() < n * 5 || points.len() < n * KEYPOINTS * 3 {
            return Err(bad(format!(
                "{} box values and {} keypoint values for {n} people",
                dets.len(),
                points.len()
            )));
        }
        let back = |v: f32| v / scale;
        let mut people: Vec<BodyPose> = (0..n)
            .filter(|&i| dets[i * 5 + 4] >= MIN_SCORE)
            .map(|i| {
                let d = &dets[i * 5..i * 5 + 5];
                let mut keypoints = [[0f32; 3]; KEYPOINTS];
                for (k, point) in keypoints.iter_mut().enumerate() {
                    let at = (i * KEYPOINTS + k) * 3;
                    *point = [
                        back(points[at]).clamp(0.0, image.width as f32),
                        back(points[at + 1]).clamp(0.0, image.height as f32),
                        points[at + 2].clamp(0.0, 1.0),
                    ];
                }
                BodyPose {
                    bbox: [
                        back(d[0]).clamp(0.0, image.width as f32),
                        back(d[1]).clamp(0.0, image.height as f32),
                        back(d[2]).clamp(0.0, image.width as f32),
                        back(d[3]).clamp(0.0, image.height as f32),
                    ],
                    score: d[4],
                    keypoints,
                }
            })
            .collect();
        people.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(people)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let adapter = RtmoPose::new("/nonexistent/rtmo.onnx");
        let image = RgbImage {
            width: 4,
            height: 4,
            rgb: vec![0; 48],
        };
        match adapter.poses(&image) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn the_picture_is_fitted_top_left_in_bgr_with_grey_around_it() {
        // A red 64 x 32 picture: scaled by 10 to 640 x 320, the rest grey.
        let mut rgb = Vec::new();
        for _ in 0..64 * 32 {
            rgb.extend_from_slice(&[255, 0, 0]);
        }
        let (tensor, scale) = letterboxed(&RgbImage {
            width: 64,
            height: 32,
            rgb,
        });
        assert_eq!(scale, 10.0);
        let plane = (SIDE * SIDE) as usize;
        let at = |ch: usize, x: u32, y: u32| tensor[ch * plane + (y * SIDE + x) as usize];
        assert_eq!(at(2, 10, 10), 255.0, "red lands in the last channel");
        assert_eq!(at(0, 10, 10), 0.0);
        assert_eq!(at(2, 10, 400), PAD, "below the picture is padding");
    }

    /// `VB_POSE_FRAME` names a raw RGB24 frame, `VB_POSE_SIZE` its size
    /// as `WxH`: what `ffmpeg -f rawvideo -pix_fmt rgb24` writes.
    #[test]
    fn a_real_model_finds_the_people_in_a_frame() {
        let (Some(model), Some(frame), Some(size)) = (
            std::env::var_os("VB_POSE_MODEL"),
            std::env::var_os("VB_POSE_FRAME"),
            std::env::var("VB_POSE_SIZE").ok(),
        ) else {
            eprintln!("skipped: VB_POSE_MODEL, VB_POSE_FRAME and VB_POSE_SIZE are not all set");
            return;
        };
        let (w, h) = size.split_once('x').expect("WxH");
        let image = RgbImage {
            width: w.parse().unwrap(),
            height: h.parse().unwrap(),
            rgb: std::fs::read(frame).unwrap(),
        };
        let adapter = RtmoPose::new(model);
        let started = std::time::Instant::now();
        let people = adapter.poses(&image).unwrap();
        let first = started.elapsed();
        let started = std::time::Instant::now();
        let _ = adapter.poses(&image).unwrap();
        eprintln!("first {first:?}, then {:?}", started.elapsed());
        for p in &people {
            eprintln!("{:.2} {:?}", p.score, p.bbox);
            for k in &p.keypoints {
                eprintln!("  {:.0},{:.0} {:.2}", k[0], k[1], k[2]);
            }
        }
        assert!(!people.is_empty(), "the frame has a person in it");
    }

    #[test]
    fn a_real_model_finds_nobody_in_an_empty_room() {
        let Some(model) = std::env::var_os("VB_POSE_MODEL") else {
            eprintln!("skipped: VB_POSE_MODEL is not set");
            return;
        };
        let adapter = RtmoPose::new(model);
        let image = RgbImage {
            width: 320,
            height: 180,
            rgb: vec![200; 320 * 180 * 3],
        };
        assert!(adapter.poses(&image).unwrap().is_empty());
    }
}
