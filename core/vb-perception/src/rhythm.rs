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

//! The beat people move to, read from their poses.
//!
//! Every limb's place relative to the body -- elbows, wrists, knees and
//! ankles, the body's own travel and where it stands taken out, in torso
//! lengths -- is followed, and a dance moves all of them at its beat. Their
//! velocities' power, summed over limbs, peaks at the tempo: the strongest
//! peak within one octave, 80 to 160 BPM, where every tempo has a reading
//! and a beat cannot be confused with its half or double. A movement with
//! no peak in that octave keeps no beat, nor one whose two halves peak at
//! different tempi: a peak chance put there is not kept through a take. Where the beats fall is where the
//! limbs come to a stop -- a hand at the end of its stroke, a foot landing
//! -- and set off again: the beats fall where the speed rises sharply, at
//! the tempo, each moved onto the sharpest rise nearest it.
//!
//! The same reading says where the people are not dancing. *Idle*: any one
//! of them standing still for more than one and a half beats -- waiting,
//! a pose held past the music, a dancer standing while the other dances.
//! *Watching*: someone keeping a hand held still in front of their chest
//! for most of four bars, as a person does with a phone that shows the
//! steps -- the couple on the trip learning the dance from one: she held
//! it, he tried the moves, and both moved enough that neither stood idle
//! long. On the trip's performances no four bars had a hand held there
//! for more than 0.47 of them; in the learning take it was most of 94% of
//! the take. Whether movement keeps a beat is recorded, not relied on: on
//! the trip's dances a window of real dancing and one of the learning came
//! back alike, one real take kept no beat the reading could find, and limbs
//! wandering at random can read as clearly as a dance. So a whole take
//! with no beat is called unrhythmic, for the record; what a cut leaves out
//! as not dancing is what is idle or watching.
//!
//! Set on the trip's 126 clips, the 13 that hold its dance: every take of
//! the dance read 86 to 102 BPM -- danced without its music, each at its
//! own pace -- and the take of the couple learning the steps read 80.
//!
//! Classical measurement, like the sweep: no model beyond the pose model
//! that wrote the samples.

use vb_contracts::Micros;
use vb_contracts::footage_graph::{PersonPose, PoseSample, Rhythm, Span};

/// Below this a keypoint is the model guessing.
const SURE: f64 = 0.3;
/// The keypoints a dance moves to its beat: elbows, wrists, knees, ankles.
const LIMBS: [usize; 8] = [7, 8, 9, 10, 13, 14, 15, 16];
/// The octave every tempo is read in.
const SLOWEST_BPM: f64 = 80.0;
const FASTEST_BPM: f64 = 160.0;
/// A tempo stands out when its power is at least this many times the
/// octave's middle power: noise summed over the limbs peaks at under two.
const STANDS_OUT: f64 = 2.5;
/// How clearly each half of a take must move at the whole take's tempo.
const HALF_STANDS_OUT: f64 = 1.5;
/// Below this, in torso lengths a second over 0.4 s, nobody is moving: on
/// the trip's dances it took 0 to 6% of a take -- the stand before the
/// first move, a pause -- where 0.4 began to take dancing.
pub const STILL: f64 = 0.3;
/// Standing still this many beats is a stop, not a held pose.
pub const LONGEST_HOLD: f64 = 1.5;
/// The wrists, among [`LIMBS`].
const WRISTS: [usize; 2] = [2, 3];
/// A hand moving less than this, torso lengths a second over 0.4 s, is
/// held still.
const HELD: f64 = 0.5;
/// In front of the chest: within this much of the neck sideways, from this
/// far above it to this far below, in torso lengths -- where a phone is
/// held to be watched.
const HELD_SIDE: f64 = 0.5;
const HELD_ABOVE: f64 = 0.15;
const HELD_BELOW: f64 = 0.6;
/// Four bars: what a watcher is judged over.
const WATCH_BEATS: f64 = 16.0;
/// A hand held up for more than this share of four bars is watching what
/// it holds. On the trip's performances no four bars came above 0.47.
const WATCH_SHARE: f64 = 0.6;
/// The fewest samples a reading can be made from.
const FEWEST: usize = 24;

/// How much the people move through `samples`, at each sample, torso
/// lengths a second: the measure idleness is judged by. For probes.
pub fn movement_of(samples: &[PoseSample], aspect: f64) -> Vec<(f64, f64)> {
    tracks(samples, aspect)
        .map(|t| {
            movement(&t)
                .into_iter()
                .map(|m| ((m.from + m.to) / 2.0, m.all))
                .collect()
        })
        .unwrap_or_default()
}

