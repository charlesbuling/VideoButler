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

//! The canonical timebase for every contract in this project.
//!
//! Integer microseconds, never floats. This is not a style preference: float
//! seconds accumulate drift across a few hundred cuts, and frame numbers are
//! ambiguous at fractional rates like 29.97. Microseconds convert exactly to
//! CapCut (which is natively microseconds) and, via a source's rational frame
//! rate, to the frame counts OTIO and FCPXML want.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

/// A duration or offset in microseconds.
///
/// Serialises as a bare integer so the JSON matches the schema exactly.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Micros(pub u64);

impl Micros {
    pub const ZERO: Micros = Micros(0);

    pub const fn from_millis(ms: u64) -> Self {
        Micros(ms.saturating_mul(1_000))
    }

    pub const fn from_secs(s: u64) -> Self {
        Micros(s.saturating_mul(1_000_000))
    }

    /// Convert from a float number of seconds, which is the only form some
    /// external tools (notably the ffmpeg CLI) will give us. Confine the float
    /// to the boundary: parse once here, and work in integers thereafter.
    pub fn from_secs_f64(s: f64) -> Self {
        if !s.is_finite() || s <= 0.0 {
            return Micros::ZERO;
        }
        Micros((s * 1_000_000.0).round() as u64)
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000
    }

    pub fn saturating_sub(self, other: Micros) -> Micros {
        Micros(self.0.saturating_sub(other.0))
    }

    /// Exact conversion to a frame index at a rational frame rate.
    ///
    /// Done in `u128` so the intermediate product cannot overflow on long
    /// sources, and truncating rather than rounding so a frame boundary lands
    /// on the frame that is actually displayed.
    pub fn to_frames(self, fps_num: u32, fps_den: u32) -> u64 {
        if fps_den == 0 {
            return 0;
        }
        let n = self.0 as u128 * fps_num as u128;
        let d = 1_000_000u128 * fps_den as u128;
        (n / d) as u64
    }

    /// The microsecond timestamp at which a frame begins.
    ///
    /// Rounds **up**, and that is load-bearing. At 30000/1001 a frame boundary
    /// lands on a fraction of a microsecond: frame 1 starts at 33366.67us.
    /// Truncating to 33366 puts the timestamp inside frame 0's display
    /// interval, so `to_frames` hands back 0 and a round trip loses a frame.
    /// Rounding up keeps the timestamp at or after the true boundary, so
    /// `from_frames` then `to_frames` is the identity at every rate.
    pub fn from_frames(frames: u64, fps_num: u32, fps_den: u32) -> Self {
        if fps_num == 0 {
            return Micros::ZERO;
        }
        let n = frames as u128 * 1_000_000u128 * fps_den as u128;
        let d = fps_num as u128;
        Micros(n.div_ceil(d) as u64)
    }
}

impl Add for Micros {
    type Output = Micros;
    fn add(self, rhs: Micros) -> Micros {
        Micros(self.0.saturating_add(rhs.0))
    }
}

impl Sub for Micros {
    type Output = Micros;
    fn sub(self, rhs: Micros) -> Micros {
        Micros(self.0.saturating_sub(rhs.0))
    }
}

impl fmt::Display for Micros {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_ms = self.0 / 1_000;
        let ms = total_ms % 1_000;
        let total_s = total_ms / 1_000;
        let s = total_s % 60;
        let m = (total_s / 60) % 60;
        let h = total_s / 3_600;
        write!(f, "{h:02}:{m:02}:{s:02}.{ms:03}")
    }
}

/// A rational frame rate, kept exact so 30000/1001 never becomes 29.97.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const fn new(num: u32, den: u32) -> Self {
        FrameRate { num, den }
    }

    pub const NTSC_30: FrameRate = FrameRate::new(30_000, 1_001);
    pub const FILM_24: FrameRate = FrameRate::new(24, 1);
    pub const PAL_25: FrameRate = FrameRate::new(25, 1);

    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            return 0.0;
        }
        self.num as f64 / self.den as f64
    }
}

impl Default for FrameRate {
    fn default() -> Self {
        FrameRate::new(30, 1)
    }
}

/// Current UTC time as RFC 3339, the format both schemas require.
///
/// `::time` disambiguates the external crate from this module, which shares
/// its name.
pub(crate) fn _now_rfc3339() -> String {
    ::time::OffsetDateTime::now_utc()
        .format(&::time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Seconds since the epoch from an RFC 3339 time such as a phone writes to
/// `creation_time`: `2023-09-29T10:47:54.000000Z`. Offsets other than `Z`
/// are honoured; anything unparseable is `None`.
pub fn capture_seconds(text: &str) -> Option<i64> {
    let text = text.trim();
    let num = |a: usize, b: usize| text.get(a..b)?.parse::<i64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil calendar date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut seconds = days * 86_400 + hour * 3600 + minute * 60 + second;

    // A zone offset, when the time carries one rather than Z.
    let zone = text[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = zone.chars().next().filter(|c| *c == '+' || *c == '-') {
        let h = zone.get(1..3)?.parse::<i64>().ok()?;
        let m = zone
            .get(4..6)
            .and_then(|m| m.parse::<i64>().ok())
            .unwrap_or(0);
        let offset = h * 3600 + m * 60;
        seconds -= if sign == '+' { offset } else { -offset };
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_timestamps_are_rfc3339() {
        let now = _now_rfc3339();
        assert!(now.len() >= 20, "unexpected timestamp shape: {now}");
        assert!(now.contains('T'), "unexpected timestamp shape: {now}");
    }

    #[test]
    fn frame_conversion_is_exact_at_ntsc() {
        // 30000/1001 is the rate that breaks naive float math. One frame is
        // 33366.666... us, so frame boundaries must not drift as we walk out.
        let fr = FrameRate::NTSC_30;
        for frame in [0u64, 1, 30, 1_000, 100_000] {
            let t = Micros::from_frames(frame, fr.num, fr.den);
            assert_eq!(
                t.to_frames(fr.num, fr.den),
                frame,
                "round trip failed at frame {frame}"
            );
        }
    }

    #[test]
    fn an_hour_of_ntsc_does_not_drift() {
        let fr = FrameRate::NTSC_30;
        let one_hour_frames = 107_892u64;
        let t = Micros::from_frames(one_hour_frames, fr.num, fr.den);
        assert_eq!(t.to_frames(fr.num, fr.den), one_hour_frames);
    }

    #[test]
    fn subtraction_saturates_rather_than_panicking() {
        // Transition overlap is subtracted from slot durations all over the
        // renderer. An underflow there must not take the process down.
        assert_eq!(Micros(100) - Micros(500), Micros::ZERO);
    }

    #[test]
    fn display_is_timecode_shaped() {
        assert_eq!(Micros::from_secs(3_661).to_string(), "01:01:01.000");
        assert_eq!(Micros::from_millis(1_500).to_string(), "00:00:01.500");
    }

    #[test]
    fn negative_and_nan_seconds_clamp_to_zero() {
        // ffprobe can hand back junk on a malformed file; never propagate it.
        assert_eq!(Micros::from_secs_f64(f64::NAN), Micros::ZERO);
        assert_eq!(Micros::from_secs_f64(-3.0), Micros::ZERO);
    }
}
