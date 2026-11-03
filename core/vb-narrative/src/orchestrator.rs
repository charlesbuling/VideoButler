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

//! Subsystem B: the Reasoning Orchestrator.
//!
//! Runs the decision loop *Footage Graph + Priors + Template -> Editing DSL*.
//!
//! [`MontageOrchestrator`] is the rule-based baseline: it consults no model at
//! all, which is what lets the whole pipeline run offline with no weights. It
//! is also the control that a future model-driven orchestrator has to beat.
//! Both satisfy [`Orchestrator`], so swapping one for the other changes
//! nothing downstream.

use crate::NarrativeError;
use crate::continuity;
use crate::ground::{GroundingReport, ground};
use crate::prior::Template;
use vb_contracts::editing_dsl::{
    Anchor, AudioItem, AudioKind, DslProvenance, FootageGraphRef, Slot, SlotKind, SourceRef,
    TextItem, TextRole, TextStyle, Transition, TransitionType, VideoItem,
};
use vb_contracts::footage_graph::{BeatGrid, BeatOrigin, Segment, TimelineSample, view_distance};
use vb_contracts::{EditingDsl, FootageGraph, Micros, Shot};

/// The tempo a beat-synced cut follows when there is no music to listen to.
const DEFAULT_BPM: f64 = 120.0;

/// The beat a cut follows, and where it came from.
#[derive(Debug, Clone)]
struct Beat {
    length: Micros,
    grid: BeatGrid,
    /// Kept by the dancers themselves, read from their poses: with no music
    /// chosen, a dance is cut to its own beat.
    from_motion: bool,
}

impl Beat {
    /// The grid for this cut, or `None` when the template does not cut on the
    /// beat.
    ///
    /// Music the user chose, or a beat found in the footage, is followed as
    /// found. With neither, a steady grid at the template's tempo stands in,
    /// and is recorded as fixed: it is a pacing choice, not a measurement.
    fn for_cut(graph: &FootageGraph, template: &Template) -> Option<Beat> {
        if !template.params.beat_align {
            return None;
        }
        let found = graph
            .priors
            .as_ref()
            .and_then(|p| p.beat_grid.clone())
            .filter(|g| g.beat_us().is_some_and(|b| b > Micros::ZERO));
        let grid = found.unwrap_or_else(|| BeatGrid {
            bpm: template
                .params
                .extra
                .get("fixedBpm")
                .and_then(|v| v.as_f64())
                .filter(|b| *b > 0.0)
                .unwrap_or(DEFAULT_BPM),
            first_beat_us: None,
            confidence: None,
            origin: Some(BeatOrigin::Fixed),
            asset_path: None,
            asset_duration_us: None,
        });
        Some(Beat {
            length: grid.beat_us()?,
            grid,
            from_motion: false,
        })
    }

    /// The beat the dancers keep, from their poses' rhythm, for a cut with
    /// no music to follow: the middle tempo of the segments that keep one
    /// clearly, a reading at half or double brought into the same octave.
    fn of_dancing(graph: &FootageGraph) -> Option<Beat> {
        let mut tempi: Vec<f64> = graph
            .shots
            .iter()
            .flat_map(|s| s.segments.iter())
            .filter_map(|s| {
                let r = s.rhythm.as_ref()?;
                // A take mostly spent not dancing -- the steps learned from
                // a phone, waiting about -- does not set the beat.
                let read = match (s.poses.first(), s.poses.last()) {
                    (Some(a), Some(b)) => b.at_us.0.saturating_sub(a.at_us.0),
                    _ => 0,
                };
                (r.stopped().0 as f64 <= 0.75 * read as f64).then_some(r)
            })
            .filter(|r| r.confidence.unwrap_or(0.0) >= 0.2 && r.bpm > 0.0)
            .map(|r| r.bpm)
            .collect();
        if tempi.is_empty() {
            return None;
        }
        tempi.sort_by(f64::total_cmp);
        let reference = tempi[tempi.len() / 2];
        for t in tempi.iter_mut() {
            while *t > reference * std::f64::consts::SQRT_2 {
                *t /= 2.0;
            }
            while *t < reference / std::f64::consts::SQRT_2 {
                *t *= 2.0;
            }
        }
        tempi.sort_by(f64::total_cmp);
        let bpm = (tempi[tempi.len() / 2] * 10.0).round() / 10.0;
        let grid = BeatGrid {
            bpm,
            first_beat_us: None,
            confidence: None,
            origin: None,
            asset_path: None,
            asset_duration_us: None,
        };
        Some(Beat {
            length: grid.beat_us()?,
            grid,
            from_motion: true,
        })
    }

    /// Round a duration to a whole number of beats, never fewer than one.
    fn quantise(&self, d: Micros) -> (Micros, u64) {
        let beats = ((d.0 as f64 / self.length.0 as f64).round() as u64).max(1);
        (Micros(beats * self.length.0), beats)
    }

    fn describe(&self, chinese: bool) -> String {
        let bpm = self.grid.bpm;
        if self.from_motion {
            return if chinese {
                format!("舞者自己的 {bpm:.0} BPM 节拍（没有选配乐）")
            } else {
                format!("the dancers' own {bpm:.0} BPM, as no music was chosen")
            };
        }
        match (self.grid.origin, chinese) {
            (Some(BeatOrigin::Music), false) => format!("{bpm:.0} BPM from the music"),
            (Some(BeatOrigin::Source), false) => format!("{bpm:.0} BPM from the footage"),
            (_, false) => format!("a steady {bpm:.0} BPM grid, as no music was chosen"),
            (Some(BeatOrigin::Music), true) => format!("配乐的 {bpm:.0} BPM 节拍"),
            (Some(BeatOrigin::Source), true) => format!("素材自带的 {bpm:.0} BPM 节拍"),
            (_, true) => format!("固定的 {bpm:.0} BPM 节奏（没有选配乐）"),
        }
    }
}

/// Composes an edit from footage and a template.
pub trait Orchestrator {
    fn name(&self) -> &'static str;

    fn compose(
        &self,
        graph: &FootageGraph,
        template: &Template,
    ) -> Result<EditingDsl, NarrativeError>;
}

/// A composed edit, together with what the grounding gate made of it.
#[derive(Debug)]
pub struct Composition {
    pub dsl: EditingDsl,
    pub grounding: GroundingReport,
}

/// Compose and then verify, which is the only supported way to produce a DSL.
///
/// Exposing this rather than [`Orchestrator::compose`] alone keeps the gate
/// from being skipped by accident: an ungrounded DSL should never reach the
/// renderer.
pub fn compose_grounded<O: Orchestrator + ?Sized>(
    orchestrator: &O,
    graph: &FootageGraph,
    template: &Template,
) -> Result<Composition, NarrativeError> {
    let mut dsl = orchestrator.compose(graph, template)?;
    let report = ground(&mut dsl, graph);
    Ok(Composition {
        dsl,
        grounding: report,
    })
}

/// Rule-based montage selection. No model is consulted.
///
/// Builds a cut that moves from scene to scene. Perception has followed
/// every take from start to end, divided it where the view changes, marked
/// the part of each piece worth using, and grouped the pieces into scenes
/// across files. From that, the cut takes the best moment of as many
/// different scenes as the length allows, keeps them in the order they were
/// filmed, never puts two stretches of one scene side by side, and places
/// each in and out point where the picture is steady and the action at rest.
///
/// Footage analysed without a sweep has no segments; each shot then stands
/// in as its own scene and moment, trimmed off the start and end of its take.
#[derive(Debug, Default, Clone)]
pub struct MontageOrchestrator;

/// A stretch of footage the cut may use: a segment's usable part, or a
/// whole shot when the footage was not swept.
#[derive(Debug, Clone)]
struct Moment<'g> {
    shot: &'g Shot,
    segment: Option<&'g Segment>,
    from: Micros,
    to: Micros,
    /// The scene it shows; a shot with no segments is a scene of its own.
    scene: String,
    /// How good it looks, for this template, 0 to 1.
    quality: f64,
    /// Where its source falls among the project's, for capture order.
    source_rank: usize,
    /// Filmed the other way up from most of the project: a portrait clip in
    /// a landscape cut, which the editor can only show with bars.
    turned: bool,
    /// For a dance cut: how many people dance in it, when poses were read.
    people: Option<u32>,
    /// A photograph: the same picture however long it is held.
    still: bool,
}

impl Moment<'_> {
    fn len(&self) -> Micros {
        self.to.saturating_sub(self.from)
    }

    /// The longest this moment may run in a cut of these lengths: all of
    /// it, or for a photo, as long as a photo is held.
    fn limit(&self, lengths: &Lengths) -> Micros {
        if self.still {
            self.len().min(lengths.still)
        } else {
            self.len()
        }
    }

    /// The shot's measurements that fall inside this moment.
    fn samples(&self) -> &[TimelineSample] {
        let timeline = &self.shot.timeline;
        let a = timeline.partition_point(|s| s.at_us < self.from);
        let b = timeline.partition_point(|s| s.at_us <= self.to);
        &timeline[a..b.max(a)]
    }

    fn signature(&self) -> &[f64] {
        self.segment.map_or(&[], |s| s.signature.as_slice())
    }
}

/// How long clips may run in this cut, in whole beats when it follows one.
#[derive(Debug, Clone)]
struct Lengths {
    beat: Option<Micros>,
    min: Micros,
    max: Micros,
    /// What most clips run: short enough to visit many scenes, long enough
    /// to read.
    usual: Micros,
    /// What the few best moments are given, to let the cut breathe.
    long: Micros,
    /// How long a photograph is held: long enough to take in, short
    /// enough that the cut keeps moving. Never longer than a clip's usual
    /// length, never shorter than the template allows.
    still: Micros,
}

impl Lengths {
    fn for_cut(template: &Template, beat: Option<&Beat>, budget: Micros, scenes: usize) -> Self {
        let mut lengths = Self::for_clips(template, beat, budget, scenes);
        let held = lengths.usual.min(STILL_HELD).max(lengths.min);
        lengths.still = match lengths.beat {
            // Whole beats, as every clip on a beat is.
            Some(b) => Micros((held.0 / b.0).max(1) * b.0).max(lengths.min),
            None => held,
        };
        lengths
    }

    fn for_clips(template: &Template, beat: Option<&Beat>, budget: Micros, scenes: usize) -> Self {
        let p = &template.params;
        let share = Micros(budget.0 / scenes.max(1) as u64);
        // More scenes than the cut holds even at the template's shortest
        // clips: a short cut of a lot of footage. Then it visits as many as
        // it can in short clips, none held longer than the rest -- a user
        // who chose forty clips for fifteen seconds got four, eight beats
        // each.
        let crowded = (scenes as u64).saturating_mul(p.min_slot_us.0) > budget.0;
        match beat {
            Some(beat) if crowded => {
                // Four beats a clip, two when the tempo is so slow that four
                // would hold over three seconds.
                let b = beat.length.0;
                let n = if 4 * b <= 3_000_000 { 4 } else { 2 };
                Lengths {
                    beat: Some(beat.length),
                    min: Micros(2.min(n) * b),
                    max: Micros(n * b),
                    usual: Micros(n * b),
                    long: Micros(n * b),
                    still: Micros::ZERO,
                }
            }
            None if crowded => {
                let min = p.min_slot_us;
                Lengths {
                    beat: None,
                    min,
                    max: min,
                    usual: min,
                    long: min,
                    still: Micros::ZERO,
                }
            }
            Some(beat) => {
                let b = beat.length.0;
                let fewest = p.min_slot_us.0.div_ceil(b).max(1);
                let most = (p.max_slot_us.0 / b).max(fewest);
                // Cuts fall on musical phrases: two, four or eight beats, the
                // shortest that is long enough for the template and gives
                // each scene its share.
                let wanted = share.0.max(p.min_slot_us.0).div_ceil(b);
                let phrase = |n: u64| {
                    [1u64, 2, 4, 8, 16]
                        .into_iter()
                        .find(|p| *p >= n)
                        .unwrap_or(n)
                };
                let usual = phrase(wanted.max(fewest)).clamp(fewest, most);
                let long = phrase(usual * 2).clamp(fewest, most);
                Lengths {
                    beat: Some(beat.length),
                    min: Micros(fewest * b),
                    max: Micros(most * b),
                    usual: Micros(usual * b),
                    long: Micros(long * b),
                    still: Micros::ZERO,
                }
            }
            None => {
                let (min, max) = (p.min_slot_us, p.max_slot_us.max(p.min_slot_us));
                // A quarter of the way up the template's range at the
                // shortest: its minimum is a floor, not a pace.
                let floor = Micros(min.0 + (max.0 - min.0) / 4);
                let usual = Micros(share.0.clamp(floor.0, max.0));
                let long = Micros((usual.0 * 3 / 2).min(max.0));
                Lengths {
                    beat: None,
                    min,
                    max,
                    usual,
                    long,
                    still: Micros::ZERO,
                }
            }
        }
    }

    /// The longest allowed length no longer than `room`, or `None` when not
    /// even the shortest fits.
    fn fit(&self, room: Micros) -> Option<Micros> {
        let room = room.min(self.max);
        let len = match self.beat {
            Some(b) => Micros(room.0 / b.0 * b.0),
            None => room,
        };
        (len >= self.min).then_some(len)
    }

    /// How much a clip grows by when there is room left over.
    fn step(&self) -> Micros {
        self.beat.unwrap_or(Micros(500_000))
    }

    fn beats(&self, len: Micros) -> Option<u64> {
        self.beat.map(|b| len.0 / b.0)
    }
}

/// One clip of the cut as planned.
#[derive(Debug, Clone)]
struct Pick {
    moment: usize,
    len: Micros,
    /// A second visit to a scene already in the cut.
    revisit: bool,
    in_us: Micros,
}

/// Two stretches whose views are closer than this read as the same place on
/// screen, whatever their scenes, and are kept apart like one scene.
const ALIKE: f64 = 0.035;

/// The longest a photograph is held when the template allows it: long
/// enough to take in, short enough not to stall a cut of moving pictures.
const STILL_HELD: Micros = Micros(3_000_000);

/// At most this share of a cut's clips are photographs, while there are
/// videos to fill the rest. A photo reads as steady and sharp as the best
/// moment of any video, so without a limit a project of both would be cut
/// as a slideshow.
const STILL_SHARE: f64 = 0.5;

/// How many photographs may follow one another when a video could go
/// between them.
const STILLS_IN_A_ROW: usize = 2;

/// How much of a dance take one moment of a dance cut covers: a long take
/// becomes several moments, so a cut can return to the dance further on.
const DANCE_STRETCH: Micros = Micros(6_000_000);

/// A join cheaper than this carries the movement across the cut.
const JOINS_WELL: f64 = 0.6;

/// What a dance cut has to go on about the people in the footage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DanceReading {
    /// Who is at each keyframe, from the director.
    Keyframe,
    /// How they move through each stretch a dance cut may use.
    Moving,
}

/// How much an in or out point at rest matters against the picture's
/// quality, when choosing where a clip starts.
const REST_WEIGHT: f64 = 0.5;

/// A take's first and last moments, when a shot with no segments reaches
/// them: the camera coming up and going down.
const TAKE_START: Micros = Micros(800_000);
const TAKE_END: Micros = Micros(500_000);

impl MontageOrchestrator {
    /// The cut's frame, upright: the way up most of the footage is filmed,
    /// by length, at the size most of that footage has. None when no source
    /// says how big it is.
    pub fn frame(graph: &FootageGraph) -> Option<(u32, u32)> {
        let (mut wide, mut tall) = (0u64, 0u64);
        let mut sizes: Vec<((u32, u32), u64)> = Vec::new();
        for source in &graph.sources {
            let Some((w, h)) = source.display_size().filter(|(w, h)| *w > 0 && *h > 0) else {
                continue;
            };
            // A photo counts for as long as a cut holds one, not for the
            // ceiling its shot is given.
            let length = if source.is_still() {
                STILL_HELD.0
            } else {
                source.duration_us.0
            };
            if h > w {
                tall += length;
            } else {
                wide += length;
            }
            match sizes.iter_mut().find(|(size, _)| *size == (w, h)) {
                Some((_, total)) => *total += length,
                None => sizes.push(((w, h), length)),
            }
        }
        let tall_project = tall > wide;
        sizes
            .into_iter()
            .filter(|((w, h), _)| (h > w) == tall_project)
            .max_by_key(|(_, length)| *length)
            .map(|(size, _)| size)
    }

    /// Each source's place in the order the footage was filmed.
    ///
    /// By the camera's own clock where it is known -- a video's creation
    /// time, a photo's EXIF -- because the order files arrive in is the
    /// order they were copied, and a phone's photos and videos are copied
    /// from different folders. A source with no capture time stays beside
    /// the one given before it.
    fn capture_order(graph: &FootageGraph) -> std::collections::HashMap<&str, usize> {
        let mut times: Vec<Option<i64>> =
            graph.sources.iter().map(|s| s.captured_seconds()).collect();
        for i in 1..times.len() {
            if times[i].is_none() {
                times[i] = times[i - 1];
            }
        }
        for i in (0..times.len().saturating_sub(1)).rev() {
            if times[i].is_none() {
                times[i] = times[i + 1];
            }
        }
        let mut order: Vec<usize> = (0..graph.sources.len()).collect();
        order.sort_by_key(|&i| (times[i], i));
        order
            .into_iter()
            .enumerate()
            .map(|(rank, i)| (graph.sources[i].id.as_str(), rank))
            .collect()
    }