/// What the poses say about the beat, or `None` when there is too little
/// of anyone to read one from. `aspect` is the frame's width over its
/// height, so a step sideways and a step down count alike.
pub fn read(samples: &[PoseSample], aspect: f64) -> Option<Rhythm> {
    let tracks = tracks(samples, aspect)?;
    let channels = velocities(&tracks);
    if channels.is_empty() {
        return None;
    }
    let (mut bpm, clarity) = tempo(&channels);
    // A beat the dance keeps is the same in its first half and its second:
    // a peak that chance put in the octave is not.
    let lasts = tracks.times[tracks.times.len() - 1] - tracks.times[0];
    // Only a take long enough that each half still tells tempi a few beats
    // a minute apart.
    if let Some(whole) = bpm.filter(|_| lasts >= 16.0) {
        let middle = (tracks.times[0] + tracks.times[tracks.times.len() - 1]) / 2.0;
        let half = |first: bool| -> Vec<Vec<(f64, f64)>> {
            channels
                .iter()
                .map(|c| {
                    c.iter()
                        .copied()
                        .filter(|(t, _)| (*t < middle) == first)
                        .collect::<Vec<_>>()
                })
                .filter(|c| c.len() >= FEWEST / 2)
                .collect()
        };
        // Each half need not peak there -- another phrase of the dance may
        // be stronger in it -- but must move clearly at the take's tempo.
        let agrees = |c: &[Vec<(f64, f64)>]| {
            if c.is_empty() {
                return true;
            }
            let mut octave: Vec<f64> = (0..)
                .map(|i| SLOWEST_BPM + 0.5 * i as f64)
                .take_while(|b| *b < FASTEST_BPM)
                .map(|b| power(c, b / 60.0))
                .collect();
            octave.sort_by(f64::total_cmp);
            let median = octave[octave.len() / 2].max(1e-12);
            let mut near = 0.0f64;
            let mut b = whole * 0.95;
            while b <= whole * 1.05 {
                near = near.max(power(c, b / 60.0));
                b += 0.5;
            }
            near >= HALF_STANDS_OUT * median
        };
        if !(agrees(&half(true)) && agrees(&half(false))) {
            bpm = None;
        }
    }
    let beat = 60.0 / bpm.unwrap_or(100.0);
    let idle = idle(&movement(&tracks), beat);
    let watching = watching(&tracks, beat);
    let (start, end) = (tracks.times[0], tracks.times[tracks.times.len() - 1]);
    let micros = |t: f64| Micros((t.max(0.0) * 1e6).round() as u64);
    let Some(bpm) = bpm else {
        // No beat to the whole take: none of it is danced to one.
        let whole = Span {
            start_us: micros(start),
            end_us: micros(end),
        };
        let stopped: Vec<Span> = idle.iter().chain(&watching).copied().collect();
        return Some(Rhythm {
            bpm: 0.0f64.max(60.0 / beat),
            confidence: Some(0.0),
            beats_us: Vec::new(),
            unrhythmic: outside(&[whole], &stopped),
            idle,
            watching,
        });
    };
    let beats = beats(&tracks, bpm / 60.0);
    Some(Rhythm {
        bpm: (bpm * 10.0).round() / 10.0,
        confidence: Some((clarity * 1000.0).round() / 1000.0),
        beats_us: beats.into_iter().map(micros).collect(),
        idle,
        watching,
        unrhythmic: Vec::new(),
    })
}

/// A place, x first, in the frame's heights or in torso lengths.
type Point = (f64, f64);
/// Limbs relative to the hips, in torso lengths; `None` where unread.
type Limbs = Vec<Option<Point>>;

/// Every limb of every person, followed through the samples: their places
/// relative to the hips in torso lengths, lightly smoothed, at the samples
/// where the same number of people can all be read.
struct Tracks {
    times: Vec<f64>,
    /// Per sample: each person's limbs, smoothed, and as read -- the
    /// smoothing blurs a stop into the stroke before it, so where the beats
    /// fall is timed on what was read.
    limbs: Vec<Limbs>,
    raw: Vec<Limbs>,
    travel: Vec<Vec<Point>>,
    torso: Vec<f64>,
    /// How many people each sample holds: person `p`'s limbs are
    /// `p * LIMBS.len()..(p + 1) * LIMBS.len()`.
    people: usize,
    /// Per sample: each person's neck relative to their hips, as read.
    neck: Vec<Vec<Point>>,
}

/// One person, as [`body`] reads them.
struct Body {
    /// Limbs relative to the hips, in torso lengths.
    limbs: Limbs,
    /// The neck relative to the hips, in torso lengths.
    neck: Point,
    /// The hips, in the frame's heights.
    hip: Point,
    torso: f64,
}

