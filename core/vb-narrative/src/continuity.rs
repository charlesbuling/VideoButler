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

//! 主体衔接 -- subject continuity: one action carried across every cut.
//!
//! The development doc's Phase 2 (§VII). Each clip's action subject is
//! followed through the body poses perception read into the graph, and each
//! continuous stretch of it acting is a *local全套动作*. Takes of one action
//! are aligned by what the bodies do and laid out on the action's own time
//! axis -- whatever order the files were read, imported or analysed in --
//! and played as one *global全套动作*: every moment once, the switch from
//! take to take at the same moment of the action, so the shot changes and
//! the movement does not. Where two takes frame the subject differently, the
//! picture whose subject is smaller is enlarged, moved and turned to match,
//! as far as its resolution and the frame allow, and eased back over the
//! clip.
//!
//! The rules for time are the doc's, in order. On the beat the dance comes
//! first: where its people stand idle more than a beat and a half is left
//! out, each piece plays at the speed -- within [`MAX_SPEED`] either way --
//! that puts its dancers' own beat on the cut's, and every clip starts on
//! one of its take's beats. (Music too short for it plays again when the
//! user asked for that, and otherwise leaves the rest silent: the
//! orchestrator's part.) The length chosen comes second: a cut longer than
//! it shortens every clip by the same share, never speeding up off the beat
//! it was put on, and only at the template's shortest clips does it end
//! where the room does. Off the beat, a room too small speeds every clip up
//! first, by one shared factor up to [`MAX_SPEED`].
//!
//! People only, for now: the subjects are the bodies the pose model found.
//! Everything here is geometry on the graph's numbers -- no pixel is read.

use vb_contracts::editing_dsl::{Transform, TransformKeyframe};
use vb_contracts::footage_graph::{PersonPose, PoseSample, Source};
use vb_contracts::{FootageGraph, Micros};

/// Below this a keypoint is the model guessing.
const SURE: f64 = 0.3;
/// Fewer shared keypoints than this and two bodies cannot be compared.
const SHARED: usize = 6;
/// What a join costs when its two sides cannot be compared at all: a
/// different number of people, or nobody to compare.
pub const MISMATCH: f64 = 3.0;
/// A join cheaper than this carries the action across the cut. The dance
/// cut's threshold, set on the same trip.
pub const JOINS_WELL: f64 = 0.6;
/// The most a clip is sped up or slowed down, either to put a dance on the
/// cut's beat or, off the beat, to fit the action into its room.
pub const MAX_SPEED: f64 = 1.2;
/// How regularly a take must keep its beat for its tempo to be trusted: the
/// rhythm reading's own threshold for dancing.
const KEEPS_BEAT: f64 = 0.2;
/// On the beat, a stretch of dancing shorter than this many of its beats,
/// with no dancing either side -- only stops, or the action's own rests --
/// is not dancing either: a step tried between looks at a phone, a move
/// before walking off.
const SHORTEST_DANCE_BEATS: f64 = 4.0;
/// An action its people dance less of than this share of is not a dance
/// at all -- the steps learned from a phone, people waiting about -- and
/// is left out whole. On the trip the take of the couple learning from a
/// phone danced 4% of its action by the rules above; no performance
/// danced less than 34% of its own.
const DANCED_AT_LEAST: f64 = 0.25;
/// On the beat, a move no other take repeats that is danced for less than
/// this many beats carries no phrase of the dance -- the couple turning to
/// walk off after it, on the trip -- and is not part of it.
const LONE_DANCE_BEATS: f64 = 8.0;

/// A stretch whose bodies move less than this, in torso lengths a second
/// (the median over it), is not an action: a pose for the camera, a walk
/// standing in line.
const ACTING: f64 = 0.25;
/// Movement below this at either end of a stretch is a rest, which the
/// action begins and ends on.
const REST: f64 = 0.15;
/// A local全套动作 shorter than this is a gesture, not an action.
const SHORTEST_ACTION: Micros = Micros(2_000_000);
/// How many pose samples a stretch may lose -- a body briefly hidden, a
/// passer-by counted -- and carry on.
const BRIDGE: usize = 2;
/// How much two takes must share to be aligned, at the pose sampling's
/// four a second: three seconds.
const MIN_OVERLAP: usize = 12;
/// Aligned takes hold the points that move to within this, in torso
/// lengths, on average over what they share.
const ALIGN_MAX: f64 = 0.35;
/// And the alignment must stand out: the best offset at most this share of
/// the typical one, or a movement that repeats would align anywhere.
const DISTINCT: f64 = 0.5;
/// A stretch repeats itself when, at an offset of at least a phrase, it
/// matches itself at most this share of the typical gap. Set on the trip:
/// the one take danced twice through matched itself at 0.50 of its typical
/// gap a performance later; no other take came below 0.84 at any offset
/// past eight seconds, and below that every take is close to itself only
/// because one pose leads into the next.
const SELF_DISTINCT: f64 = 0.6;
/// The shortest a repeated performance can be.
const SHORTEST_REPEAT: Micros = Micros(8_000_000);
/// Two poses this far apart in time are not "the same moment".
const NEAR: Micros = Micros(400_000);

/// How far a picture may be enlarged past its own resolution: a quarter.
/// A 1080p clip in a 1080p cut may be enlarged to 1.25x, a 4K one to 2.5x.
const MAX_UPSCALE: f64 = 1.25;
/// However much resolution there is, more than this is a different shot.
const MAX_ZOOM: f64 = 3.0;
/// The most a picture is turned to match the lean of the clip before.
const MAX_TURN_DEG: f64 = 10.0;

/// COCO keypoints.
const SHOULDERS: (usize, usize) = (5, 6);
const HIPS: (usize, usize) = (11, 12);
const LIMBS: [usize; 8] = [7, 8, 9, 10, 13, 14, 15, 16];
const ANCHORS: [usize; 4] = [5, 6, 11, 12];
const MOVING_POINTS: [usize; 12] = [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// Is this template a 主体衔接 cut?
pub fn is_continuity(extra: &std::collections::BTreeMap<String, serde_json::Value>) -> bool {
    extra
        .get("continuity")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// What a plan is made against.
#[derive(Debug, Clone)]
pub struct Params {
    /// The cut's frame, upright, in pixels.
    pub canvas: (u32, u32),
    /// The beat the cut follows, when it follows one: every clip is a whole
    /// number of them, and the dance is put on it.
    pub beat: Option<Micros>,
    /// How long a clip runs before the cut moves to another take.
    pub clip: Micros,
    /// The shortest a clip may be.
    pub min_clip: Micros,
    /// The length the user chose; `None` is automatic -- the action's own.
    pub room: Option<Micros>,
}

/// One clip of a 主体衔接 cut.
#[derive(Debug, Clone)]
pub struct PlannedClip {
    /// The shot and segment it is taken from, as indices into the graph.
    pub shot: usize,
    pub segment: usize,
    /// Where it starts in the source.
    pub in_us: Micros,
    /// How long it runs on screen; it uses `len x speed` of the source.
    pub len: Micros,
    /// How fast it plays: its piece's speed.
    pub speed: f64,
    /// Where the picture sits, when it is not simply fitted to the frame.
    pub transform: Option<Transform>,
    /// How it joins the clip before; `None` for the first.
    pub join: Option<JoinInfo>,
    /// Which piece of the global全套动作 it shows, counting from 0.
    pub piece: usize,
    /// How many people the action is of.
    pub people: usize,
    /// How good the take looks, as the caller ranked it.
    pub quality: f64,
}

/// How a clip joins the one before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JoinInfo {
    /// How far pose, placement and movement jump across the cut.
    pub cost: f64,
    /// Whether the action carries across it.
    pub carried: bool,
    /// Whether the two sides are the same moment of the action in two
    /// takes -- the switch the alignment found -- rather than one action
    /// running into another.
    pub same_moment: bool,
    /// How far this clip's picture is enlarged at its in point, to match.
    pub scale: f64,
    /// And turned, in degrees clockwise.
    pub turn_deg: f64,
}

/// How one piece of the action plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieceSpeed {
    /// Every clip of the piece plays at this.
    pub speed: f64,
    /// The beat its dancers keep, when they keep one.
    pub dance_bpm: Option<f64>,
    /// Whether the speed puts their beat on the cut's.
    pub on_beat: bool,
}

/// What became of the action.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Local全套动作 found in the footage.
    pub actions: usize,
    /// Pieces of the global全套动作: groups of aligned takes, and the
    /// actions no other take repeats.
    pub pieces: usize,
    /// The global全套动作 on screen, at the speeds it plays at.
    pub natural: Micros,
    /// The room it was fitted into.
    pub room: Micros,
    /// How each piece plays, in the order they play.
    pub speeds: Vec<PieceSpeed>,
    /// How much of the action the cut shows, 0..1.
    pub shown: f64,
    /// How much of the action's end the cut could not hold.
    pub dropped_tail: Micros,
    /// Whether clips were shortened, or the end left out, to fit the room
    /// -- rather than the fraction of a beat whole-beat clips leave off.
    pub shortened: bool,
    /// Whether a chosen length was longer than the action, which ends the
    /// cut early.
    pub shorter_than_chosen: bool,
    /// How much of the takes' actions was left out as not dancing: anyone
    /// standing still more than a beat and a half, someone watching what
    /// they hold, a step with no dancing either side.
    pub idle: Micros,
    /// Takes whose people danced less than a quarter of their action --
    /// learning the steps, waiting about -- left out whole, and how long
    /// their actions were.
    pub practice: usize,
    pub practice_len: Micros,
    pub joins: usize,
    pub carried: usize,
}

/// A planned 主体衔接 cut.
#[derive(Debug, Clone)]
pub struct Plan {
    pub clips: Vec<PlannedClip>,
    pub report: Report,
}

