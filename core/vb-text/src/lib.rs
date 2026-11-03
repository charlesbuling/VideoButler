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

//! Text drawn into a finished piece: subtitles, captions and titles.
//!
//! The renderer composes every frame itself, so text is a picture it lays
//! over the frame like any other: shaped and rasterised here, once per item,
//! with the fonts the system already has. No ffmpeg text filter is involved
//! -- the LGPL build carries neither FreeType nor libass -- and the same
//! code draws the same text on every platform the engine runs on.
//!
//! The editor in the app draws its preview with Flutter's own text, asking
//! for the same family at the same size, so what the user places is what
//! the export shows; [`default_family`] is the family both ends fall back
//! to when the one asked for is not installed.

use cosmic_text::{Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::sync::{Mutex, OnceLock};

/// How a piece of text looks.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// A font family by name; the platform's own Chinese sans when absent or
    /// not installed.
    pub family: Option<String>,
    /// The font's size in pixels of the frame it is drawn into.
    pub size_px: f32,
    pub bold: bool,
    /// RGBA, straight alpha.
    pub color: [u8; 4],
    /// An outline round every glyph: its colour and width in pixels.
    pub outline: Option<([u8; 4], f32)>,
    /// A box behind the text, a little larger than it.
    pub background: Option<[u8; 4]>,
    pub align: TextAlign,
    /// The widest a line may run before it wraps, in pixels.
    pub max_width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Text rasterised: RGBA with straight alpha, transparent round the text.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
    default_family: String,
}

/// The families to fall back to, in order: the Chinese sans each platform
/// ships, since the product writes Chinese first and every one of these
/// carries Latin as well.
const FALLBACK_FAMILIES: [&str; 10] = [
    "PingFang SC",
    "Hiragino Sans GB",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Microsoft YaHei",
    "Microsoft YaHei UI",
    "STHeiti",
    "WenQuanYi Micro Hei",
    "Droid Sans Fallback",
];

/// The system's fonts, read once: reading them takes up to a second.
fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let started = std::time::Instant::now();
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_system_fonts();
        // Android keeps its fonts where no desktop convention looks.
        if cfg!(target_os = "android") {
            db.load_fonts_dir("/system/fonts");
        }
        let have = |name: &str| {
            db.faces().any(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| family.eq_ignore_ascii_case(name))
            })
        };
        let default_family = FALLBACK_FAMILIES
            .iter()
            .find(|name| have(name))
            .map(|name| name.to_string())
            .or_else(|| {
                db.faces()
                    .next()
                    .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            })
            .unwrap_or_else(|| "sans-serif".into());
        tracing::debug!(
            faces = db.len(),
            default = %default_family,
            ms = started.elapsed().as_millis() as u64,
            "fonts loaded"
        );
        // Chinese first: the locale decides which fallback a glyph the
        // chosen family lacks is drawn from.
        let system = FontSystem::new_with_locale_and_db("zh-CN".into(), db);
        Mutex::new(Fonts {
            system,
            cache: SwashCache::new(),
            default_family,
        })
    })
}

/// The family text is drawn in when none is named, or the one named is not
/// installed: the platform's Chinese sans.
pub fn default_family() -> String {
    fonts()
        .lock()
        .expect("the font system is never poisoned")
        .default_family
        .clone()
}

/// Every font family installed, sorted, each once.
pub fn families() -> Vec<String> {
    let fonts = fonts().lock().expect("the font system is never poisoned");
    let mut names: Vec<String> = fonts
        .system
        .db()
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The family to draw `asked` in: itself when installed, else the default.
fn resolve_family(fonts: &Fonts, asked: Option<&str>) -> String {
    let Some(asked) = asked.map(str::trim).filter(|a| !a.is_empty()) else {
        return fonts.default_family.clone();
    };
    fonts
        .system
        .db()
        .faces()
        .flat_map(|f| f.families.iter())
        .find(|(name, _)| name.eq_ignore_ascii_case(asked))
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| fonts.default_family.clone())
}

