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

//! The director's eye: what a picture is like as a picture.
//!
//! The half of the director that looks lives here, in perception, and
//! writes readings into the Footage Graph; the half that chooses lives in the
//! orchestrator, which never sees pixels. It starts with occlusion -- a car
//! door, a sleeve, a dashboard across the bottom of the frame -- which
//! sharpness and colour cannot tell from texture and depth can.
//!
//! # How occlusion is read
//!
//! Set against a real 126-clip trip, clip by clip, until the frames it
//! flagged were the ones a person would drop:
//!
//! - **Something hanging into the frame.** The ground recedes with the rows,
//!   so each row's median nearness is the scene's depth there. A region much
//!   nearer than its row that hangs off the side or the top of the frame is
//!   in front of the scene: a door, a mirror, a sleeve, an arm.
//! - **A band across the bottom.** A dashboard or a bonnet fills whole rows,
//!   so it moves the row median itself; the profile of medians then jumps
//!   down the frame where ground would rise smoothly, and the near band runs
//!   edge to edge.
//! - **Unless it is the subject.** People stand in front of the scene too.
//!   A near region is the subject when it holds a face, touches a clear one,
//!   or is a person whose head is in the frame -- or any person, when the
//!   frame has a clear face at all, since a selfie's shoulders hang off the
//!   side. A torso with its head cut off by the top of the frame, in a frame
//!   with no face, is someone walking past the lens.

use vb_infer::{DepthMap, FaceBox, PersonMask};

/// A region this much nearer than its row stands out from the scene.
const STANDS_OUT: f32 = 0.3;
/// Share of an edge a region must cover to hang into the frame from it.
const HANGS_OFF: f64 = 0.15;
/// Regions smaller than this share of the picture are ignored.
const MIN_REGION: f64 = 0.005;
/// A rise in the row profile this big, over this many rows, is a band.
const BAND_RISE: f32 = 0.25;
const BAND_ROWS: usize = 4;
/// A band covers this much of the rows below its edge.
const BAND_COVER: f64 = 0.9;
/// A face this confident, and this big, makes the frame a picture of someone.
const CLEAR_FACE: f64 = 0.85;
const CLEAR_FACE_AREA: f64 = 0.01;
/// A face this share of a region's area makes the region its person.
const FACE_SHARE: f64 = 0.02;
/// A region that is mostly this likely to be a person is one.
const PERSON: f64 = 0.5;
/// A region covering less of the top edge than this has its head in frame.
const HEAD_IN_FRAME: f64 = 0.05;
/// Depth is read at a quarter of the model's resolution.
const SCALE: u32 = 4;

/// How much of the picture a near-plane occluder covers, 0..1.
///
/// `faces` and `people` are what the detectors found in the same frame; with
/// neither, every near person hanging into the frame counts as an occluder,
/// so a mock detector can make the reading stricter but never blinder.
pub fn occlusion(depth: &DepthMap, faces: &[FaceBox], people: Option<&PersonMask>) -> f64 {
    let grid = Grid::from(depth);
    if grid.w < 4 || grid.h < 4 {
        return 0.0;
    }
    let person = |x: usize, y: usize| -> f32 {
        people.map_or(0.0, |p| {
            p.at_normalised(
                (x as f64 + 0.5) / grid.w as f64,
                (y as f64 + 0.5) / grid.h as f64,
            )
        })
    };
    let clear_face = faces
        .iter()
        .any(|f| f.confidence >= CLEAR_FACE && f.area() >= CLEAR_FACE_AREA);

    // --- regions hanging into the frame ---------------------------------
    let rows: Vec<f32> = (0..grid.h).map(|y| grid.row_median(y)).collect();
    let stands_out: Vec<bool> = (0..grid.h * grid.w)
        .map(|i| grid.v[i] - rows[i / grid.w] >= STANDS_OUT)
        .collect();
    let total = (grid.w * grid.h) as f64;
    let mut side = 0.0;
    for region in regions(&stands_out, grid.w, grid.h) {
        let area = region.len() as f64 / total;
        if area < MIN_REGION {
            continue;
        }
        let edges = Edges::of(&region, grid.w, grid.h);
        if edges.top.max(edges.left).max(edges.right) < HANGS_OFF {
            continue;
        }
        let mask = |x: usize, y: usize| region.binary_search(&(y * grid.w + x)).is_ok();
        let personness = region
            .iter()
            .map(|&i| person(i % grid.w, i / grid.w) as f64)
            .sum::<f64>()
            / region.len() as f64;
        let subject = holds_a_face(&region, &mask, faces, grid.w, grid.h)
            || touches_a_clear_face(&mask, faces, grid.w, grid.h)
            || (personness >= PERSON && (edges.top < HEAD_IN_FRAME || clear_face));
        if !subject {
            side += area;
        }
    }

    // --- a band across the bottom ----------------------------------------
    let mut band = 0.0;
    for y in grid.h / 3..grid.h.saturating_sub(BAND_ROWS) {
        let (edge, below) = (rows[y], &rows[y + BAND_ROWS..]);
        if below[0] - edge < BAND_RISE
            || below.iter().copied().fold(f32::INFINITY, f32::min) < edge + BAND_RISE * 0.8
        {
            continue;
        }
        let level = edge + BAND_RISE * 0.8;
        let near = |x: usize, yy: usize| grid.at(x, yy) >= level;
        let span = y + BAND_ROWS..grid.h;
        let cells = (span.len() * grid.w) as f64;
        let cover = span
            .clone()
            .flat_map(|yy| (0..grid.w).map(move |x| (x, yy)))
            .filter(|&(x, yy)| near(x, yy))
            .count() as f64
            / cells;
        let column =
            |x: usize| span.clone().filter(|&yy| near(x, yy)).count() as f64 / span.len() as f64;
        let people_in_band = span
            .clone()
            .flat_map(|yy| (0..grid.w).map(move |x| (x, yy)))
            .map(|(x, yy)| person(x, yy) as f64)
            .sum::<f64>()
            / cells;
        // Edge to edge, and not a row of seated people.
        if cover >= BAND_COVER
            && column(0) >= 0.8
            && column(grid.w - 1) >= 0.8
            && people_in_band < 0.3
        {
            band = (grid.h - y - BAND_ROWS / 2) as f64 / grid.h as f64;
        }
        break;
    }

    (side + band).min(1.0)
}