fn middle(points: &[Option<Point>; 17], a: usize, b: usize) -> Option<Point> {
    match (points[a], points[b]) {
        (Some(p), Some(q)) => Some(((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)),
        (Some(p), None) | (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

/// One person's limbs and neck relative to their hips, in torso lengths,
/// and where their hips are and how long their torso.
fn body(person: &PersonPose, aspect: f64) -> Option<Body> {
    let mut points = [None; 17];
    for (slot, k) in points.iter_mut().zip(&person.keypoints) {
        if k[2] >= SURE {
            *slot = Some((k[0] * aspect, k[1]));
        }
    }
    let neck = middle(&points, 5, 6)?;
    let hip = middle(&points, 11, 12)?;
    let tall = (person.bbox[3] - person.bbox[1]).max(0.0);
    let torso = ((neck.0 - hip.0).powi(2) + (neck.1 - hip.1).powi(2))
        .sqrt()
        .max(tall / 8.0)
        .max(1e-3);
    let limbs = LIMBS
        .iter()
        .map(|&k| points[k].map(|p| ((p.0 - hip.0) / torso, (p.1 - hip.1) / torso)))
        .collect();
    Some(Body {
        limbs,
        neck: ((neck.0 - hip.0) / torso, (neck.1 - hip.1) / torso),
        hip,
        torso,
    })
}

fn tracks(samples: &[PoseSample], aspect: f64) -> Option<Tracks> {
    // The number of people the stretch is of: the most common.
    let mut counts = std::collections::BTreeMap::<usize, usize>::new();
    for s in samples {
        *counts.entry(s.subjects().len()).or_insert(0) += 1;
    }
    let people = counts
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))?
        .0;
    let mut out = Tracks {
        times: Vec::new(),
        limbs: Vec::new(),
        raw: Vec::new(),
        travel: Vec::new(),
        torso: Vec::new(),
        people,
        neck: Vec::new(),
    };
    for s in samples {
        let subjects = s.subjects();
        if subjects.len() != people {
            continue;
        }
        let read: Option<Vec<_>> = subjects.iter().map(|p| body(p, aspect)).collect();
        let Some(read) = read else { continue };
        out.times.push(s.at_us.as_secs_f64());
        out.limbs
            .push(read.iter().flat_map(|b| b.limbs.clone()).collect());
        out.travel.push(
            read.iter()
                .map(|b| (b.hip.0 / b.torso, b.hip.1 / b.torso))
                .collect(),
        );
        out.torso
            .push(read.iter().map(|b| b.torso).sum::<f64>() / read.len() as f64);
        out.neck.push(read.iter().map(|b| b.neck).collect());
    }
    if out.times.len() < FEWEST {
        return None;
    }
    // Smoothed over three samples: a keypoint jitters by a pixel or two,
    // which differencing would make into motion.
    let smooth = |rows: &Vec<Limbs>| -> Vec<Limbs> {
        (0..rows.len())
            .map(|i| {
                (0..rows[i].len())
                    .map(|c| {
                        let near: Vec<Point> = [i.wrapping_sub(1), i, i + 1]
                            .into_iter()
                            .filter(|&j| j < rows.len())
                            .filter_map(|j| rows[j].get(c).copied().flatten())
                            .collect();
                        rows[i][c].map(|_| {
                            let n = near.len() as f64;
                            (
                                near.iter().map(|p| p.0).sum::<f64>() / n,
                                near.iter().map(|p| p.1).sum::<f64>() / n,
                            )
                        })
                    })
                    .collect()
            })
            .collect()
    };
    out.raw = out.limbs.clone();
    out.limbs = smooth(&out.limbs);
    Some(out)
}

/// Each limb coordinate's velocity, torso lengths a second, at the samples
/// with a neighbour either side close enough to difference across, mean
/// taken out: one channel each. Velocity, with its direction, rather than
/// speed: on the trip's dances the speeds' spectra scattered from take to
/// take where the velocities' agreed.
fn velocities(tracks: &Tracks) -> Vec<Vec<(f64, f64)>> {
    let width = tracks.limbs[0].len();
    let mut out = Vec::new();
    for c in 0..width {
        for axis in 0..2 {
            let mut channel = Vec::new();
            for i in 1..tracks.times.len().saturating_sub(1) {
                let dt = tracks.times[i + 1] - tracks.times[i - 1];
                if dt <= 0.0 || dt > 0.4 {
                    continue;
                }
                let (Some(a), Some(b)) = (tracks.limbs[i - 1][c], tracks.limbs[i + 1][c]) else {
                    continue;
                };
                let v = if axis == 0 { b.0 - a.0 } else { b.1 - a.1 } / dt;
                channel.push((tracks.times[i], v));
            }
            if channel.len() >= FEWEST {
                let mean = channel.iter().map(|x| x.1).sum::<f64>() / channel.len() as f64;
                out.push(channel.into_iter().map(|(t, v)| (t, v - mean)).collect());
            }
        }
    }
    out
}

/// The power at one frequency, summed over the channels, each tapered at
/// its ends (a Hann window): without it, a movement much slower than any
/// dance leaks across the octave in ripples that read as peaks.
fn power(channels: &[Vec<(f64, f64)>], frequency: f64) -> f64 {
    let mut total = 0.0;
    for channel in channels {
        let (start, end) = (channel[0].0, channel[channel.len() - 1].0);
        let span = (end - start).max(1e-6);
        let (mut re, mut im, mut weights) = (0.0, 0.0, 0.0);
        for (t, v) in channel {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (t - start) / span).cos();
            let angle = 2.0 * std::f64::consts::PI * frequency * t;
            re += w * v * angle.cos();
            im += w * v * angle.sin();
            weights += w * w;
        }
        total += (re * re + im * im) / weights.max(1e-9);
    }
    total
}

/// The tempo the limbs move at, and how clearly, 0..1 -- or `None` when no
/// tempo in the octave stands out: the strongest local peak of the summed
/// power between 80 and 160 BPM, a peak at least [`STANDS_OUT`] times the
/// octave's middle. Read a little either side of the octave, so a peak on
/// its edge is told from a slope running off it.
fn tempo(channels: &[Vec<(f64, f64)>]) -> (Option<f64>, f64) {
    let step = 0.5;
    // A peak is the highest within this many steps either side: two beats
    // a minute, about a tapered peak's own width on a take of half a
    // minute.
    let reach = 4usize;
    let bpms: Vec<f64> = (0..)
        .map(|i| SLOWEST_BPM - 6.0 + step * i as f64)
        .take_while(|b| *b <= FASTEST_BPM + 6.0)
        .collect();
    let powers: Vec<f64> = bpms.iter().map(|b| power(channels, b / 60.0)).collect();
    let inside: Vec<f64> = bpms
        .iter()
        .zip(&powers)
        .filter(|(b, _)| **b >= SLOWEST_BPM && **b < FASTEST_BPM)
        .map(|(_, p)| *p)
        .collect();
    let mut sorted = inside.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2].max(1e-12);
    let peak = (reach..bpms.len() - reach)
        .filter(|&i| bpms[i] >= SLOWEST_BPM && bpms[i] < FASTEST_BPM)
        .filter(|&i| (i - reach..=i + reach).all(|j| powers[i] >= powers[j]))
        .max_by(|&a, &b| powers[a].total_cmp(&powers[b]));
    let Some(i) = peak else {
        return (None, 0.0);
    };
    let clarity = 1.0 - median / powers[i];
    if powers[i] < STANDS_OUT * median {
        return (None, clarity);
    }
    // Refined to a hundredth of a beat a minute.
    let mut best = (bpms[i], powers[i]);
    let mut b = bpms[i] - step;
    while b <= bpms[i] + step {
        let p = power(channels, b / 60.0);
        if p > best.1 {
            best = (b, p);
        }
        b += 0.01;
    }
    (Some(best.0), clarity)
}