/// Plan a 主体衔接 cut, or `None` when the footage holds no local全套动作.
///
/// `quality` ranks a take by shot and segment index, as the caller ranks
/// footage for any cut.
///
/// On the beat, the dance comes first: where its people stand idle more
/// than a beat and a half is left out of the action, each piece plays at the speed that puts its dancers'
/// own beat on the cut's (within [`MAX_SPEED`] either way), and every clip
/// starts on one of its take's beats. The room comes second: a cut longer
/// than the length chosen is shortened, every clip by one share, never sped
/// up off the beat it was put on. Off the beat, a room too small speeds the
/// cut up first, up to [`MAX_SPEED`].
pub fn plan(
    graph: &FootageGraph,
    params: &Params,
    quality: &dyn Fn(usize, usize) -> f64,
) -> Option<Plan> {
    let beat = params.beat.filter(|b| b.0 > 0);
    let (locals, left_out) = find_locals(graph, params.canvas, quality, beat.is_some());
    if locals.is_empty() {
        return None;
    }
    let pieces = layout(&locals);
    let order = order_pieces(&pieces, &locals);
    let mut pieces: Vec<Piece> = order.into_iter().map(|i| pieces[i].clone()).collect();
    // On the beat, a move on its own too short to hold a phrase is not
    // part of the dance: what of it was danced is left out with the rest.
    let mut idle = left_out.skipped;
    if let Some(b) = beat {
        pieces.retain(|piece| {
            let [(t, _)] = piece.takes[..] else {
                return true;
            };
            let local = &locals[t];
            let beat_len = local.bpm.map_or(b.0 as f64, |bpm| 60_000_000.0 / bpm);
            let danced = coverage(piece, &locals);
            let lone = (danced.0 as f64) < LONE_DANCE_BEATS * beat_len;
            if lone {
                idle = idle + danced;
            }
            !lone
        });
        if pieces.is_empty() {
            return None;
        }
    }

    let whole = |d: Micros| match beat {
        Some(b) => Micros(d.0 / b.0 * b.0),
        None => d,
    };
    let unit = beat.unwrap_or(Micros(500_000));
    let four = |v: f64| (v * 10_000.0).round() / 10_000.0;

    // First the dance on the beat: each piece at the speed that puts its
    // dancers' beat on the cut's.
    let mut speeds: Vec<PieceSpeed> = pieces
        .iter()
        .map(|piece| {
            let dance = dance_bpm(piece, &locals);
            match (beat, dance) {
                (Some(b), Some(bpm)) => match on_the_beat(bpm, b) {
                    Some(k) => PieceSpeed {
                        speed: four(k),
                        dance_bpm: Some(bpm),
                        on_beat: true,
                    },
                    None => PieceSpeed {
                        speed: 1.0,
                        dance_bpm: Some(bpm),
                        on_beat: false,
                    },
                },
                _ => PieceSpeed {
                    speed: 1.0,
                    dance_bpm: dance,
                    on_beat: false,
                },
            }
        })
        .collect();
    let covered: Vec<Micros> = pieces.iter().map(|p| coverage(p, &locals)).collect();
    let natural_at = |speeds: &[PieceSpeed]| -> Micros {
        Micros(
            covered
                .iter()
                .zip(speeds)
                .map(|(c, s)| (c.0 as f64 / s.speed) as u64)
                .sum(),
        )
    };
    let mut natural = natural_at(&speeds);
    let room = whole(params.room.unwrap_or(natural)).max(unit);
    // Off the beat, a room too small is met by speed first. Less than a beat
    // over is not sped up: whole-beat clips leave that much off anyway.
    if beat.is_none() && natural.0 > room.0 + unit.0 {
        let k = ((natural.0 as f64 / room.0 as f64).min(MAX_SPEED) * 10_000.0).ceil() / 10_000.0;
        for s in speeds.iter_mut() {
            s.speed = k;
        }
        natural = natural_at(&speeds);
    }

    let mut steps: Vec<Step> = Vec::new();
    for (p, piece) in pieces.iter().enumerate() {
        steps.extend(walk(
            p,
            piece,
            &locals,
            params,
            speeds[p].speed,
            speeds[p].on_beat,
            unit,
        ));
    }
    if steps.is_empty() {
        return None;
    }

    // Then the room: longer than it, every clip shortened by the same
    // share, then, at the shortest clips, the end left out.
    let min_clip = whole(params.min_clip).max(unit);
    let total = |steps: &[Step]| steps.iter().map(|s| s.len.0).sum::<u64>();
    let mut shortened = false;
    if total(&steps) > room.0 {
        let share = room.0 as f64 / total(&steps) as f64;
        let before: Vec<Micros> = steps.iter().map(|s| s.len).collect();
        for step in steps.iter_mut() {
            let want = whole(Micros((step.len.0 as f64 * share) as u64)).max(min_clip);
            if want < step.len {
                step.len = want;
                shortened = true;
            }
        }
        // Whole beats round every clip down; what that leaves of the room
        // goes back a beat at a time to the clips that lost most, so a
        // chosen length is delivered, not approximated.
        while total(&steps) + unit.0 <= room.0 {
            let Some(k) = (0..steps.len())
                .filter(|&k| steps[k].len + unit <= before[k])
                .max_by_key(|&k| (before[k] - steps[k].len, std::cmp::Reverse(k)))
            else {
                break;
            };
            steps[k].len = steps[k].len + unit;
        }
        if shortened {
            choose_starts_again(&mut steps, &locals);
        }
    }
    let axis_total: u64 = covered.iter().map(|c| c.0).sum();
    let mut dropped_tail = Micros::ZERO;
    while total(&steps) > room.0 {
        let excess = Micros(total(&steps) - room.0);
        let Some(last) = steps.last_mut() else { break };
        let keep = whole(last.len.saturating_sub(excess));
        if keep >= min_clip {
            dropped_tail = dropped_tail + last.span_of(last.len - keep);
            last.len = keep;
        } else {
            let gone = steps.pop().expect("there is a last step");
            dropped_tail = dropped_tail + gone.span_of(gone.len);
        }
    }
    if steps.is_empty() {
        return None;
    }

    // The joins, and the framing that makes them.
    let mut sides: Vec<(Side, Side)> = vec![(Side::FITTED, Side::FITTED); steps.len()];
    let mut joins: Vec<Option<JoinInfo>> = vec![None; steps.len()];
    for k in 1..steps.len() {
        let (a, b) = (&steps[k - 1], &steps[k]);
        // A switch moved onto the next take's own beat is still the same
        // moment, to within the three quarters of its beat it may move.
        let slack = match speeds[b.piece] {
            PieceSpeed {
                on_beat: true,
                dance_bpm: Some(bpm),
                ..
            } => (45_000_000.0 / bpm) as u64,
            _ => unit.0 / 4,
        };
        let same_moment = a.piece == b.piece
            && a.full
            && b.axis.0.abs_diff((a.axis + a.span_of(a.len)).0) <= slack;
        let out_at = a.source_in + a.span_of(a.len);
        let fit = fit_join(
            &locals[a.local],
            out_at,
            &locals[b.local],
            b.source_in,
            same_moment,
        );
        sides[k - 1].1 = fit.out;
        sides[k].0 = fit.inn;
        joins[k] = Some(JoinInfo {
            cost: fit.cost,
            // What the framing could not match is in the cost already.
            carried: fit.cost < JOINS_WELL,
            same_moment,
            scale: fit.inn.scale,
            turn_deg: fit.inn.turn.to_degrees(),
        });
    }

    let shown_axis: u64 = steps.iter().map(|s| s.span_of(s.len).0).sum();
    let clips: Vec<PlannedClip> = steps
        .iter()
        .enumerate()
        .map(|(k, step)| {
            let local = &locals[step.local];
            let (inn, out) = sides[k];
            let transform = if inn.is_fitted() && out.is_fitted() {
                None
            } else if inn.same(&out) {
                Some(Transform {
                    keyframes: vec![inn.keyframe(&local.picture, Micros::ZERO)],
                })
            } else {
                Some(Transform {
                    keyframes: vec![
                        inn.keyframe(&local.picture, Micros::ZERO),
                        out.keyframe(&local.picture, step.len),
                    ],
                })
            };
            PlannedClip {
                shot: local.shot,
                segment: local.segment,
                in_us: step.source_in,
                len: step.len,
                speed: step.speed,
                transform,
                join: joins[k],
                piece: step.piece,
                people: local.people,
                quality: local.quality,
            }
        })
        .collect();

    let joined = clips.iter().filter(|c| c.join.is_some()).count();
    let carried = clips
        .iter()
        .filter(|c| c.join.is_some_and(|j| j.carried))
        .count();
    let report = Report {
        actions: locals.len(),
        pieces: pieces.len(),
        natural,
        room,
        speeds,
        shown: (shown_axis as f64 / axis_total.max(1) as f64).min(1.0),
        dropped_tail,
        shortened: shortened || dropped_tail > Micros::ZERO,
        shorter_than_chosen: params.room.is_some_and(|r| natural < whole(r)),
        idle,
        practice: left_out.practice,
        practice_len: left_out.practice_len,
        joins: joined,
        carried,
    };
    Some(Plan { clips, report })
}

/// The tempo a piece's dancers keep: the takes' own readings, each taken
/// only where it kept its beat clearly, brought into one octave -- a beat
/// read at half or double in one take is the same beat -- and the middle
/// one by length.
fn dance_bpm(piece: &Piece, locals: &[Local]) -> Option<f64> {
    let mut readings: Vec<(f64, f64)> = piece
        .takes
        .iter()
        .filter_map(|(t, _)| {
            let local = &locals[*t];
            local.bpm.map(|bpm| (bpm, local.len().as_secs_f64()))
        })
        .collect();
    let reference = readings
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|r| r.0)?;
    for (bpm, _) in readings.iter_mut() {
        while *bpm > reference * std::f64::consts::SQRT_2 {
            *bpm /= 2.0;
        }
        while *bpm < reference / std::f64::consts::SQRT_2 {
            *bpm *= 2.0;
        }
    }
    readings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let half = readings.iter().map(|r| r.1).sum::<f64>() / 2.0;
    let mut run = 0.0;
    for (bpm, weight) in &readings {
        run += weight;
        if run >= half {
            return Some(*bpm);
        }
    }
    readings.last().map(|r| r.0)
}

/// The speed that puts a dance at `dance_bpm` on a cut's `beat`: its beats
/// on the cut's, on every other one, or two to each -- the nearest to its
/// own pace, within [`MAX_SPEED`] either way. `None` when none is.
fn on_the_beat(dance_bpm: f64, beat: Micros) -> Option<f64> {
    let cut_bpm = 60_000_000.0 / beat.0.max(1) as f64;
    [0.5, 1.0, 2.0]
        .into_iter()
        .map(|ratio| cut_bpm * ratio / dance_bpm.max(1e-6))
        .filter(|k| k.ln().abs() <= MAX_SPEED.ln() + 1e-9)
        .min_by(|a, b| a.ln().abs().total_cmp(&b.ln().abs()))
}

/// How much of a piece's axis its takes dance: every take's dancing
/// stretches placed on the axis, overlaps counted once, gaps out.
fn coverage(piece: &Piece, locals: &[Local]) -> Micros {
    let mut spans: Vec<(Micros, Micros)> = Vec::new();
    for (t, at) in &piece.takes {
        let local = &locals[*t];
        let mut from = local.from;
        let stops = local.skips.iter().copied().filter(|(a, _)| *a < local.to);
        for (a, b) in stops.chain([(local.to, local.to)]) {
            if a > from {
                spans.push((*at + (from - local.from), *at + (a - local.from)));
            }
            from = from.max(b);
        }
    }
    spans.sort();
    let mut total = Micros::ZERO;
    let mut open: Option<(Micros, Micros)> = None;
    for (a, b) in spans {
        match open {
            Some((x, y)) if a <= y => open = Some((x, y.max(b))),
            Some((x, y)) => {
                total = total + (y - x);
                open = Some((a, b));
            }
            None => open = Some((a, b)),
        }
    }
    if let Some((x, y)) = open {
        total = total + (y - x);
    }
    total
}

// ---------------------------------------------------------------------------
// Bodies, in the cut's frame
// ---------------------------------------------------------------------------

/// How one take's picture sits in the cut's frame, fitted and centred as the
/// editor places it. Lengths are in heights of the frame, so the frame is
/// `aspect` wide and 1 high, and distances mean the same either way.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Picture {
    aspect: f64,
    w: f64,
    h: f64,
    /// How far it may be enlarged before its own resolution runs out.
    max_scale: f64,
}

impl Picture {
    fn new(canvas: (u32, u32), source: &Source) -> Picture {
        let (cw, ch) = (canvas.0.max(1) as f64, canvas.1.max(1) as f64);
        let (sw, sh) = source
            .display_size()
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| (w as f64, h as f64))
            .unwrap_or((cw, ch));
        let fit = (cw / sw).min(ch / sh);
        Picture {
            aspect: cw / ch,
            w: sw * fit / ch,
            h: sh * fit / ch,
            max_scale: (MAX_UPSCALE / fit).clamp(1.0, MAX_ZOOM),
        }
    }

    /// A point given as fractions of the upright picture, in the frame.
    fn place(&self, fx: f64, fy: f64) -> (f64, f64) {
        (
            (self.aspect - self.w) / 2.0 + fx * self.w,
            (1.0 - self.h) / 2.0 + fy * self.h,
        )
    }

    fn centre(&self) -> (f64, f64) {
        (self.aspect / 2.0, 0.5)
    }

    fn fills_width(&self) -> bool {
        self.w >= self.aspect - 1e-6
    }

    fn fills_height(&self) -> bool {
        self.h >= 1.0 - 1e-6
    }

    /// Does the picture, placed this way, still cover the frame wherever
    /// it covered it fitted? A portrait picture in a landscape frame shows
    /// bars either side; it must still reach the top and the bottom.
    fn covers(&self, side: &Side) -> bool {
        let c = self.centre();
        let (sin, cos) = side.turn.sin_cos();
        for corner in [
            (0.0, 0.0),
            (self.aspect, 0.0),
            (0.0, 1.0),
            (self.aspect, 1.0),
        ] {
            let (dx, dy) = (corner.0 - c.0 - side.shift.0, corner.1 - c.1 - side.shift.1);
            // Into the picture's own axes: turned back, and shrunk back.
            let lx = (dx * cos + dy * sin) / side.scale;
            let ly = (-dx * sin + dy * cos) / side.scale;
            if self.fills_width() && lx.abs() > self.w / 2.0 + 1e-9 {
                return false;
            }
            if self.fills_height() && ly.abs() > self.h / 2.0 + 1e-9 {
                return false;
            }
        }
        true
    }
}

/// Where a picture is placed at one end of a clip: enlarged about its
/// centre, turned clockwise, and moved, in heights of the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Side {
    scale: f64,
    turn: f64,
    shift: (f64, f64),
}

impl Side {
    const FITTED: Side = Side {
        scale: 1.0,
        turn: 0.0,
        shift: (0.0, 0.0),
    };

    fn apply(&self, picture: &Picture, p: (f64, f64)) -> (f64, f64) {
        let c = picture.centre();
        let (sin, cos) = self.turn.sin_cos();
        let (dx, dy) = ((p.0 - c.0) * self.scale, (p.1 - c.1) * self.scale);
        (
            c.0 + dx * cos - dy * sin + self.shift.0,
            c.1 + dx * sin + dy * cos + self.shift.1,
        )
    }

    /// A velocity seen through this placement: enlarged and turned, never
    /// moved.
    fn carry(&self, v: (f64, f64)) -> (f64, f64) {
        let (sin, cos) = self.turn.sin_cos();
        (
            self.scale * (v.0 * cos - v.1 * sin),
            self.scale * (v.0 * sin + v.1 * cos),
        )
    }

    fn is_fitted(&self) -> bool {
        self.same(&Side::FITTED)
    }

    fn same(&self, other: &Side) -> bool {
        (self.scale - other.scale).abs() < 1e-4
            && (self.turn - other.turn).abs() < 1e-4
            && (self.shift.0 - other.shift.0).abs() < 1e-4
            && (self.shift.1 - other.shift.1).abs() < 1e-4
    }

    /// As the DSL keys it: moved as shares of the frame's width and height.
    fn keyframe(&self, picture: &Picture, offset: Micros) -> TransformKeyframe {
        let round = |v: f64, places: i32| {
            let f = 10f64.powi(places);
            (v * f).round() / f
        };
        TransformKeyframe {
            offset_us: offset,
            scale: round(self.scale, 4),
            x: round(self.shift.0 / picture.aspect, 4) + 0.0,
            y: round(self.shift.1, 4) + 0.0,
            rotation_deg: round(self.turn.to_degrees(), 2) + 0.0,
        }
    }

    /// The placement that enlarges a picture by `scale` and turns it by
    /// `turn` about its centre, and moves it so the point `from` lands on
    /// `to`.
    fn placing(picture: &Picture, scale: f64, turn: f64, from: (f64, f64), to: (f64, f64)) -> Side {
        let c = picture.centre();
        let (sin, cos) = turn.sin_cos();
        let (dx, dy) = ((from.0 - c.0) * scale, (from.1 - c.1) * scale);
        Side {
            scale,
            turn,
            shift: (
                to.0 - c.0 - (dx * cos - dy * sin),
                to.1 - c.1 - (dx * sin + dy * cos),
            ),
        }
    }
}