    fn transition_type(name: &str) -> TransitionType {
        match name {
            "xfade" => TransitionType::Xfade,
            "fadeToBlack" => TransitionType::FadeToBlack,
            "dissolve" => TransitionType::Dissolve,
            "wipeLeft" => TransitionType::WipeLeft,
            "wipeRight" => TransitionType::WipeRight,
            _ => TransitionType::Cut,
        }
    }

    /// For a dance template, how much perception has read of the people:
    /// who is at each keyframe, or how they move through the stretches a
    /// dance cut may use. `None` for any other template, and for footage
    /// read without a pose model, which is cut on motion alone.
    fn dance_reading(graph: &FootageGraph, template: &Template) -> Option<DanceReading> {
        let dance = template
            .params
            .extra
            .get("dance")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !dance {
            return None;
        }
        let segments = || graph.shots.iter().flat_map(|s| s.segments.iter());
        if segments().any(|s| s.poses.len() >= 3) {
            Some(DanceReading::Moving)
        } else if segments().any(|s| s.people.is_some()) {
            Some(DanceReading::Keyframe)
        } else {
            None
        }
    }

    /// A dance cut's order: the solos together, the duets together, each
    /// group where its first clip was filmed, and within a group each clip
    /// followed by whichever remaining one its movement flows into best,
    /// never two of one scene side by side.
    fn arrange_dance(moments: &[Moment], mut picks: Vec<Pick>) -> Vec<Pick> {
        picks.sort_by(|a, b| {
            let (ma, mb) = (&moments[a.moment], &moments[b.moment]);
            ma.source_rank
                .cmp(&mb.source_rank)
                .then(ma.from.cmp(&mb.from))
        });
        let mut groups: Vec<(u32, Vec<Pick>)> = Vec::new();
        for pick in picks {
            let n = moments[pick.moment].people.unwrap_or(0);
            match groups.iter_mut().find(|(k, _)| *k == n) {
                Some((_, list)) => list.push(pick),
                None => groups.push((n, vec![pick])),
            }
        }
        let clash = |a: &Pick, b: &Pick| {
            let (ma, mb) = (&moments[a.moment], &moments[b.moment]);
            ma.scene == mb.scene
                || (!ma.signature().is_empty()
                    && !mb.signature().is_empty()
                    && view_distance(ma.signature(), mb.signature()) < ALIKE)
        };
        let mut out: Vec<Pick> = Vec::new();
        for (_, mut group) in groups {
            while !group.is_empty() {
                let next = match out.last() {
                    None => Some(0),
                    Some(last) => {
                        let flows =
                            |p: &Pick| Self::flow(&moments[last.moment], &moments[p.moment]);
                        group
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| !clash(last, p))
                            .min_by(|a, b| flows(a.1).total_cmp(&flows(b.1)))
                            .map(|(k, _)| k)
                    }
                };
                match next {
                    Some(k) => out.push(group.remove(k)),
                    // Every stretch left is of the scene just shown: the
                    // rule against one scene twice in a row holds, and they
                    // are left out rather than cut together as jump cuts.
                    None => break,
                }
            }
        }
        out
    }

    /// How well one stretch's movement can flow into another's at best: the
    /// cheapest join between a pose of the first and one of the second. An
    /// estimate for ordering; the cut points are found after.
    fn flow(a: &Moment, b: &Moment) -> f64 {
        let (Some(sa), Some(sb)) = (a.segment, b.segment) else {
            return crate::dance::MISMATCH;
        };
        let (pa, pb) = (&sa.poses, &sb.poses);
        if pa.is_empty() || pb.is_empty() {
            return crate::dance::MISMATCH;
        }
        // A dozen of each is plenty to find the closest pair.
        let every = |n: usize| (n / 12).max(1);
        let mut best = crate::dance::MISMATCH;
        for x in pa.iter().step_by(every(pa.len())) {
            for y in pb.iter().step_by(every(pb.len())) {
                best = best.min(crate::dance::pose_distance(x, y));
            }
        }
        best
    }

    /// Where each clip of a dance cut starts, chosen together: every start
    /// where a pose was read is a candidate, and the chain of starts is the
    /// one whose joins -- the pose, the placement and the movement at each
    /// cut -- cost least in all (Viterbi over the clips in order). Returns
    /// each join's cost, into the clip, `None` for the first.
    fn place_dance(moments: &[Moment], picks: &mut [Pick]) -> Vec<Option<f64>> {
        let lists: Vec<&[vb_contracts::footage_graph::PoseSample]> = picks
            .iter()
            .map(|p| {
                moments[p.moment]
                    .segment
                    .map(|s| s.poses.as_slice())
                    .unwrap_or(&[])
            })
            .collect();
        let poses = |k: usize| lists[k];
        // The candidate starts for each clip.
        let options: Vec<Vec<Micros>> = (0..picks.len())
            .map(|k| {
                let m = &moments[picks[k].moment];
                let latest = m.to.saturating_sub(picks[k].len).max(m.from);
                let mut starts: Vec<Micros> = poses(k)
                    .iter()
                    .map(|p| p.at_us)
                    .filter(|t| *t >= m.from && *t <= latest)
                    .collect();
                // Forty candidates is ten seconds of poses; more only slows
                // the search.
                let every = (starts.len() / 40).max(1);
                starts = starts.into_iter().step_by(every).collect();
                if starts.is_empty() {
                    starts.push(picks[k].in_us);
                }
                starts
            })
            .collect();
        if options.is_empty() {
            return Vec::new();
        }
        // cost[k][c]: the cheapest chain up to clip k starting at option c.
        let mut cost: Vec<Vec<f64>> = vec![vec![0.0; options[0].len()]];
        let mut back: Vec<Vec<usize>> = vec![vec![0; options[0].len()]];
        for k in 1..picks.len() {
            let len = picks[k - 1].len;
            let (mut row, mut from) = (Vec::new(), Vec::new());
            for &start in &options[k] {
                let (best, arg) = options[k - 1]
                    .iter()
                    .enumerate()
                    .map(|(c, &prev)| {
                        let join =
                            crate::dance::join_cost(poses(k - 1), prev + len, poses(k), start);
                        (cost[k - 1][c] + join, c)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap_or((0.0, 0));
                row.push(best);
                from.push(arg);
            }
            cost.push(row);
            back.push(from);
        }
        let last = picks.len() - 1;
        let mut choice = cost[last]
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(c, _)| c)
            .unwrap_or(0);
        for k in (0..picks.len()).rev() {
            picks[k].in_us = options[k][choice];
            choice = back[k][choice];
        }
        (0..picks.len())
            .map(|k| {
                (k > 0).then(|| {
                    crate::dance::join_cost(
                        poses(k - 1),
                        picks[k - 1].in_us + picks[k - 1].len,
                        poses(k),
                        picks[k].in_us,
                    )
                })
            })
            .collect()
    }

    fn prefers_motion(template: &Template) -> bool {
        template
            .params
            .extra
            .get("preferMotion")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Whether the reasons should be written in Chinese. The caller's
    /// language arrives in the template's extras, as the title does: a
    /// template is the only thing this layer is handed.
    fn chinese(template: &Template) -> bool {
        template.params.extra.get("locale").and_then(|v| v.as_str()) == Some("zh")
    }

    /// How a shot ranks for this template, when it has no segments to rank
    /// by.
    ///
    /// The classical score counts motion against a shot, as shake. For dance
    /// that is backwards: the moving shots are the point. A template that
    /// prefers motion ranks on sharpness, motion and exposure instead.
    fn rank_score(shot: &Shot, template: &Template) -> f64 {
        if !Self::prefers_motion(template) {
            return shot.score();
        }
        shot.keyframes
            .iter()
            .filter_map(|k| k.scores.as_ref())
            .map(|s| {
                let part = |v: Option<f64>| v.unwrap_or(0.0).clamp(0.0, 1.0);
                0.40 * part(s.sharpness) + 0.35 * part(s.flow_jitter) + 0.25 * part(s.exposure)
            })
            .fold(0.0, f64::max)
    }

    fn subtitle_style() -> TextStyle {
        TextStyle {
            font_family: Some("Inter".into()),
            font_size_pt: Some(28.0),
            primary_color: Some("#FFFFFF".into()),
            outline_color: Some("#000000".into()),
            outline_width: Some(1.5),
            alignment: Some("bottomCenter".into()),
            margin_v_pct: Some(0.08),
            ..TextStyle::default()
        }
    }

    /// Everything the cut could use, from every shot.
    ///
    /// A moment's quality is its picture's, less what keeps it from filling
    /// a cut: filmed the other way up from the project, or looking past
    /// something right against the lens.
    fn moments<'g>(graph: &'g FootageGraph, template: &Template) -> Vec<Moment<'g>> {
        let rank = Self::capture_order(graph);

        // The project's way up is its frame's.
        let project_tall = Self::frame(graph).is_some_and(|(w, h)| h > w);
        let turned = |shot: &Shot| {
            graph
                .source(&shot.source_id)
                .and_then(|s| s.display_size())
                .is_some_and(|(w, h)| (h > w) != project_tall)
        };
        let motion = Self::prefers_motion(template);
        let dance = Self::dance_reading(graph, template);

        let mut out = Vec::new();
        for shot in &graph.shots {
            let source_rank = rank
                .get(shot.source_id.as_str())
                .copied()
                .unwrap_or(usize::MAX);
            let turned = turned(shot);
            // A portrait clip in a landscape cut shows with bars either side:
            // usable, but not before a clip that fills the frame.
            let orientation = if turned { 0.6 } else { 1.0 };
            let still = graph.source(&shot.source_id).is_some_and(|s| s.is_still());
            // A photograph cannot show anybody dancing.
            if still && (motion || dance.is_some()) {
                continue;
            }

            if shot.segments.is_empty() {
                let source_len = graph.source(&shot.source_id).map(|s| s.duration_us);
                let from = if shot.start_us == Micros::ZERO {
                    shot.start_us + TAKE_START.min(Micros(shot.duration().0 / 8))
                } else {
                    shot.start_us
                };
                let to = if source_len.is_some_and(|d| shot.end_us >= d) {
                    shot.end_us
                        .saturating_sub(TAKE_END.min(Micros(shot.duration().0 / 8)))
                } else {
                    shot.end_us
                };
                out.push(Moment {
                    shot,
                    segment: None,
                    from,
                    to: to.max(from),
                    scene: shot.id.clone(),
                    quality: Self::rank_score(shot, template) * orientation,
                    source_rank,
                    turned,
                    people: None,
                    still,
                });
                continue;
            }

            for segment in &shot.segments {
                let Some((mut from, mut to)) = segment.usable() else {
                    continue;
                };
                let mut people = None;
                let mut dancing = None;
                if dance == Some(DanceReading::Keyframe) {
                    // Somebody has to be there to be dancing.
                    if segment.people.unwrap_or(0) == 0 {
                        continue;
                    }
                    people = segment.people;
                }
                if dance == Some(DanceReading::Moving) {
                    // Only the stretches whose movement was read, and only
                    // where limbs are moving: dance, not a walk or a pose
                    // for the camera.
                    if segment.poses.len() < 3 {
                        continue;
                    }
                    let Some(energy) = crate::dance::dancing(&segment.poses) else {
                        continue;
                    };
                    people = crate::dance::people(&segment.poses).filter(|n| *n > 0);
                    if people.is_none() {
                        continue;
                    }
                    // Cut only where the poses say how the movement runs.
                    let first = segment.poses.first().map(|p| p.at_us).unwrap_or(from);
                    let last = segment.poses.last().map(|p| p.at_us).unwrap_or(to);
                    from = from.max(first);
                    to = to.min(last);
                    dancing = Some(energy);
                }
                let mut moment = Moment {
                    shot,
                    segment: Some(segment),
                    from,
                    to,
                    scene: segment
                        .scene_id
                        .clone()
                        .unwrap_or_else(|| segment.id.clone()),
                    quality: segment.quality.unwrap_or_else(|| shot.score()),
                    source_rank,
                    turned,
                    people,
                    still,
                };
                if let Some(energy) = dancing {
                    // Livelier dancing ranks higher, up to a point.
                    moment.quality *= 0.7 + 0.3 * (energy / (2.0 * crate::dance::MOVING)).min(1.0);
                    // A dance take runs the length of a song, and a cut
                    // comes back to it at later points of the dance: each
                    // stretch of it is a moment of its own, of one scene,
                    // so two never sit side by side.
                    moment.quality *=
                        orientation * clear_view(segment.occlusion) * composed(segment.composition);
                    let mut start = moment.from;
                    while start < moment.to {
                        let mut end = Micros((start.0 + DANCE_STRETCH.0).min(moment.to.0));
                        if moment.to.saturating_sub(end) < Micros(DANCE_STRETCH.0 / 2) {
                            end = moment.to;
                        }
                        out.push(Moment {
                            from: start,
                            to: end,
                            ..moment.clone()
                        });
                        start = end;
                    }
                    continue;
                }
                if motion {
                    // Dance wants the stretches where something moves.
                    let samples = moment.samples();
                    let moving = if samples.is_empty() {
                        0.0
                    } else {
                        samples.iter().filter_map(|s| s.motion).sum::<f64>() / samples.len() as f64
                    };
                    moment.quality *= 0.6 + 0.4 * moving.clamp(0.0, 1.0);
                }
                moment.quality *=
                    orientation * clear_view(segment.occlusion) * composed(segment.composition);
                out.push(moment);
            }
        }
        out
    }

    /// Choose which moments make the cut and how long each runs, filling
    /// `budget` without passing it.
    ///
    /// Scenes are visited best first -- the best moment of each, with a
    /// little credit for a scene the user filmed a lot of, and a little held
    /// against a source that has already given clips -- so the cut is the
    /// strongest material from as many places as fit, not the first files in
    /// the folder. The first few get the long length. With every scene used
    /// and room still left, the next best moments come back as revisits.
    fn choose(moments: &[Moment], lengths: &Lengths, budget: Micros, dance: bool) -> Vec<Pick> {
        let mut by_scene: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, m) in moments.iter().enumerate() {
            match by_scene.iter_mut().find(|(s, _)| *s == m.scene) {
                Some((_, list)) => list.push(i),
                None => by_scene.push((m.scene.clone(), vec![i])),
            }
        }
        for (_, list) in by_scene.iter_mut() {
            list.sort_by(|&a, &b| {
                moments[b]
                    .quality
                    .total_cmp(&moments[a].quality)
                    .then(a.cmp(&b))
            });
        }
        let priority = |list: &[usize]| {
            let best = moments[list[0]].quality;
            let filmed: f64 = list.iter().map(|&i| moments[i].len().as_secs_f64()).sum();
            best + 0.04 * (1.0 + filmed / 10.0).ln()
        };

        let expected = (budget.0 / lengths.usual.0.max(1)).max(1) as usize;
        let heroes = (expected / 6).max(1);
        let mut picks: Vec<Pick> = Vec::new();
        let mut used = Micros::ZERO;
        let mut taken = vec![false; moments.len()];
        let mut scene_done = vec![false; by_scene.len()];
        let mut from_source: std::collections::HashMap<&str, u32> =
            std::collections::HashMap::new();

        loop {
            let room = budget.saturating_sub(used);
            if room < lengths.min {
                break;
            }
            // Photos have had their share while videos remain to be used:
            // the next scene is one that a video shows.
            let stills_given = picks.iter().filter(|p| moments[p.moment].still).count();
            let videos_left = (0..moments.len())
                .any(|i| !taken[i] && !moments[i].still && moments[i].len() >= lengths.min);
            let enough_stills =
                videos_left && stills_given as f64 >= STILL_SHARE * (picks.len() + 1) as f64;
            let has_video = |list: &[usize]| list.iter().any(|&i| !moments[i].still);
            // The next scene: best first, with sources that have given
            // clips already counting for a little less.
            let next = by_scene
                .iter()
                .enumerate()
                .filter(|(k, _)| !scene_done[*k])
                .filter(|(_, (_, list))| !enough_stills || has_video(list))
                .map(|(k, (_, list))| {
                    let source = moments[list[0]].shot.source_id.as_str();
                    let given = from_source.get(source).copied().unwrap_or(0);
                    (k, priority(list) * 0.85f64.powi(given as i32))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));

            let (moment, revisit) = match next {
                Some((k, _)) => {
                    scene_done[k] = true;
                    let list = &by_scene[k].1;
                    match list
                        .iter()
                        .copied()
                        .filter(|&i| !enough_stills || !moments[i].still)
                        .find(|&i| moments[i].len() >= lengths.min)
                    {
                        Some(i) => (i, false),
                        None => continue,
                    }
                }
                None => {
                    // Every scene is in. Room left comes back to the best
                    // moments not yet used, from scenes already seen -- in
                    // a dance cut, from the scene given least so far, so
                    // the dance moves between places rather than staying
                    // in the one that scores best.
                    let given = |i: usize| {
                        if !dance {
                            return 0;
                        }
                        picks
                            .iter()
                            .filter(|p| moments[p.moment].scene == moments[i].scene)
                            .count()
                    };
                    // Only a revisit the cut can still place: one scene
                    // may hold at most half the clips, rounded up, or two of
                    // it end up side by side and one is dropped. With two
                    // scenes that means taking turns, whatever else scores
                    // better.
                    let placeable = |i: usize| {
                        let same = picks
                            .iter()
                            .filter(|p| moments[p.moment].scene == moments[i].scene)
                            .count();
                        same < (picks.len() + 2) / 2
                    };
                    let best_of = |cap_stills: bool| {
                        (0..moments.len())
                            .filter(|&i| !taken[i] && moments[i].len() >= lengths.min)
                            .filter(|&i| placeable(i))
                            .filter(|&i| !cap_stills || !moments[i].still)
                            .max_by(|&a, &b| {
                                given(b)
                                    .cmp(&given(a))
                                    .then(moments[a].quality.total_cmp(&moments[b].quality))
                                    .then(b.cmp(&a))
                            })
                    };
                    // Photos' share gives way to a cut that can be placed.
                    let best = best_of(enough_stills).or_else(|| best_of(false));
                    match best {
                        Some(i) => (i, true),
                        None => break,
                    }
                }
            };
            taken[moment] = true;
            // A photo is held as long as a photo is; the long lengths go to
            // moving pictures.
            let want = if moments[moment].still {
                lengths.still
            } else if picks.iter().filter(|p| !moments[p.moment].still).count() < heroes {
                lengths.long
            } else {
                lengths.usual
            };
            let Some(len) = lengths.fit(want.min(moments[moment].limit(lengths)).min(room)) else {
                continue;
            };
            *from_source
                .entry(moments[moment].shot.source_id.as_str())
                .or_insert(0) += 1;
            used = used + len;
            picks.push(Pick {
                moment,
                len,
                revisit,
                in_us: moments[moment].from,
            });
        }
        picks
    }

    /// Order the picks as they were filmed, keeping apart any two that show
    /// one scene -- or views alike enough to read as one.
    ///
    /// Built one clip at a time: the earliest remaining pick that does not
    /// clash with the clip before it goes next. When every remaining pick
    /// clashes, the earliest is fitted in wherever it clashes with neither
    /// neighbour, or left out. Each step places or drops one pick, so it
    /// always ends, and the cut stays as close to capture order as the rule
    /// allows: 1-2-3-4-5-1-6 rather than 1-1-2-3.
    fn arrange(moments: &[Moment], mut picks: Vec<Pick>) -> Vec<Pick> {
        picks.sort_by(|a, b| {
            let (ma, mb) = (&moments[a.moment], &moments[b.moment]);
            ma.source_rank
                .cmp(&mb.source_rank)
                .then(ma.from.cmp(&mb.from))
        });
        let clash = |a: &Pick, b: &Pick| {
            let (ma, mb) = (&moments[a.moment], &moments[b.moment]);
            ma.scene == mb.scene
                || (!ma.signature().is_empty()
                    && !mb.signature().is_empty()
                    && view_distance(ma.signature(), mb.signature()) < ALIKE)
        };

        // Photos run no more than a few in a row while there are videos to
        // go between them, so a mixed cut does not stall into a slideshow.
        // Looking ahead: when the videos left could no longer separate the
        // photos left into short runs if a video went now, a photo goes now,
        // however the capture order runs.
        let still = |p: &Pick| moments[p.moment].still;
        let run = STILLS_IN_A_ROW;

        let mut queue: std::collections::VecDeque<Pick> = picks.into();
        let mut out: Vec<Pick> = Vec::with_capacity(queue.len());
        while !queue.is_empty() {
            let ending = out.iter().rev().take_while(|p| still(p)).count();
            let photos = queue.iter().filter(|p| still(p)).count();
            let videos = queue.len() - photos;
            let fits = |p: &Pick| out.last().is_none_or(|last| !clash(last, p));
            let earliest = |want_still: bool| {
                queue
                    .iter()
                    .position(|p| fits(p) && still(p) == want_still)
                    .or_else(|| queue.iter().position(fits))
            };
            let next = if videos > 0 && ending >= run {
                earliest(false)
            } else if photos > run * videos && ending < run {
                earliest(true)
            } else {
                match out.last() {
                    None => Some(0),
                    Some(last) => queue.iter().position(|p| !clash(last, p)),
                }
            };
            if let Some(k) = next {
                let pick = queue.remove(k).expect("position is in range");
                out.push(pick);
                continue;
            }
            let pick = queue.pop_front().expect("queue is not empty");
            let place = (0..out.len())
                .rev()
                .find(|&p| (p == 0 || !clash(&out[p - 1], &pick)) && !clash(&pick, &out[p]));
            if let Some(p) = place {
                out.insert(p, pick);
            }
        }
        out
    }

    /// The sections a template orders its cut by, in order: category ids
    /// from the Footage Graph's `source.categories`, and `other` for the
    /// rest. `None` unless the template asks for it (`extra.order` is
    /// `sections`), as a year's memories do: people, then scenery, then
    /// food, then everything else, each in the order it was filmed.
    fn sections(template: &Template) -> Option<Vec<String>> {
        let extra = &template.params.extra;
        if extra.get("order").and_then(|v| v.as_str()) != Some("sections") {
            return None;
        }
        let list: Vec<String> = extra
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        (!list.is_empty()).then_some(list)
    }

    /// Which section a source falls in: the first listed that it is of,
    /// else `other` (or after the last, when `other` is not listed).
    fn section_of(graph: &FootageGraph, source_id: &str, sections: &[String]) -> usize {
        let other = sections
            .iter()
            .position(|s| s == "other")
            .unwrap_or(sections.len());
        let Some(source) = graph.source(source_id) else {
            return other;
        };
        sections
            .iter()
            .position(|s| s != "other" && source.categories.iter().any(|c| c == s))
            .unwrap_or(other)
    }

    /// Section by section, each arranged as a cut is -- in capture order,
    /// one scene never beside itself -- and the sections one after another.
    /// Where a section's first clip would sit beside its own scene at the
    /// end of the section before, the first of the section that does not
    /// goes first instead.
    fn arrange_sections(
        graph: &FootageGraph,
        moments: &[Moment],
        picks: Vec<Pick>,
        sections: &[String],
    ) -> Vec<Pick> {
        let mut groups: Vec<Vec<Pick>> = vec![Vec::new(); sections.len() + 1];
        for pick in picks {
            let section = Self::section_of(graph, &moments[pick.moment].shot.source_id, sections);
            groups[section].push(pick);
        }
        let clash = |a: &Pick, b: &Pick| {
            let (ma, mb) = (&moments[a.moment], &moments[b.moment]);
            ma.scene == mb.scene
                || (!ma.signature().is_empty()
                    && !mb.signature().is_empty()
                    && view_distance(ma.signature(), mb.signature()) < ALIKE)
        };
        let mut out: Vec<Pick> = Vec::new();
        for group in groups {
            let mut arranged = Self::arrange(moments, group);
            if let Some(last) = out.last()
                && arranged.first().is_some_and(|first| clash(last, first))
                && let Some(k) = arranged.iter().position(|p| !clash(last, p))
            {
                let pick = arranged.remove(k);
                arranged.insert(0, pick);
            }
            out.extend(arranged);
        }
        out
    }

    /// Grow clips into whatever room is left, best moments first, a beat (or
    /// half a second) at a time, as far as each moment and the template's
    /// longest allow.
    fn fill(moments: &[Moment], picks: &mut [Pick], lengths: &Lengths, budget: Micros) {
        let step = lengths.step();
        let mut order: Vec<usize> = (0..picks.len()).collect();
        order.sort_by(|&a, &b| {
            moments[picks[b].moment]
                .quality
                .total_cmp(&moments[picks[a].moment].quality)
        });
        loop {
            let used: u64 = picks.iter().map(|p| p.len.0).sum();
            let room = budget.0.saturating_sub(used);
            if room < step.0 {
                return;
            }
            let grew = order.iter().copied().find(|&k| {
                let longer = picks[k].len + step;
                longer <= lengths.max && longer <= moments[picks[k].moment].limit(lengths)
            });
            match grew {
                Some(k) => picks[k].len = picks[k].len + step,
                None => return,
            }
        }
    }

    /// Where in its moment a clip of `len` starts.
    ///
    /// Every start from the moment's beginning to the last that fits is
    /// tried, a tenth of a second apart. Each is scored on how good the
    /// picture is across the clip and on how still the action is at its two
    /// ends -- a cut that falls mid-gesture reads as a mistake, one that
    /// falls as a movement completes reads as a choice. Without measurements
    /// the clip is centred on the moment's best keyframe.
    fn place(moment: &Moment, len: Micros, prefers_motion: bool) -> Micros {
        // A photograph is the same picture throughout: nothing to choose.
        if moment.still {
            return moment.from;
        }
        let latest = moment.to.saturating_sub(len).max(moment.from);
        let samples = moment.samples();
        if samples.len() < 2 || latest <= moment.from {
            let centre = moment
                .segment
                .and_then(|s| s.keyframe_id.as_ref())
                .and_then(|id| moment.shot.keyframes.iter().find(|k| &k.id == id))
                .or_else(|| moment.shot.best_keyframe())
                .map(|k| k.at_us)
                .filter(|t| *t >= moment.from && *t <= moment.to)
                .unwrap_or(Micros((moment.from.0 + moment.to.0) / 2));
            return Micros(centre.0.saturating_sub(len.0 / 2)).clamp(moment.from, latest);
        }

        let at = |t: Micros, field: fn(&TimelineSample) -> Option<f64>| -> f64 {
            // Linear between the samples either side.
            let k = samples.partition_point(|s| s.at_us <= t);
            let value = |s: &TimelineSample| field(s).unwrap_or(0.0);
            match (k.checked_sub(1).map(|i| &samples[i]), samples.get(k)) {
                (Some(a), Some(b)) if b.at_us > a.at_us => {
                    let f = (t.0 - a.at_us.0) as f64 / (b.at_us.0 - a.at_us.0) as f64;
                    value(a) + (value(b) - value(a)) * f
                }
                (Some(a), _) => value(a),
                (None, Some(b)) => value(b),
                (None, None) => 0.0,
            }
        };
        let mean = |from: Micros, to: Micros, field: fn(&TimelineSample) -> Option<f64>| {
            let inside: Vec<f64> = samples
                .iter()
                .filter(|s| s.at_us >= from && s.at_us <= to)
                .filter_map(field)
                .collect();
            if inside.is_empty() {
                at(Micros((from.0 + to.0) / 2), field)
            } else {
                inside.iter().sum::<f64>() / inside.len() as f64
            }
        };

        let mut best = (moment.from, f64::MIN);
        let mut start = moment.from;
        while start <= latest {
            let end = start + len;
            let quality = mean(start, end, |s| s.quality);
            let ends = (at(start, |s| s.motion) + at(end, |s| s.motion)) / 2.0;
            let score = if prefers_motion {
                quality + 0.3 * mean(start, end, |s| s.motion) - 0.3 * ends
            } else {
                quality - REST_WEIGHT * ends
            };
            if score > best.1 + 1e-9 {
                best = (start, score);
            }
            start = start + Micros(100_000);
        }
        best.0
    }

    /// How still the action is at a clip's two ends, 0 to 1.
    fn stillness_at_ends(moment: &Moment, in_us: Micros, len: Micros) -> Option<f64> {
        let samples = moment.samples();
        if samples.len() < 2 {
            return None;
        }
        let near = |t: Micros| {
            samples
                .iter()
                .min_by_key(|s| s.at_us.0.abs_diff(t.0))
                .and_then(|s| s.motion)
                .unwrap_or(0.0)
        };
        Some(1.0 - (near(in_us) + near(in_us + len)) / 2.0)
    }

    /// Why a clip was chosen, in the caller's language.
    #[allow(clippy::too_many_arguments)]
    fn reason(
        graph: &FootageGraph,
        moment: &Moment,
        pick: &Pick,
        beat: Option<&Beat>,
        lengths: &Lengths,
        join: Option<f64>,
        chinese: bool,
    ) -> String {
        let scene = graph.scenes.iter().position(|s| s.id == moment.scene);
        let label = scene.and_then(|i| graph.scenes[i].label.clone());
        let at_rest =
            Self::stillness_at_ends(moment, pick.in_us, pick.len).is_some_and(|s| s > 0.7);
        let out_us = pick.in_us + pick.len;
        let span = format!(
            "{:.1}-{:.1}s",
            pick.in_us.as_secs_f64(),
            out_us.as_secs_f64()
        );
        let duration = pick.len;
        if chinese {
            let what = match (scene, &label) {
                (Some(i), Some(l)) => format!("场景 {}（{l}）", i + 1),
                (Some(i), None) => format!("场景 {}", i + 1),
                _ => "这个镜头".to_string(),
            };
            let piece = if moment.still { "张照片" } else { "段" };
            let mut text = if pick.revisit {
                format!(
                    "回到{what}，选中另一{piece}，画面评分 {:.2}",
                    moment.quality.min(1.0)
                )
            } else {
                format!(
                    "选中{what}里最好的一{piece}，画面评分 {:.2}",
                    moment.quality.min(1.0)
                )
            };
            if let Some(c) = moment.segment.and_then(|s| s.composition) {
                text.push_str(&format!("，构图 {:.1} 分（满分 5）", 1.0 + 4.0 * c));
            }
            if moment.turned {
                text.push_str("（竖拍素材，放进横屏会有黑边）");
            }
            if let Some(n) = moment.people {
                text.push_str(&format!("；{n} 人在跳舞"));
            }
            if let Some(cost) = join {
                if cost < JOINS_WELL {
                    text.push_str(&format!("，动作接着上一段（姿态差 {cost:.2}）"));
                } else if cost < crate::dance::MISMATCH {
                    text.push_str("，与上一段的动作没有完全接上");
                }
            }
            if moment.still {
                match (beat, lengths.beats(duration)) {
                    (Some(beat), Some(n)) => text.push_str(&format!(
                        "；照片踩着{}停留 {n} 拍（{duration}）。",
                        beat.describe(true)
                    )),
                    _ => text.push_str(&format!("；照片停留 {duration}。")),
                }
                return text;
            }
            text.push_str(&format!("；取素材 {span}"));
            if at_rest {
                text.push_str("，起止都落在动作停顿处");
            }
            match (beat, lengths.beats(duration)) {
                (Some(beat), Some(n)) => text.push_str(&format!(
                    "；踩着{}保持 {n} 拍（{duration}）。",
                    beat.describe(true)
                )),
                _ => text.push_str(&format!("；时长 {duration}。")),
            }
            text
        } else {
            let what = match (scene, &label) {
                (Some(i), Some(l)) => format!("scene {} ({l})", i + 1),
                (Some(i), None) => format!("scene {}", i + 1),
                _ => "this shot".to_string(),
            };
            let piece = if moment.still { "photo" } else { "stretch" };
            let mut text = if pick.revisit {
                format!(
                    "Back to {what} for another {piece}, picture score {:.2}",
                    moment.quality.min(1.0)
                )
            } else {
                format!(
                    "Selected as the best {piece} of {what}, picture score {:.2}",
                    moment.quality.min(1.0)
                )
            };
            if let Some(c) = moment.segment.and_then(|s| s.composition) {
                text.push_str(&format!(", composition {:.1}/5", 1.0 + 4.0 * c));
            }
            if moment.turned {
                text.push_str(" (filmed upright, so it shows with bars)");
            }
            if let Some(n) = moment.people {
                text.push_str(&format!(
                    "; {n} {} dancing",
                    if n == 1 { "person" } else { "people" }
                ));
            }
            if let Some(cost) = join {
                if cost < JOINS_WELL {
                    text.push_str(&format!(
                        ", the movement carried on from the clip before (pose difference {cost:.2})"
                    ));
                } else if cost < crate::dance::MISMATCH {
                    text.push_str(", not quite joining the movement of the clip before");
                }
            }
            if moment.still {
                match (beat, lengths.beats(duration)) {
                    (Some(beat), Some(n)) => text.push_str(&format!(
                        "; the photo is held for {n} beat{} ({duration}) on {}.",
                        if n == 1 { "" } else { "s" },
                        beat.describe(false)
                    )),
                    _ => text.push_str(&format!("; the photo is held for {duration}.")),
                }
                return text;
            }
            text.push_str(&format!("; source {span}"));
            if at_rest {
                text.push_str(", cut where the action rests");
            }
            match (beat, lengths.beats(duration)) {
                (Some(beat), Some(n)) => text.push_str(&format!(
                    "; held for {n} beat{} ({duration}) on {}.",
                    if n == 1 { "" } else { "s" },
                    beat.describe(false)
                )),
                _ => text.push_str(&format!("; runs {duration}.")),
            }
            text
        }
    }
}

