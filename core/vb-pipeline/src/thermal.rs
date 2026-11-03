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

//! Slowing a run down before the device overheats.
//!
//! A phone running the whole model host -- decoding, depth, faces, pose,
//! speech, a vision-language model -- at full speed for minutes gets hot
//! enough that the system closes the app, and the run is lost. When the
//! user leaves the guard on (Settings; on by default), a run reads how hot
//! the device is between units of work and paces itself: while it is warm
//! it rests half as long as it worked, and analyses one clip at a time
//! instead of three; when it is hot it stops until it has cooled.
//!
//! The reading is the system's own, and so are the limits: on Apple
//! platforms `NSProcessInfo.thermalState`, slowing at "serious" and pausing
//! at "critical"; on Android the thermal status and the headroom forecast
//! ten seconds ahead, slowing at the device's own "moderate" threshold and
//! pausing at "severe", both of which come before "critical", where the
//! system starts closing apps. The host writes the reading, and the
//! thresholds where Android 15 reports them, to a file beside the library,
//! because the engine cannot ask Android's power service itself.
//!
//! "Light" is not acted on. Measured on a Galaxy S21: headroom runs about
//! linearly with the skin temperature, 0.87 ("light") at 37 degrees, 0.93
//! ("moderate") at 39, 1.0 ("severe") at 41, 1.13 ("critical") at 45; the
//! phone sat at "light" for minutes after any work, and at 32 degrees --
//! its temperature at rest -- the headroom was already 0.70.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// How hot the device is, in the terms this module acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Heat {
    Cool,
    Warm,
    Hot,
}

/// Headroom at which a run starts resting between units of work, where the
/// device does not say: Android's "moderate", as the S21 reports it.
pub const WARM_HEADROOM: f64 = 0.93;
/// Headroom at which it stops until the device cools: "severe", which
/// Android defines as a headroom of 1.
pub const HOT_HEADROOM: f64 = 1.0;

/// The file the Android host writes the reading to, in the directory it
/// names with `VB_WORKDIR`: `status=<0..6>` and `headroom=<float>` lines,
/// and `warm=` and `hot=` with the device's own moderate and severe
/// headroom thresholds where it reports them.
pub const READING_FILE: &str = "thermal";

/// Work done before a warm device rests at all: short units -- a frame of
/// the sweep -- are gathered into a stretch worth resting after.
const WORK_BEFORE_REST: Duration = Duration::from_secs(2);
/// A warm device rests this share of the time it worked...
const REST_SHARE: f64 = 0.5;
/// ...but never longer than this at once, so a long unit (a caption on a
/// phone's CPU) is not followed by a minute of nothing.
const LONGEST_REST: Duration = Duration::from_secs(30);
/// The longest a hot device waits to cool before doing a little more.
const LONGEST_COOLING: Duration = Duration::from_secs(120);

static ENABLED: AtomicBool = AtomicBool::new(true);

/// What the guard is doing now, for a front end: 0 nothing, 1 slowing,
/// 2 pausing to cool.
static ACTING: AtomicU8 = AtomicU8::new(0);

/// Turn the guard on or off, from the setting. Takes effect at the next
/// unit of work, including in a run already going.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        ACTING.store(0, Ordering::Relaxed);
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// What the guard is doing now: "slowing", "cooling", or nothing.
pub fn acting() -> Option<&'static str> {
    match ACTING.load(Ordering::Relaxed) {
        1 => Some("slowing"),
        2 => Some("cooling"),
        _ => None,
    }
}

/// How hot the device is now, or None where there is no reading. Asked at
/// most twice a second; between, the last answer stands.
pub fn heat() -> Option<Heat> {
    static LAST: Mutex<Option<(Instant, Option<Heat>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at, heat)) = *last
        && at.elapsed() < Duration::from_millis(500)
    {
        return heat;
    }
    let heat = read();
    *last = Some((Instant::now(), heat));
    heat
}

fn read() -> Option<Heat> {
    if let Some(state) = vb_apple::power::thermal_state() {
        return Some(from_apple(state));
    }
    let file = std::env::var_os("VB_WORKDIR").map(|d| PathBuf::from(d).join(READING_FILE))?;
    from_android(&std::fs::read_to_string(file).ok()?)
}

/// `NSProcessInfoThermalState`: nominal, fair, serious, critical. "Fair"
/// is Apple's "slightly elevated", like Android's "light", and not acted on.
pub fn from_apple(state: u8) -> Heat {
    match state {
        0 | 1 => Heat::Cool,
        2 => Heat::Warm,
        _ => Heat::Hot,
    }
}

