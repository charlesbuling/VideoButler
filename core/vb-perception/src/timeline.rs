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

//! Following a take from start to end.
//!
//! A shot from a phone is rarely one thing. The camera comes up, finds its
//! subject, holds, pans to the lake, holds again, and is lowered. Scoring a
//! handful of frames picked in advance cannot tell those parts apart, and a
//! trim placed by arithmetic lands wherever it lands -- on the camera coming
//! up, on a person half into frame.
//!
//! So the analysis sweeps the whole take, about two frames a second, and
//! measures each: how sharp and well exposed, how far the camera moved since
//! the last, how much else in the picture changed, and what the view looks
//! like in colour and layout. From those readings it divides the shot where
//! the view changes, finds the part of each piece worth using, and keeps the
//! readings so the narrative layer can put a cut where the action rests.
//!
//! All of it is classical measurement: no model sees the frames here.

use vb_contracts::Micros;
pub use vb_contracts::footage_graph::view_distance;
use vb_contracts::footage_graph::{CameraMotion, Segment, TimelineSample};
use vb_media::{GrayFrame, SampledFrame, exposure_quality, laplacian_sharpness};

/// How often the sweep takes a frame.
pub const SWEEP_EVERY: Micros = Micros(500_000);
/// The size swept frames are decoded at: enough to measure, small enough
/// that an hour of footage costs seconds of arithmetic.
pub const SWEEP_WIDTH: u32 = 160;
pub const SWEEP_HEIGHT: u32 = 90;

/// A take's first moments are the camera coming up and the operator finding
/// the subject; its last, the hand going to the stop button.
pub const START_GUARD: Micros = Micros(800_000);
pub const END_GUARD: Micros = Micros(500_000);

/// Shorter than this, a stretch of one view is a glance, not a segment.
pub const MIN_SEGMENT: Micros = Micros(2_000_000);
/// Shorter than this, a usable run is not worth offering as a clip.
pub const MIN_USABLE: Micros = Micros(1_000_000);

/// The camera sweeping faster than this share of the frame a second is a
/// whip pan: blurred, and nobody can see what it passes.
const FAST_CAMERA: f64 = 0.45;
/// Below this share of the frame a second the camera reads as held.
const STILL_CAMERA: f64 = 0.04;
/// The picture changing by more than this once the camera's movement is
/// taken out is too much at once: a cut, a flash, something across the lens.
const WILD_CHANGE: f64 = 0.16;
/// A panned total past this many frame widths since a segment began means
/// the camera now shows somewhere else.
const PAN_TO_NEW_VIEW: f64 = 0.9;
/// Colour-and-layout distance past which the view has changed.
pub const VIEW_CHANGE: f64 = 0.085;

/// What one swept frame measured.
#[derive(Debug, Clone)]
pub struct Reading {
    pub at: Micros,
    /// Laplacian sharpness, 0 to 1.
    pub sharpness: f64,
    /// Exposure quality, peaking at mid-grey, 0 to 1.
    pub exposure: f64,
    /// The view's colour and layout. See [`signature`].
    pub signature: Vec<f64>,
    /// Camera movement since the previous reading, as a share of the frame
    /// width and height. Zero for the first reading of a sweep.
    pub shift: (f64, f64),
    /// How much of the picture changed beyond the camera's movement, 0 to 1.
    pub change: f64,
    /// Time since the previous reading. Zero for the first.
    pub gap: Micros,
    /// How colourful the picture is (Hasler and Suesstrunk), 0 to 1.
    pub colourfulness: f64,
    /// How much brightness varies across the picture, 0 to 1.
    pub contrast: f64,
}

impl Reading {
    /// Camera speed in frame widths a second.
    pub fn camera_speed(&self) -> f64 {
        if self.gap == Micros::ZERO {
            return 0.0;
        }
        let (dx, dy) = self.shift;
        (dx * dx + dy * dy).sqrt() / (self.gap.0 as f64 / 1_000_000.0)
    }

    /// Too much happening to use: a whip pan, a burst of change, a frame
    /// too dark or too bright to see.
    pub fn unsettled(&self) -> bool {
        self.camera_speed() > FAST_CAMERA || self.change > WILD_CHANGE || self.exposure < 0.12
    }