impl Orchestrator for MontageOrchestrator {
    fn name(&self) -> &'static str {
        "montage-v1"
    }

    fn compose(
        &self,
        graph: &FootageGraph,
        template: &Template,
    ) -> Result<EditingDsl, NarrativeError> {
        if graph.shots.is_empty() {
            return Err(NarrativeError::NoFootage);
        }
        if crate::continuity::is_continuity(&template.params.extra) {
            return Self::compose_continuity(graph, template).map(|(dsl, _)| dsl);
        }

        let beat = Beat::for_cut(graph, template);
        // On the beat, the title card is a whole number of beats too, or
        // every cut after it would land between two.
        let title_card_us = match &beat {
            Some(beat) => beat.quantise(template.params.title_card_duration_us).0,
            None => template.params.title_card_duration_us,
        };
        let budget = Micros(template.params.target_duration_us.0.saturating_sub(
            if template.params.title_card {
                title_card_us.0
            } else {
                0
            },
        ));

        let moments = Self::moments(graph, template);
        let scenes = {
            let mut seen: Vec<&str> = moments.iter().map(|m| m.scene.as_str()).collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len()
        };
        let lengths = Lengths::for_cut(template, beat.as_ref(), budget, scenes);
        let dance = Self::dance_reading(graph, template);
        let picks = Self::choose(&moments, &lengths, budget, dance.is_some());
        let mut picks = if dance.is_some() {
            Self::arrange_dance(&moments, picks)
        } else if let Some(sections) = Self::sections(template) {
            Self::arrange_sections(graph, &moments, picks, &sections)
        } else {
            Self::arrange(&moments, picks)
        };
        Self::fill(&moments, &mut picks, &lengths, budget);
        let prefers_motion = Self::prefers_motion(template);
        for pick in picks.iter_mut() {
            pick.in_us = Self::place(&moments[pick.moment], pick.len, prefers_motion);
        }
        // A dance cut's starts are chosen together, for the joins.
        let joins = if dance == Some(DanceReading::Moving) {
            Self::place_dance(&moments, &mut picks)
        } else {
            vec![None; picks.len()]
        };

        if picks.is_empty() {
            return Err(NarrativeError::NoUsableShots {
                shots: graph.shots.len(),
                min_slot_us: template.params.min_slot_us.0,
            });
        }

        let mut dsl = Self::start(graph, template);
        dsl.meta.target_duration_us = Some(template.params.target_duration_us);

        let mut order = 0u32;
        let chinese = Self::chinese(template);

        if template.params.title_card {
            Self::open_on_title(&mut dsl, template, title_card_us, chinese);
            order += 1;
        }

        // A transition overlaps two clips, which moves every cut after it
        // off the beat, so a cut on the beat is a hard cut whatever the
        // template says.
        let transition = if beat.is_some() {
            TransitionType::Cut
        } else {
            Self::transition_type(&template.params.transition)
        };
        let mut previous_slot: Option<String> = None;
        let mut slot_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        // What each clip shows, in a sentence, where a model wrote one.
        let mut captions: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let speech_subtitles = template
            .params
            .extra
            .get("speechSubtitles")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        for (k, pick) in picks.iter().enumerate() {
            let moment = &moments[pick.moment];
            let shot = moment.shot;
            let (in_us, out_us) = (pick.in_us, pick.in_us + pick.len);
            let base = format!(
                "slot-{}",
                moment.segment.map_or(shot.id.as_str(), |s| s.id.as_str())
            );
            let mut slot_id = base.clone();
            let mut n = 2;
            while !slot_ids.insert(slot_id.clone()) {
                slot_id = format!("{base}-{n}");
                n += 1;
            }

            dsl.slots.push(Slot {
                id: slot_id.clone(),
                order,
                kind: SlotKind::Clip,
                duration_us: pick.len,
                source_ref: Some(SourceRef {
                    source_id: shot.source_id.clone(),
                    shot_id: Some(shot.id.clone()),
                    in_us,
                    out_us,
                }),
                speed: None,
                reason: Some(Self::reason(
                    graph,
                    moment,
                    pick,
                    beat.as_ref(),
                    &lengths,
                    joins.get(k).copied().flatten(),
                    chinese,
                )),
                // Ranking may carry a well-composed moment past 1; the
                // contract's score is a unit score.
                score: Some(moment.quality.min(1.0)),
            });
            dsl.tracks.video.push(VideoItem {
                slot_id: slot_id.clone(),
                crop: None,
                filters: Vec::new(),
                transform: None,
                background: None,
            });

            if let Some(prev) = previous_slot.take() {
                dsl.transitions.push(Transition {
                    from_slot_id: prev,
                    to_slot_id: slot_id.clone(),
                    kind: transition,
                    duration_us: if transition == TransitionType::Cut {
                        Micros::ZERO
                    } else {
                        template.params.transition_duration_us
                    },
                });
            }
            previous_slot = Some(slot_id.clone());

            if let Some(caption) = moment
                .segment
                .and_then(|s| s.caption.as_ref())
                .filter(|c| !c.text.trim().is_empty())
            {
                captions.insert(slot_id.clone(), caption.text.clone());
            }

            // What was said, on screen: only where the template asks, as an
            // interview's does. Anchored to the slot, never to the finished
            // timeline: offset from the slot start is invariant under any
            // later reordering or transition change.
            if template.params.subtitles && speech_subtitles {
                for seg in &graph.transcript {
                    if seg.source_id != shot.source_id {
                        continue;
                    }
                    let overlap_start = seg.start_us.0.max(in_us.0);
                    let overlap_end = seg.end_us.0.min(out_us.0);
                    if overlap_start >= overlap_end {
                        continue;
                    }
                    dsl.tracks.text.push(TextItem {
                        id: format!("txt-{}-{}", seg.id, order),
                        anchor: Anchor {
                            slot_id: slot_id.clone(),
                            offset_us: Some((overlap_start - in_us.0) as i64),
                        },
                        duration_us: Micros(overlap_end - overlap_start),
                        content: seg.text.clone(),
                        role: Some(TextRole::Subtitle),
                        transcript_segment_id: Some(seg.id.clone()),
                        style: Some(Self::subtitle_style()),
                    });
                }
            }

            order += 1;
        }

        // Captions, on the clips the plan gives one to, where the picture
        // was described. The plan is the one the pipeline asked the model
        // for, so the two agree on which clips those are.
        if template.params.subtitles {
            for place in crate::caption_plan(&dsl) {
                let Some(text) = captions.get(&place.slot_id) else {
                    continue;
                };
                dsl.tracks.text.push(TextItem {
                    id: format!("txt-caption-{}", place.slot_id),
                    anchor: Anchor {
                        slot_id: place.slot_id.clone(),
                        offset_us: Some(place.offset_us.0 as i64),
                    },
                    duration_us: place.duration_us,
                    content: text.clone(),
                    role: Some(TextRole::Caption),
                    transcript_segment_id: None,
                    style: Some(Self::subtitle_style()),
                });
            }
        }

        let looped = template
            .params
            .extra
            .get("musicLoop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        lay_music(&mut dsl, graph, looped);

        let swept = moments.iter().any(|m| m.segment.is_some());
        let mut priors_applied = vec![if Self::prefers_motion(template) {
            "shotScore.motion".to_string()
        } else {
            "shotScore.aggregate".to_string()
        }];
        if swept {
            priors_applied.push(format!("scenes.cover@{scenes}"));
            priors_applied.push("timeline.restPoints".to_string());
        }
        if let Some(beat) = &beat {
            priors_applied.push(match beat.grid.origin {
                Some(BeatOrigin::Music) => format!("beatGrid.music@{:.1}", beat.grid.bpm),
                Some(BeatOrigin::Source) => format!("beatGrid.source@{:.1}", beat.grid.bpm),
                _ => format!("beatGrid.fixed@{:.1}", beat.grid.bpm),
            });
        }

        dsl.provenance = Some(DslProvenance {
            orchestrator: Some(self.name().to_string()),
            // Recorded as absent rather than as a mock adapter's name: this
            // strategy is rule-based and consulted no model at all, and
            // provenance that claimed otherwise would be a lie.
            inference_adapter: None,
            model: None,
            template_id: Some(template.id.clone()),
            priors_applied,
            params_hash: None,
            completed_at: Some(vb_contracts::now_rfc3339()),
            ..DslProvenance::default()
        });

        Ok(dsl)
    }
}

