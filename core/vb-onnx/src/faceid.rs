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

//! AuraFace, behind [`TrackerAdapter`]: faces grouped by who they are.
//!
//! The development doc's face embedder: fal's AuraFace v1, `glintr100.onnx`
//! only (Apache-2.0; the other files beside it on Hugging Face are
//! InsightFace's non-commercial packs and never ship). An ArcFace-style
//! ResNet-100 that turns a face, aligned to the standard 112 x 112 template
//! by its eyes, nose and mouth corners, into a 512-number vector: two faces
//! of one person point the same way, two people do not. It stands in for
//! Apple Vision's feature prints, which describe how a picture looks rather
//! than whose face it is -- on a real trip they filed a couple as one person,
//! and a whole library under four.
//!
//! Faces are found by YuNet, whose five landmarks are exactly what the
//! alignment needs. Within one video the faces are grouped by their vectors,
//! and two faces in one frame are never the same person, however alike.
//! It is still grouping, not knowing: the album says so, and the user names
//! the groups.

use crate::runtime;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_contracts::Micros;
use vb_contracts::footage_graph::EntityKind;
use vb_infer::tracker::{TrackWindow, TrackedEntity};
use vb_infer::{Adapter, FaceBox, FaceDetector, InferError, RgbImage, TrackerAdapter};

const NAME: &str = "AuraFace";
const SIDE: usize = 112;
pub const DIM: usize = 512;

/// The ArcFace template: where the eyes, the nose tip and the mouth corners
/// of an aligned face sit in its 112 x 112 crop, in YuNet's order (the
/// person's right eye, on the picture's left, first).
const TEMPLATE: [[f64; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// Two faces this close (one less their cosine) are one person within a
/// video: the light and the clothes are the same there, so one person's
/// faces sit closer than across videos (where 0.62 is the album's bar), and
/// on a real trip two people in one take came no closer than this.
pub const SAME_PERSON: f32 = 0.6;

/// Faces smaller than this, in pixels of the still, are too small to say
/// whose they are: an embedding of a few pixels is noise.
const SMALLEST_FACE_PX: f64 = 28.0;

pub struct AuraFace {
    model: PathBuf,
    detector: Box<dyn FaceDetector>,
    session: Mutex<Option<Session>>,
    /// Where face pictures are written, one directory per clip.
    pub thumbnail_dir: Option<PathBuf>,
}

impl AuraFace {
    pub fn new(model: impl Into<PathBuf>, detector: Box<dyn FaceDetector>) -> AuraFace {
        AuraFace {
            model: model.into(),
            detector,
            session: Mutex::new(None),
            thumbnail_dir: None,
        }
    }

    /// One aligned face's vector, unit length.
    pub fn embed(&self, aligned: &[u8]) -> Result<Vec<f32>, InferError> {
        let mut guard = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(runtime::open_fixed_session(NAME, &self.model)?);
        }
        let session = guard.as_mut().expect("opened above");
        let failed = |e: ort::Error| InferError::Failed {
            adapter: NAME,
            detail: e.to_string(),
        };
        // RGB, channels first, scaled to -1..1, as InsightFace feeds it.
        let mut values = vec![0f32; 3 * SIDE * SIDE];
        for i in 0..SIDE * SIDE {
            for c in 0..3 {
                values[c * SIDE * SIDE + i] = (aligned[i * 3 + c] as f32 - 127.5) / 127.5;
            }
        }
        let input = Tensor::from_array(([1usize, 3, SIDE, SIDE], values)).map_err(failed)?;
        let name = session.inputs()[0].name().to_string();
        let outputs = session.run(ort::inputs![name => input]).map_err(failed)?;
        let (_, data) =
            outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| InferError::BadOutput {
                    adapter: NAME,
                    detail: e.to_string(),
                })?;
        if data.len() != DIM {
            return Err(InferError::BadOutput {
                adapter: NAME,
                detail: format!("{} values for a {DIM}-long embedding", data.len()),
            });
        }
        let norm = data.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        Ok(data.iter().map(|v| v / norm).collect())
    }
}

impl Adapter for AuraFace {
    fn name(&self) -> &'static str {
        NAME
    }
}