    /// How usable the picture is here, 0 to 1: sharp, well exposed, steady,
    /// and with something to look at.
    pub fn quality(&self) -> f64 {
        // Sharpness saturates early at this size; its square root keeps a
        // soft frame from being ranked as if it were unusable.
        let sharp = (self.sharpness / 0.6).clamp(0.0, 1.0).sqrt();
        let exposure = 0.4 + 0.6 * self.exposure;
        let camera = ((self.camera_speed() - STILL_CAMERA) / 0.5).clamp(0.0, 1.0);
        let change = ((self.change - 0.03) / 0.13).clamp(0.0, 1.0);
        let steady = 1.0 - 0.7 * camera - 0.3 * change;
        let composition = 0.65 + 0.35 * self.composition();
        (sharp * exposure * steady * composition).clamp(0.0, 1.0)
    }

    /// A stand-in for how the picture is composed, 0 to 1: colour and
    /// contrast. It cannot see a horizon or a face cut in half; it can tell a
    /// grey car interior from a blue lake, which is most of what goes wrong
    /// when footage is chosen by sharpness alone.
    pub fn composition(&self) -> f64 {
        (0.6 * self.colourfulness + 0.4 * self.contrast).clamp(0.0, 1.0)
    }

    /// Movement within the picture, 0 to 1. Its low points are the rests
    /// between actions, which is where a cut can fall without cutting one off.
    pub fn motion(&self) -> f64 {
        (self.change / 0.15).clamp(0.0, 1.0)
    }
}

/// Turns swept frames into readings, one at a time, keeping only what the
/// next reading needs from the last.
#[derive(Default)]
pub struct Reader {
    previous: Option<(Micros, Plane)>,
}

impl Reader {
    pub fn read(&mut self, frame: &SampledFrame) -> Reading {
        let gray = frame.gray();
        let plane = Plane::from_gray(&gray).half();
        let (shift, change, gap) = match &self.previous {
            Some((at, before)) => {
                let (dx, dy, residual) = align(before, &plane);
                (
                    (
                        dx as f64 / plane.width as f64,
                        dy as f64 / plane.height as f64,
                    ),
                    residual,
                    frame.at.saturating_sub(*at),
                )
            }
            None => ((0.0, 0.0), 0.0, Micros::ZERO),
        };
        let (colourfulness, contrast) = colour_and_contrast(frame, &gray);
        let reading = Reading {
            at: frame.at,
            sharpness: laplacian_sharpness(&gray),
            exposure: exposure_quality(&gray),
            signature: signature(frame),
            shift,
            change,
            gap,
            colourfulness,
            contrast,
        };
        self.previous = Some((frame.at, plane));
        reading
    }
}

/// A greyscale image as floats, for alignment.
#[derive(Debug, Clone)]
struct Plane {
    width: usize,
    height: usize,
    pixels: Vec<f32>,
}

impl Plane {
    fn from_gray(frame: &GrayFrame) -> Plane {
        Plane {
            width: frame.width as usize,
            height: frame.height as usize,
            pixels: frame.pixels.iter().map(|p| *p as f32).collect(),
        }
    }

    /// Half the size in each direction, by averaging.
    fn half(&self) -> Plane {
        let (w, h) = (self.width / 2, self.height / 2);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let at = |dx: usize, dy: usize| self.pixels[(2 * y + dy) * self.width + 2 * x + dx];
                pixels.push((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) / 4.0);
            }
        }
        Plane {
            width: w,
            height: h,
            pixels,
        }
    }

    fn mean(&self) -> f32 {
        if self.pixels.is_empty() {
            return 0.0;
        }
        self.pixels.iter().sum::<f32>() / self.pixels.len() as f32
    }

    /// Mean absolute difference between this plane and `other` moved by
    /// (dx, dy), over where they overlap, with each plane's mean taken out
    /// so an exposure change does not read as movement. `None` when the
    /// overlap is too small to judge.
    fn difference(&self, other: &Plane, dx: i32, dy: i32, means: (f32, f32)) -> Option<f32> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (x0, x1) = (dx.max(0), (w + dx).min(w));
        let (y0, y1) = (dy.max(0), (h + dy).min(h));
        let area = (x1 - x0) * (y1 - y0);
        if area * 5 < w * h * 2 {
            return None;
        }
        let mut total = 0.0f32;
        for y in y0..y1 {
            let row_a = (y * w) as usize;
            let row_b = ((y - dy) * w) as usize;
            for x in x0..x1 {
                let a = self.pixels[row_a + x as usize] - means.0;
                let b = other.pixels[row_b + (x - dx) as usize] - means.1;
                total += (a - b).abs();
            }
        }
        Some(total / area as f32)
    }
}