impl MontageOrchestrator {
    /// A new DSL for this footage and template: its title, frame, rate and
    /// provenance of the graph it was decided against.
    fn start(graph: &FootageGraph, template: &Template) -> EditingDsl {
        // The caller's name for the piece, when it has one: a project called
        // "Kyoto in spring" should not open on a card reading "Travel diary
        // rough cut". It arrives through the template's extras because a
        // template is the only thing this layer is handed.
        let title = template
            .params
            .extra
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|t| !t.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{} rough cut", template.name));
        let mut dsl = EditingDsl::new(title);
        dsl.meta.template_id = Some(template.id.clone());
        dsl.meta.variant_id = Some("v1".into());
        // The first video's rate: a photograph has none.
        if let Some(src) = graph
            .sources
            .iter()
            .find(|s| !s.is_still())
            .or(graph.sources.first())
        {
            dsl.meta.fps_num = src.fps_num;
            dsl.meta.fps_den = src.fps_den;
        }
        // Said, so an editor that takes its frame from whatever clip comes
        // first can be given this one instead.
        dsl.meta.aspect_ratio = Self::frame(graph).map(|(w, h)| {
            let (mut a, mut b) = (w, h);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            format!("{}:{}", w / a, h / a)
        });
        if let Some(pid) = &graph.project_id {
            dsl.footage_graph_ref = Some(FootageGraphRef {
                project_id: pid.clone(),
                graph_hash: None,
            });
        }
        dsl
    }