/// Draw `text` in `style`. `None` for text with nothing to draw.
pub fn render(text: &str, style: &Style) -> Option<Image> {
    let text = text.trim_end();
    if text.trim().is_empty() || !(style.size_px.is_finite() && style.size_px > 0.0) {
        return None;
    }
    let mut guard = fonts().lock().expect("the font system is never poisoned");
    let fonts = &mut *guard;
    let family = resolve_family(fonts, style.family.as_deref());

    let size = style.size_px.clamp(1.0, 2000.0);
    let line_height = (size * 1.3).ceil();
    let max_width = style.max_width.max(size * 2.0);
    let mut buffer = Buffer::new(&mut fonts.system, Metrics::new(size, line_height));
    buffer.set_size(Some(max_width), None);
    let mut attrs = Attrs::new().family(Family::Name(&family));
    if style.bold {
        attrs = attrs.weight(cosmic_text::Weight::BOLD);
    }
    let align = match style.align {
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
    };
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(align));
    buffer.shape_until_scroll(&mut fonts.system, false);

    let lines = buffer.layout_runs().count().max(1);
    let layout_h = (lines as f32 * line_height).ceil() as i32;
    let layout_w = max_width.ceil() as i32;
    if layout_w <= 0 || layout_h <= 0 {
        return None;
    }

    // The glyphs, in their own colours (an emoji keeps its own), and how
    // much of each pixel they cover, which the outline grows from.
    let (w, h) = (layout_w as usize, layout_h as usize);
    let mut fill = vec![0u8; w * h * 4];
    let mut cover = vec![0u8; w * h];
    let base = Color::rgba(style.color[0], style.color[1], style.color[2], 255);
    buffer.draw(
        &mut fonts.system,
        &mut fonts.cache,
        base,
        |x, y, gw, gh, color| {
            let a = color.a();
            if a == 0 {
                return;
            }
            for py in y.max(0)..(y + gh as i32).min(layout_h) {
                for px in x.max(0)..(x + gw as i32).min(layout_w) {
                    let i = py as usize * w + px as usize;
                    cover[i] = cover[i].max(a);
                    blend(
                        &mut fill[i * 4..i * 4 + 4],
                        [color.r(), color.g(), color.b(), a],
                    );
                }
            }
        },
    );
    drop(guard);

    // Only what the glyphs reach, across; every line's full height down,
    // so two texts of one size sit on one baseline.
    let (mut left, mut right) = (w, 0usize);
    for y in 0..h {
        for x in 0..w {
            if cover[y * w + x] > 0 {
                left = left.min(x);
                right = right.max(x + 1);
            }
        }
    }
    if right <= left {
        return None;
    }

    let outline = style
        .outline
        .filter(|(c, width)| c[3] > 0 && *width > 0.0)
        .map(|(c, width)| (c, width.min(size)));
    let grow = outline.map_or(0.0, |(_, width)| width).ceil() as usize;
    let pad = if style.background.is_some() {
        grow + (size * 0.3).ceil() as usize
    } else {
        grow + 1
    };
    let out_w = right - left + pad * 2;
    let out_h = h + pad * 2;
    let mut out = vec![0u8; out_w * out_h * 4];

    if let Some(bg) = style.background.filter(|c| c[3] > 0) {
        let radius = (size * 0.2).max(2.0);
        rounded_box(&mut out, out_w, out_h, radius, bg);
    }

    if let Some((color, width)) = outline {
        let ring = disc(width);
        let mut grown = vec![0u8; out_w * out_h];
        for y in 0..h {
            for x in left..right {
                let a = cover[y * w + x];
                if a == 0 {
                    continue;
                }
                let (cx, cy) = ((x - left + pad) as i32, (y + pad) as i32);
                for &(dx, dy, weight) in &ring {
                    let (ox, oy) = (cx + dx, cy + dy);
                    if ox < 0 || oy < 0 || ox >= out_w as i32 || oy >= out_h as i32 {
                        continue;
                    }
                    let v = (a as u32 * weight as u32 / 255) as u8;
                    let i = oy as usize * out_w + ox as usize;
                    grown[i] = grown[i].max(v);
                }
            }
        }
        for (i, &a) in grown.iter().enumerate() {
            if a > 0 {
                let alpha = (a as u32 * color[3] as u32 / 255) as u8;
                blend(
                    &mut out[i * 4..i * 4 + 4],
                    [color[0], color[1], color[2], alpha],
                );
            }
        }
    }

    for y in 0..h {
        for x in left..right {
            let src = &fill[(y * w + x) * 4..(y * w + x) * 4 + 4];
            if src[3] == 0 {
                continue;
            }
            let alpha = (src[3] as u32 * style.color[3] as u32 / 255) as u8;
            let o = ((y + pad) * out_w + (x - left + pad)) * 4;
            blend(&mut out[o..o + 4], [src[0], src[1], src[2], alpha]);
        }
    }

    Some(Image {
        width: out_w as u32,
        height: out_h as u32,
        rgba: out,
    })
}