/// The similarity transform -- scale, turn and shift -- that best maps
/// `from` onto `to`, least squares (Umeyama's method in two dimensions).
/// Returns `[a, b, tx, ty]` for `x' = a x - b y + tx`, `y' = b x + a y + ty`.
pub fn similarity(from: &[[f64; 2]; 5], to: &[[f64; 2]; 5]) -> [f64; 4] {
    let n = from.len() as f64;
    let mean = |p: &[[f64; 2]; 5]| {
        let (sx, sy) = p.iter().fold((0.0, 0.0), |(x, y), q| (x + q[0], y + q[1]));
        (sx / n, sy / n)
    };
    let (fx, fy) = mean(from);
    let (tx, ty) = mean(to);
    let (mut sxx, mut sxy, mut norm) = (0.0, 0.0, 0.0);
    for (f, t) in from.iter().zip(to) {
        let (ax, ay) = (f[0] - fx, f[1] - fy);
        let (bx, by) = (t[0] - tx, t[1] - ty);
        sxx += ax * bx + ay * by;
        sxy += ax * by - ay * bx;
        norm += ax * ax + ay * ay;
    }
    if norm == 0.0 {
        return [1.0, 0.0, tx - fx, ty - fy];
    }
    let a = sxx / norm;
    let b = sxy / norm;
    [a, b, tx - (a * fx - b * fy), ty - (b * fx + a * fy)]
}