    /// Open the cut on a card with its title, as slot 0.
    fn open_on_title(dsl: &mut EditingDsl, template: &Template, duration: Micros, chinese: bool) {
        let id = "slot-title".to_string();
        dsl.slots.push(Slot {
            id: id.clone(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: duration,
            source_ref: None,
            speed: None,
            reason: Some(if chinese {
                format!("风格 {} 以标题卡开场。", template.id)
            } else {
                format!("Template {} opens on a title card.", template.id)
            }),
            score: None,
        });
        dsl.tracks.text.push(TextItem {
            id: "txt-title".into(),
            anchor: Anchor::at(&id),
            duration_us: duration,
            content: dsl.meta.title.clone(),
            role: Some(TextRole::Title),
            transcript_segment_id: None,
            style: Some(TextStyle {
                font_size_pt: Some(48.0),
                alignment: Some("middleCenter".into()),
                ..Self::subtitle_style()
            }),
        });
    }

    /// A 主体衔接 cut: the global全套动作 the footage holds, played once
    /// with every join matched (see [`crate::continuity`]), and what became
    /// of the action -- its speed, how much of it is shown, which joins
    /// carry it.
    pub fn compose_continuity(
        graph: &FootageGraph,
        template: &Template,
    ) -> Result<(EditingDsl, continuity::Report), NarrativeError> {
        if graph.shots.is_empty() {
            return Err(NarrativeError::NoFootage);
        }
        let p = &template.params;
        // On the beat: the music's, when the user chose some; else the one
        // the dancers keep; else whatever any beat cut follows.
        let music_chosen = graph
            .priors
            .as_ref()
            .and_then(|g| g.beat_grid.as_ref())
            .is_some_and(|g| g.asset_path.is_some());
        let beat = if p.beat_align && !music_chosen {
            Beat::of_dancing(graph).or_else(|| Beat::for_cut(graph, template))
        } else {
            Beat::for_cut(graph, template)
        };
        let title_us = match (&beat, p.title_card) {
            (_, false) => Micros::ZERO,
            (Some(beat), true) => beat.quantise(p.title_card_duration_us).0,
            (None, true) => p.title_card_duration_us,
        };
        let (clip, min_clip) = match &beat {
            Some(b) => {
                let len = b.length.0.max(1);
                let fewest = p.min_slot_us.0.div_ceil(len).max(1);
                let most = (p.max_slot_us.0 / len).max(fewest);
                // Four beats a take: long enough to see the place, short
                // enough that the action moves on between places.
                (Micros(4u64.clamp(fewest, most) * len), Micros(fewest * len))
            }
            None => (
                Micros((p.min_slot_us.0 + p.max_slot_us.0) / 2),
                p.min_slot_us,
            ),
        };
        // No length chosen: the action's own.
        let automatic = p
            .extra
            .get("autoLength")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let room = (!automatic).then(|| p.target_duration_us.saturating_sub(title_us));
        let frame = Self::frame(graph);
        let project_tall = frame.is_some_and(|(w, h)| h > w);
        let quality = |i: usize, j: usize| -> f64 {
            let shot = &graph.shots[i];
            let segment = &shot.segments[j];
            let turned = graph
                .source(&shot.source_id)
                .and_then(|s| s.display_size())
                .is_some_and(|(w, h)| (h > w) != project_tall);
            segment.quality.unwrap_or_else(|| shot.score())
                * if turned { 0.6 } else { 1.0 }
                * clear_view(segment.occlusion)
                * composed(segment.composition)
        };
        let params = continuity::Params {
            canvas: frame.unwrap_or((1920, 1080)),
            beat: beat.as_ref().map(|b| b.length),
            clip,
            min_clip,
            room,
        };
        let plan = continuity::plan(graph, &params, &quality).ok_or(NarrativeError::NoAction)?;
        let chinese = Self::chinese(template);

        let mut dsl = Self::start(graph, template);
        dsl.meta.target_duration_us = Some(plan.report.room + title_us);
        let mut order = 0u32;
        if p.title_card {
            Self::open_on_title(&mut dsl, template, title_us, chinese);
            order += 1;
        }
        let mut previous: Option<String> = None;
        let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for clip in &plan.clips {
            let shot = &graph.shots[clip.shot];
            let segment = &shot.segments[clip.segment];
            let base = format!("slot-{}", segment.id);
            let mut id = base.clone();
            let mut n = 2;
            while !ids.insert(id.clone()) {
                id = format!("{base}-{n}");
                n += 1;
            }
            let used = Micros((clip.len.0 as f64 * clip.speed) as u64);
            let speed = ((clip.speed - 1.0).abs() > 1e-9).then_some(clip.speed);
            dsl.slots.push(Slot {
                id: id.clone(),
                order,
                kind: SlotKind::Clip,
                duration_us: clip.len,
                source_ref: Some(SourceRef {
                    source_id: shot.source_id.clone(),
                    shot_id: Some(shot.id.clone()),
                    in_us: clip.in_us,
                    out_us: clip.in_us + used,
                }),
                speed,
                reason: Some(continuity_reason(
                    graph,
                    clip,
                    plan.report.pieces,
                    plan.report.speeds.get(clip.piece).copied(),
                    beat.as_ref(),
                    chinese,
                )),
                score: Some(clip.quality.min(1.0)),
            });
            dsl.tracks.video.push(VideoItem {
                slot_id: id.clone(),
                crop: None,
                filters: Vec::new(),
                transform: clip.transform.clone(),
                background: None,
            });
            // Every join is a cut: a match cut carries the movement, and a
            // dissolve would smear the one moment it was placed on.
            if let Some(prev) = previous.take() {
                dsl.transitions.push(Transition {
                    from_slot_id: prev,
                    to_slot_id: id.clone(),
                    kind: TransitionType::Cut,
                    duration_us: Micros::ZERO,
                });
            }
            previous = Some(id);
            order += 1;
        }
        // Music shorter than the cut plays again from its first beat when
        // the user chose so, and otherwise leaves the end silent.
        let looped = p
            .extra
            .get("musicLoop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        lay_music(&mut dsl, graph, looped);

        let speeds: Vec<String> = plan
            .report
            .speeds
            .iter()
            .map(|s| format!("{:.4}", s.speed))
            .collect();
        let mut priors_applied = vec![
            format!("continuity.actions@{}", plan.report.actions),
            format!("continuity.pieces@{}", plan.report.pieces),
            format!("continuity.speeds@{}", speeds.join(",")),
            format!("continuity.shown@{:.3}", plan.report.shown),
        ];
        if let Some(beat) = &beat {
            priors_applied.push(match (beat.from_motion, beat.grid.origin) {
                (true, _) => format!("beatGrid.motion@{:.1}", beat.grid.bpm),
                (_, Some(BeatOrigin::Music)) => format!("beatGrid.music@{:.1}", beat.grid.bpm),
                (_, Some(BeatOrigin::Source)) => format!("beatGrid.source@{:.1}", beat.grid.bpm),
                _ => format!("beatGrid.fixed@{:.1}", beat.grid.bpm),
            });
        }
        dsl.provenance = Some(DslProvenance {
            orchestrator: Some(MontageOrchestrator.name().to_string()),
            // Rules on the graph's numbers; no model was consulted here.
            inference_adapter: None,
            model: None,
            template_id: Some(template.id.clone()),
            priors_applied,
            params_hash: None,
            completed_at: Some(vb_contracts::now_rfc3339()),
            ..DslProvenance::default()
        });
        Ok((dsl, plan.report))
    }
}

/// Compose a 主体衔接 cut and verify it, like [`compose_grounded`], keeping
/// what became of the action for the run to report.
pub fn compose_continuity_grounded(
    graph: &FootageGraph,
    template: &Template,
) -> Result<(Composition, continuity::Report), NarrativeError> {
    let (mut dsl, report) = MontageOrchestrator::compose_continuity(graph, template)?;
    let grounding = ground(&mut dsl, graph);
    Ok((Composition { dsl, grounding }, report))
}

/// Why a clip of a 主体衔接 cut is where it is, in the caller's language.
fn continuity_reason(
    graph: &FootageGraph,
    clip: &continuity::PlannedClip,
    pieces: usize,
    pace: Option<continuity::PieceSpeed>,
    beat: Option<&Beat>,
    chinese: bool,
) -> String {
    let speed = clip.speed;
    let segment = &graph.shots[clip.shot].segments[clip.segment];
    let scene = segment
        .scene_id
        .as_ref()
        .and_then(|id| graph.scenes.iter().position(|s| &s.id == id));
    let label = scene.and_then(|i| graph.scenes[i].label.clone());
    let used = Micros((clip.len.0 as f64 * speed) as u64);
    let span = format!(
        "{:.1}-{:.1}s",
        clip.in_us.as_secs_f64(),
        (clip.in_us + used).as_secs_f64()
    );
    let duration = clip.len;
    let beats = beat.map(|b| (b, duration.0 / b.length.0.max(1)));
    let framed = clip
        .join
        .filter(|j| j.scale > 1.005 || j.turn_deg.abs() > 0.2);
    if chinese {
        let what = match (scene, &label) {
            (Some(i), Some(l)) => format!("场景 {}（{l}）", i + 1),
            (Some(i), None) => format!("场景 {}", i + 1),
            _ => "这个镜头".to_string(),
        };
        let mut text = if pieces > 1 {
            format!(
                "主体衔接：第 {}/{pieces} 段动作，取自{what}",
                clip.piece + 1
            )
        } else {
            format!("主体衔接：取自{what}")
        };
        text.push_str(&format!("，{} 人的动作", clip.people));
        match clip.join {
            None => text.push_str("，动作从这里开始"),
            Some(j) if j.carried && j.same_moment => text.push_str(&format!(
                "；与上一段是同一时刻的动作，只换了场景（衔接差 {:.2}）",
                j.cost
            )),
            Some(j) if j.carried => {
                text.push_str(&format!("；动作接着上一段（衔接差 {:.2}）", j.cost))
            }
            Some(j) if j.cost < continuity::MISMATCH => text.push_str(&format!(
                "；与上一段的动作没有完全接上（衔接差 {:.2}）",
                j.cost
            )),
            Some(_) => text.push_str("；换到另一段动作"),
        }
        if let Some(j) = framed {
            text.push_str(&format!("，画面放大到 {:.2} 倍", j.scale));
            if j.turn_deg.abs() > 0.2 {
                text.push_str(&format!("、转 {:.1}°", j.turn_deg));
            }
            text.push_str("，主体的大小和位置对上上一段");
        }
        text.push_str(&format!("；取素材 {span}"));
        match pace {
            Some(p) if p.on_beat => text.push_str(&format!(
                "，舞蹈自己的 {:.0} BPM{}，舞步落在拍上",
                p.dance_bpm.unwrap_or(0.0),
                if (speed - 1.0).abs() > 1e-9 {
                    format!("按 {speed:.2} 倍速")
                } else {
                    String::new()
                }
            )),
            Some(p) if p.dance_bpm.is_some() && beat.is_some() => text.push_str(&format!(
                "，舞蹈自己的 {:.0} BPM 与剪辑节拍相差超过 1.2 倍变速能补的范围，舞步不在拍上",
                p.dance_bpm.unwrap_or(0.0)
            )),
            _ if (speed - 1.0).abs() > 1e-9 => text.push_str(&format!("，{speed:.2} 倍速")),
            _ => {}
        }
        match beats {
            Some((beat, n)) => text.push_str(&format!(
                "；踩着{}保持 {n} 拍（{duration}）。",
                beat.describe(true)
            )),
            None => text.push_str(&format!("；时长 {duration}。")),
        }
        text
    } else {
        let what = match (scene, &label) {
            (Some(i), Some(l)) => format!("scene {} ({l})", i + 1),
            (Some(i), None) => format!("scene {}", i + 1),
            _ => "this shot".to_string(),
        };
        let mut text = if pieces > 1 {
            format!(
                "Subject continuity: part {} of {pieces} of the action, from {what}",
                clip.piece + 1
            )
        } else {
            format!("Subject continuity: from {what}")
        };
        text.push_str(&format!(
            ", {} {} acting",
            clip.people,
            if clip.people == 1 { "person" } else { "people" }
        ));
        match clip.join {
            None => text.push_str(", where the action begins"),
            Some(j) if j.carried && j.same_moment => text.push_str(&format!(
                "; the same moment of the action as the clip before, somewhere else \
                 (join {:.2})",
                j.cost
            )),
            Some(j) if j.carried => text.push_str(&format!(
                "; the movement carries on from the clip before (join {:.2})",
                j.cost
            )),
            Some(j) if j.cost < continuity::MISMATCH => text.push_str(&format!(
                "; not quite joining the movement of the clip before (join {:.2})",
                j.cost
            )),
            Some(_) => text.push_str("; on to another action"),
        }
        if let Some(j) = framed {
            text.push_str(&format!(", enlarged to {:.2}x", j.scale));
            if j.turn_deg.abs() > 0.2 {
                text.push_str(&format!(" and turned {:.1} degrees", j.turn_deg));
            }
            text.push_str(" so the subject matches the clip before in size and place");
        }
        text.push_str(&format!("; source {span}"));
        match pace {
            Some(p) if p.on_beat => text.push_str(&format!(
                ", the dance's own {:.0} BPM{} so its steps land on the beat",
                p.dance_bpm.unwrap_or(0.0),
                if (speed - 1.0).abs() > 1e-9 {
                    format!(" played at {speed:.2}x")
                } else {
                    String::new()
                }
            )),
            Some(p) if p.dance_bpm.is_some() && beat.is_some() => text.push_str(&format!(
                ", the dance's own {:.0} BPM is further from the cut's beat than a 1.2x change \
                 can bring it, so its steps are off the beat",
                p.dance_bpm.unwrap_or(0.0)
            )),
            _ if (speed - 1.0).abs() > 1e-9 => text.push_str(&format!(" at {speed:.2}x")),
            _ => {}
        }
        match beats {
            Some((beat, n)) => text.push_str(&format!(
                "; held for {n} beat{} ({duration}) on {}.",
                if n == 1 { "" } else { "s" },
                beat.describe(false)
            )),
            None => text.push_str(&format!("; runs {duration}.")),
        }
        text
    }
}

/// Lay the music the user chose under the whole cut, from its first beat,
/// so the cuts placed on its beats land on them: once, or, when `looped`
/// and the cut runs longer, again from its first beat in whole beats, so
/// the grid the cuts sit on runs straight through the join.
fn lay_music(dsl: &mut EditingDsl, graph: &FootageGraph, looped: bool) {
    let Some(music) = graph
        .priors
        .as_ref()
        .and_then(|p| p.beat_grid.as_ref())
        .filter(|g| g.asset_path.is_some())
    else {
        return;
    };
    let timeline = dsl.resolve_timeline();
    let total = timeline.total_duration_us;
    let start = music.first_beat_us.unwrap_or(Micros::ZERO);
    let available = music
        .asset_duration_us
        .map(|d| d.saturating_sub(start))
        .unwrap_or(total);
    // A song with no beat to count loops whole.
    let pass = match (looped && available < total, music.beat_us()) {
        (false, _) => available,
        (true, Some(beat)) if beat.0 > 0 && available.0 >= beat.0 => {
            Micros(available.0 / beat.0 * beat.0)
        }
        (true, _) => available,
    };
    let mut at = Micros::ZERO;
    let mut n = 0usize;
    while at < total && pass > Micros::ZERO {
        let length = Micros(pass.0.min(total.0 - at.0));
        let last = at + length >= total || !looped;
        // Anchored to the slot playing when this pass begins, never to an
        // absolute time on the finished timeline.
        let Some(slot) = timeline
            .slots
            .iter()
            .find(|s| s.start_us <= at && at < s.end_us)
            .or_else(|| timeline.slots.first())
        else {
            break;
        };
        dsl.tracks.audio.push(AudioItem {
            id: if n == 0 {
                "aud-music".into()
            } else {
                format!("aud-music-{}", n + 1)
            },
            anchor: Anchor {
                slot_id: slot.slot_id.clone(),
                offset_us: (at > slot.start_us).then(|| (at.0 - slot.start_us.0) as i64),
            },
            kind: AudioKind::Bgm,
            duration_us: Some(length),
            asset_path: music.asset_path.clone(),
            asset_in_us: Some(start),
            gain_db: None,
            muted: None,
            fade_in_us: None,
            fade_out_us: last.then(|| Micros(length.0.min(1_500_000))),
            ducking: None,
        });
        n += 1;
        at = at + length;
        if !looped {
            break;
        }
    }
}

/// What a segment's composition does to its quality.
///
/// The composition model rates the moments shortlisted for a cut, 1 to 5,
/// mapped onto 0..1. A middling frame keeps its quality; a well-composed one
/// gains up to a fifth, a badly composed one loses up to two fifths -- more
/// taken than given, because on real footage the model is surer of what is
/// wrong than of what is fine. Unjudged segments are left alone.
fn composed(composition: Option<f64>) -> f64 {
    composition.map_or(1.0, |c| (0.5 + c.clamp(0.0, 1.0)).clamp(0.6, 1.2))
}

/// How much of a segment's quality survives what blocks its view.
///
/// The director reads occlusion as the share of the picture something in
/// front of the scene covers. A tenth of the frame -- a mirror, a strip of
/// dashboard -- costs a third; a quarter -- a door, a passing sleeve -- three
/// quarters; half the frame, everything. Squared, because a small intrusion
/// is forgivable and a large one is not a picture of the scene at all. No
/// reading, when no depth model ran, costs nothing: absence is not evidence.
fn clear_view(occlusion: Option<f64>) -> f64 {
    occlusion.map_or(1.0, |o| (1.0 - 2.0 * o.clamp(0.0, 1.0)).max(0.0).powi(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn continuity_template(seconds: Option<u64>) -> Template {
        let mut t = PriorStore::with_builtins()
            .get("content.subject-continuity")
            .expect("a built-in 主体衔接 template")
            .clone();
        t.params
            .extra
            .insert("locale".into(), serde_json::Value::String("zh".into()));
        match seconds {
            Some(s) => t.params.target_duration_us = Micros::from_secs(s),
            None => {
                t.params
                    .extra
                    .insert("autoLength".into(), serde_json::Value::Bool(true));
            }
        }
        t
    }

    #[test]
    fn a_continuity_cut_plays_the_action_through_on_the_beat_and_grounds_cleanly() {
        let takes = crate::continuity::tests::three_takes();
        let g = crate::continuity::tests::graph(&takes);
        // About fifty seconds of action into forty, on the beat: shortened,
        // never sped off the beat.
        let Composition { dsl, grounding } =
            compose_grounded(&MontageOrchestrator, &g, &continuity_template(Some(40))).unwrap();
        assert!(grounding.is_clean(), "{:?}", grounding.rejected);
        vb_contracts::validate::validate_editing_dsl(&dsl).unwrap();
        assert!(dsl.slots.iter().all(|s| s.speed.is_none()));
        assert_eq!(
            dsl.resolve_timeline().total_duration_us,
            Micros::from_secs(40)
        );
        // No music and no beat in the footage: the template's steady grid,
        // every clip a whole number of its beats, every join a cut.
        for slot in &dsl.slots {
            assert_eq!(slot.duration_us.0 % 500_000, 0, "{}", slot.duration_us);
        }
        assert!(
            dsl.transitions
                .iter()
                .all(|t| t.kind == TransitionType::Cut)
        );
        assert!(dsl.resolve_timeline().total_duration_us <= Micros::from_secs(40));
        // The pictures are framed to match, and every clip says why.
        assert!(dsl.tracks.video.iter().any(|v| v.transform.is_some()));
        for slot in &dsl.slots {
            assert!(slot.reason.as_deref().unwrap_or("").starts_with("主体衔接"));
        }
    }

    /// The three takes' dancers keep `bpm`, as the rhythm reading records
    /// it.
    fn dancing_at(g: &mut FootageGraph, bpm: f64) {
        let takes = crate::continuity::tests::three_takes();
        crate::continuity::tests::keep_beat(g, &takes, bpm, &[]);
    }

    #[test]
    fn with_no_music_a_dance_is_cut_to_its_own_beat_at_its_own_speed() {
        let takes = crate::continuity::tests::three_takes();
        let mut g = crate::continuity::tests::graph(&takes);
        dancing_at(&mut g, 104.0);
        let (dsl, report) =
            MontageOrchestrator::compose_continuity(&g, &continuity_template(None)).unwrap();
        // The grid is the dancers' own 104 BPM, so nothing needs a speed.
        let beat = (60_000_000.0f64 / 104.0).round() as u64;
        for slot in &dsl.slots {
            assert!(slot.duration_us.0 % beat <= 1, "{}", slot.duration_us);
            assert!(slot.speed.is_none());
        }
        assert!(report.speeds.iter().all(|s| s.on_beat));
        let provenance = dsl.provenance.unwrap();
        assert!(
            provenance
                .priors_applied
                .iter()
                .any(|p| p == "beatGrid.motion@104.0"),
            "{:?}",
            provenance.priors_applied
        );
        assert!(
            dsl.slots[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("舞步落在拍上")
        );
    }

    #[test]
    fn music_shorter_than_a_continuity_cut_plays_again_only_when_asked() {
        let takes = crate::continuity::tests::three_takes();
        let mut g = crate::continuity::tests::graph(&takes);
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 120.0,
                first_beat_us: Some(Micros::ZERO),
                confidence: Some(0.9),
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/short.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(20)),
            }),
            ..Default::default()
        });
        let music = |dsl: &EditingDsl| -> u64 {
            dsl.tracks
                .audio
                .iter()
                .filter(|a| a.kind == AudioKind::Bgm)
                .map(|a| a.duration_us.unwrap().0)
                .sum()
        };
        // Not asked: the music plays once, and the rest of the action is
        // silent rather than cut short.
        let (dsl, _) =
            MontageOrchestrator::compose_continuity(&g, &continuity_template(None)).unwrap();
        let total = dsl.resolve_timeline().total_duration_us;
        assert!(total > Micros::from_secs(40), "{total}");
        assert_eq!(music(&dsl), 20_000_000);
        // Asked: it plays again from its first beat to the end.
        let mut looped = continuity_template(None);
        looped
            .params
            .extra
            .insert("musicLoop".into(), serde_json::Value::Bool(true));
        let (dsl, _) = MontageOrchestrator::compose_continuity(&g, &looped).unwrap();
        assert_eq!(music(&dsl), dsl.resolve_timeline().total_duration_us.0);
    }

    #[test]
    fn with_no_length_chosen_a_continuity_cut_is_as_long_as_its_action() {
        let takes = crate::continuity::tests::three_takes();
        let g = crate::continuity::tests::graph(&takes);
        let (dsl, report) =
            MontageOrchestrator::compose_continuity(&g, &continuity_template(None)).unwrap();
        assert!(report.speeds.iter().all(|s| (s.speed - 1.0).abs() < 1e-12));
        assert!(dsl.slots.iter().all(|s| s.speed.is_none()));
        let total = dsl.resolve_timeline().total_duration_us;
        assert!(
            total + Micros(500_000) >= report.natural,
            "{total} of {}",
            report.natural
        );
        assert!(total <= report.natural);
    }

    #[test]
    fn the_frame_is_the_way_most_of_the_footage_is_filmed() {
        let source = |id: &str, rotation: u32, secs: u64| Source {
            id: id.into(),
            path: format!("/{id}.mp4"),
            content_hash: Blake3Hash::of_str(id),
            media_kind: None,
            duration_us: Micros::from_secs(secs),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: None,
            audio_codec: None,
            has_audio: Some(true),
            created_at: None,
            rotation: Some(rotation),
            categories: Vec::new(),
        };
        let mut graph = FootageGraph::new("test");
        // One long landscape take, and two portrait ones stored on their side.
        graph.sources = vec![
            source("a", 0, 60),
            source("b", 270, 20),
            source("c", 90, 20),
        ];
        assert_eq!(MontageOrchestrator::frame(&graph), Some((1920, 1080)));
        graph.sources[0].duration_us = Micros::from_secs(30);
        assert_eq!(MontageOrchestrator::frame(&graph), Some((1080, 1920)));
        graph.sources.clear();
        assert_eq!(MontageOrchestrator::frame(&graph), None);
    }
    use crate::prior::PriorStore;
    use vb_contracts::Blake3Hash;
    use vb_contracts::footage_graph::{Keyframe, KeyframeScores, Source, TranscriptSegment};

    fn shot(id: &str, start_s: u64, end_s: u64, score: f64) -> Shot {
        Shot {
            id: id.into(),
            source_id: "src".into(),
            start_us: Micros::from_secs(start_s),
            end_us: Micros::from_secs(end_s),
            shot_change_confidence: None,
            keyframes: vec![Keyframe {
                id: format!("kf-{id}"),
                at_us: Micros::from_secs((start_s + end_s) / 2),
                path: None,
                scores: Some(KeyframeScores {
                    aggregate: Some(score),
                    ..Default::default()
                }),
            }],
            description: None,
            entity_refs: vec![],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: Vec::new(),
            timeline: Vec::new(),
        }
    }

    /// The same shot, but belonging to a named source.
    fn shot_from(source: &str, id: &str, start_s: u64, end_s: u64, score: f64) -> Shot {
        Shot {
            source_id: source.into(),
            ..shot(id, start_s, end_s, score)
        }
    }

    fn graph(shots: Vec<Shot>) -> FootageGraph {
        let end = shots.iter().map(|s| s.end_us.0).max().unwrap_or(0);
        let mut g = FootageGraph::new("test");
        g.sources.push(Source {
            id: "src".into(),
            path: "/x.mp4".into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros(end),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: None,
            audio_codec: None,
            has_audio: Some(true),
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        });
        g.shots = shots;
        g
    }

    fn template() -> Template {
        PriorStore::with_builtins()
            .get("rhythm.upbeat-montage")
            .cloned()
            .expect("built-in template must exist")
    }

    #[test]
    fn composing_empty_footage_fails_rather_than_emitting_an_empty_cut() {
        let err = MontageOrchestrator
            .compose(&FootageGraph::new("e"), &template())
            .unwrap_err();
        assert!(matches!(err, NarrativeError::NoFootage));
    }