/// `src` over `dst`, both straight alpha.
fn blend(dst: &mut [u8], src: [u8; 4]) {
    let sa = src[3] as u32;
    if sa == 0 {
        return;
    }
    if sa == 255 || dst[3] == 0 {
        dst.copy_from_slice(&src);
        return;
    }
    let da = dst[3] as u32;
    let out_a = sa * 255 + da * (255 - sa); // scaled by 255
    for c in 0..3 {
        let v = (src[c] as u32 * sa * 255 + dst[c] as u32 * da * (255 - sa)) / out_a.max(1);
        dst[c] = v.min(255) as u8;
    }
    dst[3] = (out_a / 255).min(255) as u8;
}

/// The offsets within `radius` of a point, each with how much of it the
/// disc covers at its edge, for a soft outline.
fn disc(radius: f32) -> Vec<(i32, i32, u8)> {
    let r = radius.ceil() as i32;
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (radius + 0.5 - d).clamp(0.0, 1.0);
            if weight > 0.0 {
                out.push((dx, dy, (weight * 255.0).round() as u8));
            }
        }
    }
    out
}

/// Fill the whole image with a box of `color` whose corners are rounded.
fn rounded_box(out: &mut [u8], w: usize, h: usize, radius: f32, color: [u8; 4]) {
    for y in 0..h {
        for x in 0..w {
            let cx = (x as f32 + 0.5).clamp(radius, w as f32 - radius);
            let cy = (y as f32 + 0.5).clamp(radius, h as f32 - radius);
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
            if cover > 0.0 {
                let a = (color[3] as f32 * cover).round() as u8;
                let i = (y * w + x) * 4;
                out[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], a]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            family: None,
            size_px: 48.0,
            bold: false,
            color: [255, 255, 255, 255],
            outline: Some(([0, 0, 0, 255], 3.0)),
            background: None,
            align: TextAlign::Center,
            max_width: 1000.0,
        }
    }

    fn opaque(image: &Image) -> usize {
        image.rgba.chunks_exact(4).filter(|p| p[3] > 128).count()
    }

    #[test]
    fn chinese_and_latin_both_draw() {
        // A font with no Chinese would draw nothing for these, or boxes;
        // the fallback is what puts ink on the page.
        let image = render("新疆 Xinjiang", &style()).expect("text draws");
        assert!(
            image.width > 100 && image.height >= 48,
            "{}x{}",
            image.width,
            image.height
        );
        assert!(opaque(&image) > 500, "only {} pixels inked", opaque(&image));
    }

    #[test]
    fn empty_text_draws_nothing() {
        assert!(render("   ", &style()).is_none());
        assert!(render("", &style()).is_none());
    }

    #[test]
    fn an_outline_surrounds_the_fill() {
        let mut plain = style();
        plain.outline = None;
        let with = render("田", &style()).unwrap();
        let without = render("田", &plain).unwrap();
        assert!(opaque(&with) > opaque(&without));
        // Black pixels exist only where there is an outline.
        let dark = |i: &Image| {
            i.rgba
                .chunks_exact(4)
                .filter(|p| p[3] > 200 && p[0] < 40 && p[1] < 40 && p[2] < 40)
                .count()
        };
        assert!(dark(&with) > 50);
        assert_eq!(dark(&without), 0);
    }

    #[test]
    fn long_text_wraps_at_the_width_given() {
        let mut narrow = style();
        narrow.max_width = 300.0;
        let one = render("一二三四五六七八九十", &style()).unwrap();
        let wrapped = render("一二三四五六七八九十", &narrow).unwrap();
        assert!(wrapped.width <= 300 + 20, "{} wide", wrapped.width);
        assert!(wrapped.height > one.height, "wrapping adds lines");
    }

    #[test]
    fn a_background_box_fills_round_the_text() {
        let mut boxed = style();
        boxed.outline = None;
        boxed.background = Some([0, 0, 0, 160]);
        let image = render("Hi", &boxed).unwrap();
        // The corner-adjacent middle of the top edge is the box, not empty.
        let i = (2 * image.width as usize + image.width as usize / 2) * 4;
        assert!(image.rgba[i + 3] > 100);
    }

    #[test]
    fn a_family_not_installed_falls_back_rather_than_failing() {
        let mut odd = style();
        odd.family = Some("No Such Family 42".into());
        assert!(render("字", &odd).is_some());
        assert!(!default_family().is_empty());
    }
}