/// The placement nearest `want` that the picture allows: never smaller
/// than fitted, no larger than its resolution bears, turned no further than
/// a lean needs, and never showing past its own edge. Whether `want` had to
/// be held back.
fn bounded(picture: &Picture, want: Side) -> (Side, bool) {
    let whole = picture.fills_width() && picture.fills_height();
    let max_turn = if whole {
        MAX_TURN_DEG.to_radians()
    } else {
        0.0
    };
    let mut side = Side {
        scale: want.scale.clamp(1.0, picture.max_scale),
        turn: want.turn.clamp(-max_turn, max_turn),
        shift: want.shift,
    };
    // Turned further than the enlargement covers: turn less.
    let still = Side {
        shift: (0.0, 0.0),
        ..side
    };
    if !picture.covers(&still) {
        let kept = bisect(|f| {
            picture.covers(&Side {
                turn: side.turn * f,
                ..still
            })
        });
        side.turn *= kept;
    }
    // Moved past the picture's edge: move less.
    if !picture.covers(&side) {
        let kept = bisect(|f| {
            picture.covers(&Side {
                shift: (side.shift.0 * f, side.shift.1 * f),
                ..side
            })
        });
        side.shift = (side.shift.0 * kept, side.shift.1 * kept);
    }
    let held = (side.scale - want.scale).abs() > 0.02 * want.scale.max(1.0)
        || (side.turn - want.turn).abs() > 1f64.to_radians()
        || distance(side.shift, want.shift) > 0.02;
    (side, held)
}

/// The largest share in 0..1 for which `ok` holds, given that it holds at 0.
fn bisect(ok: impl Fn(f64) -> bool) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// One person, in the cut's frame.
#[derive(Debug, Clone)]
struct Body {
    /// Every keypoint the model is sure of, in heights of the frame.
    points: [Option<(f64, f64)>; 17],
    /// The same, relative to the middle of the hips, in torso lengths: what
    /// the body is doing, wherever it stands and however large.
    shape: [Option<(f64, f64)>; 17],
    hip: (f64, f64),
    torso: f64,
}