    #[test]
    fn the_composed_cut_grounds_cleanly() {
        // The orchestrator must not produce anything its own gate rejects.
        let g = graph(vec![
            shot("s1", 0, 10, 0.9),
            shot("s2", 10, 20, 0.5),
            shot("s3", 20, 30, 0.7),
        ]);
        let c = compose_grounded(&MontageOrchestrator, &g, &template()).unwrap();
        assert!(
            c.grounding.is_clean(),
            "orchestrator produced slots its own gate rejected: {:?}",
            c.grounding.rejected
        );
        assert!(!c.dsl.slots.is_empty());
    }

    #[test]
    fn higher_scoring_shots_are_preferred() {
        let t = Template {
            params: crate::prior::TemplateParams {
                target_duration_us: Micros::from_secs(4),
                min_slot_us: Micros::from_secs(2),
                max_slot_us: Micros::from_secs(3),
                title_card: false,
                ..template().params
            },
            ..template()
        };
        let g = graph(vec![shot("low", 0, 10, 0.1), shot("high", 10, 20, 0.95)]);
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        assert!(
            dsl.slots.iter().any(|s| s.id.contains("high")),
            "the strongest shot was not selected"
        );
    }

    #[test]
    fn a_year_in_review_runs_people_then_scenery_then_food_then_the_rest() {
        // Filmed in this order: food, scenery, a person, something else,
        // the person again, a dish with someone at the table.
        let filmed = [
            ("a", vec!["food"], "2025-01-05T10:00:00Z"),
            ("b", vec!["scenery"], "2025-03-01T10:00:00Z"),
            ("c", vec!["people"], "2025-05-01T10:00:00Z"),
            ("d", vec![], "2025-07-01T10:00:00Z"),
            ("e", vec!["people", "scenery"], "2025-09-01T10:00:00Z"),
            ("f", vec!["food", "people"], "2025-11-01T10:00:00Z"),
        ];
        let mut g = FootageGraph::new("year");
        for (i, (id, categories, at)) in filmed.iter().enumerate() {
            g.sources.push(Source {
                id: (*id).into(),
                path: format!("/{id}.mp4"),
                content_hash: Blake3Hash::of_str(id),
                media_kind: None,
                duration_us: Micros::from_secs(10),
                fps_num: Some(30),
                fps_den: Some(1),
                width: Some(1920),
                height: Some(1080),
                video_codec: None,
                audio_codec: None,
                has_audio: Some(true),
                created_at: Some((*at).into()),
                rotation: None,
                categories: categories.iter().map(|c| c.to_string()).collect(),
            });
            g.shots.push(shot_from(
                id,
                &format!("{id}-0"),
                0,
                10,
                0.5 + i as f64 * 0.05,
            ));
        }
        let mut t = PriorStore::with_builtins()
            .get("content.annual-review")
            .cloned()
            .unwrap();
        t.params.title_card = false;
        let order = |t: &Template| -> Vec<String> {
            MontageOrchestrator
                .compose(&g, t)
                .unwrap()
                .ordered_slots()
                .iter()
                .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.clone()))
                .collect()
        };
        assert_eq!(order(&t), ["c", "e", "f", "b", "a", "d"]);

        // Ordered by time instead, it is the year as it was filmed.
        t.params.extra.remove("order");
        assert_eq!(order(&t), ["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn selected_clips_stay_in_chronological_order() {
        // Reordering footage changes what it means, so a rough cut must not.
        let g = graph(vec![
            shot("s1", 0, 10, 0.2),
            shot("s2", 10, 20, 0.9),
            shot("s3", 20, 30, 0.5),
        ]);
        let dsl = MontageOrchestrator.compose(&g, &template()).unwrap();
        let starts: Vec<u64> = dsl
            .ordered_slots()
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.in_us.0))
            .collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted, "clips are out of chronological order");
    }

    #[test]
    fn every_clip_respects_the_templates_duration_bounds() {
        let t = template();
        let g = graph(vec![shot("long", 0, 60, 0.9), shot("s2", 60, 70, 0.8)]);
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        for slot in dsl.slots.iter().filter(|s| s.kind == SlotKind::Clip) {
            assert!(
                slot.duration_us <= t.params.max_slot_us,
                "slot {} runs {}, over the template maximum",
                slot.id,
                slot.duration_us
            );
            assert!(slot.duration_us >= t.params.min_slot_us);
        }
    }

    #[test]
    fn a_placed_clip_stays_inside_its_shot() {
        // A window that escaped the shot would be rejected by grounding, so
        // this is the invariant that keeps the orchestrator self-consistent.
        let t = template();
        for (start, end) in [(0u64, 2u64), (5, 40), (100, 101)] {
            let s = shot("x", start, end, 0.5);
            let g = graph(vec![s.clone()]);
            let moments = MontageOrchestrator::moments(&g, &t);
            let m = &moments[0];
            let len = Micros(m.len().0.min(t.params.max_slot_us.0));
            let in_us = MontageOrchestrator::place(m, len, false);
            assert!(in_us >= s.start_us, "in point escaped the shot");
            assert!(in_us + len <= s.end_us, "out point escaped the shot");
        }
    }

    #[test]
    fn subtitles_anchor_relative_to_their_slot_not_the_source() {
        let mut t = template();
        t.params.subtitles = true;
        t.params.extra.insert("speechSubtitles".into(), true.into());
        t.params.title_card = false;
        t.params.min_slot_us = Micros::from_secs(2);
        t.params.max_slot_us = Micros::from_secs(10);

        let mut g = graph(vec![shot("s1", 20, 30, 0.9)]);
        g.transcript.push(TranscriptSegment {
            id: "tr-1".into(),
            source_id: "src".into(),
            start_us: Micros::from_secs(26),
            end_us: Micros::from_secs(28),
            text: "spoken here".into(),
            speaker: None,
            language: None,
            confidence: None,
        });

        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let sub = dsl
            .tracks
            .text
            .iter()
            .find(|x| x.transcript_segment_id.as_deref() == Some("tr-1"))
            .expect("the overlapping transcript segment should become a subtitle");

        let slot = dsl.slot(&sub.anchor.slot_id).unwrap();
        let in_us = slot.source_ref.as_ref().unwrap().in_us;
        // The offset is measured from the slot's own start, so it equals the
        // segment's position within the trimmed range, not its source time.
        assert_eq!(
            sub.anchor.offset(),
            (Micros::from_secs(26).0 - in_us.0) as i64
        );
        assert!(sub.anchor.offset() < Micros::from_secs(20).0 as i64);
    }

    #[test]
    fn what_was_said_stays_off_the_screen_unless_the_template_asks() {
        let mut t = template();
        t.params.subtitles = true;
        t.params.title_card = false;
        let mut g = graph(vec![shot("s1", 20, 30, 0.9)]);
        g.transcript.push(TranscriptSegment {
            id: "tr-1".into(),
            source_id: "src".into(),
            start_us: Micros::from_secs(21),
            end_us: Micros::from_secs(29),
            text: "纪利闆兵:难得疯了".into(),
            speaker: None,
            language: None,
            confidence: None,
        });
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        assert!(
            dsl.tracks
                .text
                .iter()
                .all(|x| x.transcript_segment_id.is_none()),
            "{:?}",
            dsl.tracks.text
        );
    }

    #[test]
    fn a_described_moment_carries_its_caption_where_the_plan_puts_one() {
        let mut shot = swept(
            "src-0001",
            &[("lake", 0, 10, 0.9, 0.2), ("street", 10, 20, 0.8, 0.6)],
        );
        for seg in &mut shot.segments {
            seg.caption = Some(vb_contracts::footage_graph::Description {
                text: format!("{} 的画面", seg.scene_id.clone().unwrap()),
                model: Some("Qwen3-VL-8B".into()),
                confidence: None,
            });
        }
        let g = swept_graph(vec![shot]);
        let mut t = short_cut(12);
        t.params.subtitles = true;
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let captions: Vec<&TextItem> = dsl
            .tracks
            .text
            .iter()
            .filter(|x| x.role == Some(TextRole::Caption))
            .collect();
        let plan = crate::caption_plan(&dsl);
        assert!(!plan.is_empty(), "a 12 s cut has room for a caption");
        assert_eq!(captions.len(), plan.len());
        for (caption, place) in captions.iter().zip(&plan) {
            assert_eq!(caption.anchor.slot_id, place.slot_id);
            assert_eq!(caption.anchor.offset(), place.offset_us.0 as i64);
            assert!(caption.content.ends_with("的画面"));
        }

        t.params.subtitles = false;
        let quiet = MontageOrchestrator.compose(&g, &t).unwrap();
        assert!(
            quiet
                .tracks
                .text
                .iter()
                .all(|x| x.role != Some(TextRole::Caption))
        );
    }

    #[test]
    fn a_transcript_segment_outside_the_selected_range_produces_no_subtitle() {
        let mut t = template();
        t.params.subtitles = true;
        t.params.title_card = false;
        let mut g = graph(vec![shot("s1", 20, 30, 0.9)]);
        g.transcript.push(TranscriptSegment {
            id: "tr-far".into(),
            source_id: "src".into(),
            start_us: Micros::from_secs(100),
            end_us: Micros::from_secs(102),
            text: "not in the cut".into(),
            speaker: None,
            language: None,
            confidence: None,
        });
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        assert!(
            !dsl.tracks
                .text
                .iter()
                .any(|x| x.transcript_segment_id.as_deref() == Some("tr-far"))
        );
    }

    #[test]
    fn provenance_does_not_claim_a_model_was_consulted() {
        let g = graph(vec![shot("s1", 0, 10, 0.9)]);
        let dsl = MontageOrchestrator.compose(&g, &template()).unwrap();
        let p = dsl.provenance.unwrap();
        assert_eq!(p.orchestrator.as_deref(), Some("montage-v1"));
        assert!(
            p.inference_adapter.is_none(),
            "a rule-based orchestrator must not record an inference adapter"
        );
    }

    #[test]
    fn a_title_card_template_opens_on_a_title_card() {
        let dsl = MontageOrchestrator
            .compose(&graph(vec![shot("s1", 0, 10, 0.9)]), &template())
            .unwrap();
        assert_eq!(dsl.ordered_slots()[0].kind, SlotKind::TitleCard);
    }

    /// A project of several clips must not become a cut of one of them.
    #[test]
    fn every_source_is_represented_before_any_source_repeats() {
        // One clip holds the four best shots; the other two are weaker but
        // perfectly usable. Ranking alone would spend the whole budget on the
        // first clip and silently drop the other two.
        let mut shots = vec![
            shot_from("src-0001", "a0", 0, 8, 0.99),
            shot_from("src-0001", "a1", 8, 16, 0.98),
            shot_from("src-0001", "a2", 16, 24, 0.97),
            shot_from("src-0001", "a3", 24, 32, 0.96),
            shot_from("src-0002", "b0", 0, 8, 0.50),
            shot_from("src-0003", "c0", 0, 8, 0.40),
        ];
        shots.sort_by(|a, b| a.id.cmp(&b.id));
        let mut g = graph(shots);
        g.sources = vec!["src-0001", "src-0002", "src-0003"]
            .into_iter()
            .map(|id| Source {
                id: id.into(),
                path: format!("/{id}.mp4"),
                content_hash: Blake3Hash::of_str(id),
                media_kind: None,
                duration_us: Micros::from_secs(32),
                fps_num: Some(30),
                fps_den: Some(1),
                width: Some(1920),
                height: Some(1080),
                video_codec: None,
                audio_codec: None,
                has_audio: Some(false),
                created_at: None,
                rotation: None,
                categories: Vec::new(),
            })
            .collect();

        let store = PriorStore::with_builtins();
        let template = store.get("rhythm.calm-observational").unwrap();
        let dsl = MontageOrchestrator.compose(&g, template).unwrap();

        let used: std::collections::HashSet<&str> = dsl
            .slots
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        assert_eq!(
            used.len(),
            3,
            "every clip in the project should appear in the cut, got {used:?}"
        );
    }

    #[test]
    fn a_cut_across_clips_runs_in_capture_order() {
        // Sources arrive in capture order, so the cut follows the sources and
        // then the timeline inside each one -- not the in-file timestamps,
        // which would interleave two afternoons into one.
        let g = {
            let mut g = graph(vec![
                shot_from("src-0002", "b1", 10, 18, 0.90),
                shot_from("src-0001", "a1", 20, 28, 0.90),
                shot_from("src-0002", "b0", 0, 8, 0.90),
                shot_from("src-0001", "a0", 0, 8, 0.90),
            ]);
            g.sources = vec!["src-0001", "src-0002"]
                .into_iter()
                .map(|id| Source {
                    id: id.into(),
                    path: format!("/{id}.mp4"),
                    content_hash: Blake3Hash::of_str(id),
                    media_kind: None,
                    duration_us: Micros::from_secs(28),
                    fps_num: Some(30),
                    fps_den: Some(1),
                    width: Some(1920),
                    height: Some(1080),
                    video_codec: None,
                    audio_codec: None,
                    has_audio: Some(false),
                    created_at: None,
                    rotation: None,
                    categories: Vec::new(),
                })
                .collect();
            g
        };

        let store = PriorStore::with_builtins();
        let template = store.get("rhythm.calm-observational").unwrap();
        let dsl = MontageOrchestrator.compose(&g, template).unwrap();

        let spine: Vec<(String, u64)> = dsl
            .ordered_slots()
            .iter()
            .filter_map(|s| s.source_ref.as_ref())
            .map(|r| (r.source_id.clone(), r.in_us.0))
            .collect();
        let mut expected = spine.clone();
        expected.sort();
        assert_eq!(
            spine, expected,
            "the cut is out of capture order: {spine:?}"
        );
    }

    fn beat_style() -> Template {
        use crate::prior::{Mood, Style, Topic};
        let store = PriorStore::with_builtins();
        store
            .template_for_style(
                &Style {
                    mood: Some(Mood::Happy),
                    topic: Some(Topic::Travel),
                    beat_sync: Some(true),
                },
                &FootageGraph::new("t"),
            )
            .unwrap()
    }

    fn long_shots() -> Vec<Shot> {
        (0..8)
            .map(|i| shot(&format!("s{i}"), i * 10, i * 10 + 9, 0.5 + i as f64 / 20.0))
            .collect()
    }

    #[test]
    fn on_the_beat_every_cut_lands_on_a_beat_of_the_music() {
        let mut g = graph(long_shots());
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 128.0,
                first_beat_us: Some(Micros::from_millis(250)),
                confidence: Some(0.8),
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/track.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(180)),
            }),
            ..Default::default()
        });
        let t = beat_style();
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let beat = g
            .priors
            .as_ref()
            .unwrap()
            .beat_grid
            .as_ref()
            .unwrap()
            .beat_us()
            .unwrap();

        let timeline = dsl.resolve_timeline();
        assert!(timeline.slots.len() > 2);
        for slot in &timeline.slots {
            assert_eq!(
                slot.start_us.0 % beat.0,
                0,
                "{} starts at {}, between beats of {}",
                slot.slot_id,
                slot.start_us,
                beat
            );
        }
        assert!(
            dsl.transitions
                .iter()
                .all(|t| t.kind == TransitionType::Cut)
        );

        let music = dsl
            .tracks
            .audio
            .first()
            .expect("the chosen music is laid under the cut");
        assert_eq!(music.kind, AudioKind::Bgm);
        assert_eq!(music.asset_in_us, Some(Micros::from_millis(250)));
        assert_eq!(music.duration_us, Some(timeline.total_duration_us));
        let provenance = dsl.provenance.unwrap();
        assert!(
            provenance
                .priors_applied
                .iter()
                .any(|p| p.starts_with("beatGrid.music"))
        );
    }

    #[test]
    fn with_no_music_a_fixed_grid_is_used_and_called_one() {
        // A steady tempo is a pacing choice, and must not read as a detected
        // beat anywhere a user or a later stage would look.
        let g = graph(long_shots());
        let t = beat_style();
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let clips: Vec<_> = dsl
            .slots
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .collect();
        assert!(!clips.is_empty());
        for clip in &clips {
            assert!(
                clip.reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains("no music was chosen"),
                "reason: {:?}",
                clip.reason
            );
        }
        assert!(dsl.tracks.audio.is_empty(), "no music means no music track");
        let provenance = dsl.provenance.unwrap();
        assert!(
            provenance
                .priors_applied
                .iter()
                .any(|p| p.starts_with("beatGrid.fixed"))
        );
    }

    #[test]
    fn beat_clips_never_run_past_the_templates_longest() {
        let mut g = graph(long_shots());
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 150.0,
                first_beat_us: None,
                confidence: None,
                origin: Some(BeatOrigin::Music),
                asset_path: None,
                asset_duration_us: None,
            }),
            ..Default::default()
        });
        let t = beat_style();
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        for slot in dsl.slots.iter().filter(|s| s.kind == SlotKind::Clip) {
            assert!(
                slot.duration_us <= t.params.max_slot_us,
                "{} runs {}",
                slot.id,
                slot.duration_us
            );
            assert!(
                slot.duration_us >= t.params.min_slot_us,
                "{} runs {}",
                slot.id,
                slot.duration_us
            );
        }
    }

    #[test]
    fn music_never_outruns_its_file() {
        let mut g = graph(long_shots());
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 120.0,
                first_beat_us: Some(Micros::from_secs(1)),
                confidence: None,
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/short.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(6)),
            }),
            ..Default::default()
        });
        let dsl = MontageOrchestrator.compose(&g, &beat_style()).unwrap();
        let music = &dsl.tracks.audio[0];
        assert_eq!(music.duration_us, Some(Micros::from_secs(5)));
        assert_eq!(dsl.tracks.audio.len(), 1, "no loop unless asked");
    }

    #[test]
    fn looped_music_plays_again_from_its_first_beat_in_whole_beats() {
        let mut g = graph(long_shots());
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 120.0,
                first_beat_us: Some(Micros::from_millis(1_250)),
                confidence: None,
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/short.mp3".into()),
                // 4.75 s after the first beat: nine whole beats and a bit.
                asset_duration_us: Some(Micros::from_secs(6)),
            }),
            ..Default::default()
        });
        let mut style = beat_style();
        style.params.extra.insert("musicLoop".into(), true.into());
        let dsl = MontageOrchestrator.compose(&g, &style).unwrap();
        let timeline = dsl.resolve_timeline();
        let passes = &dsl.tracks.audio;
        assert!(passes.len() >= 2, "the cut outruns one pass: {passes:?}");

        let mut expected_at = Micros::ZERO;
        for (i, pass) in passes.iter().enumerate() {
            assert_eq!(pass.asset_in_us, Some(Micros::from_millis(1_250)));
            let at = timeline.resolve_anchor(&pass.anchor).unwrap();
            assert_eq!(at, expected_at, "pass {i} starts where the last ended");
            let length = pass.duration_us.unwrap();
            if i + 1 < passes.len() {
                assert_eq!(length, Micros::from_millis(4_500), "nine beats of 0.5 s");
                assert!(pass.fade_out_us.is_none(), "only the last pass fades");
            } else {
                assert!(pass.fade_out_us.is_some());
            }
            expected_at = at + length;
        }
        assert_eq!(
            expected_at, timeline.total_duration_us,
            "music to the last frame"
        );
    }

    #[test]
    fn a_dance_cut_ranks_moving_shots_above_still_ones() {
        let scored = |sharpness: f64, jitter: f64| Shot {
            keyframes: vec![Keyframe {
                id: "kf".into(),
                at_us: Micros::from_secs(1),
                path: None,
                scores: Some(KeyframeScores {
                    sharpness: Some(sharpness),
                    flow_jitter: Some(jitter),
                    exposure: Some(0.8),
                    ..Default::default()
                }),
            }],
            ..shot("x", 0, 5, 0.0)
        };
        let still = scored(0.9, 0.0);
        let moving = scored(0.7, 0.8);
        let dance = PriorStore::with_builtins()
            .get("content.dance-beat")
            .cloned()
            .unwrap();
        let calm = PriorStore::with_builtins()
            .get("rhythm.calm-observational")
            .cloned()
            .unwrap();
        assert!(
            MontageOrchestrator::rank_score(&moving, &dance)
                > MontageOrchestrator::rank_score(&still, &dance)
        );
        assert!(
            MontageOrchestrator::rank_score(&still, &calm)
                > MontageOrchestrator::rank_score(&moving, &calm),
            "outside dance, motion still reads as shake"
        );
    }

    #[test]
    fn a_named_project_opens_on_its_own_title() {
        let mut t = template();
        t.params
            .extra
            .insert("title".into(), "Kyoto in spring".into());
        let dsl = MontageOrchestrator
            .compose(&graph(long_shots()), &t)
            .unwrap();
        assert_eq!(dsl.meta.title, "Kyoto in spring");
        let title = dsl
            .tracks
            .text
            .iter()
            .find(|t| t.role == Some(TextRole::Title))
            .expect("this template opens on a title card");
        assert_eq!(title.content, "Kyoto in spring");
    }

    #[test]
    fn reasons_are_written_in_the_language_the_caller_asked_for() {
        let mut t = beat_style();
        t.params.extra.insert("locale".into(), "zh".into());
        let dsl = MontageOrchestrator
            .compose(&graph(long_shots()), &t)
            .unwrap();
        let clip = dsl.slots.iter().find(|s| s.kind == SlotKind::Clip).unwrap();
        let reason = clip.reason.as_deref().unwrap();
        assert!(reason.contains("选中"), "{reason}");
        assert!(reason.contains("固定的 120 BPM 节奏"), "{reason}");
    }

    #[test]
    fn a_chosen_length_is_not_overrun() {
        // Plenty of footage, so the only thing stopping the cut is the
        // length asked for -- which must be the length delivered.
        for beat in [false, true] {
            let mut t = if beat { beat_style() } else { template() };
            t.params.target_duration_us = Micros::from_secs(20);
            let dsl = MontageOrchestrator
                .compose(&graph(long_shots()), &t)
                .unwrap();
            let total = dsl.resolve_timeline().total_duration_us;
            assert!(
                total <= Micros::from_secs(20),
                "asked for 20 s, got {total} (beat: {beat})"
            );
            assert!(
                total >= Micros::from_secs(15),
                "asked for 20 s and got only {total}: the room was there (beat: {beat})"
            );
            for slot in dsl.slots.iter().filter(|s| s.kind == SlotKind::Clip) {
                assert!(slot.duration_us >= t.params.min_slot_us);
            }
        }
    }

    // --- cutting between scenes -----------------------------------------------

    /// A source per id, landscape unless its id says otherwise.
    fn sources(ids: &[&str]) -> Vec<Source> {
        ids.iter()
            .map(|id| Source {
                id: (*id).into(),
                path: format!("/{id}.mp4"),
                content_hash: Blake3Hash::of_str(id),
                media_kind: None,
                duration_us: Micros::from_secs(20),
                fps_num: Some(30),
                fps_den: Some(1),
                width: Some(1920),
                height: Some(1080),
                video_codec: None,
                audio_codec: None,
                has_audio: Some(false),
                created_at: None,
                rotation: id.contains("upright").then_some(90),
                categories: Vec::new(),
            })
            .collect()
    }

    /// A 20 s swept shot of one source holding the given segments: each is
    /// (scene, start, end, quality, look), usable from a second in to a
    /// second before its end, with the look filling its signature.
    fn swept(source: &str, segments: &[(&str, u64, u64, f64, f64)]) -> Shot {
        let id = format!("shot-{source}");
        Shot {
            id: id.clone(),
            source_id: source.into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(20),
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: vec![],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: segments
                .iter()
                .enumerate()
                .map(|(n, (scene, from, to, quality, look))| Segment {
                    id: format!("{id}-seg-{n}"),
                    start_us: Micros::from_secs(*from),
                    end_us: Micros::from_secs(*to),
                    usable_start_us: Some(Micros::from_secs(from + 1)),
                    usable_end_us: Some(Micros::from_secs(to - 1)),
                    scene_id: Some((*scene).into()),
                    quality: Some(*quality),
                    camera: None,
                    keyframe_id: None,
                    description: None,
                    signature: vec![*look; 44],
                    occlusion: None,
                    composition: None,
                    ..Default::default()
                })
                .collect(),
            timeline: vec![],
        }
    }

    fn swept_graph(shots: Vec<Shot>) -> FootageGraph {
        let ids: Vec<String> = shots.iter().map(|s| s.source_id.clone()).collect();
        let mut ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        ids.dedup();
        let mut g = FootageGraph::new("test");
        g.sources = sources(&ids);
        g.shots = shots;
        g
    }

    /// Poses through a segment's usable part, every quarter second: `n`
    /// dancers side by side, arms swinging at `pace` (0 is standing still),
    /// starting `phase` of the way through a swing.
    fn dancing(segment: &mut Segment, n: usize, pace: f64, phase: f64) {
        use vb_contracts::footage_graph::PoseSample;
        let (from, to) = segment.usable().unwrap();
        let mut at = from;
        segment.poses.clear();
        while at <= to {
            let t = at.as_secs_f64() * pace + phase;
            let arm = (t * std::f64::consts::PI).sin().abs();
            segment.poses.push(PoseSample {
                at_us: at,
                people: (0..n)
                    .map(|i| crate::dance::tests::body((i as f64 + 0.5) / n as f64, 0.8, arm))
                    .collect(),
            });
            at = at + Micros(250_000);
        }
        segment.people = Some(n as u32);
    }

    fn dance_template(seconds: u64) -> Template {
        let mut t = PriorStore::with_builtins()
            .get("content.dance-beat")
            .cloned()
            .unwrap();
        t.params.target_duration_us = Micros::from_secs(seconds);
        t
    }

    #[test]
    fn a_dance_cut_is_made_of_dancing_with_solos_and_duets_kept_together() {
        let mut solo = swept(
            "src-0001",
            &[("lake", 0, 10, 0.8, 0.1), ("pier", 10, 20, 0.8, 0.3)],
        );
        dancing(&mut solo.segments[0], 1, 1.0, 0.0);
        dancing(&mut solo.segments[1], 1, 1.2, 0.3);
        let mut still = swept("src-0002", &[("street", 0, 20, 0.95, 0.5)]);
        // The best-looking footage, and nobody in it is dancing.
        dancing(&mut still.segments[0], 1, 0.0, 0.25);
        let mut duet = swept(
            "src-0003",
            &[("hill", 0, 10, 0.8, 0.7), ("square", 10, 20, 0.8, 0.9)],
        );
        dancing(&mut duet.segments[0], 2, 1.0, 0.1);
        dancing(&mut duet.segments[1], 2, 0.9, 0.6);
        let mut solo_again = swept("src-0004", &[("beach", 0, 20, 0.8, 0.2)]);
        dancing(&mut solo_again.segments[0], 1, 1.1, 0.5);
        let g = swept_graph(vec![solo, still, duet, solo_again]);

        let dsl = MontageOrchestrator
            .compose(&g, &dance_template(40))
            .unwrap();
        let scenes = clip_scenes(&dsl, &g);
        assert!(!scenes.is_empty());
        assert!(
            !scenes.iter().any(|s| s == "street"),
            "standing still is not dancing: {scenes:?}"
        );
        // Solos, then the duet, as filmed: never back and forth.
        let duet_at: Vec<bool> = scenes
            .iter()
            .map(|s| s == "hill" || s == "square")
            .collect();
        let changes = duet_at.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(changes <= 1, "solos and duets interleaved: {scenes:?}");
        // Every clip after the first says how it joins the one before.
        let reasons: Vec<&str> = dsl
            .ordered_slots()
            .iter()
            .filter_map(|s| s.reason.as_deref())
            .collect();
        assert!(reasons.iter().all(|r| r.contains("dancing")), "{reasons:?}");
    }

    #[test]
    fn a_dance_cut_joins_each_clip_where_the_movement_matches() {
        // Two scenes of the same dance, filmed half a swing apart: the join
        // should find the start in the second where the pose is the one the
        // first ends on.
        let mut a = swept("src-0001", &[("lake", 0, 20, 0.9, 0.1)]);
        let mut b = swept("src-0002", &[("hill", 0, 20, 0.9, 0.8)]);
        dancing(&mut a.segments[0], 1, 1.0, 0.0);
        dancing(&mut b.segments[0], 1, 1.0, 0.5);
        let g = swept_graph(vec![a, b]);
        let dsl = MontageOrchestrator.compose(&g, &dance_template(8)).unwrap();
        let slots = dsl.ordered_slots();
        assert!(slots.len() >= 2, "{slots:?}");
        let reason = slots[1].reason.as_deref().unwrap();
        assert!(
            reason.contains("carried on"),
            "the second clip should pick up the first's movement: {reason}"
        );
    }

    fn clip_scenes(dsl: &EditingDsl, g: &FootageGraph) -> Vec<String> {
        dsl.ordered_slots()
            .iter()
            .filter_map(|s| s.source_ref.as_ref())
            .map(|r| {
                let shot = g.shot(r.shot_id.as_deref().unwrap()).unwrap();
                shot.segments
                    .iter()
                    .find(|seg| seg.start_us <= r.in_us && r.out_us <= seg.end_us)
                    .and_then(|seg| seg.scene_id.clone())
                    .unwrap_or_else(|| shot.id.clone())
            })
            .collect()
    }

    fn short_cut(seconds: u64) -> Template {
        let mut t = template();
        t.params.target_duration_us = Micros::from_secs(seconds);
        t.params.title_card = false;
        t.params.beat_align = false;
        t.params.min_slot_us = Micros::from_secs(2);
        t.params.max_slot_us = Micros::from_secs(4);
        t
    }

    #[test]
    fn a_cut_takes_the_best_scenes_not_the_first_files() {
        // Thirty clips, each its own scene, the later ones better. The old
        // cut filled up from the top of the folder; this one must not.
        let ids: Vec<String> = (0..30).map(|i| format!("src-{i:04}")).collect();
        let shots = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let scene = format!("scene-{i:02}");
                // Looks far apart, so only the choice of scenes is tested.
                let look = (i as f64 * 0.37) % 1.0;
                swept(id, &[(scene.as_str(), 0, 20, 0.3 + i as f64 / 50.0, look)])
            })
            .collect();
        let g = swept_graph(shots);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(20)).unwrap();
        let used: Vec<&str> = dsl
            .ordered_slots()
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        assert!(used.len() >= 5, "a 20 s cut of 2-4 s clips: {used:?}");
        assert!(
            used.iter().all(|id| *id >= "src-0020"),
            "picked from the top of the folder rather than the best: {used:?}"
        );
        // And in the order they were filmed.
        let mut sorted = used.clone();
        sorted.sort_unstable();
        assert_eq!(used, sorted);
    }

    #[test]
    fn neighbouring_clips_never_show_one_scene() {
        // The lake is filmed three times, the street twice, the hill once,
        // in that jumbled order, and the cut is long enough to want them all.
        let g = swept_graph(vec![
            swept(
                "src-0001",
                &[("lake", 0, 10, 0.9, 0.2), ("street", 10, 20, 0.8, 0.6)],
            ),
            swept(
                "src-0002",
                &[("lake", 0, 10, 0.85, 0.21), ("lake", 10, 20, 0.7, 0.22)],
            ),
            swept(
                "src-0003",
                &[("street", 0, 10, 0.75, 0.61), ("hill", 10, 20, 0.8, 0.9)],
            ),
        ]);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(24)).unwrap();
        let scenes = clip_scenes(&dsl, &g);
        assert!(scenes.len() >= 4, "{scenes:?}");
        for pair in scenes.windows(2) {
            assert_ne!(
                pair[0], pair[1],
                "two clips of one scene side by side: {scenes:?}"
            );
        }
        // Every scene is in before any comes back.
        for scene in ["lake", "street", "hill"] {
            assert!(
                scenes.iter().any(|s| s == scene),
                "{scene} left out: {scenes:?}"
            );
        }
    }

    #[test]
    fn views_that_look_alike_are_kept_apart_too() {
        // Two scenes by grouping, but the same blue on screen.
        let g = swept_graph(vec![
            swept("src-0001", &[("boat-a", 0, 20, 0.9, 0.300)]),
            swept("src-0002", &[("boat-b", 0, 20, 0.9, 0.310)]),
            swept("src-0003", &[("hill", 0, 20, 0.8, 0.800)]),
        ]);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(12)).unwrap();
        let scenes = clip_scenes(&dsl, &g);
        assert_eq!(scenes.len(), 3, "{scenes:?}");
        assert_eq!(
            scenes[1], "hill",
            "the two boats sat side by side: {scenes:?}"
        );
    }

    #[test]
    fn a_cut_ends_and_starts_where_the_action_rests() {
        // Ten usable seconds where people are moving all the time except at
        // three and at five seconds in. A two-second clip belongs between.
        let mut shot = swept("src-0001", &[("lake", 0, 12, 0.8, 0.3)]);
        shot.timeline = (0..24)
            .map(|i| {
                let at = Micros::from_millis(i * 500);
                let rest = at == Micros::from_secs(3) || at == Micros::from_secs(5);
                TimelineSample {
                    at_us: at,
                    quality: Some(0.8),
                    motion: Some(if rest { 0.0 } else { 0.9 }),
                    camera: Some(0.0),
                }
            })
            .collect();
        let g = swept_graph(vec![shot]);
        let mut t = short_cut(2);
        t.params.min_slot_us = Micros::from_secs(2);
        t.params.max_slot_us = Micros::from_secs(2);
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let r = dsl.slots[0].source_ref.as_ref().unwrap();
        assert_eq!(r.in_us, Micros::from_secs(3), "cut in mid-movement");
        assert_eq!(r.out_us, Micros::from_secs(5), "cut out mid-movement");
        assert!(dsl.slots[0].reason.as_deref().unwrap().contains("rests"));
    }

    #[test]
    fn the_unusable_start_of_a_take_is_never_used() {
        let g = swept_graph(vec![swept("src-0001", &[("lake", 0, 20, 0.8, 0.3)])]);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(30)).unwrap();
        for slot in dsl.slots.iter().filter(|s| s.kind == SlotKind::Clip) {
            let r = slot.source_ref.as_ref().unwrap();
            assert!(r.in_us >= Micros::from_secs(1), "used the camera coming up");
            assert!(
                r.out_us <= Micros::from_secs(19),
                "used the camera going down"
            );
        }
        // And a shot never swept still keeps off its take's first moments.
        let dsl = MontageOrchestrator
            .compose(&graph(vec![shot("s1", 0, 30, 0.9)]), &short_cut(4))
            .unwrap();
        let r = dsl.slots[0].source_ref.as_ref().unwrap();
        assert!(r.in_us >= Micros::from_millis(800), "in at {}", r.in_us);
    }

    #[test]
    fn a_portrait_clip_waits_behind_ones_that_fill_the_frame() {
        let g = swept_graph(vec![
            swept("src-0001-upright", &[("people", 0, 20, 0.9, 0.3)]),
            swept("src-0002", &[("hill", 0, 20, 0.7, 0.8)]),
            swept("src-0003", &[("lake", 0, 20, 0.7, 0.1)]),
        ]);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(8)).unwrap();
        let used: Vec<&str> = dsl
            .slots
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        assert_eq!(used, vec!["src-0002", "src-0003"], "{used:?}");
    }

    #[test]
    fn a_view_blocked_by_a_door_waits_behind_a_clear_one() {
        // The door clip scores higher on sharpness and exposure, and would
        // lead without the director's reading.
        let mut g = swept_graph(vec![
            swept("src-0001", &[("road", 0, 20, 0.9, 0.3)]),
            swept("src-0002", &[("hill", 0, 20, 0.7, 0.8)]),
            swept("src-0003", &[("lake", 0, 20, 0.7, 0.1)]),
        ]);
        g.shots[0].segments[0].occlusion = Some(0.2);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(8)).unwrap();
        let used: Vec<&str> = dsl
            .slots
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        assert_eq!(used, vec!["src-0002", "src-0003"], "{used:?}");
    }

    #[test]
    fn a_badly_composed_frame_waits_behind_a_well_composed_one() {
        let mut g = swept_graph(vec![
            swept("src-0001", &[("road", 0, 20, 0.8, 0.3)]),
            swept("src-0002", &[("hill", 0, 20, 0.7, 0.8)]),
            swept("src-0003", &[("lake", 0, 20, 0.7, 0.1)]),
        ]);
        // The judge thought little of the sharpest clip.
        g.shots[0].segments[0].composition = Some(0.2);
        g.shots[1].segments[0].composition = Some(0.6);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(8)).unwrap();
        let used: Vec<&str> = dsl
            .slots
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        assert_eq!(used, vec!["src-0002", "src-0003"], "{used:?}");
        let reason = dsl.slots.iter().find_map(|s| s.reason.clone()).unwrap();
        assert!(reason.contains("composition 3.4/5"), "{reason}");
    }

    #[test]
    fn composition_takes_more_than_it_gives() {
        assert_eq!(composed(None), 1.0);
        assert!((composed(Some(0.5)) - 1.0).abs() < 1e-9);
        assert_eq!(composed(Some(1.0)), 1.2);
        assert_eq!(composed(Some(0.0)), 0.6);
    }

    #[test]
    fn occlusion_costs_more_the_more_it_covers() {
        assert_eq!(clear_view(None), 1.0);
        assert_eq!(clear_view(Some(0.0)), 1.0);
        assert!((clear_view(Some(0.1)) - 0.64).abs() < 1e-9);
        assert!((clear_view(Some(0.25)) - 0.25).abs() < 1e-9);
        assert_eq!(clear_view(Some(0.6)), 0.0);
    }

    #[test]
    fn with_few_scenes_the_cut_comes_back_to_them_rather_than_stopping() {
        let g = swept_graph(vec![
            swept(
                "src-0001",
                &[("lake", 0, 10, 0.9, 0.2), ("hill", 10, 20, 0.8, 0.8)],
            ),
            swept(
                "src-0002",
                &[("lake", 0, 10, 0.85, 0.21), ("hill", 10, 20, 0.75, 0.81)],
            ),
        ]);
        let dsl = MontageOrchestrator.compose(&g, &short_cut(16)).unwrap();
        let scenes = clip_scenes(&dsl, &g);
        assert!(scenes.len() >= 4, "{scenes:?}");
        let total = dsl.resolve_timeline().total_duration_us;
        assert!(total >= Micros::from_secs(14), "stopped at {total}");
        for pair in scenes.windows(2) {
            assert_ne!(pair[0], pair[1], "{scenes:?}");
        }
        assert!(
            dsl.slots
                .iter()
                .any(|s| s.reason.as_deref().unwrap_or("").starts_with("Back to")),
            "a second visit should say so"
        );
    }

    #[test]
    fn on_the_beat_clips_run_whole_phrases() {
        let ids: Vec<String> = (0..12).map(|i| format!("src-{i:04}")).collect();
        let shots = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let scene = format!("scene-{i:02}");
                swept(
                    id,
                    &[(
                        scene.as_str(),
                        0,
                        20,
                        0.6 + i as f64 / 40.0,
                        i as f64 / 12.0,
                    )],
                )
            })
            .collect();
        let mut g = swept_graph(shots);
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 103.4,
                first_beat_us: Some(Micros::from_millis(534)),
                confidence: Some(0.8),
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/get-money.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(155)),
            }),
            ..Default::default()
        });
        let mut t = beat_style();
        t.params.target_duration_us = Micros::from_secs(30);
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let beat = g
            .priors
            .as_ref()
            .unwrap()
            .beat_grid
            .as_ref()
            .unwrap()
            .beat_us()
            .unwrap();
        let clips: Vec<&Slot> = dsl
            .slots
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .collect();
        assert!(clips.len() >= 6, "{} clips", clips.len());
        for clip in &clips {
            let beats = clip.duration_us.0 / beat.0;
            assert_eq!(clip.duration_us.0 % beat.0, 0);
            assert!(beats >= 2, "{} beats is a flicker", beats);
        }
        assert!(dsl.resolve_timeline().total_duration_us <= Micros::from_secs(30));
    }

    #[test]
    fn a_short_cut_of_a_lot_of_footage_visits_many_scenes_in_short_clips() {
        use crate::prior::{Style, Topic};
        // Twenty scenes for fifteen seconds of travel, on a 120 BPM beat.
        let ids: Vec<String> = (0..20).map(|i| format!("src-{i:04}")).collect();
        let shots = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let scene = format!("scene-{i:02}");
                swept(
                    id,
                    &[(
                        scene.as_str(),
                        0,
                        20,
                        0.6 + i as f64 / 60.0,
                        i as f64 / 20.0,
                    )],
                )
            })
            .collect();
        let mut g = swept_graph(shots);
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 120.0,
                first_beat_us: Some(Micros::ZERO),
                confidence: Some(0.8),
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/song.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(120)),
            }),
            ..Default::default()
        });
        let mut t = PriorStore::with_builtins()
            .template_for_style(
                &Style {
                    mood: None,
                    topic: Some(Topic::Travel),
                    beat_sync: Some(true),
                },
                &FootageGraph::new("t"),
            )
            .unwrap();
        t.params.target_duration_us = Micros::from_secs(15);
        let dsl = MontageOrchestrator.compose(&g, &t).unwrap();
        let beat = Micros(500_000);
        let clips: Vec<&Slot> = dsl
            .slots
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .collect();
        for clip in &clips {
            let beats = clip.duration_us.0 / beat.0;
            assert!((2..=4).contains(&beats), "{beats} beats");
            assert_eq!(clip.duration_us.0 % beat.0, 0);
        }
        assert!(
            clips.len() >= 6,
            "only {} clips of twenty scenes",
            clips.len()
        );
        assert!(dsl.resolve_timeline().total_duration_us <= Micros::from_secs(15));
    }

    /// A project of videos and photos: `videos` sources of two scenes each,
    /// and `photos` photographs, each a scene of its own and sharper than
    /// any video -- which is what a photo's reading usually says.
    fn mixed(videos: usize, photos: usize) -> FootageGraph {
        let mut shots: Vec<Shot> = (0..videos)
            .map(|i| {
                let id = format!("src-v{i:02}");
                let a = format!("v{i}a");
                let b = format!("v{i}b");
                let mut shot = swept(
                    &id,
                    &[
                        (a.as_str(), 0, 10, 0.6, 0.1),
                        (b.as_str(), 10, 20, 0.55, 0.3),
                    ],
                );
                // Distinct looks, so no two scenes read as one view.
                shot.segments[0].signature = vec![0.1 + 0.05 * i as f64; 44];
                shot.segments[1].signature = vec![0.12 + 0.05 * i as f64; 44];
                shot
            })
            .collect();
        for i in 0..photos {
            let id = format!("src-p{i:02}");
            let scene = format!("p{i}");
            let mut shot = swept(&id, &[(scene.as_str(), 0, 10, 0.95, 0.0)]);
            shot.end_us = Micros::from_secs(10);
            let seg = &mut shot.segments[0];
            seg.usable_start_us = Some(Micros::ZERO);
            seg.usable_end_us = Some(Micros::from_secs(10));
            seg.signature = vec![0.6 + 0.03 * i as f64; 44];
            shots.push(shot);
        }
        let mut g = swept_graph(shots);
        for source in g.sources.iter_mut().filter(|s| s.id.starts_with("src-p")) {
            source.media_kind = Some(vb_contracts::MediaKind::Image);
            source.duration_us = Micros::from_secs(10);
            source.fps_num = None;
            source.fps_den = None;
        }
        g
    }

    fn is_photo(g: &FootageGraph, slot: &Slot) -> bool {
        slot.source_ref
            .as_ref()
            .and_then(|r| g.source(&r.source_id))
            .is_some_and(|s| s.is_still())
    }

    #[test]
    fn a_photo_is_held_a_few_seconds_however_long_a_clip_may_run() {
        let g = mixed(4, 4);
        let mut t = short_cut(40);
        t.params.max_slot_us = Micros::from_secs(8);
        let dsl = compose_grounded(&MontageOrchestrator, &g, &t).unwrap().dsl;
        let photos: Vec<&Slot> = dsl.slots.iter().filter(|s| is_photo(&g, s)).collect();
        assert!(
            !photos.is_empty(),
            "a mixed project's cut should use its photos"
        );
        for slot in &photos {
            assert!(
                slot.duration_us <= STILL_HELD,
                "a photo held for {}",
                slot.duration_us
            );
            assert_eq!(slot.source_ref.as_ref().unwrap().in_us, Micros::ZERO);
            assert!(slot.reason.as_deref().unwrap().contains("photo"));
        }
        // The videos still get the long lengths.
        assert!(
            dsl.slots
                .iter()
                .filter(|s| !is_photo(&g, s) && s.kind == SlotKind::Clip)
                .any(|s| s.duration_us > STILL_HELD)
        );
    }

    #[test]
    fn photos_take_no_more_than_half_the_cut_while_videos_remain() {
        // Twenty photos that each look better than any video, and eight
        // stretches of video, more than half of what the cut holds.
        let g = mixed(4, 20);
        let dsl = compose_grounded(&MontageOrchestrator, &g, &short_cut(24))
            .unwrap()
            .dsl;
        let clips: Vec<&Slot> = dsl
            .slots
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .collect();
        let photos = clips.iter().filter(|s| is_photo(&g, s)).count();
        assert!(photos > 0);
        assert!(
            photos * 2 <= clips.len() + 1,
            "{photos} of {} clips are photos",
            clips.len()
        );

        // With the videos all used, photos fill the length asked for.
        let dsl = compose_grounded(&MontageOrchestrator, &g, &short_cut(40))
            .unwrap()
            .dsl;
        let clips = dsl.slots.iter().filter(|s| s.kind == SlotKind::Clip);
        let (photos, videos): (Vec<&Slot>, Vec<&Slot>) = clips.partition(|s| is_photo(&g, s));
        assert_eq!(videos.len(), 8);
        assert!(photos.len() > videos.len());
    }

    #[test]
    fn no_three_photos_in_a_row_when_a_video_can_go_between() {
        let g = mixed(3, 12);
        let dsl = compose_grounded(&MontageOrchestrator, &g, &short_cut(40))
            .unwrap()
            .dsl;
        let kinds: Vec<bool> = dsl
            .ordered_slots()
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .map(|s| is_photo(&g, s))
            .collect();
        let videos_used = kinds.iter().filter(|p| !**p).count();
        let longest = kinds
            .split(|photo| !photo)
            .map(<[bool]>::len)
            .max()
            .unwrap_or(0);
        assert!(videos_used > 0);
        assert!(
            longest <= STILLS_IN_A_ROW,
            "{longest} photos in a row: {kinds:?}"
        );
    }

    #[test]
    fn two_scenes_take_turns_rather_than_leaving_the_cut_short() {
        // An evening's five videos all by the lake, and its photos all at
        // the station: two scenes. Revisits of the lake alone could never be
        // placed apart, and a cut of them came out a third short.
        let mut shots: Vec<Shot> = (0..5)
            .map(|i| {
                let mut shot = swept(&format!("src-v{i}"), &[("lake", 0, 20, 0.7, 0.1)]);
                shot.segments[0].signature = vec![0.1 + 0.01 * i as f64; 44];
                shot
            })
            .collect();
        for i in 0..9 {
            let mut shot = swept(&format!("src-p{i}"), &[("station", 0, 10, 0.5, 0.6)]);
            shot.end_us = Micros::from_secs(10);
            shot.segments[0].usable_start_us = Some(Micros::ZERO);
            shot.segments[0].usable_end_us = Some(Micros::from_secs(10));
            shot.segments[0].signature = vec![0.6 + 0.01 * i as f64; 44];
            shots.push(shot);
        }
        let mut g = swept_graph(shots);
        for source in g.sources.iter_mut().filter(|s| s.id.starts_with("src-p")) {
            source.media_kind = Some(vb_contracts::MediaKind::Image);
            source.duration_us = Micros::from_secs(10);
        }
        let mut t = short_cut(30);
        t.params.max_slot_us = Micros::from_secs(8);
        let dsl = compose_grounded(&MontageOrchestrator, &g, &t).unwrap().dsl;
        let scenes = clip_scenes(&dsl, &g);
        assert!(
            scenes.windows(2).all(|w| w[0] != w[1]),
            "one scene twice in a row: {scenes:?}"
        );
        let made = dsl.resolve_timeline().total_duration_us;
        assert!(made >= Micros::from_secs(27), "a 30 s cut came out {made}");
    }

    #[test]
    fn capture_order_is_the_cameras_not_the_order_files_arrived_in() {
        // Videos given first, photos after; the photos were taken first.
        let mut g = mixed(2, 2);
        let when = [
            ("src-v00", Some("2023-09-29T10:47:54.000000Z")),
            ("src-v01", Some("2023-09-29T11:12:35.000000Z")),
            ("src-p00", Some("2023-09-29T18:31:43+08:00")),
            ("src-p01", None),
        ];
        for (id, at) in when {
            let source = g.sources.iter_mut().find(|s| s.id == id).unwrap();
            source.created_at = at.map(str::to_string);
        }
        let rank = MontageOrchestrator::capture_order(&g);
        assert_eq!(rank["src-p00"], 0);
        // No time of its own: it stays beside the one before it.
        assert_eq!(rank["src-p01"], 1);
        assert_eq!(rank["src-v00"], 2);
        assert_eq!(rank["src-v01"], 3);
    }

    #[test]
    fn a_project_of_photos_alone_is_cut_from_its_photos() {
        let g = mixed(0, 8);
        let dsl = compose_grounded(&MontageOrchestrator, &g, &short_cut(20))
            .unwrap()
            .dsl;
        let clips: Vec<&Slot> = dsl
            .slots
            .iter()
            .filter(|s| s.kind == SlotKind::Clip)
            .collect();
        assert!(clips.len() >= 6, "only {} clips", clips.len());
        assert!(clips.iter().all(|s| is_photo(&g, s)));
        // No video to take a frame rate from: the DSL says none rather than
        // inventing one.
        assert_eq!(dsl.meta.fps_num, None);
    }

    #[test]
    fn a_photo_on_the_beat_is_held_whole_beats() {
        let mut g = mixed(3, 6);
        g.priors = Some(vb_contracts::footage_graph::MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 100.0,
                first_beat_us: Some(Micros::ZERO),
                confidence: Some(0.9),
                origin: Some(BeatOrigin::Music),
                asset_path: None,
                asset_duration_us: None,
            }),
            ..Default::default()
        });
        let mut t = short_cut(30);
        t.params.beat_align = true;
        let dsl = compose_grounded(&MontageOrchestrator, &g, &t).unwrap().dsl;
        let beat = 600_000;
        for slot in dsl.slots.iter().filter(|s| is_photo(&g, s)) {
            assert_eq!(slot.duration_us.0 % beat, 0, "{}", slot.duration_us);
            assert!(slot.duration_us <= STILL_HELD);
        }
    }

    #[test]
    fn a_dance_cut_leaves_the_photos_out() {
        let mut g = mixed(2, 6);
        for shot in g.shots.iter_mut() {
            for seg in shot.segments.iter_mut() {
                dancing(seg, 1, 1.0, 0.0);
            }
        }
        let dsl = compose_grounded(&MontageOrchestrator, &g, &dance_template(20))
            .unwrap()
            .dsl;
        assert!(dsl.slots.iter().all(|s| !is_photo(&g, s)));
    }

    #[test]
    fn a_photo_counts_for_one_hold_in_choosing_the_frame() {
        // Two minutes of landscape video and ten portrait photos: the photos
        // would be a hundred seconds by their shots, thirty by what a cut
        // holds of them.
        let mut g = mixed(6, 10);
        for source in g.sources.iter_mut().filter(|s| s.is_still()) {
            source.width = Some(3000);
            source.height = Some(4000);
        }
        assert_eq!(MontageOrchestrator::frame(&g), Some((1920, 1080)));
    }
}