/// Does a face sit in this region, and make up enough of it to be its person?
fn holds_a_face(
    region: &[usize],
    mask: &dyn Fn(usize, usize) -> bool,
    faces: &[FaceBox],
    w: usize,
    h: usize,
) -> bool {
    faces.iter().any(|f| {
        let (cx, cy) = f.centre();
        let centre = mask(cell(cx, w), cell(cy, h));
        let (x0, x1) = (cell(f.x, w), cell(f.x + f.w, w).max(cell(f.x, w) + 1));
        let (y0, y1) = (cell(f.y, h), cell(f.y + f.h, h).max(cell(f.y, h) + 1));
        let boxed = (y0..y1.min(h))
            .flat_map(|y| (x0..x1.min(w)).map(move |x| (x, y)))
            .collect::<Vec<_>>();
        let overlap = !boxed.is_empty()
            && boxed.iter().filter(|&&(x, y)| mask(x, y)).count() as f64 / boxed.len() as f64 > 0.3;
        let share = f.area() * (w * h) as f64 / region.len().max(1) as f64;
        (centre || overlap) && share >= FACE_SHARE
    })
}

/// Does this region touch a clear face -- a cap's brim, a hood, the arm
/// holding the phone?
fn touches_a_clear_face(
    mask: &dyn Fn(usize, usize) -> bool,
    faces: &[FaceBox],
    w: usize,
    h: usize,
) -> bool {
    faces
        .iter()
        .filter(|f| f.confidence >= CLEAR_FACE)
        .any(|f| {
            let x0 = cell((f.x - f.w * 0.5).max(0.0), w);
            let x1 = cell((f.x + f.w * 1.5).min(1.0), w).min(w - 1);
            let y0 = cell((f.y - f.h * 0.5).max(0.0), h);
            let y1 = cell((f.y + f.h * 1.5).min(1.0), h).min(h - 1);
            (y0..=y1).any(|y| (x0..=x1).any(|x| mask(x, y)))
        })
}

fn cell(v: f64, n: usize) -> usize {
    ((v * n as f64) as usize).min(n - 1)
}

/// The depth map at a quarter of its resolution, by block mean.
struct Grid {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Grid {
    fn from(map: &DepthMap) -> Grid {
        let (w, h) = ((map.width / SCALE) as usize, (map.height / SCALE) as usize);
        let mut v = vec![0f32; w * h];
        for (y, row) in v.chunks_mut(w.max(1)).enumerate().take(h) {
            for (x, out) in row.iter_mut().enumerate() {
                let mut sum = 0.0;
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        sum += map.at(x as u32 * SCALE + dx, y as u32 * SCALE + dy);
                    }
                }
                *out = sum / (SCALE * SCALE) as f32;
            }
        }
        Grid { w, h, v }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.v[y * self.w + x]
    }

    fn row_median(&self, y: usize) -> f32 {
        let mut row = self.v[y * self.w..(y + 1) * self.w].to_vec();
        row.sort_by(f32::total_cmp);
        let n = row.len();
        if n % 2 == 1 {
            row[n / 2]
        } else {
            (row[n / 2 - 1] + row[n / 2]) / 2.0
        }
    }
}

