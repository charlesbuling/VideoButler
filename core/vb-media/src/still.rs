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

//! Photographs.
//!
//! A photo is read by the same backends as a video -- one decode of one
//! picture -- but two things about it live in its EXIF rather than in
//! anything libav reports the same way for every format: which way up it
//! is, and when it was taken. Both are read here, once, for both backends,
//! so the CLI and linked libav cannot disagree about them.

use std::path::Path;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

/// File extensions read as photographs.
pub const STILL_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "heic", "heif", "webp"];

/// The longest side of a still written from a photograph, for the models
/// that read keyframes. A phone's 12 MP original is several megabytes a
/// still and no model looks at more than about a thousand pixels a side;
/// a folder of two thousand photos would otherwise fill the library with
/// full-size copies.
pub const STILL_MAX_SIDE: u32 = 1920;

/// Is this file a photograph, by its name?
///
/// By name because that is how every picker and folder walk already
/// chooses what to offer, and because it costs nothing: a photo is never
/// opened only to learn that it is one.
pub fn is_still_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| STILL_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// What a photograph's EXIF says about it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoMeta {
    /// The clockwise turn, 0, 90, 180 or 270, that shows it upright. A
    /// mirrored orientation (a front camera set to mirror) keeps its turn
    /// and loses the mirror, which nothing downstream can represent.
    pub rotation: u32,
    /// When it was taken, as RFC 3339 with the offset it was taken at, when
    /// the camera recorded a time.
    pub taken_at: Option<String>,
    /// Where it was taken, from its GPS fix.
    pub location: Option<crate::GeoPoint>,
}

/// Read a photograph's orientation and capture time. A file with no EXIF,
/// or EXIF that will not parse, is upright and undated: a photo is never
/// refused for its metadata.
pub fn photo_meta(path: &Path) -> PhotoMeta {
    let Ok(file) = std::fs::File::open(path) else {
        return PhotoMeta::default();
    };
    let mut reader = std::io::BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return PhotoMeta::default();
    };
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(OffsetDateTime::from);
    PhotoMeta {
        rotation: rotation(&exif),
        taken_at: taken_at(&exif, modified),
        location: gps_location(&exif),
    }
}

/// The GPS fix, as degrees, minutes and seconds with a hemisphere each.
fn gps_location(exif: &exif::Exif) -> Option<crate::GeoPoint> {
    let degrees = |tag: exif::Tag, negative: &str, reference: exif::Tag| -> Option<f64> {
        let field = exif.get_field(tag, exif::In::PRIMARY)?;
        let exif::Value::Rational(parts) = &field.value else {
            return None;
        };
        let part = |i: usize| parts.get(i).map(|r| r.to_f64()).filter(|v| v.is_finite());
        let value = part(0)? + part(1).unwrap_or(0.0) / 60.0 + part(2).unwrap_or(0.0) / 3600.0;
        let sign = match ascii(exif, reference) {
            Some(r) if r.eq_ignore_ascii_case(negative) => -1.0,
            _ => 1.0,
        };
        Some(sign * value)
    };
    let lat = degrees(exif::Tag::GPSLatitude, "S", exif::Tag::GPSLatitudeRef)?;
    let lon = degrees(exif::Tag::GPSLongitude, "W", exif::Tag::GPSLongitudeRef)?;
    crate::GeoPoint::plausible(lat, lon)
}

fn rotation(exif: &exif::Exif) -> u32 {
    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1);
    match orientation {
        3 | 4 => 180,
        6 | 7 => 90,
        5 | 8 => 270,
        _ => 0,
    }
}