/// How fast the limbs move between samples, summed over limbs and people,
/// at the time between.
fn speed(tracks: &Tracks) -> Vec<(f64, f64)> {
    tracks
        .times
        .windows(2)
        .zip(tracks.raw.windows(2))
        .filter_map(|(t, rows)| {
            let dt = t[1] - t[0];
            if dt <= 0.0 || dt > 0.4 {
                return None;
            }
            let total: f64 = rows[0]
                .iter()
                .zip(&rows[1])
                .filter_map(|(a, b)| {
                    let (a, b) = ((*a)?, (*b)?);
                    Some(((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() / dt)
                })
                .sum();
            Some(((t[0] + t[1]) / 2.0, total))
        })
        .collect()
}

/// Every beat over the take: where, at `frequency`, the limbs set off on a
/// new stroke after coming to a stop -- the sharpest rise in their speed --
/// each moved onto the sharpest rise within a fifth of a beat of it.
fn beats(tracks: &Tracks, frequency: f64) -> Vec<f64> {
    let speed = speed(tracks);
    // How sharply the speed rises between two readings, at the time
    // between them: the moment a stroke starts.
    let rises: Vec<(f64, f64)> = speed
        .windows(2)
        .filter(|w| w[1].0 - w[0].0 <= 0.25)
        .map(|w| ((w[0].0 + w[1].0) / 2.0, (w[1].1 - w[0].1).max(0.0)))
        .collect();
    if rises.len() < FEWEST {
        return Vec::new();
    }
    let mean = rises.iter().map(|s| s.1).sum::<f64>() / rises.len() as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (t, v) in &rises {
        let angle = 2.0 * std::f64::consts::PI * frequency * t;
        re += (v - mean) * angle.cos();
        im += (v - mean) * angle.sin();
    }
    // The rises' swing at the tempo is highest where 2 pi f t = atan2(im,
    // re): there the beats fall.
    let period = 1.0 / frequency;
    let phase = im.atan2(re) / (2.0 * std::f64::consts::PI) * period;
    let (start, end) = (rises[0].0, rises[rises.len() - 1].0);
    let mut sorted: Vec<f64> = rises.iter().map(|r| r.1).collect();
    sorted.sort_by(f64::total_cmp);
    let typical = sorted[sorted.len() / 2];
    let mut t = start + (phase - start).rem_euclid(period);
    let mut out: Vec<f64> = Vec::new();
    while t <= end + 1e-9 {
        let at = rises
            .iter()
            .filter(|(at, _)| (at - t).abs() <= 0.2 * period)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, v)| *v > typical)
            .map(|(at, _)| *at)
            .unwrap_or(t);
        if out.last().is_none_or(|last| at > *last + 0.5 * period) {
            out.push(at);
        }
        t += period;
    }
    out
}

