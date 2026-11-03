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

//! Dance: reading how the people move, where a dance cut may join.
//!
//! The director reads one frame of each segment, which says who is there but
//! not whether they are dancing, and not how their movement runs. So once a
//! first cut shows which moments a dance cut is choosing among, those are
//! read densely -- body poses four times a second through each one's usable
//! part -- and written into the graph as the segment's `poses`. The
//! orchestrator then keeps only the stretches where limbs are moving, holds
//! a solo with solos and a duet with duets, and finds the in and out points
//! where one clip's movement carries into the next (`vb_narrative::dance`).
//!
//! Poses are cached by the clip's content, the stretch, the model and how
//! they were sampled, so a second run of the same footage reads nothing.

use crate::Cache;
use std::path::Path;
use vb_contracts::footage_graph::PoseSample;
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros};
use vb_infer::{PoseEstimator, RgbImage};
use vb_media::MediaBackend;

/// Part of every cached reading's key: change it whenever the same stretch
/// could be read differently.
pub const POSE_VERSION: &str = "pose-1;every-250ms;long-640;cap-20s";

/// A pose every quarter second: fine enough to follow a gesture across a
/// cut on the beat, coarse enough to read a hundred stretches in minutes.
pub const EVERY: Micros = Micros(250_000);
/// The frame's long side as the pose model reads it.
const LONG_SIDE: u32 = 640;
/// The most of one stretch a dance cut reads; a longer one is read around
/// its middle, which is where a cut takes it from.
pub const LONGEST: Micros = Micros(20_000_000);

/// A 主体衔接 cut plays a take's whole action, so reads up to this much of
/// it -- a song's length.
pub const CONTINUITY_LONGEST: Micros = Micros(180_000_000);
/// And reads at most this much footage in all, best stretches first: the
/// poses of a few hundred stretches would take longer than the analysis.
pub const CONTINUITY_BUDGET: Micros = Micros(600_000_000);
/// Ten poses a second: the beat a dance keeps is read from its poses, and
/// at four a second a beat at 120 BPM is two samples.
pub const CONTINUITY_EVERY: Micros = Micros(100_000);

/// How many stretches to read, as a multiple of the first cut's clips, and
/// the bounds either side.
pub const CANDIDATE_FACTOR: usize = 3;
pub const CANDIDATE_MIN: usize = 30;
pub const CANDIDATE_MAX: usize = 150;