/// The face cut out of the picture and turned upright onto the template,
/// 112 x 112 RGB, by bilinear sampling.
pub fn align(image: &RgbImage, landmarks: &[[f64; 2]; 5]) -> Vec<u8> {
    let [a, b, tx, ty] = similarity(landmarks, &TEMPLATE);
    // The inverse: from a template pixel back to the picture.
    let det = a * a + b * b;
    let (ia, ib) = (a / det, -b / det);
    let (w, h) = (image.width as usize, image.height as usize);
    let mut out = vec![0u8; SIDE * SIDE * 3];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (u, v) = (x as f64 - tx, y as f64 - ty);
            let sx = ia * u - ib * v;
            let sy = ib * u + ia * v;
            if sx < 0.0 || sy < 0.0 || sx > (w - 1) as f64 || sy > (h - 1) as f64 {
                continue;
            }
            let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| image.rgb[(yy * w + xx) * 3 + c] as f64;
                let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
                let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
                out[(y * SIDE + x) * 3 + c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    out
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    1.0 - a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>()
}

struct Group {
    /// The mean of its faces' vectors, unit length.
    centre: Vec<f32>,
    count: usize,
    stills: Vec<usize>,
    shot_ids: Vec<String>,
    first: Micros,
    last: Micros,
    confidence: f64,
    area: f64,
    thumbnail: Option<PathBuf>,
}

impl TrackerAdapter for AuraFace {
    fn track(
        &self,
        _path: &Path,
        windows: &[(String, TrackWindow)],
    ) -> Result<Vec<TrackedEntity>, InferError> {
        let mut groups: Vec<Group> = Vec::new();
        let mut still_index = 0usize;
        for (shot_id, window) in windows {
            for still in &window.stills {
                still_index += 1;
                let image = match crate::labels::read_jpeg(still) {
                    Ok(image) => image,
                    Err(e) => {
                        tracing::warn!(still = %still.display(), error = %e, "a still could not be read");
                        continue;
                    }
                };
                let faces = self.detector.faces(&image)?;
                for face in faces {
                    let (pw, ph) = (face.w * image.width as f64, face.h * image.height as f64);
                    if pw.min(ph) < SMALLEST_FACE_PX {
                        continue;
                    }
                    let points = face
                        .landmarks
                        .map(|[x, y]| [x * image.width as f64, y * image.height as f64]);
                    let vector = self.embed(&align(&image, &points))?;
                    // The nearest group that has no face from this still.
                    let best = groups
                        .iter()
                        .enumerate()
                        .filter(|(_, g)| !g.stills.contains(&still_index))
                        .map(|(i, g)| (i, cosine_distance(&g.centre, &vector)))
                        .filter(|(_, d)| *d < SAME_PERSON)
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    let index = match best {
                        Some((i, _)) => i,
                        None => {
                            groups.push(Group {
                                centre: vec![0.0; DIM],
                                count: 0,
                                stills: Vec::new(),
                                shot_ids: Vec::new(),
                                first: Micros(u64::MAX),
                                last: Micros::ZERO,
                                confidence: 0.0,
                                area: 0.0,
                                thumbnail: self.thumbnail(still, &image, &face, groups.len()),
                            });
                            groups.len() - 1
                        }
                    };
                    let g = &mut groups[index];
                    let n = g.count as f32;
                    for (c, v) in g.centre.iter_mut().zip(&vector) {
                        *c = (*c * n + v) / (n + 1.0);
                    }
                    let norm = g
                        .centre
                        .iter()
                        .map(|v| v * v)
                        .sum::<f32>()
                        .sqrt()
                        .max(1e-12);
                    g.centre.iter_mut().for_each(|v| *v /= norm);
                    g.count += 1;
                    g.stills.push(still_index);
                    if !g.shot_ids.contains(shot_id) {
                        g.shot_ids.push(shot_id.clone());
                    }
                    g.first = Micros(g.first.0.min(window.start.0));
                    g.last = Micros(g.last.0.max(window.end.0));
                    g.confidence += (face.confidence - g.confidence) / g.count as f64;
                    g.area = g.area.max(face.area());
                }
            }
        }
        Ok(groups
            .into_iter()
            .enumerate()
            .map(|(i, g)| TrackedEntity {
                id: format!("person-{:03}", i + 1),
                kind: EntityKind::Person,
                label: None,
                first_seen: g.first,
                last_seen: g.last,
                shot_ids: g.shot_ids,
                confidence: Some(g.confidence),
                thumbnail: g.thumbnail,
                signature: g.centre,
                face_area: Some(g.area),
            })
            .collect())
    }
}

impl AuraFace {
    /// The face with a quarter again around it, as a PNG under the clip's
    /// own directory, for the album to show.
    fn thumbnail(
        &self,
        still: &Path,
        image: &RgbImage,
        face: &FaceBox,
        index: usize,
    ) -> Option<PathBuf> {
        let clip = still.parent()?.file_name()?.to_string_lossy().to_string();
        let dir = self.thumbnail_dir.as_ref()?.join(clip);
        std::fs::create_dir_all(&dir).ok()?;
        let stem = still.file_stem()?.to_string_lossy().to_string();
        let dest = dir.join(format!("{stem}-face-{index}.png"));
        let (iw, ih) = (image.width as f64, image.height as f64);
        let side = (face.w * iw).max(face.h * ih) * 1.5;
        let (cx, cy) = ((face.x + face.w / 2.0) * iw, (face.y + face.h / 2.0) * ih);
        let x0 = (cx - side / 2.0).clamp(0.0, iw - 1.0) as usize;
        let y0 = (cy - side / 2.0).clamp(0.0, ih - 1.0) as usize;
        let x1 = ((cx + side / 2.0).min(iw) as usize).max(x0 + 1);
        let y1 = ((cy + side / 2.0).min(ih) as usize).max(y0 + 1);
        let (cw, ch) = (x1 - x0, y1 - y0);
        // At most 256 pixels a side: a face in a list, not a portrait.
        let scale = (256.0 / cw.max(ch) as f64).min(1.0);
        let (ow, oh) = (
            ((cw as f64 * scale) as usize).max(1),
            ((ch as f64 * scale) as usize).max(1),
        );
        let mut rgb = vec![0u8; ow * oh * 3];
        for y in 0..oh {
            for x in 0..ow {
                let sx = x0 + ((x as f64 / scale) as usize).min(cw - 1);
                let sy = y0 + ((y as f64 / scale) as usize).min(ch - 1);
                let s = (sy * image.width as usize + sx) * 3;
                rgb[(y * ow + x) * 3..(y * ow + x) * 3 + 3].copy_from_slice(&image.rgb[s..s + 3]);
            }
        }
        let file = std::fs::File::create(&dest).ok()?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), ow as u32, oh as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgb).ok()?;
        writer.finish().ok()?;
        Some(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_maps_onto_itself() {
        let [a, b, tx, ty] = similarity(&TEMPLATE, &TEMPLATE);
        assert!((a - 1.0).abs() < 1e-9 && b.abs() < 1e-9 && tx.abs() < 1e-9 && ty.abs() < 1e-9);
    }

    #[test]
    fn a_face_twice_the_size_and_turned_is_brought_back_onto_the_template() {
        // The template doubled, turned a quarter and moved.
        let turned = TEMPLATE.map(|[x, y]| [300.0 - 2.0 * y, 100.0 + 2.0 * x]);
        let [a, b, tx, ty] = similarity(&turned, &TEMPLATE);
        for (p, q) in turned.iter().zip(&TEMPLATE) {
            let x = a * p[0] - b * p[1] + tx;
            let y = b * p[0] + a * p[1] + ty;
            assert!(
                (x - q[0]).abs() < 1e-6 && (y - q[1]).abs() < 1e-6,
                "{x},{y} vs {q:?}"
            );
        }
    }

    #[test]
    fn alignment_samples_the_face_and_leaves_outside_the_picture_black() {
        // A picture of one grey; the face's landmarks as the template, so
        // the aligned crop is that grey wherever it falls in the picture.
        let image = RgbImage {
            width: 200,
            height: 200,
            rgb: vec![90; 200 * 200 * 3],
        };
        let aligned = align(&image, &TEMPLATE);
        assert_eq!(aligned.len(), SIDE * SIDE * 3);
        assert!(aligned.iter().all(|v| *v == 90));
        // Landmarks near the corner put part of the crop off the picture.
        let near = TEMPLATE.map(|[x, y]| [x - 60.0, y - 60.0]);
        let aligned = align(&image, &near);
        assert_eq!(aligned[0], 0);
        assert_eq!(aligned[(111 * SIDE + 111) * 3], 90);
    }
}