/// How much the people move around one sample, torso lengths a second,
/// over the time it stands for.
struct Moving {
    from: f64,
    to: f64,
    /// All of them: the four limbs that move most and the most any body
    /// travels.
    all: f64,
    /// Each of them: their own two limbs that move most and their own
    /// travel; `None` where none of their limbs was read.
    each: Vec<Option<f64>>,
}

/// How much the people move around each sample, measured over two samples
/// either side -- a stillness lasting a beat and a half is long enough to
/// measure across, and a pixel of the pose model's jitter is then a quarter
/// of what it would be from one sample to the next.
fn movement(tracks: &Tracks) -> Vec<Moving> {
    let n = tracks.times.len();
    let width = LIMBS.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (i.saturating_sub(2), (i + 2).min(n - 1));
        let dt = tracks.times[b] - tracks.times[a];
        if dt <= 0.0 || dt > 0.8 {
            continue;
        }
        let speeds: Vec<Option<f64>> = tracks.limbs[a]
            .iter()
            .zip(&tracks.limbs[b])
            .map(|(p, q)| {
                let (p, q) = ((*p)?, (*q)?);
                Some(((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt() / dt)
            })
            .collect();
        let travel: Vec<f64> = tracks.travel[a]
            .iter()
            .zip(&tracks.travel[b])
            .map(|(p, q)| ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt() / dt)
            .collect();
        // The mean of the `most` fastest, or `None` with none read.
        let fastest = |limbs: &[Option<f64>], most: usize| -> Option<f64> {
            let mut read: Vec<f64> = limbs.iter().flatten().copied().collect();
            read.sort_by(|x, y| y.total_cmp(x));
            read.truncate(most);
            (!read.is_empty()).then(|| read.iter().sum::<f64>() / read.len() as f64)
        };
        let all = fastest(&speeds, 4).unwrap_or(0.0) + travel.iter().copied().fold(0.0, f64::max);
        let each = (0..tracks.people)
            .map(|p| fastest(&speeds[p * width..(p + 1) * width], 2).map(|m| m + travel[p]))
            .collect();
        // Each sample stands for the time halfway to its neighbours.
        let from = if i == 0 {
            tracks.times[0]
        } else {
            (tracks.times[i - 1] + tracks.times[i]) / 2.0
        };
        let to = if i + 1 == n {
            tracks.times[i]
        } else {
            (tracks.times[i] + tracks.times[i + 1]) / 2.0
        };
        out.push(Moving {
            from,
            to,
            all,
            each,
        });
    }
    out
}

/// The stretches, `(from, to)`, where `flags` hold for more than `longer`
/// seconds; a gap of more than `bridge` seconds between two that hold ends
/// one.
fn runs(
    flags: impl Iterator<Item = (f64, f64, bool)>,
    bridge: f64,
    longer: f64,
) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut open: Option<(f64, f64)> = None;
    for (from, to, on) in flags {
        if on {
            open = match open {
                Some((a, b)) if from - b <= bridge => Some((a, to)),
                Some(done) => {
                    out.push(done);
                    Some((from, to))
                }
                None => Some((from, to)),
            };
        } else if let Some(done) = open.take() {
            out.push(done);
        }
    }
    out.extend(open);
    out.retain(|(a, b)| b - a > longer);
    out
}