/// Is this a dance template?
pub fn is_dance(template: &vb_narrative::Template) -> bool {
    template
        .params
        .extra
        .get("dance")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The stretches a dance cut may use, as (shot, segment) indices: every one
/// the first cut used, then every other one with somebody in it, best and
/// most moving first, up to `size`.
pub fn candidates(graph: &FootageGraph, cut: &EditingDsl, size: usize) -> Vec<(usize, usize)> {
    // A photograph has no movement to read.
    let still = |shot: &vb_contracts::Shot| {
        graph
            .source(&shot.source_id)
            .is_some_and(vb_contracts::Source::is_still)
    };
    let mut out: Vec<(usize, usize)> = Vec::new();
    for slot in &cut.slots {
        let Some(r) = &slot.source_ref else { continue };
        let Some(shot) = graph
            .shots
            .iter()
            .position(|s| Some(&s.id) == r.shot_id.as_ref())
        else {
            continue;
        };
        if still(&graph.shots[shot]) {
            continue;
        }
        if let Some(seg) = graph.shots[shot].segments.iter().position(|g| {
            g.usable()
                .is_some_and(|(from, to)| r.in_us >= from && r.in_us < to)
        }) && !out.contains(&(shot, seg))
        {
            out.push((shot, seg));
        }
    }
    let rest = ranked_with_people(graph, &out);
    for (at, _) in rest {
        if out.len() >= size {
            break;
        }
        out.push(at);
    }
    out
}

/// The stretches a 主体衔接 cut may follow, as (shot, segment) indices:
/// every video segment someone is in, best and most moving first, until
/// `budget` of footage -- each counted up to [`CONTINUITY_LONGEST`].
pub fn with_people(graph: &FootageGraph, budget: Micros) -> Vec<(usize, usize)> {
    let mut ranked = ranked_with_people(graph, &[]);
    let mut out = Vec::new();
    let mut used = Micros::ZERO;
    for (at, _) in ranked.drain(..) {
        let (i, j) = at;
        let Some((from, to)) = graph.shots[i].segments[j].usable() else {
            continue;
        };
        let len = to.saturating_sub(from).min(CONTINUITY_LONGEST);
        if used + len > budget && !out.is_empty() {
            continue;
        }
        used = used + len;
        out.push(at);
    }
    out
}

/// Every video segment someone is in, with how much it promises, best
/// first, leaving out those in `skip`.
fn ranked_with_people(graph: &FootageGraph, skip: &[(usize, usize)]) -> Vec<((usize, usize), f64)> {
    let mut rest: Vec<((usize, usize), f64)> = Vec::new();
    for (i, shot) in graph.shots.iter().enumerate() {
        // A photograph has no movement to read.
        if graph
            .source(&shot.source_id)
            .is_some_and(vb_contracts::Source::is_still)
        {
            continue;
        }
        for (j, seg) in shot.segments.iter().enumerate() {
            let Some((from, to)) = seg.usable() else {
                continue;
            };
            if seg.people.unwrap_or(0) == 0 || skip.contains(&(i, j)) {
                continue;
            }
            let moving: Vec<f64> = shot
                .timeline
                .iter()
                .filter(|s| s.at_us >= from && s.at_us <= to)
                .filter_map(|s| s.motion)
                .collect();
            let motion = if moving.is_empty() {
                0.0
            } else {
                moving.iter().sum::<f64>() / moving.len() as f64
            };
            rest.push((
                (i, j),
                seg.quality.unwrap_or(0.5) * (0.5 + motion.clamp(0.0, 1.0)),
            ));
        }
    }
    rest.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    rest
}

/// What a pass of the pose reader did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Read {
    pub asked: usize,
    pub remembered: usize,
    pub failed: usize,
    pub samples: usize,
}

/// The upright frame size the pose model reads a source at.
fn frame_size(source: &vb_contracts::Source) -> (u32, u32) {
    let (w, h) = source.display_size().unwrap_or((16, 9));
    let scale = LONG_SIDE as f64 / w.max(h).max(1) as f64;
    let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
    (even(w as f64 * scale), even(h as f64 * scale))
}

/// A stretch's poses as a worker found them: which segment, the poses, and
/// whether they came from the cache.
type Found = ((usize, usize), Vec<PoseSample>, bool);

/// One stretch to read: where it is, and where its reading is cached.
struct Stretch {
    at: (usize, usize),
    path: std::path::PathBuf,
    from: Micros,
    to: Micros,
    size: (u32, u32),
    every: Micros,
    key: Blake3Hash,
}

