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

//! Each frame of the finished piece, drawn from the layout.
//!
//! The clips on screen at a moment -- one, or two while a transition joins
//! them -- are each drawn over their background with their placement, then
//! blended, then the text on screen is laid over the top. A clip's frames
//! come from a reader opened when it first appears and dropped when it
//! ends, so a piece plays its clips straight through without a seek per
//! frame.

use crate::RenderError;
use crate::caption::{self, Placed};
use crate::layers::{Content, Layer, Layout};
use crate::picture::{self, Adjust, Blend, Placement};
use std::collections::HashMap;
use vb_contracts::Micros;
use vb_contracts::editing_dsl::{BackgroundKind, TransitionType};
use vb_media::{FrameReader, MediaBackend, VideoFrame};

/// A picture ready to draw: RGBA, its size, already adjusted.
struct Picture {
    rgba: Vec<u8>,
    width: usize,
    height: usize,
}

/// What a clip on screen is showing, and where its next frame comes from.
struct Showing {
    reader: Option<Box<dyn FrameReader>>,
    current: Option<Picture>,
    current_at: Micros,
    pending: Option<VideoFrame>,
    ended: bool,
    /// The size the reader hands frames over at.
    size: (usize, usize),
    adjust: Option<Adjust>,
}

pub(crate) struct Composer<'a> {
    media: &'a dyn MediaBackend,
    layout: &'a Layout,
    width: usize,
    height: usize,
    showing: HashMap<usize, Showing>,
    texts: HashMap<usize, Option<Placed>>,
    /// A second frame for the incoming clip of a transition.
    spare: Vec<u8>,
    /// Draw one moment only: each clip's picture is decoded at that moment
    /// alone, as a photograph is, rather than through a reader that plays
    /// on. What the editor's still preview wants, and it needs nothing of
    /// the backend but `rgb_frame`.
    one_frame: bool,
    pub warnings: Vec<String>,
}