fn middle(points: &[Option<(f64, f64)>; 17], pair: (usize, usize)) -> Option<(f64, f64)> {
    match (points[pair.0], points[pair.1]) {
        (Some(p), Some(q)) => Some(((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)),
        (Some(p), None) | (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

impl Body {
    /// A person whose shoulders and hips can be seen, or `None`.
    fn read(person: &PersonPose, picture: &Picture) -> Option<Body> {
        let mut points = [None; 17];
        for (slot, k) in points.iter_mut().zip(&person.keypoints) {
            if k[2] >= SURE {
                *slot = Some(picture.place(k[0], k[1]));
            }
        }
        let neck = middle(&points, SHOULDERS)?;
        let hip = middle(&points, HIPS)?;
        let tall = (person.bbox[3] - person.bbox[1]).max(0.0) * picture.h;
        let torso = distance(neck, hip).max(tall / 8.0).max(1e-3);
        let mut shape = [None; 17];
        for (slot, p) in shape.iter_mut().zip(points.iter()) {
            *slot = p.map(|p| ((p.0 - hip.0) / torso, (p.1 - hip.1) / torso));
        }
        Some(Body {
            points,
            shape,
            hip,
            torso,
        })
    }
}

/// How differently two people hold themselves: the root-mean-square
/// distance between their shared keypoints, in torso lengths, with the
/// second turned clockwise by `turn` first.
fn shape_distance(a: &Body, b: &Body, turn: f64) -> Option<f64> {
    let (sin, cos) = turn.sin_cos();
    let mut sum = 0.0;
    let mut n = 0usize;
    for (p, q) in a.shape.iter().zip(&b.shape) {
        if let (Some(p), Some(q)) = (p, q) {
            let q = (q.0 * cos - q.1 * sin, q.0 * sin + q.1 * cos);
            sum += (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2);
            n += 1;
        }
    }
    (n >= SHARED).then(|| (sum / n as f64).sqrt())
}

/// One pose sample: the people it is of, left to right, when every one of
/// them can be read.
#[derive(Debug, Clone)]
struct Frame {
    at: Micros,
    bodies: Option<Vec<Body>>,
}

impl Frame {
    fn read(sample: &PoseSample, picture: &Picture) -> Frame {
        let bodies: Option<Vec<Body>> = sample
            .subjects()
            .iter()
            .map(|p| Body::read(p, picture))
            .collect();
        Frame {
            at: sample.at_us,
            bodies: bodies.filter(|b| !b.is_empty()),
        }
    }

    fn people(&self) -> usize {
        self.bodies.as_ref().map_or(0, Vec::len)
    }
}

/// How far apart two frames' people hold the points that move, matched left
/// to right: the root-mean-square distance over `points`, in torso lengths.
///
/// Only the moving points, because a body still from the waist down holds
/// two thirds of its keypoints in the same place in any two frames, and
/// counting them would make any two arm movements look alike.
fn frame_distance(a: &Frame, b: &Frame, points: &[usize]) -> Option<f64> {
    let (x, y) = (a.bodies.as_ref()?, b.bodies.as_ref()?);
    if x.len() != y.len() {
        return None;
    }
    let mut total = 0.0;
    for (p, q) in x.iter().zip(y) {
        let mut sum = 0.0;
        let mut n = 0usize;
        for &k in points {
            if let (Some(u), Some(v)) = (p.shape[k], q.shape[k]) {
                sum += (u.0 - v.0).powi(2) + (u.1 - v.1).powi(2);
                n += 1;
            }
        }
        if n * 2 < points.len().max(2) {
            return None;
        }
        total += (sum / n as f64).sqrt();
    }
    Some(total / x.len() as f64)
}

/// The keypoints that move in a stretch: those whose place relative to the
/// hips varies by at least this many torso lengths (one standard deviation).
const MOVES: f64 = 0.08;

fn moving_points<'f>(frames: impl Iterator<Item = &'f Frame>) -> Vec<usize> {
    let mut sums = [(0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize); 17];
    for frame in frames {
        let Some(bodies) = frame.bodies.as_ref() else {
            continue;
        };
        for body in bodies {
            for (acc, p) in sums.iter_mut().zip(body.shape.iter()) {
                if let Some((x, y)) = p {
                    acc.0 += x;
                    acc.1 += y;
                    acc.2 += x * x;
                    acc.3 += y * y;
                    acc.4 += 1;
                }
            }
        }
    }
    sums.iter()
        .enumerate()
        .filter(|(_, acc)| {
            let n = acc.4 as f64;
            if acc.4 < 3 {
                return false;
            }
            let var = (acc.2 / n - (acc.0 / n).powi(2)) + (acc.3 / n - (acc.1 / n).powi(2));
            var.max(0.0).sqrt() >= MOVES
        })
        .map(|(k, _)| k)
        .collect()
}

/// How much the people move between two frames, in torso lengths a second:
/// their limbs, with the body's own travel taken out, and the travel itself.
/// The limbs that move most count -- the arms of a hand dance, the legs of a
/// run -- so a body still from the waist down is not read as half still.
fn movement(a: &Frame, b: &Frame) -> Option<f64> {
    let seconds = b.at.0.checked_sub(a.at.0)? as f64 / 1e6;
    if seconds <= 0.0 || seconds > 1.0 {
        return None;
    }
    let (x, y) = (a.bodies.as_ref()?, b.bodies.as_ref()?);
    if x.len() != y.len() {
        return None;
    }
    let mut total = 0.0;
    for (p, q) in x.iter().zip(y) {
        let mut moved: Vec<f64> = LIMBS
            .iter()
            .filter_map(|&k| Some(distance(p.shape[k]?, q.shape[k]?)))
            .collect();
        moved.sort_by(|u, v| v.total_cmp(u));
        moved.truncate(4);
        let limbs = if moved.is_empty() {
            0.0
        } else {
            moved.iter().sum::<f64>() / moved.len() as f64
        };
        let travel = distance(p.hip, q.hip) / p.torso.max(q.torso);
        total += (limbs + travel) / seconds;
    }
    Some(total / x.len() as f64)
}

// ---------------------------------------------------------------------------
// Local全套动作
// ---------------------------------------------------------------------------

/// One take's complete action of its subject.
#[derive(Debug, Clone)]
struct Local {
    shot: usize,
    segment: usize,
    picture: Picture,
    /// Every readable frame of the stretch the subject was followed through.
    frames: Vec<Frame>,
    /// The action itself: from the rest it starts on to the rest it ends on.
    from: Micros,
    to: Micros,
    /// Where the footage around it may be used to.
    usable_to: Micros,
    people: usize,
    quality: f64,
    scene: String,
    /// The beat its people keep, when they keep one clearly, and every beat
    /// they hit, in the source's time.
    bpm: Option<f64>,
    beats: Vec<Micros>,
    /// On the beat, where its people are not dancing -- anyone idle more
    /// than a beat and a half, someone watching what they hold, a step on
    /// its own -- in order, from the action's start to where its footage
    /// may be used to: aligned with the rest of the take, never shown.
    skips: Vec<(Micros, Micros)>,
    /// Capture time, then content: an order that does not depend on the
    /// order the files were read in.
    key: (i64, String, u64, u64),
}

impl Local {
    fn len(&self) -> Micros {
        self.to.saturating_sub(self.from)
    }

    /// The frames of the action itself.
    fn acting(&self) -> impl Iterator<Item = &Frame> {
        self.frames
            .iter()
            .filter(|f| f.at >= self.from && f.at <= self.to)
    }

    /// How far from `t` its people dance on uninterrupted: to the next
    /// stretch they do not, or past the action's end into the footage
    /// after it. `None` when they are not dancing at `t`.
    fn dancing_until(&self, t: Micros) -> Option<Micros> {
        if self.skips.iter().any(|(a, b)| t >= *a && t < *b) {
            return None;
        }
        Some(
            self.skips
                .iter()
                .map(|(a, _)| *a)
                .filter(|a| *a > t)
                .min()
                .unwrap_or(self.usable_to),
        )
    }

    /// The first moment at or after `t` its people dance.
    fn dances_from(&self, t: Micros) -> Option<Micros> {
        let mut at = t.max(self.from);
        while let Some((_, end)) = self.skips.iter().find(|(a, b)| at >= *a && at < *b) {
            at = *end;
        }
        (at < self.to).then_some(at)
    }

    /// How long its people dance within the action.
    fn danced(&self) -> Micros {
        let skipped: u64 = self
            .skips
            .iter()
            .map(|(a, b)| (*b).min(self.to).saturating_sub((*a).max(self.from)).0)
            .sum();
        Micros(self.len().0.saturating_sub(skipped))
    }

    /// The readable frame nearest `t`, when there is one near it.
    fn frame_near(&self, t: Micros) -> Option<usize> {
        let (i, f) = self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.bodies.is_some())
            .min_by_key(|(_, f)| f.at.0.abs_diff(t.0))?;
        (f.at.0.abs_diff(t.0) <= NEAR.0).then_some(i)
    }
}

/// What [`find_locals`] left out as not dancing.
#[derive(Debug, Clone, Copy, Default)]
struct LeftOut {
    /// Within the actions kept: idle, watching, steps on their own.
    skipped: Micros,
    /// Actions left out whole as not a dance, and how long they were.
    practice: usize,
    practice_len: Micros,
}

/// Every local全套动作 in the footage, in capture order, and what of the
/// takes was left out as not dancing.
///
/// On the beat, an action is what its people dance. Where the rhythm
/// reading found anyone standing still more than a beat and a half, or
/// someone watching what they hold, is marked to be skipped -- kept in the
/// take, so it still lines up with the others whole, and never shown -- and
/// so is a stretch of dancing shorter than a bar with none either side. An
/// action danced less than [`DANCED_AT_LEAST`] of is not a dance, and is
/// left out whole. Moving to no beat is not skipped: the reading cannot
/// tell it from dancing surely enough to cut a take on it.
fn find_locals(
    graph: &FootageGraph,
    canvas: (u32, u32),
    quality: &dyn Fn(usize, usize) -> f64,
    on_beat: bool,
) -> (Vec<Local>, LeftOut) {
    let mut out = Vec::new();
    let mut left_out = LeftOut::default();
    for (i, shot) in graph.shots.iter().enumerate() {
        let Some(source) = graph.source(&shot.source_id) else {
            continue;
        };
        // A photograph has no action.
        if source.is_still() {
            continue;
        }
        let picture = Picture::new(canvas, source);
        for (j, segment) in shot.segments.iter().enumerate() {
            let Some((_, usable_to)) = segment.usable() else {
                continue;
            };
            if segment.poses.len() < 3 {
                continue;
            }
            let frames: Vec<Frame> = segment
                .poses
                .iter()
                .map(|s| Frame::read(s, &picture))
                .collect();
            let rhythm = segment.rhythm.as_ref();
            let bpm = rhythm
                .filter(|r| r.confidence.unwrap_or(0.0) >= KEEPS_BEAT && r.bpm > 0.0)
                .map(|r| r.bpm);
            let beats = rhythm.map(|r| r.beats_us.clone()).unwrap_or_default();
            for run in runs(&frames) {
                let Some((from, to)) = acting_span(&run) else {
                    continue;
                };
                // On the beat, where its people are not dancing is skipped,
                // but stays in the take so it lines up with the others whole.
                // Through the footage after the action too, which a clip
                // near its end may run on into.
                let usable = usable_to.max(to);
                let mut skips: Vec<(Micros, Micros)> = Vec::new();
                if on_beat && let Some(r) = rhythm {
                    for span in r.idle.iter().chain(&r.watching) {
                        let (a, b) = (span.start_us.max(from), span.end_us.min(usable));
                        if b > a {
                            skips.push((a, b));
                        }
                    }
                    // A stretch of dancing shorter than a bar with no
                    // dancing either side is a step on its own.
                    let bar = Micros((SHORTEST_DANCE_BEATS * r.beat().0 as f64) as u64);
                    let stops = merge(std::mem::take(&mut skips));
                    let mut at = from;
                    let within = stops.iter().copied().filter(|(a, _)| *a < to);
                    for (a, b) in within.chain([(to, to)]) {
                        if a > at && a - at < bar {
                            skips.push((at, a));
                        }
                        at = at.max(b);
                    }
                    skips.extend(stops);
                }
                let merged = merge(skips);
                let people = run[0].people();
                let whole = Local {
                    shot: i,
                    segment: j,
                    picture,
                    frames: run,
                    from,
                    to,
                    usable_to: usable,
                    people,
                    quality: quality(i, j),
                    scene: segment
                        .scene_id
                        .clone()
                        .unwrap_or_else(|| segment.id.clone()),
                    bpm,
                    beats: beats.clone(),
                    skips: merged,
                    key: (
                        source.captured_seconds().unwrap_or(i64::MAX),
                        source.content_hash.to_hex(),
                        shot.start_us.0,
                        from.0,
                    ),
                };
                // Danced so little, it is not a dance.
                if (whole.danced().0 as f64) < DANCED_AT_LEAST * whole.len().0 as f64 {
                    left_out.practice += 1;
                    left_out.practice_len = left_out.practice_len + whole.len();
                    continue;
                }
                left_out.skipped = left_out.skipped + (whole.len() - whole.danced());
                // A take that performs the action more than once holds one
                // local全套动作 a performance.
                out.extend(performances(whole));
            }
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    (out, left_out)
}

/// Stretches in order, overlapping ones made one.
fn merge(mut stretches: Vec<(Micros, Micros)>) -> Vec<(Micros, Micros)> {
    stretches.sort();
    let mut merged: Vec<(Micros, Micros)> = Vec::new();
    for (a, b) in stretches {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// A followed stretch as the performances it holds: itself, or, when the
/// subject does the same thing over again -- a dance danced twice through
/// in one take -- one local全套动作 a time through.
fn performances(whole: Local) -> Vec<Local> {
    let Some(period) = repeat_period(&whole) else {
        return vec![whole];
    };
    let mut out: Vec<Local> = Vec::new();
    let mut start = whole.from;
    while start < whole.to {
        let mut end = start + period;
        // A remainder too short to be an action stays with the last one.
        if whole.to.saturating_sub(end) < SHORTEST_ACTION {
            end = whole.to;
        }
        let mut part = whole.clone();
        part.from = start;
        part.to = end.min(whole.to);
        part.key.3 = start.0;
        // Its own stops, and those of the footage after it.
        part.skips = whole
            .skips
            .iter()
            .map(|(a, b)| ((*a).max(part.from), *b))
            .filter(|(a, b)| b > a)
            .collect();
        out.push(part);
        start = end;
    }
    out
}

/// How long the action takes to come round again within one stretch, when
/// it does: the shortest offset at which the stretch matches itself as
/// closely and as distinctly as two takes of one action must, over at least
/// as much as two takes must share.
fn repeat_period(local: &Local) -> Option<Micros> {
    let frames: Vec<&Frame> = local.acting().filter(|f| f.bodies.is_some()).collect();
    let n = frames.len();
    if n < 2 * MIN_OVERLAP {
        return None;
    }
    let points = moving_points(frames.iter().copied());
    if points.len() < 2 {
        return None;
    }
    let step = ((n * n) as f64 / 40_000.0).sqrt().ceil() as usize;
    let frames: Vec<&Frame> = frames.into_iter().step_by(step.max(1)).collect();
    let n = frames.len();
    let need = (MIN_OVERLAP / step.max(1)).max(4);
    let mut lags: Vec<(usize, f64)> = Vec::new();
    for lag in need..=(n - need) {
        let mut sum = 0.0;
        let mut count = 0usize;
        for i in 0..(n - lag) {
            let j = i + lag;
            let best = [j - 1, j, j + 1]
                .into_iter()
                .filter(|&jj| jj < n && jj > i)
                .filter_map(|jj| frame_distance(frames[i], frames[jj], &points))
                .fold(f64::INFINITY, f64::min);
            if best.is_finite() {
                sum += best;
                count += 1;
            }
        }
        if count >= need {
            lags.push((lag, sum / count as f64));
        }
    }
    if lags.len() < 3 {
        return None;
    }
    let mut means: Vec<f64> = lags.iter().map(|l| l.1).collect();
    means.sort_by(f64::total_cmp);
    let typical = means[means.len() / 2];
    // The shortest lag of at least a phrase that matches well: a dance
    // danced three times matches itself at two periods too, and the first
    // is the period.
    // Matched over most of a performance, too: a stretch that resembles
    // itself for a few seconds near its two ends has not been danced twice.
    let seconds = |lag: usize| frames[lag].at.saturating_sub(frames[0].at);
    let shared = |lag: usize| frames[n - 1].at.saturating_sub(frames[lag].at);
    let long: Vec<&(usize, f64)> = lags
        .iter()
        .filter(|(lag, _)| {
            let (period, overlap) = (seconds(*lag), shared(*lag));
            period >= SHORTEST_REPEAT
                && overlap >= SHORTEST_REPEAT
                && overlap.0 as f64 >= 0.8 * period.0 as f64
        })
        .collect();
    let best = long.iter().map(|l| l.1).fold(f64::INFINITY, f64::min);
    let good = |m: f64| m <= ALIGN_MAX && m <= SELF_DISTINCT * typical && m <= best * 1.15;
    let (lag, _) = long.into_iter().find(|(_, m)| good(*m))?;
    Some(seconds(*lag))
}

/// The stretches a subject is followed through: readable frames of one
/// number of people, each allowed to lose a few samples and carry on.
fn runs(frames: &[Frame]) -> Vec<Vec<Frame>> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < frames.len() {
        let n = frames[k].people();
        if n == 0 {
            k += 1;
            continue;
        }
        let mut run = vec![frames[k].clone()];
        let mut missed = 0;
        let mut m = k + 1;
        while m < frames.len() {
            if frames[m].people() == n {
                run.push(frames[m].clone());
                missed = 0;
            } else {
                missed += 1;
                if missed > BRIDGE {
                    break;
                }
            }
            m += 1;
        }
        k = m.max(k + 1);
        if run.len() >= 3 {
            out.push(run);
        }
    }
    out
}

/// Where a followed stretch acts: `None` when it barely moves, else from
/// the rest before its movement to the rest after.
fn acting_span(run: &[Frame]) -> Option<(Micros, Micros)> {
    let steps: Vec<Option<f64>> = run.windows(2).map(|w| movement(&w[0], &w[1])).collect();
    let mut known: Vec<f64> = steps.iter().flatten().copied().collect();
    if known.len() < 2 {
        return None;
    }
    known.sort_by(f64::total_cmp);
    if known[known.len() / 2] < ACTING {
        return None;
    }
    let first = steps.iter().position(|s| s.is_some_and(|v| v >= REST))?;
    let last = steps.iter().rposition(|s| s.is_some_and(|v| v >= REST))?;
    let (from, to) = (run[first].at, run[last + 1].at);
    (to.saturating_sub(from) >= SHORTEST_ACTION).then_some((from, to))
}

// ---------------------------------------------------------------------------
// The global全套动作
// ---------------------------------------------------------------------------

/// How two takes of one action line up.
#[derive(Debug, Clone, Copy)]
struct Aligned {
    /// The mean pose difference over what they share, in torso lengths.
    cost: f64,
    /// How many samples they share.
    overlap: usize,
    /// Where the second starts on the action's axis, relative to the first.
    offset: i64,
}

/// The best way two local全套动作 line up, whether or not it is good enough,
/// and the typical pose gap over every offset tried.
///
/// Every offset is tried, each sample allowed a quarter second either way
/// for a dancer a little ahead or behind, comparing only the points that
/// move.
fn best_alignment(p: &Local, q: &Local) -> Option<(Aligned, f64)> {
    if p.people != q.people {
        return None;
    }
    let a: Vec<&Frame> = p.acting().filter(|f| f.bodies.is_some()).collect();
    let b: Vec<&Frame> = q.acting().filter(|f| f.bodies.is_some()).collect();
    // Long takes are compared at a coarser step, so a pair costs about the
    // same whatever its length.
    let step = (((a.len() * b.len()) as f64 / 40_000.0).sqrt().ceil() as usize).max(1);
    let a: Vec<&Frame> = a.into_iter().step_by(step).collect();
    let b: Vec<&Frame> = b.into_iter().step_by(step).collect();
    let need = (MIN_OVERLAP / step).max(4);
    if a.len() < need || b.len() < need {
        return None;
    }
    // What moves in either take is what the two are compared on.
    let mut points = moving_points(a.iter().copied());
    for k in moving_points(b.iter().copied()) {
        if !points.contains(&k) {
            points.push(k);
        }
    }
    if points.len() < 2 {
        return None;
    }
    let d: Vec<Vec<Option<f64>>> = a
        .iter()
        .map(|x| b.iter().map(|y| frame_distance(x, y, &points)).collect())
        .collect();
    let (al, bl) = (a.len() as i64, b.len() as i64);
    let mut lags: Vec<(i64, f64, usize)> = Vec::new();
    for lag in -(al - need as i64)..=(bl - need as i64) {
        let mut sum = 0.0;
        let mut n = 0usize;
        for (i, row) in d.iter().enumerate() {
            let j = i as i64 + lag;
            if j < 0 || j >= bl {
                continue;
            }
            let j = j as usize;
            let best = [j.checked_sub(1), Some(j), Some(j + 1)]
                .into_iter()
                .flatten()
                .filter_map(|jj| row.get(jj).copied().flatten())
                .fold(f64::INFINITY, f64::min);
            if best.is_finite() {
                sum += best;
                n += 1;
            }
        }
        if n >= need {
            lags.push((lag, sum / n as f64, n));
        }
    }
    if lags.len() < 3 {
        return None;
    }
    let mut means: Vec<f64> = lags.iter().map(|l| l.1).collect();
    means.sort_by(f64::total_cmp);
    let typical = means[means.len() / 2];
    let (lag, cost, overlap) = lags
        .iter()
        .copied()
        .min_by(|x, y| x.1.total_cmp(&y.1).then(y.2.cmp(&x.2)))?;
    // Sample i of the first is the same moment as sample i + lag of the
    // second; where each starts on the action's axis follows.
    let mut sum = 0i64;
    let mut n = 0i64;
    for (i, x) in a.iter().enumerate() {
        let j = i as i64 + lag;
        if j < 0 || j >= bl {
            continue;
        }
        let y = b[j as usize];
        sum += (x.at.0 as i64 - p.from.0 as i64) - (y.at.0 as i64 - q.from.0 as i64);
        n += 1;
    }
    Some((
        Aligned {
            cost,
            overlap: overlap * step,
            offset: sum / n.max(1),
        },
        typical,
    ))
}

/// One piece of the global全套动作: takes of one action, each placed on the
/// action's own time axis, or a single take no other repeats.
#[derive(Debug, Clone)]
struct Piece {
    /// Each take and where its action starts on the axis.
    takes: Vec<(usize, Micros)>,
    length: Micros,
}

/// Two offsets agree when they put a take within this much of one place.
const AGREE: i64 = 600_000;

/// Lay the local全套动作 out: takes of one action on one axis, every other
/// action on its own -- by what the takes show, and by nothing about the
/// order they were read in.
///
/// Real dance looks like itself: on the trip, two takes of different moves
/// line up nearly as well as two takes of the same ones, so no threshold on
/// one pair tells them apart. What a chance match cannot do is agree with
/// the rest -- if A lines up with B, and B with C, A must line up with C at
/// the sum. So the takes are placed one at a time, the take most aligned
/// with the others first; each next take goes where most of those already
/// placed put it, and joins only when more of them agree on the place than
/// not, and at least two do, or one alignment stands out on its own as a
/// pair's must. Two performances in one clip are one witness. What
/// disagrees with the place chosen is left out of it.
fn layout(locals: &[Local]) -> Vec<Piece> {
    let n = locals.len();
    // Every pair's best alignment close enough to consider, both ways:
    // edges[i] holds (j, where j starts relative to i, weight, distinct).
    let mut edges: Vec<Vec<(usize, i64, f64, bool)>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            let Some((a, typical)) = best_alignment(&locals[i], &locals[j]) else {
                continue;
            };
            if a.cost > ALIGN_MAX {
                continue;
            }
            let weight = a.overlap as f64 / (a.cost + 0.05);
            let distinct = a.cost <= DISTINCT * typical;
            edges[i].push((j, a.offset, weight, distinct));
            edges[j].push((i, -a.offset, weight, distinct));
        }
    }

    let mut placed: Vec<Option<i64>> = vec![None; n];
    let mut pieces = Vec::new();
    loop {
        // The next action starts from the take most aligned with those not
        // yet placed; ties go to the earlier capture.
        let strength = |i: usize| -> f64 {
            edges[i]
                .iter()
                .filter(|(j, ..)| placed[*j].is_none())
                .map(|(_, _, w, _)| w)
                .sum()
        };
        let Some(root) = (0..n)
            .filter(|&i| placed[i].is_none())
            .max_by(|&x, &y| strength(x).total_cmp(&strength(y)).then(y.cmp(&x)))
        else {
            break;
        };
        placed[root] = Some(0);
        let mut members = vec![root];
        loop {
            // For each take not yet placed: where the placed takes put it,
            // one vote a clip -- a take danced twice through is one witness,
            // not two -- and whether more of them agree than not.
            let mut best: Option<(usize, i64, (i64, usize, f64))> = None;
            for j in (0..n).filter(|&j| placed[j].is_none()) {
                let mut votes: Vec<(usize, i64, f64, bool)> = Vec::new();
                for (m, offset, w, distinct) in &edges[j] {
                    // j relative to m is minus m relative to j.
                    let Some(at) = placed[*m].map(|at| at - offset) else {
                        continue;
                    };
                    let clip = locals[*m].shot;
                    match votes.iter_mut().find(|v| v.0 == clip) {
                        Some(v) if v.2 >= *w => {}
                        Some(v) => *v = (clip, at, *w, *distinct),
                        None => votes.push((clip, at, *w, *distinct)),
                    }
                }
                for (_, centre, _, _) in &votes {
                    let near: Vec<&(usize, i64, f64, bool)> = votes
                        .iter()
                        .filter(|(_, at, ..)| (at - centre).abs() <= AGREE)
                        .collect();
                    let agree = near.len();
                    let disagree = votes.len() - agree;
                    let weight: f64 = near.iter().map(|(_, _, w, _)| w).sum();
                    let stands = agree > disagree
                        && (agree >= 2 || near.iter().any(|(.., distinct)| *distinct));
                    if !stands {
                        continue;
                    }
                    let at = (near.iter().map(|(_, at, w, _)| *at as f64 * w).sum::<f64>()
                        / weight.max(1e-9))
                    .round() as i64;
                    let rank = (agree as i64 - disagree as i64, agree, weight);
                    let better = match &best {
                        None => true,
                        Some((bj, _, br)) => {
                            (rank.0, rank.1) > (br.0, br.1)
                                || ((rank.0, rank.1) == (br.0, br.1)
                                    && (rank.2 > br.2 || (rank.2 == br.2 && j < *bj)))
                        }
                    };
                    if better {
                        best = Some((j, at, rank));
                    }
                }
            }
            match best {
                Some((j, at, _)) => {
                    placed[j] = Some(at);
                    members.push(j);
                }
                None => break,
            }
        }
        let lowest = members.iter().filter_map(|&m| placed[m]).min().unwrap_or(0);
        let mut takes: Vec<(usize, Micros)> = members
            .iter()
            .map(|&m| (m, Micros((placed[m].unwrap_or(0) - lowest).max(0) as u64)))
            .collect();
        takes.sort_by_key(|(m, at)| (*at, *m));
        let length = takes
            .iter()
            .map(|(m, at)| *at + locals[*m].len())
            .max()
            .unwrap_or(Micros::ZERO);
        pieces.push(Piece { takes, length });
    }
    pieces
}

/// The order the pieces play in: each running into the one whose start
/// its end joins best, capture order breaking ties -- so actions that do
/// not join at all keep the order they were filmed in.
fn order_pieces(pieces: &[Piece], locals: &[Local]) -> Vec<usize> {
    let m = pieces.len();
    if m <= 1 {
        return (0..m).collect();
    }
    // A piece's first moment and last, and the takes that show them.
    let first = |p: &Piece| p.takes.iter().find(|(_, at)| *at == Micros::ZERO).copied();
    let last = |p: &Piece| {
        p.takes
            .iter()
            .max_by_key(|(t, at)| (*at + locals[*t].len(), std::cmp::Reverse(*t)))
            .copied()
    };
    let mut cost = vec![vec![MISMATCH; m]; m];
    for (x, row) in cost.iter_mut().enumerate() {
        for (y, cell) in row.iter_mut().enumerate() {
            if x == y {
                continue;
            }
            if let (Some((a, _)), Some((b, _))) = (last(&pieces[x]), first(&pieces[y])) {
                let (a, b) = (&locals[a], &locals[b]);
                *cell = fit_join(a, a.to, b, b.from, false).cost;
            }
        }
    }
    let filmed = |p: &Piece| {
        p.takes
            .iter()
            .map(|(t, _)| locals[*t].key.clone())
            .min()
            .expect("a piece has a take")
    };
    let mut by_capture: Vec<usize> = (0..m).collect();
    by_capture.sort_by_key(|&i| filmed(&pieces[i]));

    let total = |order: &[usize]| -> f64 { order.windows(2).map(|w| cost[w[0]][w[1]]).sum() };
    if m <= 6 {
        // Every order; the cheapest, and among equals the one closest to
        // capture order.
        let mut best = by_capture.clone();
        let mut best_cost = total(&best);
        let mut order = by_capture.clone();
        permutations(&mut order, 0, &mut |candidate| {
            let c = total(candidate);
            if c < best_cost - 1e-9 {
                best_cost = c;
                best = candidate.to_vec();
            }
        });
        return best;
    }
    // Too many to try every order: from the first filmed, each next the
    // cheapest join from the one before.
    let mut out = vec![by_capture[0]];
    let mut left: Vec<usize> = by_capture[1..].to_vec();
    while !left.is_empty() {
        let from = *out.last().expect("not empty");
        let k = left
            .iter()
            .enumerate()
            .min_by(|(i, a), (j, b)| cost[from][**a].total_cmp(&cost[from][**b]).then(i.cmp(j)))
            .map(|(k, _)| k)
            .expect("not empty");
        out.push(left.remove(k));
    }
    out
}

/// Visit every ordering of `items[k..]`, in lexicographic order of swaps.
fn permutations(items: &mut Vec<usize>, k: usize, visit: &mut dyn FnMut(&[usize])) {
    if k == items.len() {
        visit(items);
        return;
    }
    for i in k..items.len() {
        items.swap(k, i);
        permutations(items, k + 1, visit);
        items.swap(k, i);
    }
}

// ---------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------

/// One clip as walked out of a piece.
#[derive(Debug, Clone)]
struct Step {
    local: usize,
    piece: usize,
    /// Where on the piece's axis it starts.
    axis: Micros,
    source_in: Micros,
    /// On screen.
    len: Micros,
    /// How fast it plays.
    speed: f64,
    /// Shows all of the axis it spans: no shortening left a gap after it.
    full: bool,
}

impl Step {
    /// How much of its source `len` on screen plays.
    fn span_of(&self, len: Micros) -> Micros {
        Micros((len.0 as f64 * self.speed) as u64)
    }
}

/// Walk a piece's axis from start to end in clips of the cut's length,
/// each from whichever take shows that stretch best -- a different take
/// from the clip before where there is one, the switch at the same moment
/// of the action.
fn walk(
    p: usize,
    piece: &Piece,
    locals: &[Local],
    params: &Params,
    speed: f64,
    snap: bool,
    unit: Micros,
) -> Vec<Step> {
    let whole = |d: Micros| match params.beat.filter(|b| b.0 > 0) {
        Some(b) => Micros(d.0 / b.0 * b.0),
        None => d,
    };
    let on_screen = |axis: Micros| Micros((axis.0 as f64 / speed) as u64);
    let on_axis = |len: Micros| Micros((len.0 as f64 * speed) as u64);
    let end = piece.length;
    let mut out: Vec<Step> = Vec::new();
    let mut x = Micros::ZERO;
    // Whether the next clip starts the action, or starts it again past a
    // stretch nobody dances, rather than switching from the clip before.
    let mut fresh = true;
    while x < end {
        let left = on_screen(end - x);
        let mut want = whole(params.clip.min(left));
        if want < unit.min(left) || want == Micros::ZERO {
            // Less than a beat of the action is left: it is not cut to.
            break;
        }
        // The takes dancing at x, and how long each dances on from there.
        let dancing: Vec<(usize, Micros, Micros)> = piece
            .takes
            .iter()
            .filter(|(t, at)| *at <= x && *at + locals[*t].len() > x)
            .filter_map(|&(t, at)| {
                let local = &locals[t];
                let source_in = local.from + (x - at);
                local
                    .dancing_until(source_in)
                    .map(|until| (t, source_in, until - source_in))
            })
            .collect();
        // Those whose footage holds the clip.
        let mut options: Vec<(usize, Micros)> = dancing
            .iter()
            .filter(|(_, _, room)| *room >= on_axis(want))
            .map(|(t, source_in, _)| (*t, *source_in))
            .collect();
        if options.is_empty() {
            // No take holds a whole clip from here: the one that dances
            // furthest, for as many whole beats as it holds.
            let reach = dancing
                .iter()
                .copied()
                .max_by_key(|(t, _, room)| (*room, std::cmp::Reverse(*t)))
                .filter(|(_, _, room)| whole(on_screen(*room)) >= unit);
            let Some((t, source_in, room)) = reach else {
                // Nobody dances a beat from here -- the takes stand idle, or
                // are about to -- so the action goes on where the next one
                // dances again: past the idle stretch ahead of a take that
                // has started, from the start of one that has not.
                let next = piece
                    .takes
                    .iter()
                    .filter_map(|&(t, at)| {
                        let local = &locals[t];
                        let here = local.from + x.saturating_sub(at);
                        let past = if at <= x {
                            local.dancing_until(here).unwrap_or(here)
                        } else {
                            here
                        };
                        local.dances_from(past).map(|from| at + (from - local.from))
                    })
                    .filter(|axis| *axis > x)
                    .min();
                match next {
                    Some(axis) => {
                        x = axis;
                        fresh = true;
                        continue;
                    }
                    None => break,
                }
            };
            want = whole(on_screen(room));
            options.push((t, source_in));
        }
        let previous = out.last().cloned();
        // The clips just shown, so the action travels between places
        // rather than ping-ponging between two.
        let recent: Vec<usize> = out
            .iter()
            .rev()
            .take(3)
            .map(|step| locals[step.local].shot)
            .collect();
        let score = |&(t, source_in): &(usize, Micros)| -> f64 {
            let local = &locals[t];
            let mut s = local.quality;
            // A take whose action ends partway through the clip would show
            // its subject standing still for the rest.
            let acting = local.to.saturating_sub(source_in).0 as f64;
            s -= 1.0 - (acting / on_axis(want).0.max(1) as f64).min(1.0);
            if let Some(prev) = &previous {
                let before = &locals[prev.local];
                if prev.local == t {
                    // Carrying on is one longer clip: only when no other
                    // take shows the moment as well.
                    s -= 0.4;
                } else {
                    if before.shot == local.shot {
                        // The same clip's other performance: the place does
                        // not change, and the cut reads as a jump.
                        s -= 0.4;
                    } else if before.scene == local.scene {
                        s -= 0.2;
                    }
                    if recent.contains(&local.shot) {
                        s -= 0.15;
                    }
                    let out_at = prev.source_in + on_axis(prev.len);
                    s -= 0.5 * fit_join(before, out_at, local, source_in, true).cost;
                }
            }
            s
        };
        let (t, mut source_in) = options
            .iter()
            .copied()
            .max_by(|a, b| {
                score(a)
                    .total_cmp(&score(b))
                    .then(locals[b.0].key.cmp(&locals[a.0].key))
            })
            .expect("options is not empty");
        // To within a millisecond: a clip grown a beat at a time and the
        // axis it covers round apart by a microsecond a beat.
        let carrying_on = out.last().is_some_and(|prev| {
            prev.local == t && (prev.source_in + on_axis(prev.len)).0.abs_diff(source_in.0) <= 1_000
        });
        // On the beat, a new clip starts on its take's own beat, so the
        // dancers' beats fall on the cut's from its first frame. A switch
        // moves to the nearest of them: a read beat sits up to a fifth of a
        // period off an even grid, so one is always within three quarters
        // of a period. A start -- the action's, or after an idle stretch --
        // moves on to the first beat it reaches, never back into the rest.
        if snap && !carrying_on {
            let local = &locals[t];
            let period = local
                .bpm
                .map(|bpm| Micros((60_000_000.0 / bpm) as u64))
                .unwrap_or(Micros(2 * unit.0));
            // How many whole beats of the cut its people dance on from a
            // beat of theirs: a clip may lose a few to start on one.
            let holds = |b: Micros| {
                local
                    .dancing_until(b)
                    .map(|u| whole(on_screen(u.saturating_sub(b))))
                    .filter(|_| b >= local.from)
                    .unwrap_or(Micros::ZERO)
            };
            let candidates = local.beats.iter().copied().filter(|b| holds(*b) >= unit);
            let nearest = if fresh {
                candidates
                    .filter(|b| *b >= source_in)
                    .min()
                    .filter(|b| (*b - source_in).0 <= period.0 * 3 / 2)
            } else {
                candidates
                    .min_by_key(|b| b.0.abs_diff(source_in.0))
                    .filter(|b| b.0.abs_diff(source_in.0) <= period.0 * 3 / 4)
            };
            if let Some(beat) = nearest {
                want = want.min(holds(beat));
                x = Micros((x.0 as i64 + beat.0 as i64 - source_in.0 as i64).max(0) as u64);
                source_in = beat;
            }
        }
        match out.last_mut() {
            // The same take carrying on: one longer clip, not a jump cut.
            Some(prev) if carrying_on => {
                prev.len = prev.len + want;
            }
            _ => out.push(Step {
                local: t,
                piece: p,
                axis: x,
                source_in,
                len: want,
                speed,
                full: true,
            }),
        }
        x = x + on_axis(want);
        fresh = false;
    }
    out
}

/// After shortening, each clip's start is chosen again within the stretch
/// it used to cover, for the least jump from the clip before.
fn choose_starts_again(steps: &mut [Step], locals: &[Local]) {
    for k in 0..steps.len() {
        steps[k].full = false;
        if k == 0 {
            continue;
        }
        let (before, rest) = steps.split_at_mut(k);
        let prev = &before[k - 1];
        let step = &mut rest[0];
        let local = &locals[step.local];
        let out_at = prev.source_in + prev.span_of(prev.len);
        let span = step.span_of(step.len);
        // As far on as the clip's old stretch and its footage allow.
        let latest = local
            .usable_to
            .saturating_sub(span)
            .min(step.source_in + span);
        let mut best = (
            step.source_in,
            fit_join(&locals[prev.local], out_at, local, step.source_in, false).cost,
        );
        for frame in &local.frames {
            if frame.at <= step.source_in || frame.at > latest {
                continue;
            }
            let c = fit_join(&locals[prev.local], out_at, local, frame.at, false).cost;
            if c < best.1 - 1e-9 {
                best = (frame.at, c);
            }
        }
        step.source_in = best.0;
    }
}

// ---------------------------------------------------------------------------
// Joins
// ---------------------------------------------------------------------------

/// What one join needs and costs.
#[derive(Debug, Clone, Copy)]
struct Fit {
    /// The outgoing clip's placement at its last frame.
    out: Side,
    /// The incoming clip's at its first.
    inn: Side,
    cost: f64,
}

/// The similarity carrying points `from` onto points `to` -- enlarged,
/// turned clockwise and moved, `p -> s R p + t` -- by least squares; turned
/// only when `turn` allows. `None` when the points cannot fix one.
fn similarity(
    from: &[(f64, f64)],
    to: &[(f64, f64)],
    turn: bool,
) -> Option<(f64, f64, (f64, f64))> {
    let n = from.len();
    if n < 2 || n != to.len() {
        return None;
    }
    let mean = |v: &[(f64, f64)]| {
        (
            v.iter().map(|p| p.0).sum::<f64>() / n as f64,
            v.iter().map(|p| p.1).sum::<f64>() / n as f64,
        )
    };
    let (ms, md) = (mean(from), mean(to));
    let (mut a, mut b, mut spread) = (0.0, 0.0, 0.0);
    for (p, q) in from.iter().zip(to) {
        let (xs, ys) = (p.0 - ms.0, p.1 - ms.1);
        let (xd, yd) = (q.0 - md.0, q.1 - md.1);
        a += xs * xd + ys * yd;
        b += xs * yd - ys * xd;
        spread += xs * xs + ys * ys;
    }
    if spread < 1e-12 {
        return None;
    }
    let (scale, angle) = if turn {
        ((a * a + b * b).sqrt() / spread, b.atan2(a))
    } else {
        (a / spread, 0.0)
    };
    if !(scale.is_finite() && scale > 1e-3) {
        return None;
    }
    let (sin, cos) = angle.sin_cos();
    let t = (
        md.0 - scale * (ms.0 * cos - ms.1 * sin),
        md.1 - scale * (ms.0 * sin + ms.1 * cos),
    );
    Some((scale, angle, t))
}

/// How the outgoing take at `a_at` joins the incoming one at `b_at`, and the
/// placements that make the join: the subject's size, place and -- at the
/// same moment of an action, where a lean is the camera's -- its lean
/// matched by enlarging whichever picture shows it smaller.
fn fit_join(a: &Local, a_at: Micros, b: &Local, b_at: Micros, same_moment: bool) -> Fit {
    let mismatch = Fit {
        out: Side::FITTED,
        inn: Side::FITTED,
        cost: MISMATCH,
    };
    let (Some(ia), Some(ib)) = (a.frame_near(a_at), b.frame_near(b_at)) else {
        return mismatch;
    };
    let (fa, fb) = (&a.frames[ia], &b.frames[ib]);
    let (Some(xa), Some(xb)) = (fa.bodies.as_ref(), fb.bodies.as_ref()) else {
        return mismatch;
    };
    if xa.len() != xb.len() {
        return mismatch;
    }
    let mut from = Vec::new();
    let mut to = Vec::new();
    for (p, q) in xa.iter().zip(xb) {
        for k in ANCHORS {
            if let (Some(u), Some(v)) = (q.points[k], p.points[k]) {
                from.push(u);
                to.push(v);
            }
        }
    }
    let Some((scale, turn, _)) = similarity(&from, &to, same_moment) else {
        return mismatch;
    };
    // Only ever enlarge: the side whose subject is smaller moves to meet
    // the other -- enlarged to the other's size, never past it, and its
    // subject's middle carried onto the other's as far as the frame's edge
    // allows at that size. The size is the one thing the picture is
    // enlarged for; a place it cannot reach at that size stays in the cost.
    let enlarge_in = scale >= 1.0;
    let (picture, own, other, base, lean) = if enlarge_in {
        (&b.picture, &from, &to, scale, turn)
    } else {
        (&a.picture, &to, &from, 1.0 / scale, -turn)
    };
    let centre = |v: &[(f64, f64)]| {
        let n = v.len().max(1) as f64;
        (
            v.iter().map(|p| p.0).sum::<f64>() / n,
            v.iter().map(|p| p.1).sum::<f64>() / n,
        )
    };
    let torso = |bodies: &[Body]| bodies.iter().map(|b| b.torso).sum::<f64>() / bodies.len() as f64;
    let (placed, _) = bounded(
        picture,
        Side::placing(picture, base, lean, centre(own), centre(other)),
    );
    let (out, inn) = if enlarge_in {
        (Side::FITTED, placed)
    } else {
        (placed, Side::FITTED)
    };

    // What the viewer sees either side of the cut.
    let seen_a: Vec<(f64, f64)> = to.iter().map(|p| out.apply(&a.picture, *p)).collect();
    let seen_b: Vec<(f64, f64)> = from.iter().map(|p| inn.apply(&b.picture, *p)).collect();
    let moved = seen_a
        .iter()
        .zip(&seen_b)
        .map(|(p, q)| distance(*p, *q))
        .sum::<f64>()
        / seen_a.len().max(1) as f64;
    let (ta, tb) = (torso(xa) * out.scale, torso(xb) * inn.scale);
    let resized = (ta.max(1e-6) / tb.max(1e-6)).ln().abs();
    let mut shape = 0.0;
    for (p, q) in xa.iter().zip(xb) {
        shape += shape_distance(p, q, inn.turn - out.turn).unwrap_or(MISMATCH);
    }
    let shape = shape / xa.len() as f64;
    let motion = velocity_gap(a, ia, out, b, ib, inn, ta).unwrap_or(2.0);
    Fit {
        out,
        inn,
        cost: shape + 0.5 * (2.0 * moved + resized) + 0.15 * motion,
    }
}

/// How differently the bodies move either side of a cut, as the viewer
/// sees them: the mean gap between the two motions of each shared point, in
/// torso lengths a second. Into the outgoing frame, and out of the incoming
/// one.
fn velocity_gap(
    a: &Local,
    ia: usize,
    out: Side,
    b: &Local,
    ib: usize,
    inn: Side,
    torso: f64,
) -> Option<f64> {
    let before = &a.frames[ia.checked_sub(1)?];
    let at_a = &a.frames[ia];
    let at_b = &b.frames[ib];
    let after = b.frames.get(ib + 1)?;
    let da = at_a.at.0.checked_sub(before.at.0)? as f64 / 1e6;
    let db = after.at.0.checked_sub(at_b.at.0)? as f64 / 1e6;
    if !(da > 0.0 && da <= 1.0 && db > 0.0 && db <= 1.0) {
        return None;
    }
    let (x0, x1) = (before.bodies.as_ref()?, at_a.bodies.as_ref()?);
    let (y0, y1) = (at_b.bodies.as_ref()?, after.bodies.as_ref()?);
    if x0.len() != x1.len() || y0.len() != y1.len() || x1.len() != y0.len() {
        return None;
    }
    let mut sum = 0.0;
    let mut n = 0usize;
    for (((p0, p1), q0), q1) in x0.iter().zip(x1).zip(y0).zip(y1) {
        for k in MOVING_POINTS {
            let (Some(u0), Some(u1), Some(v0), Some(v1)) =
                (p0.points[k], p1.points[k], q0.points[k], q1.points[k])
            else {
                continue;
            };
            let va = out.carry(((u1.0 - u0.0) / da, (u1.1 - u0.1) / da));
            let vb = inn.carry(((v1.0 - v0.0) / db, (v1.1 - v0.1) / db));
            sum += distance(va, vb) / torso.max(1e-6);
            n += 1;
        }
    }
    (n >= SHARED).then(|| sum / n as f64)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vb_contracts::footage_graph::{Segment, Shot};
    use vb_contracts::{Blake3Hash, FootageGraph};

    /// The choreography the tests' dancers perform: where each wrist is, as
    /// a share of the way from the hip to above the head, at time `t`
    /// seconds into it. Never the same twice in the tests' minute.
    fn routine(t: f64) -> (f64, f64) {
        let m = moves(t);
        (m[0], m[1])
    }

    /// Every limb of the choreography at `t`: each wrist's height, how far
    /// the arms reach out, how far the knees bend. Each a sweep whose pace
    /// drifts, so no stretch of it comes round again in the tests' minute,
    /// as no stretch of a real dance does.
    fn moves(t: f64) -> [f64; 6] {
        let sweep = |pace: f64, phase: f64, drift: f64| (pace * t + phase + drift * t * t).sin();
        [
            0.5 + 0.45 * sweep(1.7, 0.0, 0.031),
            0.5 + 0.45 * sweep(1.2, 1.0, 0.017),
            0.5 * sweep(0.9, 2.0, 0.011),
            0.5 * sweep(1.4, 0.7, -0.013),
            0.5 + 0.5 * sweep(0.75, 0.4, 0.023),
            0.5 + 0.5 * sweep(1.05, 2.6, -0.019),
        ]
    }

    /// A person `size` of the frame tall standing at `x`, wrists raised as
    /// the routine has them at `t` -- or, kicking, the arms held and the
    /// knees raised instead.
    fn dancer(x: f64, size: f64, t: f64, kicks: bool) -> PersonPose {
        let [left, right, reach_l, reach_r, bend_l, bend_r] = if kicks {
            [0.3, 0.3, 0.0, 0.0, 0.0, 0.0]
        } else {
            moves(t)
        };
        let (left_leg, right_leg) = if kicks {
            routine(1.6 * t)
        } else {
            (0.6 * bend_l, 0.6 * bend_r)
        };
        let top = 0.5 - size / 2.0;
        let y = |f: f64| top + f * size;
        let w = size / 4.0;
        let mut k = vec![[x, y(0.05), 0.9]; 17];
        k[5] = [x - w / 2.0, y(0.2), 0.9];
        k[6] = [x + w / 2.0, y(0.2), 0.9];
        k[7] = [x - w * 0.8, y(0.4 - 0.25 * left), 0.9];
        k[8] = [x + w * 0.8, y(0.4 - 0.25 * right), 0.9];
        k[9] = [x - w * (1.0 + reach_l), y(0.55 - 0.55 * left), 0.9];
        k[10] = [x + w * (1.0 + reach_r), y(0.55 - 0.55 * right), 0.9];
        k[11] = [x - w / 3.0, y(0.55), 0.9];
        k[12] = [x + w / 3.0, y(0.55), 0.9];
        k[13] = [x - w / 3.0, y(0.75 - 0.2 * left_leg), 0.9];
        k[14] = [x + w / 3.0, y(0.75 - 0.2 * right_leg), 0.9];
        k[15] = [x - w / 3.0, y(0.98 - 0.35 * left_leg), 0.9];
        k[16] = [x + w / 3.0, y(0.98 - 0.35 * right_leg), 0.9];
        PersonPose {
            bbox: [x - w, top, x + w, top + size],
            score: 0.95,
            keypoints: k,
        }
    }

    /// A take of the routine: its seconds `from..to` of the routine, filmed
    /// with the dancer at `x` and `size` of the frame tall, the recording
    /// starting `lead` seconds before the routine's `from`.
    pub(crate) struct Take {
        name: &'static str,
        captured: i64,
        from: f64,
        to: f64,
        x: f64,
        size: f64,
        lead: f64,
        scene: &'static str,
        kicks: bool,
    }

    pub(crate) fn graph(takes: &[Take]) -> FootageGraph {
        let mut g = FootageGraph::new("continuity");
        for take in takes {
            let length = take.lead + (take.to - take.from) + 1.0;
            g.sources.push(Source {
                id: take.name.into(),
                path: format!("/{}.mp4", take.name),
                content_hash: Blake3Hash::of_str(take.name),
                media_kind: None,
                duration_us: Micros::from_secs_f64(length),
                fps_num: Some(30),
                fps_den: Some(1),
                width: Some(1920),
                height: Some(1080),
                video_codec: None,
                audio_codec: None,
                has_audio: Some(false),
                created_at: Some(chrono_like(take.captured)),
                rotation: None,
                categories: Vec::new(),
            });
            let mut poses = Vec::new();
            let mut t = 0.0;
            while t <= length - 0.5 {
                // Standing still before the routine and after it.
                let routine_t = (take.from + (t - take.lead)).clamp(take.from, take.to);
                poses.push(PoseSample {
                    at_us: Micros::from_secs_f64(t),
                    people: vec![dancer(take.x, take.size, routine_t, take.kicks)],
                });
                t += 0.25;
            }
            let end = Micros::from_secs_f64(length);
            g.shots.push(Shot {
                id: format!("{}-shot", take.name),
                source_id: take.name.into(),
                start_us: Micros::ZERO,
                end_us: end,
                shot_change_confidence: None,
                keyframes: vec![],
                description: None,
                entity_refs: vec![],
                actions: vec![],
                audio: None,
                tags: vec![],
                segments: vec![Segment {
                    id: format!("{}-seg", take.name),
                    start_us: Micros::ZERO,
                    end_us: end,
                    usable_start_us: Some(Micros::ZERO),
                    usable_end_us: Some(end),
                    scene_id: Some(take.scene.into()),
                    quality: Some(0.8),
                    camera: None,
                    keyframe_id: None,
                    description: None,
                    signature: vec![],
                    occlusion: None,
                    composition: None,
                    caption: None,
                    people: Some(1),
                    poses,
                    rhythm: None,
                }],
                timeline: vec![],
            });
        }
        g
    }

    /// An RFC 3339 time `minutes` past a fixed morning.
    fn chrono_like(minutes: i64) -> String {
        format!("2026-09-29T10:{:02}:00Z", minutes.clamp(0, 59))
    }

    /// On the beat, at 120 BPM.
    fn params(room: Option<Micros>) -> Params {
        Params {
            canvas: (1920, 1080),
            beat: Some(Micros(500_000)),
            clip: Micros(2_000_000),
            min_clip: Micros(1_000_000),
            room,
        }
    }

    fn off_the_beat(room: Option<Micros>) -> Params {
        Params {
            beat: None,
            ..params(room)
        }
    }

    /// The beat the takes' dancers keep, as the rhythm reading records it:
    /// `bpm`, a beat every period of the routine from its start, and, for
    /// each take, the routine's seconds it stands idle.
    pub(crate) fn keep_beat(
        g: &mut FootageGraph,
        takes: &[Take],
        bpm: f64,
        idle: &[(&str, f64, f64)],
    ) {
        let period = 60.0 / bpm;
        for (shot, take) in g.shots.iter_mut().zip(takes) {
            let source = |routine: f64| Micros::from_secs_f64(take.lead + routine - take.from);
            let mut beats = Vec::new();
            let mut n = (take.from / period).ceil();
            while n * period <= take.to {
                beats.push(source(n * period));
                n += 1.0;
            }
            let idle = idle
                .iter()
                .filter(|(name, ..)| *name == take.name)
                .map(|(_, a, b)| vb_contracts::footage_graph::Span {
                    start_us: source(*a),
                    end_us: source(*b),
                })
                .collect();
            shot.segments[0].rhythm = Some(vb_contracts::footage_graph::Rhythm {
                bpm,
                confidence: Some(0.6),
                beats_us: beats,
                idle,
                watching: Vec::new(),
                unrhythmic: Vec::new(),
            });
        }
    }

    /// The routine's seconds each named take spends with someone watching
    /// what they hold, as the rhythm reading records it, after
    /// [`keep_beat`].
    fn watched(g: &mut FootageGraph, takes: &[Take], spans: &[(&str, f64, f64)]) {
        for (shot, take) in g.shots.iter_mut().zip(takes) {
            let source = |routine: f64| Micros::from_secs_f64(take.lead + routine - take.from);
            let rhythm = shot.segments[0].rhythm.as_mut().expect("keep_beat first");
            rhythm.watching = spans
                .iter()
                .filter(|(name, ..)| *name == take.name)
                .map(|(_, a, b)| vb_contracts::footage_graph::Span {
                    start_us: source(*a),
                    end_us: source(*b),
                })
                .collect();
        }
    }

    fn quality(_: usize, _: usize) -> f64 {
        0.8
    }

    /// Three takes of one routine, overlapping: 0-20 s, 15-35 s, 30-50 s,
    /// filmed in three places with the dancer standing differently.
    pub(crate) fn three_takes() -> Vec<Take> {
        vec![
            Take {
                name: "lake",
                captured: 10,
                from: 0.0,
                to: 20.0,
                x: 0.5,
                size: 0.8,
                lead: 1.0,
                scene: "lake",
                kicks: false,
            },
            Take {
                name: "hill",
                captured: 20,
                from: 15.0,
                to: 35.0,
                x: 0.45,
                size: 0.65,
                lead: 1.5,
                scene: "hill",
                kicks: false,
            },
            Take {
                name: "beach",
                captured: 30,
                from: 30.0,
                to: 50.0,
                x: 0.55,
                size: 0.7,
                lead: 0.5,
                scene: "beach",
                kicks: false,
            },
        ]
    }

    /// Where in the routine a clip's start falls, from the take it is cut
    /// from.
    fn routine_time(g: &FootageGraph, takes: &[Take], clip: &PlannedClip) -> f64 {
        let take = &takes[g
            .sources
            .iter()
            .position(|s| s.id == g.shots[clip.shot].source_id)
            .unwrap()];
        take.from + (clip.in_us.as_secs_f64() - take.lead)
    }

    #[test]
    fn a_routine_filmed_in_three_places_plays_through_once() {
        let takes = three_takes();
        let g = graph(&takes);
        let plan = plan(&g, &params(None), &quality).expect("an action");
        assert_eq!(plan.report.pieces, 1, "{:?}", plan.report);
        assert!(plan.clips.iter().all(|c| (c.speed - 1.0).abs() < 1e-9));
        // The clips walk the routine forward, never back, from near its
        // start to near its end.
        let mut t = -1.0;
        for clip in &plan.clips {
            let at = routine_time(&g, &takes, clip);
            assert!(at > t - 0.3, "the routine went back from {t} to {at}");
            t = at + clip.len.as_secs_f64();
        }
        let first = routine_time(&g, &takes, &plan.clips[0]);
        assert!(first < 1.0, "starts {first} s into the routine");
        assert!(t > 47.0, "ends {t} s into the routine");
        // All three places are in it.
        let mut shots: Vec<usize> = plan.clips.iter().map(|c| c.shot).collect();
        shots.sort_unstable();
        shots.dedup();
        assert_eq!(shots.len(), 3);
    }

    #[test]
    fn the_takes_join_at_the_same_moment_and_carry_the_action() {
        let takes = three_takes();
        let g = graph(&takes);
        let plan = plan(&g, &params(None), &quality).unwrap();
        let switches: Vec<&PlannedClip> = plan
            .clips
            .iter()
            .filter(|c| c.join.is_some_and(|j| j.same_moment))
            .collect();
        assert!(!switches.is_empty());
        for clip in switches {
            let join = clip.join.unwrap();
            assert!(join.carried, "{join:?}");
        }
    }

    #[test]
    fn the_order_the_files_arrive_in_changes_nothing() {
        let takes = three_takes();
        let mut reversed = three_takes();
        reversed.reverse();
        let (a, b) = (graph(&takes), graph(&reversed));
        let (pa, pb) = (
            plan(&a, &params(None), &quality).unwrap(),
            plan(&b, &params(None), &quality).unwrap(),
        );
        let seq = |g: &FootageGraph, p: &Plan| -> Vec<(String, u64, u64)> {
            p.clips
                .iter()
                .map(|c| (g.shots[c.shot].source_id.clone(), c.in_us.0, c.len.0))
                .collect()
        };
        assert_eq!(seq(&a, &pa), seq(&b, &pb));
    }

    #[test]
    fn a_smaller_subject_is_enlarged_to_match_and_eased_back() {
        let takes = three_takes();
        let g = graph(&takes);
        let plan = plan(&g, &params(None), &quality).unwrap();
        // The lake take frames the dancer at 0.8 of the frame, the hill at
        // 0.65: cutting lake to hill enlarges the hill picture, within what
        // a 1080p picture in a 1080p cut allows.
        let into_hill = plan
            .clips
            .iter()
            .find(|c| g.shots[c.shot].source_id == "hill" && c.join.is_some_and(|j| j.same_moment))
            .expect("a cut into the hill take");
        let join = into_hill.join.unwrap();
        assert!(join.scale > 1.05 && join.scale <= 1.25 + 1e-9, "{join:?}");
        let keys = &into_hill.transform.as_ref().expect("framed").keyframes;
        assert!(keys[0].scale > 1.05);
        if let Some(last) = keys.get(1) {
            assert_eq!(last.offset_us, into_hill.len);
        }
    }

    #[test]
    fn off_the_beat_a_room_too_small_speeds_every_clip_up_alike_up_to_a_fifth() {
        let takes = three_takes();
        let g = graph(&takes);
        // About fifty seconds of action into forty-five: one shared speed.
        let fit = plan(&g, &off_the_beat(Some(Micros::from_secs(45))), &quality).unwrap();
        let first = fit.clips[0].speed;
        assert!(first > 1.05 && first <= MAX_SPEED, "{first}");
        assert!(fit.clips.iter().all(|c| (c.speed - first).abs() < 1e-12));
        let total: u64 = fit.clips.iter().map(|c| c.len.0).sum();
        assert!(total <= 45_000_000);
        // Into thirty, the top speed is not enough: every clip is shortened.
        let tight = plan(&g, &off_the_beat(Some(Micros::from_secs(30))), &quality).unwrap();
        assert!(
            tight
                .clips
                .iter()
                .all(|c| (c.speed - MAX_SPEED).abs() < 1e-9)
        );
        assert!(tight.report.shortened);
    }

    #[test]
    fn on_the_beat_a_room_too_small_shortens_every_clip_and_speeds_nothing_up() {
        let takes = three_takes();
        let g = graph(&takes);
        let plan = plan(&g, &params(Some(Micros::from_secs(20))), &quality).unwrap();
        assert!(plan.clips.iter().all(|c| (c.speed - 1.0).abs() < 1e-9));
        let total: u64 = plan.clips.iter().map(|c| c.len.0).sum();
        // Exactly the length chosen: whole beats given back where rounding
        // took them.
        assert_eq!(total, 20_000_000);
        assert!(plan.report.shortened && plan.report.shown < 0.9);
        // Still from the routine's start into its last part: each clip is
        // shortened from where it began, none left out.
        let last = plan.clips.last().unwrap();
        assert!(routine_time(&g, &takes, last) >= 30.0);
    }

    #[test]
    fn a_dance_plays_at_the_speed_that_puts_its_own_beat_on_the_cuts() {
        let takes = three_takes();
        let mut g = graph(&takes);
        keep_beat(&mut g, &takes, 112.0, &[]);
        let plan = plan(&g, &params(None), &quality).unwrap();
        // 112 BPM onto 120: every clip at 120/112.
        let piece = plan.report.speeds[0];
        assert!(piece.on_beat, "{piece:?}");
        assert!((piece.speed - 120.0 / 112.0).abs() < 1e-3, "{piece:?}");
        assert!(
            plan.clips
                .iter()
                .all(|c| (c.speed - piece.speed).abs() < 1e-12)
        );
        // And every clip starts on one of its take's own beats.
        for clip in &plan.clips {
            let beats = &g.shots[clip.shot].segments[clip.segment]
                .rhythm
                .as_ref()
                .unwrap()
                .beats_us;
            assert!(beats.contains(&clip.in_us), "{} is not a beat", clip.in_us);
        }
    }

    #[test]
    fn a_dance_too_far_from_the_beat_for_a_fifth_either_way_keeps_its_speed() {
        let takes = three_takes();
        let mut g = graph(&takes);
        // 80 onto 120 is 1.5x, onto 60 0.75x: neither within a fifth.
        keep_beat(&mut g, &takes, 80.0, &[]);
        let plan = plan(&g, &params(None), &quality).unwrap();
        let piece = plan.report.speeds[0];
        assert!(!piece.on_beat);
        assert_eq!(piece.dance_bpm, Some(80.0));
        assert!(plan.clips.iter().all(|c| (c.speed - 1.0).abs() < 1e-9));
    }

    #[test]
    fn standing_idle_more_than_a_beat_and_a_half_is_left_out_of_the_action() {
        let takes = three_takes();
        let mut g = graph(&takes);
        // Every take that shows the routine's 18th to 21st second stands
        // idle through it.
        keep_beat(
            &mut g,
            &takes,
            120.0,
            &[("lake", 18.0, 20.0), ("hill", 18.0, 21.0)],
        );
        let plan = plan(&g, &params(None), &quality).unwrap();
        assert!(
            plan.report.idle >= Micros::from_secs(4),
            "{:?}",
            plan.report
        );
        for clip in &plan.clips {
            let from = routine_time(&g, &takes, clip);
            let to = from + clip.len.as_secs_f64() * clip.speed;
            assert!(
                to <= 18.3 || from >= 20.7,
                "a clip shows the routine from {from} to {to}, through the idle stretch"
            );
        }
        // The routine still runs on to its end after it.
        let last = plan.clips.last().unwrap();
        let end = routine_time(&g, &takes, last) + last.len.as_secs_f64() * last.speed;
        assert!(end > 47.0, "ends {end} s into the routine");
    }

    #[test]
    fn a_take_alone_dances_on_past_every_idle_stretch_starting_on_its_beats() {
        // One take at a tempo the cut speeds up a little, idle three times,
        // each stretch starting a fraction of a beat after a clip could end.
        let takes = vec![Take {
            name: "lake",
            captured: 10,
            from: 0.0,
            to: 40.0,
            x: 0.5,
            size: 0.8,
            lead: 1.0,
            scene: "lake",
            kicks: false,
        }];
        let mut g = graph(&takes);
        let idle = [
            ("lake", 9.3, 11.3),
            ("lake", 19.8, 22.1),
            ("lake", 30.2, 32.6),
        ];
        keep_beat(&mut g, &takes, 112.0, &idle);
        let plan = plan(&g, &params(None), &quality).unwrap();
        let mut shown = 0.0;
        let mut previous: Option<f64> = None;
        for clip in &plan.clips {
            let from = routine_time(&g, &takes, clip);
            let to = from + clip.len.as_secs_f64() * clip.speed;
            for (_, a, b) in &idle {
                assert!(
                    to <= a + 0.05 || from >= b - 0.05,
                    "a clip shows {from}-{to}, through {a}-{b}"
                );
            }
            // Every clip, the first and those after an idle stretch too,
            // starts on one of the dancers' beats.
            let beats = &g.shots[clip.shot].segments[clip.segment]
                .rhythm
                .as_ref()
                .unwrap()
                .beats_us;
            assert!(beats.contains(&clip.in_us), "{} is not a beat", clip.in_us);
            // The take carrying on is one clip: a new one only past idle.
            if let Some(end) = previous {
                assert!(
                    from - end > 1.0,
                    "a jump of {} s within the take",
                    from - end
                );
            }
            previous = Some(to);
            shown += to - from;
        }
        let end = previous.unwrap();
        assert!(end > 38.0, "the routine stops {end} s in");
        // All of it but the idle stretches and the rests at its ends.
        assert!(shown > 40.0 - 6.9 - 2.5, "{shown} s of the routine shown");
    }

    #[test]
    fn a_take_mostly_spent_watching_a_phone_is_left_out_whole() {
        let takes = three_takes();
        let mut g = graph(&takes);
        keep_beat(&mut g, &takes, 120.0, &[]);
        // On the beach someone holds the phone up nearly throughout.
        watched(&mut g, &takes, &[("beach", 31.0, 49.0)]);
        let plan = plan(&g, &params(None), &quality).unwrap();
        assert_eq!(plan.report.practice, 1, "{:?}", plan.report);
        assert!(plan.report.practice_len > Micros::from_secs(15));
        let beach = g
            .shots
            .iter()
            .position(|s| g.source(&s.source_id).unwrap().path.contains("beach"))
            .unwrap();
        assert!(plan.clips.iter().all(|c| c.shot != beach));
    }

    #[test]
    fn a_step_on_its_own_between_stops_is_not_dancing() {
        let takes = vec![Take {
            name: "lake",
            captured: 10,
            from: 0.0,
            to: 30.0,
            x: 0.5,
            size: 0.8,
            lead: 1.0,
            scene: "lake",
            kicks: false,
        }];
        let mut g = graph(&takes);
        // A second and a half of dancing between two stops: less than a
        // bar at 120 BPM.
        keep_beat(&mut g, &takes, 120.0, &[("lake", 9.0, 11.0)]);
        watched(&mut g, &takes, &[("lake", 12.5, 14.5)]);
        let plan = plan(&g, &params(None), &quality).unwrap();
        for clip in &plan.clips {
            let from = routine_time(&g, &takes, clip);
            let to = from + clip.len.as_secs_f64() * clip.speed;
            assert!(
                to <= 9.05 || from >= 14.45,
                "a clip shows the routine from {from} to {to}"
            );
        }
        assert!(
            plan.report.idle >= Micros::from_millis(5_400),
            "{:?}",
            plan.report
        );
    }

    #[test]
    fn a_stop_after_the_action_still_ends_a_clip_running_on_into_it() {
        let takes = vec![Take {
            name: "lake",
            captured: 10,
            from: 0.0,
            to: 20.0,
            x: 0.5,
            size: 0.8,
            lead: 1.0,
            scene: "lake",
            kicks: false,
        }];
        let mut g = graph(&takes);
        // Standing still from a fifth of a second after the routine ends,
        // in the footage a clip near its end may run on into.
        keep_beat(&mut g, &takes, 120.0, &[("lake", 20.2, 20.9)]);
        let (locals, _) = find_locals(&g, (1920, 1080), &quality, true);
        let local = &locals[0];
        assert!(local.to <= Micros::from_secs_f64(21.2), "{}", local.to);
        let until = local
            .dancing_until(Micros::from_secs_f64(20.0))
            .expect("dancing there");
        assert!(
            until <= Micros::from_secs_f64(21.25),
            "dances on to {until}"
        );
    }

    #[test]
    fn on_the_beat_a_move_on_its_own_shorter_than_two_bars_is_not_part_of_the_dance() {
        let mut takes = three_takes();
        // Three seconds of other moves on a path, matching no take.
        takes.push(Take {
            name: "path",
            captured: 40,
            from: 0.0,
            to: 3.0,
            x: 0.5,
            size: 0.8,
            lead: 1.0,
            scene: "path",
            kicks: true,
        });
        let mut g = graph(&takes);
        keep_beat(&mut g, &takes, 120.0, &[]);
        let path = g
            .shots
            .iter()
            .position(|s| g.source(&s.source_id).unwrap().path.contains("path"))
            .unwrap();
        let on = plan(&g, &params(None), &quality).unwrap();
        assert!(on.clips.iter().all(|c| c.shot != path), "{:?}", on.report);
        assert!(on.report.idle >= Micros::from_secs(2), "{:?}", on.report);
        // Off the beat it is an action like any other.
        let off = plan(&g, &off_the_beat(None), &quality).unwrap();
        assert!(off.clips.iter().any(|c| c.shot == path), "{:?}", off.report);
    }

    #[test]
    fn a_chosen_length_longer_than_the_action_ends_with_the_action() {
        let takes = three_takes();
        let g = graph(&takes);
        let plan = plan(&g, &params(Some(Micros::from_secs(90))), &quality).unwrap();
        assert!(plan.clips.iter().all(|c| (c.speed - 1.0).abs() < 1e-9));
        assert!(plan.report.shorter_than_chosen);
        let total: u64 = plan.clips.iter().map(|c| c.len.0).sum();
        assert!(total < 52_000_000, "{total}");
    }

    #[test]
    fn standing_still_is_not_an_action() {
        let takes = vec![Take {
            name: "pose",
            captured: 10,
            from: 3.0,
            to: 3.0,
            x: 0.5,
            size: 0.8,
            lead: 0.0,
            scene: "a",
            kicks: false,
        }];
        let mut g = graph(&takes);
        // Ten seconds of the same pose.
        let shot = &mut g.shots[0];
        shot.end_us = Micros::from_secs(10);
        shot.segments[0].end_us = Micros::from_secs(10);
        shot.segments[0].usable_end_us = Some(Micros::from_secs(10));
        g.sources[0].duration_us = Micros::from_secs(10);
        shot.segments[0].poses = (0..40)
            .map(|i| PoseSample {
                at_us: Micros(i * 250_000),
                people: vec![dancer(0.5, 0.8, 3.0, false)],
            })
            .collect();
        assert!(plan(&g, &params(None), &quality).is_none());
    }

    #[test]
    fn two_unrelated_actions_play_in_the_order_filmed() {
        // Two routines nobody repeats: they cannot be aligned, and do not
        // join, so the first filmed plays first.
        let mut takes = vec![Take {
            name: "morning",
            captured: 5,
            from: 0.0,
            to: 12.0,
            x: 0.5,
            size: 0.8,
            lead: 0.5,
            scene: "a",
            kicks: false,
        }];
        takes.push(Take {
            name: "evening",
            captured: 40,
            from: 200.0,
            to: 212.0,
            x: 0.5,
            size: 0.8,
            lead: 0.5,
            scene: "b",
            kicks: true,
        });
        let g = graph(&takes);
        let plan = plan(&g, &params(None), &quality).unwrap();
        assert_eq!(plan.report.pieces, 2, "{:?}", plan.report);
        assert_eq!(g.shots[plan.clips[0].shot].source_id, "morning");
    }

    #[test]
    fn a_placement_never_shows_past_the_pictures_edge() {
        let source = Source {
            id: "s".into(),
            path: "/s.mp4".into(),
            content_hash: Blake3Hash::of_str("s"),
            media_kind: None,
            duration_us: Micros::from_secs(10),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(3840),
            height: Some(2160),
            video_codec: None,
            audio_codec: None,
            has_audio: None,
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        };
        let picture = Picture::new((1920, 1080), &source);
        assert!((picture.max_scale - 2.5).abs() < 1e-9);
        let want = Side {
            scale: 1.2,
            turn: 25f64.to_radians(),
            shift: (0.6, -0.4),
        };
        let (side, held) = bounded(&picture, want);
        assert!(held);
        assert!(picture.covers(&side));
        assert!(side.turn.to_degrees() <= MAX_TURN_DEG + 1e-9);
        // Fitted is always allowed, and nothing is held back.
        let (same, held) = bounded(&picture, Side::FITTED);
        assert_eq!(same, Side::FITTED);
        assert!(!held);
    }

    #[test]
    fn a_similarity_recovers_the_enlargement_turn_and_move_between_two_framings() {
        let points = [(0.8, 0.3), (1.0, 0.3), (0.85, 0.7), (0.95, 0.7)];
        let (s, a, t) = (1.3f64, 4f64.to_radians(), (0.05, -0.02));
        let moved: Vec<(f64, f64)> = points
            .iter()
            .map(|p| {
                let (sin, cos) = a.sin_cos();
                (
                    s * (p.0 * cos - p.1 * sin) + t.0,
                    s * (p.0 * sin + p.1 * cos) + t.1,
                )
            })
            .collect();
        let (fs, fa, ft) = similarity(&points, &moved, true).unwrap();
        assert!((fs - s).abs() < 1e-9 && (fa - a).abs() < 1e-9);
        assert!((ft.0 - t.0).abs() < 1e-9 && (ft.1 - t.1).abs() < 1e-9);
        // As a placement about a picture's centre, it carries the points
        // the same way.
        let picture = Picture {
            aspect: 16.0 / 9.0,
            w: 16.0 / 9.0,
            h: 1.0,
            max_scale: 2.0,
        };
        let side = Side::placing(&picture, fs, fa, points[0], moved[0]);
        for (p, q) in points.iter().zip(&moved) {
            let r = side.apply(&picture, *p);
            assert!(distance(r, *q) < 1e-9);
        }
        // And the other way, enlarging by the inverse.
        let back = Side::placing(&picture, 1.0 / fs, -fa, moved[2], points[2]);
        for (p, q) in points.iter().zip(&moved) {
            assert!(distance(back.apply(&picture, *q), *p) < 1e-9);
        }
    }

    /// What 主体衔接 makes of an analysed project: every local全套动作, every
    /// pair of them that aligns and how well, and the plan. For setting the
    /// thresholds against footage a person has looked at:
    ///
    /// `VB_PROBE_GRAPH=<project.footage-graph.json> cargo test -p vb-narrative
    /// probe_continuity -- --nocapture`
    #[test]
    fn probe_continuity() {
        let Ok(path) = std::env::var("VB_PROBE_GRAPH") else {
            return;
        };
        let text = std::fs::read_to_string(&path).expect("a footage graph");
        let g: FootageGraph = serde_json::from_str(&text).expect("a footage graph");
        let canvas = (1920, 1080);
        let q = |i: usize, j: usize| g.shots[i].segments[j].quality.unwrap_or(0.5);
        let (locals, left_out) = find_locals(&g, canvas, &q, true);
        println!("{left_out:?}");
        let name = |l: &Local| {
            let source = &g.shots[l.shot].source_id;
            let path = &g.source(source).unwrap().path;
            std::path::Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string()
        };
        println!("{} local actions", locals.len());
        if let Ok(spec) = std::env::var("VB_PROBE_JOIN") {
            let parts: Vec<&str> = spec.split(':').collect();
            let find = |stem: &str, at: u64| {
                locals
                    .iter()
                    .find(|l| name(l) == stem && l.frame_near(Micros(at)).is_some())
                    .expect("a local with a frame there")
            };
            let (ta, tb) = (parts[1].parse().unwrap(), parts[3].parse().unwrap());
            let (a, b) = (find(parts[0], ta), find(parts[2], tb));
            for (l, t) in [(a, ta), (b, tb)] {
                let f = &l.frames[l.frame_near(Micros(t)).unwrap()];
                println!(
                    "  {} at {:.2}s picture {:?}",
                    name(l),
                    f.at.as_secs_f64(),
                    l.picture
                );
                for body in f.bodies.as_ref().unwrap() {
                    let p = |k: usize| body.points[k].map(|(x, y)| format!("({x:.3},{y:.3})"));
                    println!(
                        "    torso {:.3} hip ({:.3},{:.3}) shoulders {:?} {:?} hips {:?} {:?}",
                        body.torso,
                        body.hip.0,
                        body.hip.1,
                        p(5),
                        p(6),
                        p(11),
                        p(12)
                    );
                }
            }
            let fit = fit_join(a, Micros(ta), b, Micros(tb), true);
            println!("  fit {fit:?}");
        }
        if let Ok(which) = std::env::var("VB_PROBE_SELF") {
            let which: usize = which.parse().unwrap();
            let l = &locals[which];
            let frames: Vec<&Frame> = l.acting().filter(|f| f.bodies.is_some()).collect();
            let points = moving_points(frames.iter().copied());
            let n = frames.len();
            for lag in (8..n - 8).step_by(4) {
                let mut sum = 0.0;
                let mut c = 0;
                for i in 0..(n - lag) {
                    let best = [lag - 1, lag, lag + 1]
                        .into_iter()
                        .filter(|&d| i + d < n)
                        .filter_map(|d| frame_distance(frames[i], frames[i + d], &points))
                        .fold(f64::INFINITY, f64::min);
                    if best.is_finite() {
                        sum += best;
                        c += 1;
                    }
                }
                println!(
                    "  self lag {:.1}s: {:.3} over {c}",
                    lag as f64 * 0.25,
                    sum / c.max(1) as f64
                );
            }
            println!("  period {:?}", repeat_period(l));
        }
        for (i, l) in locals.iter().enumerate() {
            let acting: Vec<&Frame> = l.acting().collect();
            let mut moving: Vec<f64> = acting
                .windows(2)
                .filter_map(|w| movement(w[0], w[1]))
                .collect();
            moving.sort_by(f64::total_cmp);
            let at = |q: f64| {
                moving
                    .get((q * moving.len() as f64) as usize)
                    .copied()
                    .unwrap_or(0.0)
            };
            println!(
                "  [{i}] {} {:.1}-{:.1}s, {} people, {} frames, scene {}, movement p25 {:.2} median {:.2} p75 {:.2}",
                name(l),
                l.from.as_secs_f64(),
                l.to.as_secs_f64(),
                l.people,
                l.frames.len(),
                l.scene,
                at(0.25),
                at(0.5),
                at(0.75)
            );
        }
        let mut found = Vec::new();
        for i in 0..locals.len() {
            for j in (i + 1)..locals.len() {
                if let Some((a, typical)) = best_alignment(&locals[i], &locals[j])
                    && a.cost <= ALIGN_MAX
                {
                    found.push((i, j, a, typical));
                }
            }
        }
        for (i, j, a, typical) in &found {
            println!(
                "  [{i}] {} ~ [{j}] {}: gap {:.3}, typical {:.3}, overlap {}, offset {:.2}s",
                name(&locals[*i]),
                name(&locals[*j]),
                a.cost,
                typical,
                a.overlap,
                a.offset as f64 / 1e6
            );
        }
        for (k, piece) in layout(&locals).iter().enumerate() {
            let takes: Vec<String> = piece
                .takes
                .iter()
                .map(|(t, at)| format!("[{t}]@{:.1}s", at.as_secs_f64()))
                .collect();
            println!(
                "  piece {k}: {:.1}s, takes {}",
                piece.length.as_secs_f64(),
                takes.join(" ")
            );
        }
        // The beat a run used, from its DSL's priors (`beatGrid.motion@...`
        // or the music's): VB_PROBE_BEAT=<us>.
        let beat = std::env::var("VB_PROBE_BEAT")
            .ok()
            .and_then(|b| b.parse().ok())
            .map(Micros)
            .unwrap_or(Micros(500_000));
        let params = Params {
            canvas,
            beat: Some(beat),
            clip: Micros(4 * beat.0),
            min_clip: Micros(2 * beat.0),
            room: None,
        };
        if let Some(plan) = plan(&g, &params, &q) {
            println!("{:?}", plan.report);
            for c in &plan.clips {
                let source = &g.shots[c.shot].source_id;
                let path = &g.source(source).unwrap().path;
                println!(
                    "  piece {} {} in {:.2}s for {:.2}s join {:?} transform {:?}",
                    c.piece,
                    std::path::Path::new(path)
                        .file_stem()
                        .unwrap()
                        .to_string_lossy(),
                    c.in_us.as_secs_f64(),
                    c.len.as_secs_f64(),
                    c.join,
                    c.transform.as_ref().map(|t| &t.keyframes)
                );
            }
        }
    }
}
