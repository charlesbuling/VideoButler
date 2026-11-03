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

//! YuNet, behind [`FaceDetector`].
//!
//! OpenCV's own face detector, MIT, 232 KB, and ONNX as released. MediaPipe's
//! detector, which the development doc names, is published only as TFLite;
//! YuNet runs on the engine the app already carries, and finds the small
//! and turned faces a travel video is full of.

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_infer::{Adapter, FaceBox, FaceDetector, InferError, RgbImage};

const NAME: &str = "YuNet";

/// The released model takes a fixed square.
const SIDE: u32 = 640;
const STRIDES: [u32; 3] = [8, 16, 32];

pub struct YuNet {
    model: PathBuf,
    session: Mutex<Option<Session>>,
    /// Detections below this are dropped. OpenCV's sample uses 0.9, tuned
    /// for frontal faces at a desk; 0.5 keeps turned heads and sunglasses,
    /// and is what the director's occlusion reading was set against. A weak
    /// face never excuses an occluder on its own -- it has to make up a
    /// share of the region, or be clear.
    pub min_confidence: f32,
}

impl YuNet {
    pub fn new(model: impl Into<PathBuf>) -> YuNet {
        YuNet {
            model: model.into(),
            session: Mutex::new(None),
            min_confidence: 0.5,
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }
}

impl Adapter for YuNet {
    fn name(&self) -> &'static str {
        NAME
    }
}

impl FaceDetector for YuNet {
    fn faces(&self, image: &RgbImage) -> Result<Vec<FaceBox>, InferError> {
        if image.width == 0 || image.height == 0 {
            return Ok(Vec::new());
        }
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(runtime::open_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");

        // Scale the picture into the square, keeping its shape, and pad the
        // rest with black. The model wants BGR, 0..255, channels first.
        let scale = (SIDE as f32 / image.width as f32).min(SIDE as f32 / image.height as f32);
        let (sw, sh) = (
            ((image.width as f32 * scale).round() as u32).clamp(1, SIDE),
            ((image.height as f32 * scale).round() as u32).clamp(1, SIDE),
        );
        let plane = (SIDE * SIDE) as usize;
        let mut input = vec![0f32; plane * 3];
        for y in 0..sh {
            let sy = ((y as f32 + 0.5) / scale).min((image.height - 1) as f32) as u32;
            for x in 0..sw {
                let sx = ((x as f32 + 0.5) / scale).min((image.width - 1) as f32) as u32;
                let [r, g, b] = image.pixel(sx, sy);
                let i = (y * SIDE + x) as usize;
                input[i] = b as f32;
                input[plane + i] = g as f32;
                input[2 * plane + i] = r as f32;
            }
        }
        let failed = |e: ort::Error| InferError::Failed {
            adapter: NAME,
            detail: e.to_string(),
        };
        let tensor = Tensor::from_array(([1usize, 3, SIDE as usize, SIDE as usize], input))
            .map_err(failed)?;
        let outputs = session
            .run(ort::inputs!["input" => tensor])
            .map_err(failed)?;
        let get = |name: &str| -> Result<Vec<f32>, InferError> {
            let value = outputs.get(name).ok_or_else(|| InferError::BadOutput {
                adapter: NAME,
                detail: format!("no output {name}"),
            })?;
            let (_, data) =
                value
                    .try_extract_tensor::<f32>()
                    .map_err(|e| InferError::BadOutput {
                        adapter: NAME,
                        detail: e.to_string(),
                    })?;
            Ok(data.to_vec())
        };

        let mut found = Vec::new();
        for stride in STRIDES {
            let (cls, obj, bbox, kps) = (
                get(&format!("cls_{stride}"))?,
                get(&format!("obj_{stride}"))?,
                get(&format!("bbox_{stride}"))?,
                get(&format!("kps_{stride}"))?,
            );
            let cols = SIDE / stride;
            for (idx, (&c, &o)) in cls.iter().zip(&obj).enumerate() {
                let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
                if score < self.min_confidence {
                    continue;
                }
                let (col, row) = ((idx as u32 % cols) as f32, (idx as u32 / cols) as f32);
                let s = stride as f32;
                let b = &bbox[idx * 4..idx * 4 + 4];
                let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
                let (w, h) = (b[2].exp() * s, b[3].exp() * s);
                let k = &kps[idx * 10..idx * 10 + 10];
                let mut landmarks = [[0f32; 2]; 5];
                for (n, point) in landmarks.iter_mut().enumerate() {
                    *point = [(k[2 * n] + col) * s, (k[2 * n + 1] + row) * s];
                }
                found.push(Detection {
                    x: cx - w / 2.0,
                    y: cy - h / 2.0,
                    w,
                    h,
                    score,
                    landmarks,
                });
            }
        }

        // Back from the square to the picture, normalised.
        let (pw, ph) = (image.width as f32 * scale, image.height as f32 * scale);
        Ok(suppress(found, 0.3)
            .into_iter()
            .map(|d| {
                let nx = |v: f32| (v / pw).clamp(0.0, 1.0) as f64;
                let ny = |v: f32| (v / ph).clamp(0.0, 1.0) as f64;
                let (x0, y0) = (nx(d.x), ny(d.y));
                FaceBox {
                    x: x0,
                    y: y0,
                    w: nx(d.x + d.w) - x0,
                    h: ny(d.y + d.h) - y0,
                    confidence: d.score as f64,
                    landmarks: d.landmarks.map(|[x, y]| [nx(x), ny(y)]),
                }
            })
            .filter(|f| f.w > 0.0 && f.h > 0.0)
            .collect())
    }
}

#[derive(Debug, Clone)]
struct Detection {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
    landmarks: [[f32; 2]; 5],
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let union = a.w * a.h + b.w * b.h - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Keep the best of each cluster of overlapping detections.
fn suppress(mut found: Vec<Detection>, max_iou: f32) -> Vec<Detection> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::new();
    for d in found {
        if kept.iter().all(|k| iou(k, &d) <= max_iou) {
            kept.push(d);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(x: f32, score: f32) -> Detection {
        Detection {
            x,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            score,
            landmarks: [[0.0; 2]; 5],
        }
    }

    #[test]
    fn overlapping_detections_collapse_to_the_best() {
        let kept = suppress(vec![det(0.0, 0.8), det(1.0, 0.95), det(30.0, 0.75)], 0.3);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.95);
        assert_eq!(kept[1].x, 30.0);
    }

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let adapter = YuNet::new("/nonexistent/yunet.onnx");
        let image = RgbImage {
            width: 4,
            height: 4,
            rgb: vec![0; 48],
        };
        match adapter.faces(&image) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn a_blank_picture_has_no_faces() {
        let Some(model) = std::env::var_os("VB_FACE_MODEL") else {
            eprintln!("skipped: VB_FACE_MODEL is not set");
            return;
        };
        let image = RgbImage {
            width: 320,
            height: 180,
            rgb: vec![128; 320 * 180 * 3],
        };
        assert!(
            YuNet::new(PathBuf::from(model))
                .faces(&image)
                .unwrap()
                .is_empty()
        );
    }
}