/// Stretches as spans, overlapping ones made one.
fn spans(mut stretches: Vec<(f64, f64)>) -> Vec<Span> {
    stretches.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in stretches {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let micros = |t: f64| Micros((t.max(0.0) * 1e6).round() as u64);
    merged
        .into_iter()
        .map(|(a, b)| Span {
            start_us: micros(a),
            end_us: micros(b),
        })
        .collect()
}

/// Where anyone stands still for more than [`LONGEST_HOLD`] beats: the
/// people together, or any one of them alone -- a dancer waiting while the
/// other dances is not dancing either.
fn idle(movement: &[Moving], beat: f64) -> Vec<Span> {
    let longer = LONGEST_HOLD * beat;
    // A gap in the samples ends a stop.
    let mut stops = runs(
        movement.iter().map(|m| (m.from, m.to, m.all < STILL)),
        0.3,
        longer,
    );
    let people = movement.first().map_or(0, |m| m.each.len());
    for p in 0..people {
        stops.extend(runs(
            movement
                .iter()
                .map(|m| (m.from, m.to, m.each[p].is_some_and(|v| v < STILL))),
            0.3,
            longer,
        ));
    }
    spans(stops)
}

/// Where someone watches what they hold: a hand held still in front of
/// their chest ([`HELD`], [`HELD_SIDE`], [`HELD_ABOVE`], [`HELD_BELOW`]),
/// in holds of more than [`LONGEST_HOLD`] beats that fill more than
/// [`WATCH_SHARE`] of the four bars around -- those four bars' middles,
/// and every hold they touch. A dance holds a hand there a beat or two at
/// a time; a phone is held there for minutes.
fn watching(tracks: &Tracks, beat: f64) -> Vec<Span> {
    let n = tracks.times.len();
    let width = LIMBS.len();
    let window = WATCH_BEATS * beat;
    let mut out = Vec::new();
    for p in 0..tracks.people {
        // Where a wrist is, relative to the neck, at sample `j`.
        let wrist = |j: usize, w: usize| -> Option<Point> {
            let limb = tracks.raw[j][p * width + w]?;
            let neck = tracks.neck[j][p];
            Some((limb.0 - neck.0, limb.1 - neck.1))
        };
        let held = (0..n).map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 2).min(n - 1));
            let dt = tracks.times[b] - tracks.times[a];
            let on = dt > 0.0
                && dt <= 0.8
                && WRISTS.iter().any(|&w| {
                    let (Some(x), Some(y), Some(at)) = (wrist(a, w), wrist(b, w), wrist(i, w))
                    else {
                        return false;
                    };
                    let speed = ((y.0 - x.0).powi(2) + (y.1 - x.1).powi(2)).sqrt() / dt;
                    speed < HELD
                        && at.0.abs() <= HELD_SIDE
                        && at.1 >= -HELD_ABOVE
                        && at.1 <= HELD_BELOW
                });
            (tracks.times[i], tracks.times[i], on)
        });
        // A hand let go of for less than half a beat is still holding.
        let holds = runs(held, 0.5 * beat, LONGEST_HOLD * beat);
        if holds.is_empty() {
            continue;
        }
        let watched = runs(
            tracks.times.iter().map(|&t| {
                let (low, high) = (t - window / 2.0, t + window / 2.0);
                let covered: f64 = holds
                    .iter()
                    .map(|&(a, b)| (b.min(high) - a.max(low)).max(0.0))
                    .sum();
                (t, t, covered / window > WATCH_SHARE)
            }),
            0.3,
            0.0,
        );
        for &(a, b) in &holds {
            if watched.iter().any(|&(x, y)| a <= y && b >= x) {
                out.push((a, b));
            }
        }
        out.extend(watched);
    }
    spans(out)
}