fn ascii(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let field = exif.get_field(tag, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Ascii(parts) => parts
            .first()
            .map(|p| String::from_utf8_lossy(p).trim().to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// `2023:09:29 18:31:43`, the camera's clock with no zone.
fn parse_exif_time(text: &str) -> Option<PrimitiveDateTime> {
    let (date, clock) = text.trim().split_once(' ')?;
    let d: Vec<&str> = date.split(':').collect();
    let t: Vec<&str> = clock.split(':').collect();
    if d.len() != 3 || t.len() < 3 {
        return None;
    }
    let num = |s: &str| s.trim().parse::<u32>().ok();
    let date = Date::from_calendar_date(
        num(d[0])? as i32,
        Month::try_from(num(d[1])? as u8).ok()?,
        num(d[2])? as u8,
    )
    .ok()?;
    // Some cameras write fractional seconds into the seconds field.
    let seconds = t[2].split('.').next().and_then(num)?;
    let time = Time::from_hms(num(t[0])? as u8, num(t[1])? as u8, seconds.min(59) as u8).ok()?;
    Some(PrimitiveDateTime::new(date, time))
}

/// `+08:00`, EXIF 2.31's OffsetTimeOriginal.
fn parse_offset(text: &str) -> Option<UtcOffset> {
    let text = text.trim();
    let sign = match text.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (h, m) = text[1..].split_once(':')?;
    let (h, m) = (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?);
    UtcOffset::from_hms(sign * h, sign * m, 0).ok()
}

/// The GPS receiver's own clock, which is UTC.
fn gps_time(exif: &exif::Exif) -> Option<OffsetDateTime> {
    let date = ascii(exif, exif::Tag::GPSDateStamp)?;
    let field = exif.get_field(exif::Tag::GPSTimeStamp, exif::In::PRIMARY)?;
    let exif::Value::Rational(parts) = &field.value else {
        return None;
    };
    if parts.len() < 3 || parts.iter().any(|r| r.denom == 0) {
        return None;
    }
    let seconds = parts[2].to_f64();
    let text = format!(
        "{date} {:02}:{:02}:{:02}",
        parts[0].to_f64() as u32,
        parts[1].to_f64() as u32,
        seconds as u32
    );
    Some(parse_exif_time(&text)?.assume_utc())
}

/// The zone the camera's clock was in, from an instant known in UTC that
/// was taken at about the same moment: the whole quarter-hour between the
/// two, when the two agree to within two minutes of one. Time zones are
/// whole quarter-hours; anything else means the two instants are not the
/// same moment -- a file copied later, a GPS fix from before a flight.
fn offset_from(clock: PrimitiveDateTime, instant: OffsetDateTime) -> Option<UtcOffset> {
    let apart = (clock.assume_utc() - instant).whole_seconds();
    let quarters = (apart as f64 / 900.0).round() as i64;
    if quarters.abs() > 14 * 4 || (apart - quarters * 900).abs() > 120 {
        return None;
    }
    UtcOffset::from_whole_seconds((quarters * 900) as i32).ok()
}

/// When the photo was taken, in the zone it was taken in.
///
/// Videos carry their capture time in UTC, and a photo is grouped with the
/// videos filmed around it by time, so a photo's clock reading needs its
/// zone. In order: the zone the camera wrote (EXIF 2.31); the zone implied
/// by the GPS fix, which is UTC; the zone implied by the file's own time,
/// which a phone sets when it writes the photo and a copy usually keeps.
/// With none of them the reading is kept as if it were UTC, which is the
/// best there is and hours out at worst.
fn taken_at(exif: &exif::Exif, modified: Option<OffsetDateTime>) -> Option<String> {
    let clock = [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ]
    .into_iter()
    .find_map(|tag| ascii(exif, tag).and_then(|t| parse_exif_time(&t)))?;

    let written = [exif::Tag::OffsetTimeOriginal, exif::Tag::OffsetTime]
        .into_iter()
        .find_map(|tag| ascii(exif, tag).and_then(|t| parse_offset(&t)));
    let offset = written
        .or_else(|| gps_time(exif).and_then(|gps| offset_from(clock, gps)))
        .or_else(|| modified.and_then(|m| offset_from(clock, m)))
        .unwrap_or(UtcOffset::UTC);
    clock
        .assume_offset(offset)
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn photos_are_known_by_their_names() {
        assert!(is_still_path(Path::new("/a/P30929-183143.JPG")));
        assert!(is_still_path(Path::new("IMG_0001.heic")));
        assert!(!is_still_path(Path::new("V30929-184730.mp4")));
        assert!(!is_still_path(Path::new("notes")));
    }

    #[test]
    fn a_camera_clock_reads_with_or_without_fractions() {
        assert_eq!(
            parse_exif_time("2023:09:29 18:31:43"),
            Some(datetime!(2023-09-29 18:31:43))
        );
        assert_eq!(
            parse_exif_time("2023:09:29 18:31:43.25"),
            Some(datetime!(2023-09-29 18:31:43))
        );
        assert_eq!(parse_exif_time("0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif_time(""), None);
    }

    #[test]
    fn written_offsets_parse_both_ways() {
        assert_eq!(
            parse_offset("+08:00"),
            Some(UtcOffset::from_hms(8, 0, 0).unwrap())
        );
        assert_eq!(
            parse_offset("-03:30"),
            Some(UtcOffset::from_hms(-3, -30, 0).unwrap())
        );
        assert_eq!(parse_offset("08:00"), None);
    }

    #[test]
    fn the_zone_is_the_quarter_hour_between_the_clock_and_a_utc_instant() {
        // A Meizu in Xinjiang: 18:31:43 on the camera, 10:31:42 UTC on the
        // file it wrote.
        let clock = datetime!(2023-09-29 18:31:43);
        let file = datetime!(2023-09-29 10:31:42 UTC);
        assert_eq!(
            offset_from(clock, file),
            Some(UtcOffset::from_hms(8, 0, 0).unwrap())
        );
        // India's half hour.
        let file = datetime!(2023-09-29 13:01:40 UTC);
        assert_eq!(
            offset_from(clock, file),
            Some(UtcOffset::from_hms(5, 30, 0).unwrap())
        );
        // A copy made a week later says nothing about the zone.
        let later = datetime!(2023-10-06 09:12:00 UTC);
        assert_eq!(offset_from(clock, later), None);
        // Nor does a time twelve minutes off any zone.
        let off = datetime!(2023-09-29 10:19:43 UTC);
        assert_eq!(offset_from(clock, off), None);
    }
}