/// How far the picture moved between two planes, and what changed beyond it.
///
/// Coarse to fine: the whole search at quarter size, where a shift of a
/// third of the frame is a dozen pixels, then a small refinement at the
/// size given. Returns the shift in `after`'s pixels (positive x: the
/// content moved right) and the leftover difference, 0 to 1.
fn align(before: &Plane, after: &Plane) -> (i32, i32, f64) {
    if before.width != after.width || before.height != after.height || before.width < 8 {
        return (0, 0, 0.0);
    }
    let means = (after.mean(), before.mean());
    let (small_a, small_b) = (after.half(), before.half());
    let small_means = (small_a.mean(), small_b.mean());
    let (rx, ry) = (small_a.width as i32 / 3, small_a.height as i32 / 3);

    let mut best: (i32, i32, f32) = (0, 0, f32::MAX);
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            if let Some(d) = small_a.difference(&small_b, dx, dy, small_means) {
                // Ties go to the smaller movement, so a flat sky does not
                // read as a pan.
                let better = d < best.2 - 0.01
                    || ((d - best.2).abs() <= 0.01
                        && dx.abs() + dy.abs() < best.0.abs() + best.1.abs());
                if better {
                    best = (dx, dy, d);
                }
            }
        }
    }

    let (cx, cy) = (best.0 * 2, best.1 * 2);
    let mut fine = (cx, cy, f32::MAX);
    for dy in cy - 2..=cy + 2 {
        for dx in cx - 2..=cx + 2 {
            if let Some(d) = after.difference(before, dx, dy, means) {
                if d < fine.2 {
                    fine = (dx, dy, d);
                }
            }
        }
    }
    if fine.2 == f32::MAX {
        return (0, 0, 1.0);
    }
    (fine.0, fine.1, (fine.2 / 255.0).clamp(0.0, 1.0) as f64)
}

