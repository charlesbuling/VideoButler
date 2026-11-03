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

//! Pixels: where a clip's picture lands in the frame, what fills the frame
//! round it, the adjustments, the transitions, and text laid over the top.
//!
//! Everything is RGBA, eight bits a channel, straight alpha, rows with no
//! padding. The frame is the finished piece's; a picture is a decoded frame
//! at whatever size the reader was opened with.

/// How many threads a frame's rows are split over.
fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
}

/// Run `f` over every row of an image, the rows split between threads.
pub(crate) fn par_rows<F>(buf: &mut [u8], width: usize, f: F)
where
    F: Fn(usize, &mut [u8]) + Sync,
{
    let stride = width * 4;
    if stride == 0 {
        return;
    }
    let height = buf.len() / stride;
    let workers = threads().min(height.max(1));
    if workers <= 1 || height < 32 {
        for (y, row) in buf.chunks_mut(stride).enumerate() {
            f(y, row);
        }
        return;
    }
    let rows_per = height.div_ceil(workers);
    std::thread::scope(|scope| {
        for (band, rows) in buf.chunks_mut(rows_per * stride).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (i, row) in rows.chunks_mut(stride).enumerate() {
                    f(band * rows_per + i, row);
                }
            });
        }
    });
}

/// Where a picture lands: its centre and size in the frame, how far it is
/// turned, and which part of it is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    pub cx: f64,
    pub cy: f64,
    /// The shown part's size on the frame, in pixels.
    pub w: f64,
    pub h: f64,
    /// Clockwise, in radians.
    pub rotation: f64,
    /// The part of the picture shown, as shares of it: x, y, w, h.
    pub crop: [f64; 4],
}

impl Placement {
    fn uncropped(&self) -> bool {
        self.crop == [0.0, 0.0, 1.0, 1.0]
    }
}

/// Draw `pic` (`pw` x `ph`) into `frame` (`fw` x `fh`) where `place` says.
pub(crate) fn place(
    frame: &mut [u8],
    fw: usize,
    fh: usize,
    pic: &[u8],
    pw: usize,
    ph: usize,
    place: &Placement,
) {
    if pw == 0 || ph == 0 || place.w <= 0.0 || place.h <= 0.0 {
        return;
    }
    let left = place.cx - place.w / 2.0;
    let top = place.cy - place.h / 2.0;
    // The common case -- a clip fitted to the frame, nothing moved -- is a
    // copy of rows, which is all most frames of most cuts are.
    if place.rotation == 0.0
        && place.uncropped()
        && (place.w - pw as f64).abs() < 1e-6
        && (place.h - ph as f64).abs() < 1e-6
        && (left - left.round()).abs() < 1e-6
        && (top - top.round()).abs() < 1e-6
    {
        copy_rect(
            frame,
            fw,
            fh,
            pic,
            pw,
            ph,
            left.round() as i64,
            top.round() as i64,
        );
        return;
    }

    let (sin, cos) = place.rotation.sin_cos();
    // The turned rectangle's bounds, so only the pixels it can reach are
    // visited.
    let (hw, hh) = (place.w / 2.0, place.h / 2.0);
    let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (x, y) in corners {
        let rx = x * cos - y * sin + place.cx;
        let ry = x * sin + y * cos + place.cy;
        x0 = x0.min(rx);
        x1 = x1.max(rx);
        y0 = y0.min(ry);
        y1 = y1.max(ry);
    }
    let x_from = x0.floor().max(0.0) as usize;
    let x_to = (x1.ceil().max(0.0) as usize).min(fw);
    let y_from = y0.floor().max(0.0) as usize;
    let y_to = (y1.ceil().max(0.0) as usize).min(fh);
    if x_from >= x_to || y_from >= y_to {
        return;
    }
    let [crop_x, crop_y, crop_w, crop_h] = place.crop;
    let p = *place;
    par_rows(frame, fw, |y, row| {
        if y < y_from || y >= y_to {
            return;
        }
        let dy = y as f64 + 0.5 - p.cy;
        for x in x_from..x_to {
            let dx = x as f64 + 0.5 - p.cx;
            // Turned back, then as a share of the shown part.
            let ux = dx * cos + dy * sin;
            let uy = -dx * sin + dy * cos;
            let u = ux / p.w + 0.5;
            let v = uy / p.h + 0.5;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let sx = (crop_x + u * crop_w) * pw as f64 - 0.5;
            let sy = (crop_y + v * crop_h) * ph as f64 - 0.5;
            let px = bilinear(pic, pw, ph, sx, sy);
            row[x * 4..x * 4 + 4].copy_from_slice(&px);
        }
    });
}

