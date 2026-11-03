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

//! Text items drawn over the frame.
//!
//! A style's sizes are points of a frame whose shorter side is 540 points
//! (the schema's definition), so a subtitle reads the same at 720p and 4K
//! and on a portrait frame as on a landscape one. The app's preview uses
//! the same rule, which is what lets a user place text and see it land.

use vb_contracts::editing_dsl::{TextItem, TextRole, TextStyle};
use vb_text::{Image, Style, TextAlign};

/// The points a frame's shorter side holds.
pub const SHORT_SIDE_POINTS: f64 = 540.0;

/// Pixels per point on a frame of `width` x `height`.
pub fn pixels_per_point(width: u32, height: u32) -> f64 {
    width.min(height) as f64 / SHORT_SIDE_POINTS
}

/// The style a text item is drawn in when its own says nothing: a
/// subtitle's, or a title's for a title. The orchestrator writes the same.
fn defaults(role: Option<TextRole>) -> (f64, &'static str) {
    match role {
        Some(TextRole::Title) => (48.0, "middleCenter"),
        _ => (28.0, "bottomCenter"),
    }
}

/// `#RRGGBB` or `#RRGGBBAA` as RGBA.
pub fn parse_color(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// A text item rasterised for a frame, with where its top left goes.
#[derive(Debug, Clone)]
pub struct Placed {
    pub image: Image,
    pub left: i64,
    pub top: i64,
}

/// Rasterise `item` for a frame of `width` x `height` and place it.
pub fn place(item: &TextItem, width: u32, height: u32) -> Option<Placed> {
    let style = item.style.clone().unwrap_or_default();
    let (default_size, default_align) = defaults(item.role);
    let ppp = pixels_per_point(width, height);
    let size_pt = style
        .font_size_pt
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(default_size);
    let alignment = style
        .alignment
        .clone()
        .unwrap_or_else(|| default_align.into());
    let column = column_of(&alignment);
    let rendered = vb_text::render(
        &item.content,
        &Style {
            family: style.font_family.clone(),
            size_px: (size_pt * ppp) as f32,
            bold: style.bold.unwrap_or(false),
            color: style
                .primary_color
                .as_deref()
                .and_then(parse_color)
                .unwrap_or([255, 255, 255, 255]),
            outline: outline_of(&style, ppp),
            background: style.background_color.as_deref().and_then(parse_color),
            align: column,
            max_width: (width as f64 * 0.9) as f32,
        },
    )?;
    let (left, top) = position(&style, &alignment, width, height, &rendered);
    Some(Placed {
        image: rendered,
        left,
        top,
    })
}

/// The outline a style asks for, in pixels. A style with an outline colour
/// and no width gets the subtitle's 1.5 points; one with no colour gets
/// none.
fn outline_of(style: &TextStyle, ppp: f64) -> Option<([u8; 4], f32)> {
    match (&style.outline_color, style.outline_width) {
        (_, Some(w)) if w <= 0.0 => None,
        (Some(color), width) => {
            let color = parse_color(color)?;
            Some((color, (width.unwrap_or(1.5) * ppp) as f32))
        }
        // No colour named: a style the editor or the orchestrator wrote
        // always names one, so this is text styled by hand -- black, as
        // subtitles are.
        (None, Some(w)) => Some(([0, 0, 0, 255], (w * ppp) as f32)),
        (None, None) if style.background_color.is_none() => {
            Some(([0, 0, 0, 255], (1.5 * ppp) as f32))
        }
        (None, None) => None,
    }
}

fn column_of(alignment: &str) -> TextAlign {
    if alignment.ends_with("Left") {
        TextAlign::Left
    } else if alignment.ends_with("Right") {
        TextAlign::Right
    } else {
        TextAlign::Center
    }
}

/// Where the text's top left lands: the user's place when they set one,
/// else the alignment's, with the vertical margin from the top or bottom
/// edge and a twentieth of the width from the sides.
fn position(
    style: &TextStyle,
    alignment: &str,
    width: u32,
    height: u32,
    image: &Image,
) -> (i64, i64) {
    let (w, h) = (width as f64, height as f64);
    let (iw, ih) = (image.width as f64, image.height as f64);
    if let (Some(x), Some(y)) = (style.x, style.y) {
        let cx = x.clamp(0.0, 1.0) * w;
        let cy = y.clamp(0.0, 1.0) * h;
        return (
            (cx - iw / 2.0).round() as i64,
            (cy - ih / 2.0).round() as i64,
        );
    }
    let margin_v = style.margin_v_pct.unwrap_or(0.08).clamp(0.0, 0.5) * h;
    let margin_h = 0.05 * w;
    let left = match column_of(alignment) {
        TextAlign::Left => margin_h,
        TextAlign::Center => (w - iw) / 2.0,
        TextAlign::Right => w - margin_h - iw,
    };
    let top = if alignment.starts_with("top") {
        margin_v
    } else if alignment.starts_with("middle") {
        (h - ih) / 2.0
    } else {
        h - margin_v - ih
    };
    (left.round() as i64, top.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Micros;
    use vb_contracts::editing_dsl::Anchor;

    fn item(content: &str, style: Option<TextStyle>) -> TextItem {
        TextItem {
            id: "t".into(),
            anchor: Anchor::at("a"),
            duration_us: Micros::from_secs(2),
            content: content.into(),
            role: None,
            transcript_segment_id: None,
            style,
        }
    }

    #[test]
    fn colours_read_with_and_without_alpha() {
        assert_eq!(parse_color("#FF8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_color("#00000080"), Some([0, 0, 0, 128]));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn a_subtitle_sits_above_the_bottom_margin() {
        let placed = place(&item("字幕 subtitle", None), 1920, 1080).unwrap();
        let bottom = placed.top + placed.image.height as i64;
        // 8% of 1080 is 86 pixels up from the bottom.
        assert!((bottom - (1080 - 86)).abs() <= 2, "bottom at {bottom}");
        // Centred across.
        let centre = placed.left + placed.image.width as i64 / 2;
        assert!((centre - 960).abs() <= 2, "centre at {centre}");
    }

    #[test]
    fn the_same_points_are_twice_the_pixels_at_twice_the_resolution() {
        let small = place(&item("同", None), 960, 540).unwrap();
        let large = place(&item("同", None), 1920, 1080).unwrap();
        let ratio = large.image.height as f64 / small.image.height as f64;
        assert!((ratio - 2.0).abs() < 0.2, "ratio {ratio}");
    }

    #[test]
    fn a_placed_text_centres_where_the_user_put_it() {
        let style = TextStyle {
            x: Some(0.25),
            y: Some(0.2),
            ..TextStyle::default()
        };
        let placed = place(&item("这里", Some(style)), 1000, 1000).unwrap();
        let cx = placed.left + placed.image.width as i64 / 2;
        let cy = placed.top + placed.image.height as i64 / 2;
        assert!((cx - 250).abs() <= 2 && (cy - 200).abs() <= 2, "{cx},{cy}");
    }
}