/// The adjustments a clip's `eq` filter asks for.
fn adjust_of(layer: &Layer) -> Option<Adjust> {
    let filter = layer
        .video
        .as_ref()?
        .filters
        .iter()
        .find(|f| f.name == "eq")?;
    let param = |name: &str, default: f64| {
        filter
            .params
            .get(name)
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let a = Adjust {
        brightness: param("brightness", 0.0).clamp(-1.0, 1.0),
        contrast: param("contrast", 1.0).clamp(0.0, 3.0),
        saturation: param("saturation", 1.0).clamp(0.0, 3.0),
    };
    (!a.is_identity()).then_some(a)
}

/// The part of the picture a clip shows, as shares of it.
fn crop_of(layer: &Layer) -> [f64; 4] {
    layer
        .video
        .as_ref()
        .and_then(|v| v.crop)
        .map(|c| {
            let x = c.x.clamp(0.0, 1.0);
            let y = c.y.clamp(0.0, 1.0);
            let w = c.w.clamp(0.01, 1.0 - x);
            let h = c.h.clamp(0.01, 1.0 - y);
            [x, y, w, h]
        })
        .unwrap_or([0.0, 0.0, 1.0, 1.0])
}

impl<'a> Composer<'a> {
    pub fn new(media: &'a dyn MediaBackend, layout: &'a Layout, width: u32, height: u32) -> Self {
        Composer {
            media,
            layout,
            width: width as usize,
            height: height as usize,
            showing: HashMap::new(),
            texts: HashMap::new(),
            spare: Vec::new(),
            one_frame: false,
            warnings: Vec::new(),
        }
    }

    /// A composer for one moment of the piece, which decodes each clip at
    /// that moment alone.
    pub fn one_frame(
        media: &'a dyn MediaBackend,
        layout: &'a Layout,
        width: u32,
        height: u32,
    ) -> Self {
        Composer {
            one_frame: true,
            ..Composer::new(media, layout, width, height)
        }
    }

    /// The fitted scale of a clip's shown part: how much it is enlarged to
    /// sit in the frame whole.
    fn fit(&self, size: (u32, u32), crop: [f64; 4]) -> f64 {
        let (w, h) = (size.0 as f64 * crop[2], size.1 as f64 * crop[3]);
        (self.width as f64 / w).min(self.height as f64 / h)
    }

    /// The size a clip's reader hands frames over at: as large as the
    /// picture is ever shown, never larger than the file unless fitting
    /// itself enlarges it.
    fn reader_size(&self, layer: &Layer, size: (u32, u32)) -> (usize, usize) {
        let crop = crop_of(layer);
        let fit = self.fit(size, crop);
        let f = (fit * layer.max_scale()).min(fit.max(1.0));
        let w = ((size.0 as f64 * f).round() as usize).max(2);
        let h = ((size.1 as f64 * f).round() as usize).max(2);
        (w, h)
    }

    /// Draw the piece at `t` into `out`, `width * height * 4` bytes.
    pub fn frame(&mut self, t: Micros, out: &mut [u8]) -> Result<(), RenderError> {
        let active: Vec<usize> = self
            .layout
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.start <= t && t < l.end)
            .map(|(i, _)| i)
            .collect();
        // Clips that have ended let their readers go.
        let first_active = active.first().copied().unwrap_or(usize::MAX);
        self.showing
            .retain(|i, _| *i >= first_active || active.contains(i));

        match active.as_slice() {
            [] => picture::fill(out, [0, 0, 0, 255]),
            [only] => self.draw_layer(*only, t, out)?,
            [.., before, last] => {
                let (before, last) = (*before, *last);
                self.draw_layer(before, t, out)?;
                let layer = &self.layout.layers[last];
                match layer.incoming {
                    Some((kind, overlap))
                        if kind != TransitionType::Cut && overlap > Micros::ZERO =>
                    {
                        let mut spare = std::mem::take(&mut self.spare);
                        spare.resize(out.len(), 0);
                        self.draw_layer(last, t, &mut spare)?;
                        let p = t.saturating_sub(layer.start).0 as f64 / overlap.0 as f64;
                        let blend = match kind {
                            TransitionType::FadeToBlack => Blend::FadeToBlack,
                            TransitionType::WipeLeft => Blend::WipeLeft,
                            TransitionType::WipeRight => Blend::WipeRight,
                            _ => Blend::Dissolve,
                        };
                        picture::transition(out, &spare, self.width, blend, p);
                        self.spare = spare;
                    }
                    // Overlapping with no transition to say how: the later
                    // slot is on top.
                    _ => self.draw_layer(last, t, out)?,
                }
            }
        }
        self.draw_texts(t, out);
        Ok(())
    }

    fn draw_texts(&mut self, t: Micros, out: &mut [u8]) {
        let (w, h) = (self.width, self.height);
        for (i, text) in self.layout.texts.iter().enumerate() {
            if !(text.start <= t && t < text.end) {
                continue;
            }
            let placed = self
                .texts
                .entry(i)
                .or_insert_with(|| caption::place(&text.item, w as u32, h as u32));
            if let Some(p) = placed {
                picture::overlay(
                    out,
                    w,
                    h,
                    &p.image.rgba,
                    p.image.width as usize,
                    p.image.height as usize,
                    p.left,
                    p.top,
                    1.0,
                );
            }
        }
    }

    /// One slot's picture at `t`, over its background, into `out`.
    fn draw_layer(&mut self, index: usize, t: Micros, out: &mut [u8]) -> Result<(), RenderError> {
        let layer = &self.layout.layers[index];
        let Content::Clip {
            size, still, path, ..
        } = &layer.content
        else {
            picture::fill(out, [0, 0, 0, 255]);
            return Ok(());
        };
        let (size, still, path) = (*size, *still, path.clone());
        let source_t = layer.source_time(t);
        self.advance(index, source_t, still, &path, size)?;
        let layer = &self.layout.layers[index];
        let Some(showing) = self.showing.get(&index) else {
            picture::fill(out, [0, 0, 0, 255]);
            return Ok(());
        };
        let Some(pic) = &showing.current else {
            picture::fill(out, [0, 0, 0, 255]);
            return Ok(());
        };

        let crop = crop_of(layer);
        let kf = layer.placement_at(t);
        let fit = self.fit(size, crop);
        let plain = kf.is_fitted() && crop == [0.0, 0.0, 1.0, 1.0];
        let place = if plain {
            // Exactly the reader's pixels, on whole pixels: a copy.
            let left = (self.width as i64 - pic.width as i64) / 2;
            let top = (self.height as i64 - pic.height as i64) / 2;
            Placement {
                cx: left as f64 + pic.width as f64 / 2.0,
                cy: top as f64 + pic.height as f64 / 2.0,
                w: pic.width as f64,
                h: pic.height as f64,
                rotation: 0.0,
                crop,
            }
        } else {
            Placement {
                cx: self.width as f64 / 2.0 + kf.x * self.width as f64,
                cy: self.height as f64 / 2.0 + kf.y * self.height as f64,
                w: size.0 as f64 * crop[2] * fit * kf.scale,
                h: size.1 as f64 * crop[3] * fit * kf.scale,
                rotation: kf.rotation_deg.to_radians(),
                crop,
            }
        };
        let covers = plain && pic.width >= self.width && pic.height >= self.height;
        if !covers {
            let background = layer.video.as_ref().and_then(|v| v.background.clone());
            match background.as_ref().map(|b| b.kind) {
                Some(BackgroundKind::Blur) => picture::blurred_cover(
                    out,
                    self.width,
                    self.height,
                    &pic.rgba,
                    pic.width,
                    pic.height,
                ),
                Some(BackgroundKind::Color) => picture::fill(
                    out,
                    background
                        .and_then(|b| b.color)
                        .as_deref()
                        .and_then(caption::parse_color)
                        .map(|c| [c[0], c[1], c[2], 255])
                        .unwrap_or([0, 0, 0, 255]),
                ),
                _ => picture::fill(out, [0, 0, 0, 255]),
            }
        }
        picture::place(
            out,
            self.width,
            self.height,
            &pic.rgba,
            pic.width,
            pic.height,
            &place,
        );
        Ok(())
    }

    /// Bring clip `index` to the frame on screen at `source_t`.
    fn advance(
        &mut self,
        index: usize,
        source_t: Micros,
        still: bool,
        path: &std::path::Path,
        size: (u32, u32),
    ) -> Result<(), RenderError> {
        if !self.showing.contains_key(&index) {
            let layer = &self.layout.layers[index];
            let reader_size = self.reader_size(layer, size);
            let adjust = adjust_of(layer);
            let mut showing = Showing {
                reader: None,
                current: None,
                current_at: Micros::ZERO,
                pending: None,
                ended: false,
                size: reader_size,
                adjust,
            };
            if !path.exists() {
                return Err(RenderError::MissingFile(path.display().to_string()));
            }
            if still || self.one_frame {
                // A photograph is one picture, decoded once and held; so is
                // a clip drawn at one moment only.
                let at = if still { Micros::ZERO } else { source_t };
                let frame = self
                    .media
                    .rgb_frame(path, at, reader_size.0 as u32, reader_size.1 as u32)
                    .map_err(RenderError::Media)?;
                let mut rgba = Vec::with_capacity(frame.rgb.len() / 3 * 4);
                for px in frame.rgb.chunks_exact(3) {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                if let Some(a) = adjust {
                    picture::adjust(&mut rgba, reader_size.0, a);
                }
                showing.current = Some(Picture {
                    rgba,
                    width: reader_size.0,
                    height: reader_size.1,
                });
                showing.ended = true;
            } else {
                showing.reader = Some(
                    self.media
                        .read_frames(path, source_t, reader_size.0 as u32, reader_size.1 as u32)
                        .map_err(RenderError::Media)?,
                );
            }
            self.showing.insert(index, showing);
        }
        let showing = self.showing.get_mut(&index).expect("inserted above");
        if still || self.one_frame {
            return Ok(());
        }
        loop {
            if showing.pending.is_none() && !showing.ended {
                match showing.reader.as_mut().map(|r| r.next_frame()) {
                    Some(Ok(Some(frame))) => showing.pending = Some(frame),
                    Some(Ok(None)) | None => {
                        showing.ended = true;
                        showing.reader = None;
                    }
                    Some(Err(e)) => {
                        showing.ended = true;
                        showing.reader = None;
                        self.warnings
                            .push(format!("{} stopped decoding early: {e}", path.display()));
                    }
                }
            }
            // The pending frame is due -- or nothing is on screen yet, and
            // the first frame after the moment is the best there is.
            let due = match &showing.pending {
                Some(f) => f.at <= source_t || showing.current.is_none(),
                None => false,
            };
            if !due {
                break;
            }
            let frame = showing.pending.take().expect("checked above");
            let (w, h) = (frame.width as usize, frame.height as usize);
            let mut rgba = frame.rgba;
            if let Some(a) = showing.adjust {
                picture::adjust(&mut rgba, w, a);
            }
            showing.current_at = frame.at;
            showing.current = Some(Picture {
                rgba,
                width: w,
                height: h,
            });
            if showing.current_at > source_t {
                break;
            }
        }
        let _ = showing.size;
        Ok(())
    }
}