/// Copy a picture's rows into the frame at (`left`, `top`), clipped to it.
#[allow(clippy::too_many_arguments)]
fn copy_rect(
    frame: &mut [u8],
    fw: usize,
    fh: usize,
    pic: &[u8],
    pw: usize,
    ph: usize,
    left: i64,
    top: i64,
) {
    let x_from = left.max(0) as usize;
    let x_to = ((left + pw as i64).max(0) as usize).min(fw);
    if x_from >= x_to {
        return;
    }
    let src_x = (x_from as i64 - left) as usize;
    let span = (x_to - x_from) * 4;
    for py in 0..ph {
        let y = top + py as i64;
        if y < 0 || y as usize >= fh {
            continue;
        }
        let d = (y as usize * fw + x_from) * 4;
        let s = (py * pw + src_x) * 4;
        frame[d..d + span].copy_from_slice(&pic[s..s + span]);
    }
}

/// The colour at (`x`, `y`) of a picture, between its pixel centres.
fn bilinear(pic: &[u8], w: usize, h: usize, x: f64, y: f64) -> [u8; 4] {
    let x = x.clamp(0.0, (w - 1) as f64);
    let y = y.clamp(0.0, (h - 1) as f64);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let at = |xx: usize, yy: usize| (yy * w + xx) * 4;
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mut out = [0u8; 4];
    for (i, o) in out.iter_mut().enumerate() {
        let top = pic[a + i] as f64 * (1.0 - fx) + pic[b + i] as f64 * fx;
        let bottom = pic[c + i] as f64 * (1.0 - fx) + pic[d + i] as f64 * fx;
        *o = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Fill the frame with one colour.
pub(crate) fn fill(frame: &mut [u8], color: [u8; 4]) {
    for px in frame.chunks_exact_mut(4) {
        px.copy_from_slice(&color);
    }
}

/// Cover the frame with the picture enlarged and blurred: what fills the
/// bars round a portrait clip in a landscape cut, the way phone editors do.
pub(crate) fn blurred_cover(
    frame: &mut [u8],
    fw: usize,
    fh: usize,
    pic: &[u8],
    pw: usize,
    ph: usize,
) {
    if pw == 0 || ph == 0 || fw == 0 || fh == 0 {
        return;
    }
    // Small, then blurred, then stretched back: a blur this wide costs
    // nothing at a sixteenth of the size and looks the same.
    let sw = (fw / 16).max(4);
    let sh = (fh / 16).max(4);
    let scale = (fw as f64 / pw as f64).max(fh as f64 / ph as f64);
    let (vis_w, vis_h) = (fw as f64 / scale, fh as f64 / scale);
    let (off_x, off_y) = ((pw as f64 - vis_w) / 2.0, (ph as f64 - vis_h) / 2.0);
    let mut small = vec![0f32; sw * sh * 3];
    for y in 0..sh {
        for x in 0..sw {
            // The average of the picture's pixels under this one.
            let sx0 = off_x + vis_w * x as f64 / sw as f64;
            let sx1 = off_x + vis_w * (x + 1) as f64 / sw as f64;
            let sy0 = off_y + vis_h * y as f64 / sh as f64;
            let sy1 = off_y + vis_h * (y + 1) as f64 / sh as f64;
            let (ix0, ix1) = (sx0.floor().max(0.0) as usize, (sx1.ceil() as usize).min(pw));
            let (iy0, iy1) = (sy0.floor().max(0.0) as usize, (sy1.ceil() as usize).min(ph));
            let step_x = ((ix1 - ix0) / 4).max(1);
            let step_y = ((iy1 - iy0) / 4).max(1);
            let mut sum = [0f32; 3];
            let mut n = 0f32;
            let mut yy = iy0;
            while yy < iy1.max(iy0 + 1).min(ph) {
                let mut xx = ix0;
                while xx < ix1.max(ix0 + 1).min(pw) {
                    let i = (yy * pw + xx) * 4;
                    sum[0] += pic[i] as f32;
                    sum[1] += pic[i + 1] as f32;
                    sum[2] += pic[i + 2] as f32;
                    n += 1.0;
                    xx += step_x;
                }
                yy += step_y;
            }
            let o = (y * sw + x) * 3;
            for c in 0..3 {
                small[o + c] = sum[c] / n.max(1.0);
            }
        }
    }
    for _ in 0..3 {
        box_blur(&mut small, sw, sh, 2);
    }
    // A little darker, so the picture in front stands out from it.
    const DIM: f32 = 0.85;
    let small = &small;
    par_rows(frame, fw, |y, row| {
        let sy = ((y as f64 + 0.5) / fh as f64 * sh as f64 - 0.5).clamp(0.0, (sh - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let fy = (sy - y0 as f64) as f32;
        for x in 0..fw {
            let sx = ((x as f64 + 0.5) / fw as f64 * sw as f64 - 0.5).clamp(0.0, (sw - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let fx = (sx - x0 as f64) as f32;
            for c in 0..3 {
                let at = |xx: usize, yy: usize| small[(yy * sw + xx) * 3 + c];
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                row[x * 4 + c] = ((top * (1.0 - fy) + bottom * fy) * DIM)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            row[x * 4 + 3] = 255;
        }
    });
}

/// One pass of a box blur of `radius`, across then down, on RGB floats.
fn box_blur(img: &mut [f32], w: usize, h: usize, radius: usize) {
    let mut tmp = vec![0f32; img.len()];
    let r = radius as i64;
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0.0;
                let mut n = 0.0;
                for dx in -r..=r {
                    let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    sum += img[(y * w + xx) * 3 + c];
                    n += 1.0;
                }
                tmp[(y * w + x) * 3 + c] = sum / n;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0.0;
                let mut n = 0.0;
                for dy in -r..=r {
                    let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    sum += tmp[(yy * w + x) * 3 + c];
                    n += 1.0;
                }
                img[(y * w + x) * 3 + c] = sum / n;
            }
        }
    }
}

/// Brightness, contrast and saturation, as the DSL's `eq` filter names
/// them: brightness added (-1..1), contrast about the middle (1 unchanged),
/// saturation away from each pixel's own grey (1 unchanged). The editor's
/// preview applies the same matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Adjust {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
}

impl Adjust {
    pub fn is_identity(&self) -> bool {
        self.brightness.abs() < 1e-6
            && (self.contrast - 1.0).abs() < 1e-6
            && (self.saturation - 1.0).abs() < 1e-6
    }
}

pub(crate) fn adjust(pic: &mut [u8], width: usize, a: Adjust) {
    if a.is_identity() {
        return;
    }
    // Brightness and contrast are one line per channel, so a table.
    let mut table = [0f32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let v = i as f64 / 255.0;
        *t = ((v - 0.5) * a.contrast + 0.5 + a.brightness) as f32;
    }
    let s = a.saturation as f32;
    par_rows(pic, width, |_, row| {
        for px in row.chunks_exact_mut(4) {
            let r = table[px[0] as usize];
            let g = table[px[1] as usize];
            let b = table[px[2] as usize];
            let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let out = [l + (r - l) * s, l + (g - l) * s, l + (b - l) * s];
            for c in 0..3 {
                px[c] = (out[c] * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
}

/// How one picture gives way to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blend {
    Dissolve,
    FadeToBlack,
    WipeLeft,
    WipeRight,
}

/// `outgoing` giving way to `incoming` at `p` (0 all outgoing, 1 all
/// incoming), into `outgoing`.
pub(crate) fn transition(outgoing: &mut [u8], incoming: &[u8], width: usize, blend: Blend, p: f64) {
    let p = p.clamp(0.0, 1.0);
    let stride = width * 4;
    match blend {
        Blend::Dissolve => {
            let q = (p * 256.0).round() as u32;
            par_rows(outgoing, width, |y, row| {
                let src = &incoming[y * stride..(y + 1) * stride];
                for (o, i) in row.iter_mut().zip(src) {
                    *o = ((*o as u32 * (256 - q) + *i as u32 * q + 128) >> 8) as u8;
                }
            });
        }
        Blend::FadeToBlack => {
            // Down to black over the first half, up from it over the second.
            let (from_incoming, level) = if p < 0.5 {
                (false, 1.0 - 2.0 * p)
            } else {
                (true, 2.0 * p - 1.0)
            };
            let k = (level * 256.0).round() as u32;
            par_rows(outgoing, width, |y, row| {
                let src = &incoming[y * stride..(y + 1) * stride];
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let base = if from_incoming {
                        &src[x * 4..x * 4 + 4]
                    } else {
                        &*px
                    };
                    let v = [
                        ((base[0] as u32 * k) >> 8) as u8,
                        ((base[1] as u32 * k) >> 8) as u8,
                        ((base[2] as u32 * k) >> 8) as u8,
                    ];
                    px[..3].copy_from_slice(&v);
                    px[3] = 255;
                }
            });
        }
        Blend::WipeLeft | Blend::WipeRight => {
            // The edge runs right to left for a wipe left: the incoming
            // picture appears from the right.
            let edge = if blend == Blend::WipeLeft {
                ((1.0 - p) * width as f64).round() as usize
            } else {
                (p * width as f64).round() as usize
            };
            par_rows(outgoing, width, |y, row| {
                let src = &incoming[y * stride..(y + 1) * stride];
                let (from, to) = if blend == Blend::WipeLeft {
                    (edge, width)
                } else {
                    (0, edge)
                };
                row[from * 4..to * 4].copy_from_slice(&src[from * 4..to * 4]);
            });
        }
    }
}

/// Lay `overlay` (`ow` x `oh`, straight alpha) over the frame with its top
/// left at (`left`, `top`), scaled by `opacity`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn overlay(
    frame: &mut [u8],
    fw: usize,
    fh: usize,
    over: &[u8],
    ow: usize,
    oh: usize,
    left: i64,
    top: i64,
    opacity: f64,
) {
    let k = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    if k == 0 {
        return;
    }
    for oy in 0..oh {
        let y = top + oy as i64;
        if y < 0 || y as usize >= fh {
            continue;
        }
        for ox in 0..ow {
            let x = left + ox as i64;
            if x < 0 || x as usize >= fw {
                continue;
            }
            let s = (oy * ow + ox) * 4;
            let a = over[s + 3] as u32 * k / 255;
            if a == 0 {
                continue;
            }
            let d = (y as usize * fw + x as usize) * 4;
            for c in 0..3 {
                let v = (over[s + c] as u32 * a + frame[d + c] as u32 * (255 - a) + 127) / 255;
                frame[d + c] = v as u8;
            }
            frame[d + 3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        let mut v = vec![0u8; w * h * 4];
        fill(&mut v, c);
        v
    }

    fn at(img: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn a_fitted_picture_is_copied_into_the_middle() {
        let mut frame = solid(16, 9, [0, 0, 0, 255]);
        let pic = solid(8, 9, [200, 10, 10, 255]);
        place(
            &mut frame,
            16,
            9,
            &pic,
            8,
            9,
            &Placement {
                cx: 8.0,
                cy: 4.5,
                w: 8.0,
                h: 9.0,
                rotation: 0.0,
                crop: [0.0, 0.0, 1.0, 1.0],
            },
        );
        assert_eq!(at(&frame, 16, 3, 4), [0, 0, 0, 255]);
        assert_eq!(at(&frame, 16, 4, 4), [200, 10, 10, 255]);
        assert_eq!(at(&frame, 16, 11, 4), [200, 10, 10, 255]);
        assert_eq!(at(&frame, 16, 12, 4), [0, 0, 0, 255]);
    }

    #[test]
    fn a_picture_moved_right_shows_frame_on_its_left() {
        let mut frame = solid(20, 10, [0, 0, 0, 255]);
        let pic = solid(20, 10, [0, 255, 0, 255]);
        place(
            &mut frame,
            20,
            10,
            &pic,
            20,
            10,
            &Placement {
                cx: 15.0,
                cy: 5.0,
                w: 20.0,
                h: 10.0,
                rotation: 0.0,
                crop: [0.0, 0.0, 1.0, 1.0],
            },
        );
        assert_eq!(at(&frame, 20, 2, 5), [0, 0, 0, 255]);
        assert_eq!(at(&frame, 20, 8, 5), [0, 255, 0, 255]);
    }

    #[test]
    fn a_quarter_turn_lands_a_wide_picture_tall() {
        let mut frame = solid(40, 40, [0, 0, 0, 255]);
        let pic = solid(30, 10, [255, 255, 255, 255]);
        place(
            &mut frame,
            40,
            40,
            &pic,
            30,
            10,
            &Placement {
                cx: 20.0,
                cy: 20.0,
                w: 30.0,
                h: 10.0,
                rotation: std::f64::consts::FRAC_PI_2,
                crop: [0.0, 0.0, 1.0, 1.0],
            },
        );
        // Tall now: white above and below the centre, black beside it.
        assert_eq!(at(&frame, 40, 20, 8)[0], 255);
        assert_eq!(at(&frame, 40, 20, 32)[0], 255);
        assert_eq!(at(&frame, 40, 8, 20)[0], 0);
    }

    #[test]
    fn a_dissolve_halfway_is_the_average() {
        let mut a = solid(4, 4, [200, 0, 0, 255]);
        let b = solid(4, 4, [0, 0, 200, 255]);
        transition(&mut a, &b, 4, Blend::Dissolve, 0.5);
        assert_eq!(at(&a, 4, 1, 1), [100, 0, 100, 255]);
    }

    #[test]
    fn a_fade_through_black_is_black_in_the_middle() {
        let mut a = solid(4, 4, [200, 200, 200, 255]);
        let b = solid(4, 4, [100, 100, 100, 255]);
        transition(&mut a, &b, 4, Blend::FadeToBlack, 0.5);
        assert_eq!(at(&a, 4, 0, 0), [0, 0, 0, 255]);
        let mut a = solid(4, 4, [200, 200, 200, 255]);
        transition(&mut a, &b, 4, Blend::FadeToBlack, 1.0);
        assert_eq!(at(&a, 4, 0, 0), [100, 100, 100, 255]);
    }

    #[test]
    fn a_wipe_left_brings_the_next_picture_in_from_the_right() {
        let mut a = solid(10, 2, [255, 0, 0, 255]);
        let b = solid(10, 2, [0, 0, 255, 255]);
        transition(&mut a, &b, 10, Blend::WipeLeft, 0.3);
        assert_eq!(at(&a, 10, 9, 0), [0, 0, 255, 255]);
        assert_eq!(at(&a, 10, 0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn adjustments_move_brightness_and_saturation() {
        let mut pic = solid(2, 2, [100, 150, 200, 255]);
        adjust(
            &mut pic,
            2,
            Adjust {
                brightness: 0.1,
                contrast: 1.0,
                saturation: 1.0,
            },
        );
        assert!((at(&pic, 2, 0, 0)[0] as i32 - 126).abs() <= 1);
        let mut grey = solid(2, 2, [100, 150, 200, 255]);
        adjust(
            &mut grey,
            2,
            Adjust {
                brightness: 0.0,
                contrast: 1.0,
                saturation: 0.0,
            },
        );
        let p = at(&grey, 2, 0, 0);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }

    #[test]
    fn a_blurred_cover_fills_every_pixel() {
        let mut frame = solid(64, 36, [0, 0, 0, 0]);
        let pic = solid(9, 16, [90, 180, 30, 255]);
        blurred_cover(&mut frame, 64, 36, &pic, 9, 16);
        for px in frame.chunks_exact(4) {
            assert_eq!(px[3], 255);
            assert!(px[1] > 100, "{px:?}");
        }
    }

    #[test]
    fn text_is_laid_over_with_its_alpha() {
        let mut frame = solid(4, 4, [0, 0, 0, 255]);
        let over = solid(2, 2, [255, 255, 255, 128]);
        overlay(&mut frame, 4, 4, &over, 2, 2, 1, 1, 1.0);
        assert_eq!(at(&frame, 4, 0, 0), [0, 0, 0, 255]);
        let p = at(&frame, 4, 1, 1);
        assert!((p[0] as i32 - 128).abs() <= 1, "{p:?}");
    }
}