/// Read the candidates' poses into the graph.
///
/// Several stretches at once: decoding is most of the time and runs in
/// parallel, while the pose model takes one frame at a time.
#[allow(clippy::too_many_arguments)]
pub fn read(
    graph: &mut FootageGraph,
    list: &[(usize, usize)],
    longest: Micros,
    every: Micros,
    pose: &dyn PoseEstimator,
    media: &dyn MediaBackend,
    cache: &Cache,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Read {
    // The dance cut's readings keep the key they were cached under.
    let version = if every == EVERY {
        POSE_VERSION.to_string()
    } else {
        format!("pose-1;every-{}ms;long-{LONG_SIDE}", every.as_millis())
    };
    let mut stretches = Vec::new();
    let mut failed = 0usize;
    for &(i, j) in list {
        let shot = &graph.shots[i];
        let seg = &shot.segments[j];
        let Some((from, to)) = seg.usable() else {
            continue;
        };
        let Some(source) = graph.sources.iter().find(|s| s.id == shot.source_id) else {
            failed += 1;
            continue;
        };
        let (from, to) = if to.saturating_sub(from) > longest {
            let middle = Micros((from.0 + to.0) / 2);
            (
                Micros(middle.0 - longest.0 / 2),
                Micros(middle.0 + longest.0 / 2),
            )
        } else {
            (from, to)
        };
        stretches.push(Stretch {
            at: (i, j),
            path: std::path::PathBuf::from(&source.path),
            from,
            to,
            size: frame_size(source),
            every,
            key: Blake3Hash::of_parts(&[
                b"pose",
                version.as_bytes(),
                pose.name().as_bytes(),
                source.content_hash.to_hex().as_bytes(),
                from.0.to_string().as_bytes(),
                to.0.to_string().as_bytes(),
            ]),
        });
    }

    let total = stretches.len();
    let queue = std::sync::Mutex::new(stretches.into_iter());
    let found: std::sync::Mutex<Vec<Found>> = std::sync::Mutex::new(Vec::new());
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::atomic::AtomicUsize::new(failed);
    progress(0, total);
    std::thread::scope(|scope| {
        for _ in 0..crate::analysis_workers() {
            scope.spawn(|| {
                vb_apple::power::user_initiated_thread();
                loop {
                    if cancelled() {
                        break;
                    }
                    let Some(stretch) = queue.lock().ok().and_then(|mut q| q.next()) else {
                        break;
                    };
                    match read_one(&stretch, pose, media, cache, cancelled) {
                        Some((samples, remembered)) => {
                            if let Ok(mut found) = found.lock() {
                                found.push((stretch.at, samples, remembered));
                            }
                        }
                        None => {
                            failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    let n = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    progress(n, total);
                }
            });
        }
    });

    let mut done = Read {
        failed: failures.into_inner(),
        ..Read::default()
    };
    for ((i, j), samples, remembered) in found.into_inner().unwrap_or_default() {
        if remembered {
            done.remembered += 1;
        } else {
            done.asked += 1;
        }
        done.samples += samples.len();
        let people = vb_narrative::dance::people(&samples);
        let seg = &mut graph.shots[i].segments[j];
        seg.people = people.or(seg.people);
        seg.poses = samples;
    }
    done
}

/// The beat each read stretch's people move to, from its poses, written to
/// the segment: their tempo, every beat, and where they stand idle or keep
/// no beat. Only stretches read densely enough to hear a beat in.
pub fn rhythms(graph: &mut FootageGraph, list: &[(usize, usize)]) -> usize {
    let mut found = 0usize;
    for &(i, j) in list {
        let aspect = graph
            .source(&graph.shots[i].source_id)
            .and_then(vb_contracts::Source::display_size)
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| w as f64 / h as f64)
            .unwrap_or(16.0 / 9.0);
        let segment = &mut graph.shots[i].segments[j];
        segment.rhythm = vb_perception::rhythm::read(&segment.poses, aspect);
        if segment.rhythm.is_some() {
            found += 1;
        }
    }
    found
}

/// One stretch's poses, from the cache or read afresh, and whether they
/// came from the cache. `None` when it could not be read.
fn read_one(
    stretch: &Stretch,
    pose: &dyn PoseEstimator,
    media: &dyn MediaBackend,
    cache: &Cache,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<PoseSample>, bool)> {
    if let Some(samples) = cache
        .get(&stretch.key)
        .and_then(|text| serde_json::from_str::<Vec<PoseSample>>(&text).ok())
    {
        return Some((samples, true));
    }
    let (w, h) = stretch.size;
    let mut samples = Vec::new();
    let mut failed = false;
    let outcome = media.sample_range(
        Path::new(&stretch.path),
        stretch.from,
        stretch.to,
        stretch.every,
        w,
        h,
        &mut |frame| {
            if cancelled() {
                return false;
            }
            let image = RgbImage {
                width: frame.width,
                height: frame.height,
                rgb: frame.rgb,
            };
            match pose.poses(&image) {
                Ok(found) => samples.push(vb_perception::pose::sample(
                    frame.at,
                    &found,
                    image.width,
                    image.height,
                )),
                Err(e) => {
                    tracing::warn!(error = %e, "pose estimation failed");
                    failed = true;
                    return false;
                }
            }
            true
        },
    );
    if let Err(e) = outcome {
        tracing::warn!(path = %stretch.path.display(), error = %e, "could not read the stretch");
        failed = true;
    }
    if failed || samples.is_empty() || cancelled() {
        return None;
    }
    if let Ok(text) = serde_json::to_string(&samples) {
        let _ = cache.put(&stretch.key, &text);
    }
    Some((samples, false))
}