/// The Android host's reading. Status is `PowerManager.THERMAL_STATUS_*`
/// (2 moderate, 3 severe); headroom is `getThermalHeadroom`, NaN or absent
/// where the device does not report it, and then the status alone decides.
pub fn from_android(text: &str) -> Option<Heat> {
    let finite = |v: &str| v.trim().parse::<f64>().ok().filter(|h| h.is_finite());
    let mut status: Option<u8> = None;
    let mut headroom: Option<f64> = None;
    let (mut warm_at, mut hot_at) = (WARM_HEADROOM, HOT_HEADROOM);
    for line in text.lines() {
        match line.split_once('=') {
            Some(("status", v)) => status = v.trim().parse().ok(),
            Some(("headroom", v)) => headroom = finite(v),
            Some(("warm", v)) => warm_at = finite(v).unwrap_or(warm_at),
            Some(("hot", v)) => hot_at = finite(v).unwrap_or(hot_at),
            _ => {}
        }
    }
    if status.is_none() && headroom.is_none() {
        return None;
    }
    let status = status.unwrap_or(0);
    let headroom = headroom.unwrap_or(0.0);
    Some(if status >= 3 || headroom >= hot_at {
        Heat::Hot
    } else if status >= 2 || headroom >= warm_at {
        Heat::Warm
    } else {
        Heat::Cool
    })
}

/// May a run widen to several clips at once? Not while the guard is on
/// and the device is anything but cool.
pub fn may_widen() -> bool {
    !enabled() || heat().is_none_or(|h| h == Heat::Cool)
}