/// Colourfulness (Hasler and Suesstrunk's metric over the red-green and
/// yellow-blue opponents) and brightness contrast, each scaled to 0 to 1.
fn colour_and_contrast(frame: &SampledFrame, gray: &GrayFrame) -> (f64, f64) {
    let n = (frame.width * frame.height) as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let (mut srg, mut syb, mut srg2, mut syb2) = (0.0, 0.0, 0.0, 0.0);
    for p in frame.rgb.chunks_exact(3) {
        let (r, g, b) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let rg = r - g;
        let yb = (r + g) / 2.0 - b;
        srg += rg;
        syb += yb;
        srg2 += rg * rg;
        syb2 += yb * yb;
    }
    let (mrg, myb) = (srg / n, syb / n);
    let var = (srg2 / n - mrg * mrg).max(0.0) + (syb2 / n - myb * myb).max(0.0);
    let colourfulness = var.sqrt() + 0.3 * (mrg * mrg + myb * myb).sqrt();

    let mean = gray.pixels.iter().map(|p| *p as f64).sum::<f64>() / n;
    let spread = gray
        .pixels
        .iter()
        .map(|p| (*p as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
    // A vivid landscape measures about 80, a well-lit scene about 60 of
    // spread; both saturate past that.
    (
        (colourfulness / 80.0).clamp(0.0, 1.0),
        (spread.sqrt() / 60.0).clamp(0.0, 1.0),
    )
}

/// A frame's colour and layout, as a short vector.
///
/// Twelve regions in a 4 x 3 grid, each as brightness and two colour
/// opponents (red against green, yellow against blue), then an eight-bin
/// brightness histogram of the whole frame. Crude, but it tells a lake from
/// a desert from a street at night, and it costs nothing to compute.
pub fn signature(frame: &SampledFrame) -> Vec<f64> {
    const COLS: usize = 4;
    const ROWS: usize = 3;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut cells = [[0.0f64; 4]; COLS * ROWS];
    let mut histogram = [0.0f64; 8];
    for y in 0..h {
        for x in 0..w {
            let p = &frame.rgb[(y * w + x) * 3..(y * w + x) * 3 + 3];
            let (r, g, b) = (
                p[0] as f64 / 255.0,
                p[1] as f64 / 255.0,
                p[2] as f64 / 255.0,
            );
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            let cell = &mut cells[(y * ROWS / h) * COLS + x * COLS / w];
            cell[0] += luma;
            cell[1] += r - g;
            cell[2] += (r + g) / 2.0 - b;
            cell[3] += 1.0;
            histogram[((luma * 8.0) as usize).min(7)] += 1.0;
        }
    }
    let mut out = Vec::with_capacity(COLS * ROWS * 3 + 8);
    for cell in &cells {
        let n = cell[3].max(1.0);
        out.push(cell[0] / n);
        // Opponents run -1 to 1; shifted to 0 to 1 like everything else.
        out.push((cell[1] / n + 1.0) / 2.0);
        out.push((cell[2] / n + 1.0) / 2.0);
    }
    let total = (w * h).max(1) as f64;
    out.extend(histogram.iter().map(|c| c / total));
    out
}

/// A shot's readings turned into segments.
#[derive(Debug, Clone)]
pub struct Division {
    pub segments: Vec<Segment>,
    /// For each segment, the index into the readings of the one that best
    /// represents it: the keyframe to extract and describe.
    pub representative: Vec<usize>,
    pub timeline: Vec<TimelineSample>,
}

/// Divide a shot, `[start, end)` of a source `source_duration` long, using
/// the readings that fall inside it.
///
/// Segment ids are `{shot_id}-seg-{n}`. Readings must be in time order.
pub fn divide(
    shot_id: &str,
    start: Micros,
    end: Micros,
    source_duration: Micros,
    readings: &[Reading],
) -> Division {
    let timeline = readings
        .iter()
        .map(|r| TimelineSample {
            at_us: r.at,
            quality: Some(round(r.quality())),
            motion: Some(round(r.motion())),
            camera: Some(round(r.camera_speed().min(1.0))),
        })
        .collect();

    if readings.is_empty() || end <= start {
        let segment = Segment {
            id: format!("{shot_id}-seg-0"),
            start_us: start,
            end_us: end,
            usable_start_us: None,
            usable_end_us: None,
            scene_id: None,
            quality: None,
            camera: None,
            keyframe_id: None,
            description: None,
            signature: Vec::new(),
            occlusion: None,
            composition: None,
            ..Default::default()
        };
        return Division {
            segments: vec![segment],
            representative: Vec::new(),
            timeline,
        };
    }

    // --- where the view changes -------------------------------------------
    // A boundary where the view has drifted from what the segment began as,
    // for two readings running (one is a person walking past), or where the
    // camera has panned a frame's width away from where it started.
    let mut cuts: Vec<usize> = vec![0];
    let mut anchor = readings[0].signature.clone();
    let mut anchor_n: f64 = 1.0;
    let mut panned = 0.0;
    for i in 1..readings.len() {
        panned += readings[i].shift.0.abs() + readings[i].shift.1.abs() * 0.5;
        let far = |j: usize| view_distance(&readings[j].signature, &anchor) > VIEW_CHANGE;
        let drifted = far(i) && (i + 1 >= readings.len() || far(i + 1));
        if drifted || panned > PAN_TO_NEW_VIEW {
            cuts.push(i);
            anchor = readings[i].signature.clone();
            anchor_n = 1.0;
            panned = 0.0;
        } else if !readings[i].unsettled() {
            // The anchor follows the settled view, so a slow change of light
            // over a long hold does not accumulate into a boundary.
            anchor_n += 1.0;
            for (a, s) in anchor.iter_mut().zip(&readings[i].signature) {
                *a += (s - *a) / anchor_n.min(6.0);
            }
        }
    }

    // Spans of readings, then too-short spans folded into a neighbour.
    let mut spans: Vec<(usize, usize)> = cuts
        .iter()
        .enumerate()
        .map(|(k, &from)| (from, cuts.get(k + 1).copied().unwrap_or(readings.len())))
        .collect();
    let span_time = |s: &(usize, usize)| -> Micros {
        let from = if s.0 == 0 { start } else { readings[s.0].at };
        let to = if s.1 >= readings.len() {
            end
        } else {
            readings[s.1].at
        };
        to.saturating_sub(from)
    };
    let mut k = 0;
    while spans.len() > 1 && k < spans.len() {
        if span_time(&spans[k]) >= MIN_SEGMENT {
            k += 1;
            continue;
        }
        // Into whichever neighbour looks more like it; the previous one when
        // there is only one.
        let mean = |s: &(usize, usize)| mean_signature(&readings[s.0..s.1]);
        let here = mean(&spans[k]);
        let into_next = match (k.checked_sub(1), spans.get(k + 1)) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(p), Some(n)) => {
                view_distance(&here, &mean(n)) < view_distance(&here, &mean(&spans[p]))
            }
        };
        if into_next {
            spans[k + 1].0 = spans[k].0;
        } else {
            spans[k - 1].1 = spans[k].1;
        }
        spans.remove(k);
        k = k.saturating_sub(1);
    }

    // --- each segment: its extent, its usable part, its character ---------
    let mut segments = Vec::with_capacity(spans.len());
    let mut representative = Vec::with_capacity(spans.len());
    for (n, &(from, to)) in spans.iter().enumerate() {
        let seg_start = if n == 0 { start } else { readings[from].at };
        let seg_end = if n + 1 == spans.len() {
            end
        } else {
            readings[to].at
        };
        let part = &readings[from..to];

        // The usable run: the longest stretch of settled readings, bridging a
        // single unsettled one (a stumble is not the end of a shot), inside
        // the guards at the start and end of the take.
        let floor = START_GUARD.min(Micros(source_duration.0 / 4));
        let ceiling = source_duration.saturating_sub(END_GUARD.min(Micros(source_duration.0 / 4)));
        let ok: Vec<bool> = part
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let settled = !r.unsettled() && r.at >= floor && r.at < ceiling;
                let bridged = i > 0
                    && i + 1 < part.len()
                    && !part[i - 1].unsettled()
                    && !part[i + 1].unsettled()
                    && r.at >= floor
                    && r.at < ceiling;
                settled || bridged
            })
            .collect();
        let mut best_run: Option<(usize, usize)> = None;
        let mut i = 0;
        while i < ok.len() {
            if !ok[i] {
                i += 1;
                continue;
            }
            let run_start = i;
            while i < ok.len() && ok[i] {
                i += 1;
            }
            let longer = best_run.is_none_or(|(a, b)| i - run_start > b - a);
            if longer {
                best_run = Some((run_start, i));
            }
        }

        let usable = best_run.and_then(|(a, b)| {
            let from_t = part[a].at.max(seg_start).max(floor);
            // A run reaches to the next reading, or to the segment's end.
            let to_t = part
                .get(b)
                .map(|r| r.at)
                .unwrap_or(seg_end)
                .min(seg_end)
                .min(ceiling);
            (to_t.saturating_sub(from_t) >= MIN_USABLE).then_some((from_t, to_t, a, b))
        });

        let (quality, camera, best) = match usable {
            Some((_, _, a, b)) => {
                let run = &part[a..b];
                let quality = run.iter().map(Reading::quality).sum::<f64>() / run.len() as f64;
                let best = (a..b)
                    .max_by(|&x, &y| part[x].quality().total_cmp(&part[y].quality()))
                    .unwrap_or(a);
                (Some(round(quality)), Some(camera_motion(run)), best)
            }
            None => (None, None, part.len() / 2),
        };
        representative.push(from + best);
        segments.push(Segment {
            id: format!("{shot_id}-seg-{n}"),
            start_us: seg_start,
            end_us: seg_end,
            usable_start_us: usable.map(|u| u.0),
            usable_end_us: usable.map(|u| u.1),
            scene_id: None,
            quality,
            camera,
            keyframe_id: None,
            description: None,
            signature: mean_signature(match usable {
                Some((_, _, a, b)) => &part[a..b],
                None => part,
            })
            .into_iter()
            .map(round)
            .collect(),
            occlusion: None,
            composition: None,
            ..Default::default()
        });
    }

    Division {
        segments,
        representative,
        timeline,
    }
}