/// `spans` less what `idle` covers -- what is idle or watching is said
/// once, as that -- keeping only pieces of a second or more.
fn outside(spans: &[Span], idle: &[Span]) -> Vec<Span> {
    let mut out = Vec::new();
    for span in spans {
        let mut pieces = vec![(span.start_us, span.end_us)];
        for gap in idle {
            pieces = pieces
                .into_iter()
                .flat_map(|(x, y)| {
                    if gap.end_us <= x || gap.start_us >= y {
                        vec![(x, y)]
                    } else {
                        [(x, gap.start_us), (gap.end_us, y)]
                            .into_iter()
                            .filter(|(p, q)| q > p)
                            .collect()
                    }
                })
                .collect();
        }
        for (x, y) in pieces {
            if y.saturating_sub(x) >= Micros::from_secs(1) {
                out.push(Span {
                    start_us: x,
                    end_us: y,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person dancing to `bpm`: on every beat the wrists set off upwards
    /// fast, and come back down slowly to land at rest on the next --
    /// standing still through `rest` (seconds).
    fn bouncing(bpm: f64, seconds: f64, rest: Option<(f64, f64)>, every: f64) -> Vec<PoseSample> {
        let period = 60.0 / bpm;
        let mut out = Vec::new();
        let mut t = 0.0;
        while t <= seconds {
            let resting = rest.is_some_and(|(a, b)| t >= a && t <= b);
            let phase = (t / period).fract();
            // Off the beat fast, up by a third of it, then down slowly to
            // land at rest on the next.
            let up = if phase < 0.3 {
                1.0 - (1.0 - phase / 0.3).powi(2)
            } else {
                (1.0 - (phase - 0.3) / 0.7).powi(2)
            };
            let raise = if resting { 0.3 } else { 0.15 + 0.3 * up };
            pose_with(&mut out, t, raise);
            t += every;
        }
        out
    }

    /// A pixel or two of the pose model's own jitter, the same every run.
    fn jitter(t: f64, k: usize) -> f64 {
        let seed = (t * 1000.0) as u64 * 131 + k as u64 * 7919;
        let mixed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((mixed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.004
    }

    fn pose_with(out: &mut Vec<PoseSample>, t: f64, raise: f64) {
        out.push(PoseSample {
            at_us: Micros((t * 1e6).round() as u64),
            people: vec![person(0.5, t, raise, false, 0)],
        });
    }

    /// The wrists' raise at `t`, dancing to `bpm` as [`bouncing`] does.
    fn dancing(t: f64, bpm: f64) -> f64 {
        let phase = (t / (60.0 / bpm)).fract();
        let up = if phase < 0.3 {
            1.0 - (1.0 - phase / 0.3).powi(2)
        } else {
            (1.0 - (phase - 0.3) / 0.7).powi(2)
        };
        0.15 + 0.3 * up
    }

    /// Two people, ten samples a second: on the left one dancing to 120
    /// BPM, on the right one whose raise and phone `right` gives.
    fn duet(seconds: f64, right: impl Fn(f64) -> (f64, bool)) -> Vec<PoseSample> {
        let mut out = Vec::new();
        let mut t = 0.0;
        while t <= seconds {
            let (raise, phone) = right(t);
            out.push(PoseSample {
                at_us: Micros((t * 1e6).round() as u64),
                people: vec![
                    person(0.3, t, dancing(t, 120.0), false, 0),
                    person(0.7, t, raise, phone, 100),
                ],
            });
            t += 0.1;
        }
        out
    }

    /// One person standing at `x`, wrists raised by `raise` -- or, with a
    /// `phone`, the right hand held still in front of the chest. `seed`
    /// keeps two people's jitter apart.
    fn person(x: f64, t: f64, raise: f64, phone: bool, seed: usize) -> PersonPose {
        let mut k = vec![[x, 0.2, 0.9]; 17];
        k[5] = [x - 0.05, 0.3, 0.9];
        k[6] = [x + 0.05, 0.3, 0.9];
        k[11] = [x - 0.03, 0.6, 0.9];
        k[12] = [x + 0.03, 0.6, 0.9];
        k[7] = [x - 0.08, 0.45 - raise / 2.0, 0.9];
        k[8] = [x + 0.08, 0.45 - raise / 2.0, 0.9];
        k[9] = [x - 0.1, 0.55 - raise, 0.9];
        k[10] = [x + 0.1, 0.55 - raise, 0.9];
        for j in [13, 14] {
            k[j] = [x, 0.75, 0.9];
        }
        for j in [15, 16] {
            k[j] = [x, 0.9, 0.9];
        }
        if phone {
            k[8] = [x + 0.07, 0.45, 0.9];
            k[10] = [x + 0.03, 0.4, 0.9];
        }
        for (j, point) in k.iter_mut().enumerate() {
            point[0] += jitter(t, 2 * j + seed);
            point[1] += jitter(t, 2 * j + 1 + seed);
        }
        PersonPose {
            bbox: [x - 0.12, 0.15, x + 0.12, 0.95],
            score: 0.95,
            keypoints: k,
        }
    }

    #[test]
    fn a_dance_to_a_steady_beat_is_read_at_its_tempo_with_its_beats_on_the_hits() {
        let samples = bouncing(112.0, 20.0, None, 0.1);
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        assert!((rhythm.bpm - 112.0).abs() < 1.5, "{}", rhythm.bpm);
        assert!(rhythm.confidence.unwrap() > 0.6, "{:?}", rhythm.confidence);
        // The rests are at every multiple of the period.
        let period = 60.0 / 112.0;
        for b in &rhythm.beats_us[1..rhythm.beats_us.len() - 1] {
            let t = b.as_secs_f64();
            let off = (t / period).round() * period - t;
            assert!(off.abs() < 0.08, "beat at {t} is {off} s off the rest");
        }
        assert!(rhythm.idle.is_empty(), "{:?}", rhythm.idle);
        assert!(rhythm.watching.is_empty(), "{:?}", rhythm.watching);
        assert!(rhythm.unrhythmic.is_empty(), "{:?}", rhythm.unrhythmic);
    }

    #[test]
    fn a_dancer_waiting_while_the_other_dances_is_idle() {
        // She stands for the first six seconds, then joins him.
        let samples = duet(20.0, |t| {
            (if t < 6.0 { 0.3 } else { dancing(t, 120.0) }, false)
        });
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        assert_eq!(rhythm.idle.len(), 1, "{:?}", rhythm.idle);
        let span = rhythm.idle[0];
        assert!(span.start_us.as_secs_f64() < 0.5, "{span:?}");
        let end = span.end_us.as_secs_f64();
        assert!(end > 5.5 && end < 6.5, "{span:?}");
        assert!(rhythm.watching.is_empty(), "{:?}", rhythm.watching);
    }

    #[test]
    fn holding_a_phone_up_while_the_other_tries_the_steps_is_watching() {
        // She holds it in front of her chest and waves the other arm about,
        // never still for long; he dances.
        let samples = duet(30.0, |t| (0.2 + 0.2 * (2.5 * t).sin(), true));
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        let watched: f64 = rhythm
            .watching
            .iter()
            .map(|s| (s.end_us.0 - s.start_us.0) as f64 / 1e6)
            .sum();
        assert!(
            watched > 24.0,
            "{watched} s watching: {:?}",
            rhythm.watching
        );
    }

    #[test]
    fn a_hand_held_to_the_chest_for_two_beats_at_a_time_is_part_of_the_dance() {
        // Every two bars the right hand goes to the chest for two beats.
        let mut samples = Vec::new();
        let mut t = 0.0f64;
        while t <= 30.0 {
            let beat = (t / 0.5) as u64;
            samples.push(PoseSample {
                at_us: Micros((t * 1e6).round() as u64),
                people: vec![person(0.5, t, dancing(t, 120.0), beat % 8 >= 6, 0)],
            });
            t += 0.1;
        }
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        assert!(rhythm.watching.is_empty(), "{:?}", rhythm.watching);
    }

    #[test]
    fn standing_still_for_more_than_a_beat_and_a_half_is_idle() {
        // Three seconds of nothing in the middle of a dance.
        let samples = bouncing(120.0, 20.0, Some((8.0, 11.0)), 0.1);
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        assert_eq!(rhythm.idle.len(), 1, "{:?}", rhythm.idle);
        let span = rhythm.idle[0];
        assert!(span.start_us.as_secs_f64() > 7.6 && span.start_us.as_secs_f64() < 8.4);
        assert!(span.end_us.as_secs_f64() > 10.6 && span.end_us.as_secs_f64() < 11.4);
    }

    #[test]
    fn a_held_pose_of_one_beat_is_part_of_the_dance() {
        // Half a second still at 120 BPM: one beat, a hold, not a stop.
        let samples = bouncing(120.0, 20.0, Some((8.0, 8.5)), 0.1);
        let rhythm = read(&samples, 16.0 / 9.0).expect("a rhythm");
        assert!(rhythm.idle.is_empty(), "{:?}", rhythm.idle);
    }

    #[test]
    fn standing_in_a_daze_is_idle_throughout() {
        // Nothing but the pose model's own jitter, for twenty seconds.
        let mut samples = Vec::new();
        let mut t = 0.0;
        while t <= 20.0 {
            pose_with(&mut samples, t, 0.3);
            t += 0.1;
        }
        let rhythm = read(&samples, 16.0 / 9.0).expect("a reading");
        let idle: f64 = rhythm
            .idle
            .iter()
            .map(|s| (s.end_us.0 - s.start_us.0) as f64 / 1e6)
            .sum();
        assert!(idle > 19.0, "only {idle} s idle: {rhythm:?}");
    }

    #[test]
    fn too_few_samples_say_nothing() {
        let samples = bouncing(120.0, 1.5, None, 0.1);
        assert!(read(&samples, 16.0 / 9.0).is_none());
    }
}
