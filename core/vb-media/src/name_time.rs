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

//! When a camera app's file name says a file was filmed.
//!
//! Some cameras write no capture time into the file. WeChat's own camera is
//! the one that matters: its clips carry no `creation_time`, and of one
//! user's 465 only the 25 a phone's camera had recorded did. It names what
//! it records `wx_camera_<milliseconds since 1970, UTC>` as it saves it, and
//! on those 25 the name was 0.3 to 16 seconds after the creation time -- the
//! end of the recording. So that name is the camera's clock, and read as
//! one for both backends.
//!
//! Only names that are a clock. `mmexport<ms>` is when a picture was saved
//! out of a chat, not when anyone filmed it, and a 剪映 export's name is when
//! it was exported: neither is read.

use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// WeChat began in 2011; nothing it named is older.
const EARLIEST_MS: i64 = 1_293_840_000_000;
/// Nor from after 2100, which a misread name would be.
const LATEST_MS: i64 = 4_102_444_800_000;

/// The capture time a file's name gives, RFC 3339 in UTC, when its name is
/// one a camera app writes as it saves.
pub fn from_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let rest = stem.strip_prefix("wx_camera_")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() != 13 {
        return None;
    }
    let ms: i64 = digits.parse().ok()?;
    if !(EARLIEST_MS..=LATEST_MS).contains(&ms) {
        return None;
    }
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wechats_camera_names_its_clips_by_the_clock() {
        assert_eq!(
            from_name(Path::new("/m/wx_camera_1578129938334.mp4")).as_deref(),
            Some("2020-01-04T09:25:38.334Z")
        );
        // Forwarded within WeChat, the name keeps its time.
        assert_eq!(
            from_name(Path::new("wx_camera_1768822068039_from999.mp4")).as_deref(),
            Some("2026-01-19T11:27:48.039Z")
        );
        assert!(from_name(Path::new("wx_camera_1578129938334.jpg")).is_some());
    }

    #[test]
    fn names_that_are_not_a_cameras_clock_say_nothing() {
        // Saved out of a chat, exported from 剪映, a phone's own clip.
        for name in [
            "mmexport1731035606862.mp4",
            "lv_0_20241221215345.mp4",
            "VID20241129223408.mp4",
            "wx_camera_123.mp4",
            "wx_camera_99991231235959.mp4",
            "wx_camera_0000000000001.mp4",
        ] {
            assert_eq!(from_name(Path::new(name)), None, "{name}");
        }
    }
}