/// How much of each edge a region covers.
struct Edges {
    top: f64,
    right: f64,
    left: f64,
}

impl Edges {
    fn of(region: &[usize], w: usize, h: usize) -> Edges {
        let (mut top, mut right, mut left) = (0usize, 0usize, 0usize);
        for &i in region {
            let (x, y) = (i % w, i / w);
            top += (y == 0) as usize;
            left += (x == 0) as usize;
            right += (x == w - 1) as usize;
        }
        Edges {
            top: top as f64 / w as f64,
            right: right as f64 / h as f64,
            left: left as f64 / h as f64,
        }
    }
}

/// Four-connected regions of a mask, each as its sorted cell indices.
fn regions(mask: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut region = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            region.push(i);
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        region.sort_unstable();
        out.push(region);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map at the model's resolution, from a function of 0..1 coordinates.
    fn map(f: impl Fn(f64, f64) -> f32) -> DepthMap {
        let (w, h) = (320u32, 180u32);
        let mut values = Vec::new();
        for y in 0..h {
            for x in 0..w {
                values.push(f(x as f64 / w as f64, y as f64 / h as f64));
            }
        }
        DepthMap {
            width: w,
            height: h,
            values,
        }
    }

    /// Open country: sky far, ground rising smoothly to the bottom edge.
    fn country(y: f64) -> f32 {
        if y < 0.45 {
            0.0
        } else {
            ((y - 0.45) / 0.55) as f32
        }
    }

    fn face(x: f64, y: f64, size: f64, confidence: f64) -> FaceBox {
        FaceBox {
            x,
            y,
            w: size,
            h: size * 1.3,
            confidence,
            landmarks: [[0.0; 2]; 5],
        }
    }

    fn everyone(value: f32) -> PersonMask {
        PersonMask {
            width: 1,
            height: 1,
            values: vec![value],
        }
    }

    #[test]
    fn open_country_is_clear() {
        assert_eq!(occlusion(&map(|_, y| country(y)), &[], None), 0.0);
    }

    #[test]
    fn a_door_down_the_right_side_is_an_occluder() {
        let m = map(|x, y| if x > 0.75 { 1.0 } else { country(y) * 0.5 });
        let o = occlusion(&m, &[], None);
        assert!((0.2..0.3).contains(&o), "{o}");
    }

    #[test]
    fn someone_standing_in_the_scene_is_not() {
        // A near figure in the middle, feet on the ground, head well below
        // the top edge: it touches no side, so it hangs into nothing.
        let m = map(|x, y| {
            if (0.45..0.55).contains(&x) && y > 0.3 {
                0.8
            } else {
                country(y) * 0.5
            }
        });
        assert_eq!(occlusion(&m, &[], None), 0.0);
    }

    #[test]
    fn a_dashboard_across_the_bottom_is_a_band() {
        let m = map(|_, y| if y > 0.75 { 1.0 } else { country(y) * 0.4 });
        let o = occlusion(&m, &[], None);
        assert!((0.2..0.3).contains(&o), "{o}");
    }

    #[test]
    fn a_selfie_is_a_subject_not_an_occluder() {
        // A face and shoulders hanging off the right side, close to the lens.
        let near = |x: f64, y: f64| x > 0.6 && y > 0.15;
        let m = map(|x, y| if near(x, y) { 1.0 } else { country(y) * 0.5 });
        assert!(
            occlusion(&m, &[], None) > 0.2,
            "with no face it reads as occlusion"
        );
        let f = face(0.7, 0.2, 0.15, 0.93);
        assert_eq!(occlusion(&m, &[f], None), 0.0);
    }

    #[test]
    fn a_masked_selfie_is_a_subject_by_the_person_mask() {
        // No face found -- mask, sunglasses, hood -- but the region is a
        // person with the head inside the frame.
        let m = map(|x, y| {
            if x > 0.6 && y > 0.1 {
                1.0
            } else {
                country(y) * 0.5
            }
        });
        assert_eq!(occlusion(&m, &[], Some(&everyone(0.9))), 0.0);
    }

    #[test]
    fn a_torso_cut_off_at_the_top_is_someone_passing_the_lens() {
        // A person, but hanging from the top edge with no face in the frame.
        let m = map(|x, y| if x > 0.6 { 1.0 } else { country(y) * 0.5 });
        let o = occlusion(&m, &[], Some(&everyone(0.9)));
        assert!(o > 0.3, "{o}");
    }

    #[test]
    fn a_face_in_the_mirror_does_not_excuse_the_door() {
        // A small, unsure face low in the door: a reflection.
        let m = map(|x, y| if x > 0.75 { 1.0 } else { country(y) * 0.5 });
        let reflection = face(0.8, 0.8, 0.02, 0.78);
        let o = occlusion(&m, &[reflection], None);
        assert!(o > 0.2, "{o}");
    }
}