thread_local! {
    /// Which analysis worker this thread is: 0 for the first, and for any
    /// thread that is not one.
    static WORKER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Mark the calling thread as analysis worker `n`. Past the first, a worker
/// holds still while the device is warm, clip or no clip: measured on the
/// S21, three clips going on together took the skin from 39.6 to 42.6
/// degrees in 25 seconds, so leaving the ones already started to finish
/// meant a pause to cool after every burst.
pub fn set_worker(n: usize) {
    WORKER.with(|w| w.set(n));
}

/// Paces the threads of a run together, so they rest together rather than
/// one working while another rests.
pub struct Governor {
    state: Mutex<Pace>,
    /// How often a wait looks again at the device and at the user.
    step: Duration,
}

struct Pace {
    working_since: Instant,
    resting_until: Instant,
}

impl Governor {
    pub fn new(step: Duration) -> Governor {
        Governor::started(Instant::now(), step)
    }

    /// A governor whose current stretch of work began at `since`.
    pub fn started(since: Instant, step: Duration) -> Governor {
        Governor {
            state: Mutex::new(Pace {
                working_since: since,
                resting_until: since,
            }),
            step,
        }
    }

    /// One unit of work is done: go on at once while the device is cool,
    /// rest while it is warm, wait while it is hot -- asking `reread` how
    /// hot it is now -- and stop waiting as soon as `cancelled` says so.
    /// Returns how long it waited.
    pub fn pace(
        &self,
        heat: Option<Heat>,
        reread: &dyn Fn() -> Option<Heat>,
        cancelled: &dyn Fn() -> bool,
    ) -> Duration {
        let started = Instant::now();
        match heat {
            None | Some(Heat::Cool) => {
                ACTING.store(0, Ordering::Relaxed);
                let mut pace = self.state.lock().unwrap_or_else(|p| p.into_inner());
                // A cool device still finishes a rest another thread began.
                let until = pace.resting_until;
                if started >= until {
                    pace.working_since = pace.working_since.max(until);
                    return Duration::ZERO;
                }
                drop(pace);
                self.sleep_until(until, cancelled);
            }
            Some(Heat::Warm) => {
                ACTING.store(1, Ordering::Relaxed);
                let until = {
                    let mut pace = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    if started < pace.resting_until {
                        pace.resting_until
                    } else {
                        let worked = started.saturating_duration_since(pace.working_since);
                        if worked < WORK_BEFORE_REST {
                            return Duration::ZERO;
                        }
                        let rest = worked.mul_f64(REST_SHARE).min(LONGEST_REST);
                        pace.resting_until = started + rest;
                        pace.working_since = pace.resting_until;
                        pace.resting_until
                    }
                };
                self.sleep_until(until, cancelled);
            }
            Some(Heat::Hot) => {
                ACTING.store(2, Ordering::Relaxed);
                while started.elapsed() < LONGEST_COOLING && !cancelled() && enabled() {
                    std::thread::sleep(self.step);
                    if reread() != Some(Heat::Hot) {
                        break;
                    }
                }
                let mut pace = self.state.lock().unwrap_or_else(|p| p.into_inner());
                pace.working_since = Instant::now();
                ACTING.store(1, Ordering::Relaxed);
            }
        }
        started.elapsed()
    }

    fn sleep_until(&self, until: Instant, cancelled: &dyn Fn() -> bool) {
        while !cancelled() && enabled() {
            let now = Instant::now();
            if now >= until {
                break;
            }
            std::thread::sleep((until - now).min(self.step));
        }
    }
}

/// Called between units of work of a run, with the device's own reading.
pub fn pace(cancelled: &dyn Fn() -> bool) {
    static GOVERNOR: std::sync::OnceLock<Governor> = std::sync::OnceLock::new();
    if !enabled() {
        return;
    }
    // An extra worker waits out any warmth; the first carries on, paced.
    if WORKER.with(|w| w.get()) > 0 {
        while !may_widen() && !cancelled() {
            ACTING.store(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    GOVERNOR
        .get_or_init(|| Governor::new(Duration::from_millis(250)))
        .pace(heat(), &heat, cancelled);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_readings_slow_at_moderate_and_pause_at_severe() {
        // The S21 at rest (32 degrees) and just after work (37, "light").
        assert_eq!(from_android("status=0\nheadroom=0.70\n"), Some(Heat::Cool));
        assert_eq!(from_android("status=1\nheadroom=0.85\n"), Some(Heat::Cool));
        // Moderate, 39 degrees: slower. Severe, 41: stop until cooler.
        assert_eq!(from_android("status=1\nheadroom=0.94\n"), Some(Heat::Warm));
        assert_eq!(from_android("status=1\nheadroom=1.01\n"), Some(Heat::Hot));
        // The device's own thresholds, where it reports them.
        let own = "warm=0.90\nhot=0.97\n";
        assert_eq!(
            from_android(&format!("status=1\nheadroom=0.91\n{own}")),
            Some(Heat::Warm)
        );
        assert_eq!(
            from_android(&format!("status=1\nheadroom=0.98\n{own}")),
            Some(Heat::Hot)
        );
        // The status decides where headroom is not reported.
        assert_eq!(from_android("status=1\nheadroom=NaN\n"), Some(Heat::Cool));
        assert_eq!(from_android("status=2\n"), Some(Heat::Warm));
        assert_eq!(from_android("status=4\nheadroom=0.2\n"), Some(Heat::Hot));
        // Nothing known is no reading, not "cool".
        assert_eq!(from_android(""), None);
        assert_eq!(from_android("headroom=nan\n"), None);
    }

    #[test]
    fn apple_states_map_to_heat() {
        assert_eq!(from_apple(0), Heat::Cool);
        assert_eq!(from_apple(1), Heat::Cool);
        assert_eq!(from_apple(2), Heat::Warm);
        assert_eq!(from_apple(3), Heat::Hot);
    }

    const STEP: Duration = Duration::from_millis(20);

    #[test]
    fn a_cool_device_and_a_short_stretch_of_warm_work_go_on_at_once() {
        let g = Governor::new(STEP);
        assert_eq!(
            g.pace(Some(Heat::Cool), &|| None, &|| false),
            Duration::ZERO
        );
        assert_eq!(g.pace(None, &|| None, &|| false), Duration::ZERO);
        // Warm, but not two seconds of work since the last rest yet.
        assert_eq!(
            g.pace(Some(Heat::Warm), &|| None, &|| false),
            Duration::ZERO
        );
    }

    #[test]
    fn a_warm_device_rests_half_as_long_as_it_worked() {
        let g = Governor::started(Instant::now() - Duration::from_millis(2400), STEP);
        let waited = g.pace(Some(Heat::Warm), &|| Some(Heat::Warm), &|| false);
        assert!(
            waited >= Duration::from_millis(1150) && waited < Duration::from_millis(1500),
            "{waited:?}"
        );
        // The rest was taken: the next unit goes on.
        assert!(g.pace(Some(Heat::Warm), &|| None, &|| false) < STEP);
    }

    #[test]
    fn a_hot_device_waits_until_it_has_cooled() {
        let g = Governor::new(STEP);
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let reread = || {
            let n = reads.fetch_add(1, Ordering::Relaxed);
            Some(if n < 5 { Heat::Hot } else { Heat::Warm })
        };
        let waited = g.pace(Some(Heat::Hot), &reread, &|| false);
        assert_eq!(reads.load(Ordering::Relaxed), 6);
        assert!(waited >= STEP * 6, "{waited:?}");
    }

    #[test]
    fn a_wait_ends_when_the_run_is_stopped() {
        let g = Governor::new(STEP);
        let waited = g.pace(Some(Heat::Hot), &|| Some(Heat::Hot), &|| true);
        assert!(waited < STEP, "{waited:?}");
    }
}
