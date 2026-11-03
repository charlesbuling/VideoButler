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

//! Where a video or photo was taken.
//!
//! A phone writes its GPS fix into the file: Android and most cameras as the
//! container's `location` tag, an iPhone as
//! `com.apple.quicktime.location.ISO6709`, both in ISO 6709
//! (`+30.7735+114.2143/`); a photo in its EXIF. Read here once for both
//! backends, so the CLI and linked libav cannot disagree about it.

use serde::{Deserialize, Serialize};

/// A point on the earth, in degrees: latitude north positive, longitude
/// east positive. WGS 84, as a GPS receiver reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    /// A fix the file can be believed about: on the earth, and not
    /// `+00.0000+000.0000`, which is what a phone writes when it had none --
    /// one real clip here says it was filmed in the Gulf of Guinea.
    pub fn plausible(lat: f64, lon: f64) -> Option<GeoPoint> {
        let on_earth = lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon);
        let null_island = lat.abs() < 1e-4 && lon.abs() < 1e-4;
        (on_earth && !null_island).then_some(GeoPoint { lat, lon })
    }
}

/// The container tags a location is found under, in the order they are
/// read: what most phones write, then what an iPhone writes.
pub const LOCATION_TAGS: [&str; 3] = [
    "location",
    "com.apple.quicktime.location.ISO6709",
    "location-eng",
];

/// Read an ISO 6709 point such as `+30.7735+114.2143/` or
/// `+37.3318-122.0312+012.000/`: signed decimal degrees, latitude then
/// longitude, an optional altitude, an optional `CRS` suffix and slash.
/// Only the decimal-degree form is read; the degree-minute forms phones do
/// not write are `None`, as is anything else.
pub fn parse_iso6709(text: &str) -> Option<GeoPoint> {
    let text = text.trim().trim_end_matches('/');
    let mut numbers = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c == '+' || c == '-' {
            if let Some(s) = start {
                numbers.push(&text[s..i]);
            }
            start = Some(i);
        } else if !(c.is_ascii_digit() || c == '.') {
            // `CRSWGS_84` and the like end the numbers.
            if let Some(s) = start.take() {
                numbers.push(&text[s..i]);
            }
            break;
        }
    }
    if let Some(s) = start {
        numbers.push(&text[s..]);
    }
    if numbers.len() < 2 {
        return None;
    }
    // Decimal degrees have at most two (latitude) and three (longitude)
    // digits before the point; more is the DDMM or DDMMSS form.
    let whole = |n: &str| n[1..].split('.').next().map_or(0, str::len);
    if whole(numbers[0]) > 2 || whole(numbers[1]) > 3 {
        return None;
    }
    let lat = numbers[0].parse::<f64>().ok()?;
    let lon = numbers[1].parse::<f64>().ok()?;
    GeoPoint::plausible(lat, lon)
}

/// The location among a file's tags: the first of [`LOCATION_TAGS`] that is
/// there and not empty, read. The first one present decides, not the first
/// that reads, because that is all linked libav's shim hands over and the
/// two backends must agree.
pub fn from_tags<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Option<GeoPoint> {
    LOCATION_TAGS
        .iter()
        .find_map(|tag| get(tag).filter(|v| !v.is_empty()))
        .and_then(parse_iso6709)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_android_location_tag_reads() {
        // Los Angeles, west of Greenwich.
        let p = parse_iso6709("+34.0522-118.2437/").unwrap();
        assert!((p.lat - 34.0522).abs() < 1e-9);
        assert!((p.lon + 118.2437).abs() < 1e-9);
        // Xinjiang, with the longitude's leading zero.
        let p = parse_iso6709("+43.3372+087.4084/").unwrap();
        assert!((p.lon - 87.4084).abs() < 1e-9);
    }

    #[test]
    fn an_iphone_location_with_altitude_reads() {
        let p = parse_iso6709("+37.3318-122.0312+012.000/").unwrap();
        assert!((p.lat - 37.3318).abs() < 1e-9);
        assert!((p.lon + 122.0312).abs() < 1e-9);
        let p = parse_iso6709("-33.8688+151.2093CRSWGS_84/").unwrap();
        assert!((p.lat + 33.8688).abs() < 1e-9);
    }

    #[test]
    fn no_fix_is_not_a_place() {
        // VID_20260214_035333.mp4 says this; it was filmed at home.
        assert_eq!(parse_iso6709("+00.0000+000.0000/"), None);
        assert_eq!(parse_iso6709(""), None);
        assert_eq!(parse_iso6709("+91.0+010.0/"), None);
        assert_eq!(parse_iso6709("nonsense"), None);
        // Degrees and minutes, which phones do not write, are not guessed.
        assert_eq!(parse_iso6709("+3046.41+11412.86/"), None);
    }

    #[test]
    fn the_first_tag_present_decides() {
        let tags = [
            ("location", ""),
            ("com.apple.quicktime.location.ISO6709", "+48.8566+002.3522/"),
            ("location-eng", "+30.7735+114.2143/"),
        ];
        let p = from_tags(|name| tags.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)).unwrap();
        assert!((p.lat - 48.8566).abs() < 1e-9);
        assert_eq!(from_tags(|_| None), None);
    }
}