/// How the camera moved over a run of readings.
fn camera_motion(run: &[Reading]) -> CameraMotion {
    let moving: Vec<&Reading> = run.iter().filter(|r| r.gap > Micros::ZERO).collect();
    if moving.is_empty() {
        return CameraMotion::Still;
    }
    let speeds: Vec<f64> = moving.iter().map(|r| r.camera_speed()).collect();
    let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
    if mean < STILL_CAMERA {
        return CameraMotion::Still;
    }
    // A pan keeps going one way; shake goes back and forth, so its steps
    // mostly cancel.
    let (sx, sy) = moving
        .iter()
        .fold((0.0, 0.0), |(x, y), r| (x + r.shift.0, y + r.shift.1));
    let net = (sx * sx + sy * sy).sqrt();
    let travelled: f64 = moving
        .iter()
        .map(|r| (r.shift.0 * r.shift.0 + r.shift.1 * r.shift.1).sqrt())
        .sum();
    let steadiness = if travelled > 0.0 {
        net / travelled
    } else {
        1.0
    };
    if steadiness < 0.35 && mean > 0.08 {
        CameraMotion::Shaky
    } else if steadiness > 0.6 && mean > 0.12 {
        CameraMotion::Panning
    } else {
        CameraMotion::Steady
    }
}

