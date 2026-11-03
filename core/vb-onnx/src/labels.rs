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

//! SigLIP 2, behind [`VlmAdapter`]: scene labels where there is no Apple
//! Vision.
//!
//! Google's SigLIP 2 (base, patch 16, 224 px; Apache-2.0) as onnx-community
//! exported it, in half precision, 186 MB: the image half only. Not the
//! export's int8, which ONNX Runtime ran one way on a Mac and another on
//! Android -- a horse ride there read as nothing, a lake as a desert; this
//! one answers as full precision does on both. It embeds a
//! picture, and the embedding is compared with sentences naming each label
//! of `data/scene-labels.tsv` -- "this is a photo of a lake." -- whose
//! embeddings `scripts/make-scene-labels.sh` made once with the text half and
//! the app carries as `data/scene-labels.f32`. Zero-shot, trained on
//! nothing here.
//!
//! Like Apple Vision it gives labels, not a sentence: at most one place, one
//! subject and one activity, `"lake, couple, dancing"`. Checked against 92
//! frames of two real sets looked at one by one, a place it names is right
//! more than four times in five and it names one for two frames in five; a
//! subject, four in five and half the frames; an activity, six in ten and a
//! quarter of them -- activities are where it errs, a masked selfie read as
//! skiing, and a higher bar only says less without saying it better. Below
//! those bars it says nothing, which the pipeline records as no
//! description. About 60 ms a still on the reference Mac's CPU, decoding
//! included; 0.4 s on an arm64 Android emulator's.
//! (`vb-pipeline/tests/label_probe.rs` prints it beside Apple Vision.)

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use vb_infer::{Adapter, InferError, RgbImage, ScoredLabel, VlmAdapter, VlmRequest};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

const NAME: &str = "SigLIP2";
const SIDE: u32 = 224;
const DIM: usize = 768;

/// The model's own logit scale (stored as its logarithm) and bias, read from
/// google/siglip2-base-patch16-224's weights at revision 75de2d5: a label's
/// probability is `sigmoid(exp(scale) * cosine + bias)`.
const LOGIT_SCALE_LN: f32 = 4.724_453_4;
const LOGIT_BIAS: f32 = -16.771_725;

/// A label's share of its kind, among all of that kind, below which the
/// picture is not distinctly that: two labels close together are a guess.
const MIN_SHARE: f32 = 0.3;

const TABLE: &str = include_str!("../data/scene-labels.tsv");
const EMBEDDINGS: &[u8] = include_bytes!("../data/scene-labels.f32");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Place,
    Subject,
    Activity,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Place, Kind::Subject, Kind::Activity];

    /// The probability a label must reach to be said. Activities need
    /// more: they are the labels the model confuses most.
    fn floor(self) -> f32 {
        match self {
            Kind::Place | Kind::Subject => 0.01,
            Kind::Activity => 0.03,
        }
    }
}

/// One row of the label table.
#[derive(Debug)]
pub struct SceneLabel {
    pub label: &'static str,
    pub zh: &'static str,
    pub kind: Kind,
    embedding: Vec<f32>,
}

/// The label table and its embeddings, checked against each other once.
pub fn table() -> &'static [SceneLabel] {
    static TABLE_ROWS: OnceLock<Vec<SceneLabel>> = OnceLock::new();
    TABLE_ROWS.get_or_init(|| {
        let rows: Vec<&str> = TABLE
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(
            EMBEDDINGS.len(),
            rows.len() * DIM * 4,
            "scene-labels.f32 does not match scene-labels.tsv; run scripts/make-scene-labels.sh"
        );
        rows.iter()
            .enumerate()
            .map(|(i, line)| {
                let cols: Vec<&'static str> = line.split('\t').collect();
                let kind = match cols.get(2) {
                    Some(&"place") => Kind::Place,
                    Some(&"subject") => Kind::Subject,
                    Some(&"activity") => Kind::Activity,
                    other => panic!("scene-labels.tsv row {i}: kind {other:?}"),
                };
                let bytes = &EMBEDDINGS[i * DIM * 4..(i + 1) * DIM * 4];
                SceneLabel {
                    label: cols[0],
                    zh: cols.get(1).copied().unwrap_or(""),
                    kind,
                    embedding: bytes
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect(),
                }
            })
            .collect()
    })
}

/// How well one label fits a picture.
#[derive(Debug, Clone)]
pub struct Scored {
    pub label: &'static str,
    pub kind: Kind,
    /// The model's own probability that the sentence describes the picture.
    pub probability: f32,
    /// This label's softmax share among the labels of its kind.
    pub share: f32,
}