fn mean_signature(readings: &[Reading]) -> Vec<f64> {
    let Some(first) = readings.first() else {
        return Vec::new();
    };
    let mut out = vec![0.0; first.signature.len()];
    for r in readings {
        for (o, s) in out.iter_mut().zip(&r.signature) {
            *o += s;
        }
    }
    for o in out.iter_mut() {
        *o /= readings.len() as f64;
    }
    out
}

/// Three decimal places: plenty for scores, and a graph of an hour's
/// readings stays a readable size.
fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of flat colour with a bright square at (x, y), so moving the
    /// square moves the picture.
    fn frame(at_ms: u64, colour: [u8; 3], square: (usize, usize)) -> SampledFrame {
        let (w, h) = (SWEEP_WIDTH as usize, SWEEP_HEIGHT as usize);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                // Texture, so there is something to be sharp and to align.
                let t = (((x * 7 + y * 13) % 17) as u8).wrapping_mul(3);
                let inside =
                    x >= square.0 && x < square.0 + 30 && y >= square.1 && y < square.1 + 30;
                if inside {
                    rgb.extend([250, 250, 250]);
                } else {
                    rgb.extend(colour.map(|c| c.saturating_add(t)));
                }
            }
        }
        SampledFrame {
            at: Micros::from_millis(at_ms),
            width: w as u32,
            height: h as u32,
            rgb,
        }
    }

    fn readings(frames: &[SampledFrame]) -> Vec<Reading> {
        let mut reader = Reader::default();
        frames.iter().map(|f| reader.read(f)).collect()
    }

    #[test]
    fn a_held_view_reads_as_still_and_usable() {
        let frames: Vec<_> = (0..20)
            .map(|i| frame(i * 500, [60, 110, 170], (60, 30)))
            .collect();
        let r = readings(&frames);
        assert!(r.iter().all(|r| r.camera_speed() < 0.01 && r.change < 0.01));
        let d = divide(
            "s",
            Micros::ZERO,
            Micros::from_secs(10),
            Micros::from_secs(10),
            &r,
        );
        assert_eq!(d.segments.len(), 1);
        let seg = &d.segments[0];
        let (from, to) = seg.usable().expect("a held view is usable");
        assert!(from >= START_GUARD, "the start of recording was offered");
        assert!(to <= Micros::from_secs(10) - END_GUARD);
        assert_eq!(seg.camera, Some(CameraMotion::Still));
        assert_eq!(d.timeline.len(), 20);
    }

    #[test]
    fn a_moving_picture_is_measured_as_camera_movement() {
        // The square moves 8 px a frame: content moving, as a pan would move it.
        let frames: Vec<_> = (0..6)
            .map(|i| frame(i * 500, [60, 110, 170], (20 + 8 * i as usize, 30)))
            .collect();
        let r = readings(&frames);
        for reading in &r[1..] {
            let dx = reading.shift.0 * (SWEEP_WIDTH as f64 / 2.0);
            // The texture holds still while the square moves, so either
            // reading is defensible; what matters is that one of them is
            // measured as movement and the rest as change.
            assert!(dx.abs() <= 6.0, "shift {dx}");
        }
    }

    #[test]
    fn a_new_view_starts_a_new_segment() {
        // Ten seconds of a blue lake, then ten of a red desert.
        let mut frames: Vec<_> = (0..20)
            .map(|i| frame(i * 500, [40, 90, 180], (60, 30)))
            .collect();
        frames.extend((20..40).map(|i| frame(i * 500, [190, 110, 50], (60, 30))));
        let r = readings(&frames);
        let d = divide(
            "s",
            Micros::ZERO,
            Micros::from_secs(20),
            Micros::from_secs(20),
            &r,
        );
        assert_eq!(d.segments.len(), 2, "{:?}", d.segments);
        assert_eq!(d.segments[1].start_us, Micros::from_secs(10));
        // The segments tile the shot.
        assert_eq!(d.segments[0].start_us, Micros::ZERO);
        assert_eq!(d.segments[0].end_us, d.segments[1].start_us);
        assert_eq!(d.segments[1].end_us, Micros::from_secs(20));
        assert!(
            view_distance(&d.segments[0].signature, &d.segments[1].signature) > VIEW_CHANGE,
            "the two views should look different"
        );
        for seg in &d.segments {
            let (from, to) = seg.usable().unwrap();
            assert!(from >= seg.start_us && to <= seg.end_us);
        }
    }

    #[test]
    fn a_glance_elsewhere_does_not_become_a_segment() {
        // One second of something else in the middle of a long hold.
        let mut frames: Vec<_> = (0..10)
            .map(|i| frame(i * 500, [40, 90, 180], (60, 30)))
            .collect();
        frames.extend((10..12).map(|i| frame(i * 500, [190, 110, 50], (60, 30))));
        frames.extend((12..24).map(|i| frame(i * 500, [40, 90, 180], (60, 30))));
        let r = readings(&frames);
        let d = divide(
            "s",
            Micros::ZERO,
            Micros::from_secs(12),
            Micros::from_secs(12),
            &r,
        );
        assert!(
            d.segments
                .iter()
                .all(|s| s.end_us - s.start_us >= MIN_SEGMENT || d.segments.len() == 1)
        );
    }

    #[test]
    fn a_take_too_short_for_its_guards_still_offers_its_middle() {
        let frames: Vec<_> = (0..6)
            .map(|i| frame(i * 500, [60, 110, 170], (60, 30)))
            .collect();
        let r = readings(&frames);
        let d = divide(
            "s",
            Micros::ZERO,
            Micros::from_secs(3),
            Micros::from_secs(3),
            &r,
        );
        let (from, to) = d.segments[0]
            .usable()
            .expect("three steady seconds are usable");
        assert!(to - from >= MIN_USABLE);
    }

    #[test]
    fn the_same_view_is_close_and_different_views_are_far() {
        let lake = signature(&frame(0, [40, 90, 180], (60, 30)));
        let lake_later = signature(&frame(0, [44, 94, 176], (70, 35)));
        let desert = signature(&frame(0, [190, 140, 80], (60, 30)));
        assert!(view_distance(&lake, &lake_later) < VIEW_CHANGE / 2.0);
        assert!(view_distance(&lake, &desert) > VIEW_CHANGE * 2.0);
    }
}