/// What is worth saying: the best place, subject and activity, each only if
/// it is both likely and distinct, in that order.
pub fn choose(scores: &[Scored]) -> Vec<&Scored> {
    Kind::ALL
        .iter()
        .filter_map(|&kind| {
            scores
                .iter()
                .filter(|s| s.kind == kind)
                // The share orders a kind as the probability does, and does
                // not saturate: two labels at a probability of 1.0 would tie.
                .max_by(|a, b| a.share.total_cmp(&b.share))
                .filter(|s| s.probability >= kind.floor() && s.share >= MIN_SHARE)
        })
        .collect()
}

pub struct SiglipLabels {
    model: PathBuf,
    session: Mutex<Option<Session>>,
}

impl SiglipLabels {
    pub fn new(model: impl Into<PathBuf>) -> SiglipLabels {
        SiglipLabels {
            model: model.into(),
            session: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }

    /// The picture's embedding, unit length.
    pub fn embed(&self, image: &RgbImage) -> Result<Vec<f32>, InferError> {
        if image.width == 0 || image.height == 0 {
            return Err(InferError::Failed {
                adapter: NAME,
                detail: "empty image".into(),
            });
        }
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            // The CPU, even on a Mac, where the labels were checked: CoreML
            // ran this model's int8 export its own way -- "climbing" for a
            // horse ride the CPU calls "riding a horse" -- and more slowly.
            *guard = Some(runtime::open_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");

        let failed = |e: ort::Error| InferError::Failed {
            adapter: NAME,
            detail: e.to_string(),
        };
        let input = Tensor::from_array((
            [1usize, 3, SIDE as usize, SIDE as usize],
            pixel_values(image),
        ))
        .map_err(failed)?;
        let outputs = session
            .run(ort::inputs!["pixel_values" => input])
            .map_err(failed)?;
        let bad = |detail: String| InferError::BadOutput {
            adapter: NAME,
            detail,
        };
        let (_, data) = outputs
            .get("pooler_output")
            .ok_or_else(|| bad("no pooler_output".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| bad(e.to_string()))?;
        if data.len() != DIM {
            return Err(bad(format!(
                "{} values for a {DIM}-long embedding",
                data.len()
            )));
        }
        let norm = data.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        Ok(data.iter().map(|v| v / norm).collect())
    }

    /// Every label's fit to the picture.
    pub fn score(&self, image: &RgbImage) -> Result<Vec<Scored>, InferError> {
        Ok(score_embedding(&self.embed(image)?))
    }
}

/// Every label's fit to an image embedding.
pub fn score_embedding(image: &[f32]) -> Vec<Scored> {
    let scale = LOGIT_SCALE_LN.exp();
    let rows = table();
    let logits: Vec<f32> = rows
        .iter()
        .map(|row| scale * dot(&row.embedding, image))
        .collect();
    // The share is a softmax within each kind, so a label is judged against
    // its own kind rather than against, say, every place.
    let mut total = [0f32; 3];
    let mut top = [f32::MIN; 3];
    for (row, &logit) in rows.iter().zip(&logits) {
        let k = row.kind as usize;
        top[k] = top[k].max(logit);
    }
    for (row, &logit) in rows.iter().zip(&logits) {
        let k = row.kind as usize;
        total[k] += (logit - top[k]).exp();
    }
    rows.iter()
        .zip(&logits)
        .map(|(row, &logit)| {
            let k = row.kind as usize;
            Scored {
                label: row.label,
                kind: row.kind,
                probability: 1.0 / (1.0 + (-(logit + LOGIT_BIAS)).exp()),
                share: (logit - top[k]).exp() / total[k],
            }
        })
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Adapter for SiglipLabels {
    fn name(&self) -> &'static str {
        NAME
    }

    /// The model file and the label table decide the answers, so both are
    /// in the key: an edited table must not be served labels from the last.
    fn cache_identity(&self) -> String {
        let file = self
            .model
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{NAME}:{file}:{:016x}", table_fingerprint())
    }
}

/// FNV-1a over the label table and its embeddings: stable across builds
/// and platforms, which a cache key needs.
fn table_fingerprint() -> u64 {
    TABLE
        .as_bytes()
        .iter()
        .chain(EMBEDDINGS)
        .fold(0xcbf2_9ce4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

impl VlmAdapter for SiglipLabels {
    /// Every label of the table the picture reaches its kind's floor for,
    /// with its share of its kind as the confidence: the measure `choose`
    /// says a label by, so 0.3 and over is a label this adapter would say.
    fn labels(&self, request: &VlmRequest) -> Result<Vec<ScoredLabel>, InferError> {
        let image = read_jpeg(&request.image_path)?;
        let scores = self.score(&image)?;
        let mut labels: Vec<ScoredLabel> = scores
            .iter()
            .filter(|s| s.probability >= s.kind.floor())
            .map(|s| ScoredLabel {
                label: s.label.to_string(),
                confidence: s.share as f64,
            })
            .collect();
        labels.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        labels.truncate(12);
        Ok(labels)
    }

    fn describe(&self, request: &VlmRequest) -> Result<String, InferError> {
        let image = read_jpeg(&request.image_path)?;
        let scores = self.score(&image)?;
        // Nothing distinct is an empty answer, which the pipeline records as
        // no description rather than inventing one.
        Ok(choose(&scores)
            .iter()
            .map(|s| s.label)
            .collect::<Vec<_>>()
            .join(", "))
    }
}

/// A keyframe still, as RGB.
pub fn read_jpeg(path: &Path) -> Result<RgbImage, InferError> {
    let failed = |detail: String| InferError::Failed {
        adapter: NAME,
        detail: format!("{}: {detail}", path.display()),
    };
    let bytes = std::fs::read(path).map_err(|e| failed(e.to_string()))?;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let rgb = decoder.decode().map_err(|e| failed(format!("{e:?}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| failed("no image header".into()))?;
    let (width, height) = (info.width as u32, info.height as u32);
    if rgb.len() != (width * height * 3) as usize {
        return Err(failed(format!(
            "{} bytes for {width}x{height} RGB",
            rgb.len()
        )));
    }
    Ok(RgbImage { width, height, rgb })
}

/// The model's input: squashed to 224 x 224 with the bilinear filter the
/// SigLIP processor uses (Pillow's, which widens with the reduction, so a
/// 4K still is averaged rather than sampled), then scaled to -1..1, channels
/// first. Rounded to bytes between the passes, as Pillow does, so the
/// embedding is the one the labels were checked with.
fn pixel_values(image: &RgbImage) -> Vec<f32> {
    let (w, h) = (image.width as usize, image.height as usize);
    let side = SIDE as usize;
    let across = filter_weights(w, side);
    let down = filter_weights(h, side);

    let mut wide = vec![0u8; side * h * 3];
    for y in 0..h {
        let row = &image.rgb[y * w * 3..(y + 1) * w * 3];
        for (x, (start, weights)) in across.iter().enumerate() {
            for c in 0..3 {
                let v: f32 = weights
                    .iter()
                    .enumerate()
                    .map(|(t, k)| k * row[(start + t) * 3 + c] as f32)
                    .sum();
                wide[(y * side + x) * 3 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let plane = side * side;
    let mut out = vec![0f32; plane * 3];
    for (y, (start, weights)) in down.iter().enumerate() {
        for x in 0..side {
            for c in 0..3 {
                let v: f32 = weights
                    .iter()
                    .enumerate()
                    .map(|(t, k)| k * wide[((start + t) * side + x) * 3 + c] as f32)
                    .sum();
                let byte = v.round().clamp(0.0, 255.0);
                out[c * plane + y * side + x] = (byte / 255.0 - 0.5) / 0.5;
            }
        }
    }
    out
}

/// For each output position along one axis, the first input index and the
/// normalised weights of Pillow's bilinear (triangle) filter, whose support
/// grows with the reduction.
fn filter_weights(input: usize, output: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = input as f64 / output as f64;
    let filter_scale = scale.max(1.0);
    let support = filter_scale;
    (0..output)
        .map(|x| {
            let centre = (x as f64 + 0.5) * scale;
            let first = ((centre - support + 0.5).floor().max(0.0)) as usize;
            let last = ((centre + support + 0.5).floor() as usize).min(input);
            let mut weights: Vec<f64> = (first..last)
                .map(|i| (1.0 - ((i as f64 - centre + 0.5) / filter_scale).abs()).max(0.0))
                .collect();
            let total: f64 = weights.iter().sum();
            if total > 0.0 {
                weights.iter_mut().for_each(|k| *k /= total);
            }
            (first, weights.into_iter().map(|k| k as f32).collect())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_table_matches_its_embeddings() {
        let rows = table();
        assert!(rows.len() > 100);
        for row in rows {
            assert!(!row.label.contains(','), "{}", row.label);
            assert!(!row.zh.is_empty(), "{} has no Chinese name", row.label);
            let norm: f32 = row.embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-3, "{} norm {norm}", row.label);
        }
        let mut labels: Vec<_> = rows.iter().map(|r| r.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), rows.len(), "a label appears twice");
        for kind in Kind::ALL {
            assert!(rows.iter().any(|r| r.kind == kind));
        }
    }

    #[test]
    fn a_labels_own_sentence_is_its_best_fit() {
        // A picture whose embedding were exactly "a lake" would be called a
        // lake, and nothing of the other kinds would clear its bar.
        let lake = table().iter().find(|r| r.label == "lake").unwrap();
        let scores = score_embedding(&lake.embedding);
        let chosen: Vec<_> = choose(&scores).iter().map(|s| s.label).collect();
        assert_eq!(chosen.first(), Some(&"lake"));
    }

    fn scored(label: &'static str, kind: Kind, probability: f32, share: f32) -> Scored {
        Scored {
            label,
            kind,
            probability,
            share,
        }
    }

    #[test]
    fn only_a_likely_and_distinct_label_is_said_one_of_each_kind() {
        let scores = vec![
            scored("couple", Kind::Subject, 0.02, 0.6),
            scored("selfie", Kind::Subject, 0.015, 0.3),
            scored("lake", Kind::Place, 0.02, 0.5),
            // Likely enough for a place, not for an activity.
            scored("skiing", Kind::Activity, 0.02, 0.6),
        ];
        let chosen: Vec<_> = choose(&scores).iter().map(|s| s.label).collect();
        assert_eq!(chosen, ["lake", "couple"]);

        // Two places close together: neither is said.
        let scores = vec![
            scored("lake", Kind::Place, 0.05, 0.25),
            scored("sea", Kind::Place, 0.045, 0.24),
        ];
        assert!(choose(&scores).is_empty());
    }

    #[test]
    fn a_flat_picture_stays_flat_through_the_resize() {
        let image = RgbImage {
            width: 1920,
            height: 1080,
            rgb: [200u8, 100, 50].repeat(1920 * 1080),
        };
        let values = pixel_values(&image);
        let plane = (SIDE * SIDE) as usize;
        for (c, byte) in [200.0f32, 100.0, 50.0].into_iter().enumerate() {
            let want = (byte / 255.0 - 0.5) / 0.5;
            assert!(
                values[c * plane..(c + 1) * plane]
                    .iter()
                    .all(|v| (v - want).abs() < 1e-6)
            );
        }
    }

    #[test]
    fn the_filter_widens_with_the_reduction_and_sums_to_one() {
        for (input, output) in [(1920, 224), (224, 224), (100, 224), (3840, 224)] {
            for (start, weights) in filter_weights(input, output) {
                assert!(start + weights.len() <= input);
                let sum: f32 = weights.iter().sum();
                assert!((sum - 1.0).abs() < 1e-4, "{input}->{output}: {sum}");
            }
        }
        // A 4K row averages about thirty-five pixels into each of 224.
        let taps = filter_weights(3840, 224)[100].1.len();
        assert!((33..=36).contains(&taps), "{taps} taps");
    }

    #[test]
    fn another_model_file_is_another_cache_key() {
        let a = SiglipLabels::new("/models/siglip2-base-patch16-224-vision-fp16.onnx");
        let b = SiglipLabels::new("/models/siglip2-base-patch16-224-vision-int8.onnx");
        let moved = SiglipLabels::new("/elsewhere/siglip2-base-patch16-224-vision-fp16.onnx");
        assert_ne!(a.cache_identity(), b.cache_identity());
        // Where the library is does not change the answers.
        assert_eq!(a.cache_identity(), moved.cache_identity());
        assert!(a.cache_identity().starts_with("SigLIP2:"));
    }

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let adapter = SiglipLabels::new("/nonexistent/siglip2.onnx");
        let image = RgbImage {
            width: 4,
            height: 4,
            rgb: vec![0; 48],
        };
        match adapter.embed(&image) {
            Err(InferError::Unavailable { .. }) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    /// With the model and a still:
    /// `VB_LABEL_MODEL=<vision_model_fp16.onnx> VB_LABEL_IMAGE=<still.jpg>`.
    #[test]
    fn a_real_model() {
        let (Some(model), Some(image)) = (
            std::env::var_os("VB_LABEL_MODEL"),
            std::env::var_os("VB_LABEL_IMAGE"),
        ) else {
            eprintln!("skipped: VB_LABEL_MODEL and VB_LABEL_IMAGE are not set");
            return;
        };
        let adapter = SiglipLabels::new(PathBuf::from(model));
        let request = VlmRequest::describe(PathBuf::from(&image));
        for _ in 0..2 {
            let started = std::time::Instant::now();
            let text = adapter.describe(&request).unwrap();
            eprintln!("{text:?} in {:?}", started.elapsed());
        }
        let mut scores = adapter
            .score(&read_jpeg(Path::new(&image)).unwrap())
            .unwrap();
        scores.sort_by(|a, b| b.probability.total_cmp(&a.probability));
        for s in scores.iter().take(8) {
            eprintln!(
                "  {:?} {} {:.4} share {:.2}",
                s.kind, s.label, s.probability, s.share
            );
        }
    }
}
