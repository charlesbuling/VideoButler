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

//! The finished piece, rendered from the Editing DSL.
//!
//! This is the built-in editor's export: the decision manifest made into a
//! video file. The renderer composes every frame itself -- each clip's
//! picture placed, adjusted and blended through its transitions, the text
//! laid over it -- and mixes the sound, and hands both to the media
//! backend's encoder. The backend only decodes and encodes; nothing about
//! the edit is expressed as an ffmpeg filtergraph, so no path ever goes
//! into a filtergraph string and the escaping that bit the previous
//! implementation has nowhere to bite.
//!
//! # The renderer never computes its own timeline
//!
//! Absolute positions come from [`EditingDsl::resolve_timeline`] and
//! nowhere else. If the renderer derived time independently -- summing
//! durations, say -- it would disagree with the exporters as soon as a
//! transition or title card appeared, and the drift would be back.
//!
//! # Non-destructive
//!
//! Rendering reads the DSL and the footage and writes a new file. It never
//! writes the DSL, and the footage is never touched: every cut, title and
//! music choice stays a decision the user can change and render again.

pub mod caption;
mod compose;
mod layers;
pub mod lossless;
mod picture;
mod sound;

use std::path::{Path, PathBuf};
use vb_contracts::editing_dsl::SlotKind;
use vb_contracts::{EditingDsl, FootageGraph, Micros};
use vb_media::{EncodeSettings, MediaBackend, VideoCodec};

pub use caption::{SHORT_SIDE_POINTS, parse_color, pixels_per_point};
pub use sound::{CHANNELS, SAMPLE_RATE};

/// One piece of the finished piece, placed on the real timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSegment {
    pub slot_id: String,
    /// Absolute start on the finished timeline, from `resolve_timeline`.
    pub timeline_start: Micros,
    pub timeline_end: Micros,
    /// Where to read from, for a clip. Absent for a title card or gap.
    pub source_path: Option<PathBuf>,
    pub source_in: Micros,
    pub source_out: Micros,
    pub is_title_card: bool,
    /// How fast the source plays: `source_out - source_in` is the segment's
    /// length times this.
    pub speed: f64,
}

/// What will be rendered, slot by slot.
#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub segments: Vec<RenderSegment>,
    pub total_duration: Micros,
}

/// Lay out a grounded DSL as the segments a render produces.
pub fn plan(dsl: &EditingDsl, graph: &FootageGraph) -> Result<RenderPlan, RenderError> {
    let layout = layers::layout(dsl, graph)?;
    let segments = layout
        .layers
        .iter()
        .map(|layer| {
            let slot = dsl.slot(&layer.slot_id);
            let (source_path, source_in, source_out, speed) = match &layer.content {
                layers::Content::Clip {
                    path,
                    in_us,
                    out_us,
                    speed,
                    ..
                } => (Some(path.clone()), *in_us, *out_us, *speed),
                layers::Content::Black => (None, Micros::ZERO, Micros::ZERO, 1.0),
            };
            RenderSegment {
                slot_id: layer.slot_id.clone(),
                timeline_start: layer.start,
                timeline_end: layer.end,
                source_path,
                source_in,
                source_out,
                is_title_card: slot.is_some_and(|s| s.kind == SlotKind::TitleCard),
                speed,
            }
        })
        .collect();
    Ok(RenderPlan {
        segments,
        total_duration: layout.total,
    })
}

/// How a finished piece is written.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub codec: VideoCodec,
    /// Bits a second for the picture.
    pub video_bitrate: u64,
}

impl RenderOptions {
    /// The piece's own frame at `short_side` (720, 1080, 2160...), its own
    /// frame rate, and a bitrate for `bits_per_pixel` of every frame.
    pub fn for_dsl(
        dsl: &EditingDsl,
        graph: &FootageGraph,
        short_side: u32,
        fps: Option<(u32, u32)>,
        codec: VideoCodec,
        bits_per_pixel: f64,
    ) -> RenderOptions {
        let (width, height) = frame_size(dsl, graph, short_side);
        let (fps_num, fps_den) = fps.unwrap_or_else(|| frame_rate(dsl));
        let rate = fps_num as f64 / fps_den.max(1) as f64;
        let mut video_bitrate = (width as f64 * height as f64 * rate * bits_per_pixel) as u64;
        if codec == VideoCodec::Hevc {
            video_bitrate = video_bitrate * 6 / 10;
        }
        RenderOptions {
            width,
            height,
            fps_num,
            fps_den,
            codec,
            video_bitrate: video_bitrate.max(500_000),
        }
    }
}

/// The piece's frame rate: the DSL's, which is its first video's, held to
/// at most 60 frames a second; 30 when it has none.
pub fn frame_rate(dsl: &EditingDsl) -> (u32, u32) {
    match (dsl.meta.fps_num, dsl.meta.fps_den) {
        (Some(num), Some(den)) if num > 0 && den > 0 && num as f64 / den as f64 <= 60.5 => {
            (num, den)
        }
        (Some(num), Some(den)) if num > 0 && den > 0 => (60, 1),
        _ => (30, 1),
    }
}

/// `16:9` as its two numbers.
pub fn parse_aspect(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once(':')?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// The frame a piece is rendered at: its aspect ratio -- the DSL's, else
/// its first video's, else 16:9 -- with the shorter side `short_side`, both
/// sides even, as encoders require.
pub fn frame_size(dsl: &EditingDsl, graph: &FootageGraph, short_side: u32) -> (u32, u32) {
    let aspect = dsl
        .meta
        .aspect_ratio
        .as_deref()
        .and_then(parse_aspect)
        .or_else(|| {
            dsl.ordered_slots()
                .iter()
                .filter_map(|s| s.source_ref.as_ref())
                .filter_map(|r| graph.source(&r.source_id))
                .find(|s| !s.is_still())
                .and_then(|s| s.display_size())
        })
        .unwrap_or((16, 9));
    let short = short_side.max(2) as f64;
    let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
    if aspect.0 >= aspect.1 {
        (even(short * aspect.0 as f64 / aspect.1 as f64), even(short))
    } else {
        (even(short), even(short * aspect.1 as f64 / aspect.0 as f64))
    }
}

/// What a render did.
#[derive(Debug, Clone)]
pub struct RenderReport {
    pub path: PathBuf,
    pub frames: u64,
    pub duration: Micros,
    pub width: u32,
    pub height: u32,
    /// What did not go as asked without stopping the render: a clip whose
    /// sound could not be read, say.
    pub warnings: Vec<String>,
}

/// The moment frame `n` starts, at `num`/`den` frames a second.
fn frame_time(n: u64, num: u32, den: u32) -> Micros {
    Micros((n as u128 * 1_000_000 * den as u128 / num as u128) as u64)
}

/// How many frames a piece of `total` holds.
fn frame_count(total: Micros, num: u32, den: u32) -> u64 {
    let d = 1_000_000u128 * den as u128;
    ((total.0 as u128 * num as u128).div_ceil(d)) as u64
}

/// Render the piece the DSL describes to `dest` (`.mp4` or `.mov`).
///
/// `progress` is told how many frames are done of how many after each one,
/// and stops the render by returning false: nothing is left at `dest` then.
/// The file is written beside `dest` under another name and moved there
/// once whole, so a half-written piece is never mistaken for a finished one.
pub fn render(
    dsl: &EditingDsl,
    graph: &FootageGraph,
    media: &dyn MediaBackend,
    dest: &Path,
    options: &RenderOptions,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<RenderReport, RenderError> {
    let layout = layers::layout(dsl, graph)?;
    if layout.total == Micros::ZERO {
        return Err(RenderError::Empty);
    }
    if options.width == 0 || options.height == 0 || options.fps_num == 0 || options.fps_den == 0 {
        return Err(RenderError::BadOptions(format!(
            "{}x{} at {}/{}",
            options.width, options.height, options.fps_num, options.fps_den
        )));
    }
    if !media.can_encode(options.codec) {
        return Err(RenderError::NoEncoder(options.codec.id()));
    }

    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "render.mp4".into());
    let ext = dest
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp4".into());
    let partial = dest.with_file_name(format!(".{name}.partial.{ext}"));
    let _ = std::fs::remove_file(&partial);

    let settings = EncodeSettings {
        width: options.width,
        height: options.height,
        fps_num: options.fps_num,
        fps_den: options.fps_den,
        codec: options.codec,
        video_bitrate: options.video_bitrate,
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        audio_bitrate: 192_000,
    };
    let mut encoder = media
        .encode(&partial, &settings)
        .map_err(RenderError::Media)?;
    let mut composer = compose::Composer::new(media, &layout, options.width, options.height);
    let mut mixer = sound::Mixer::new(media, &layout);

    let total_frames = frame_count(layout.total, options.fps_num, options.fps_den);
    let mut frame = vec![0u8; options.width as usize * options.height as usize * 4];
    let sample_of = |n: u64| sound::samples_at(frame_time(n, options.fps_num, options.fps_den));
    for n in 0..total_frames {
        if !progress(n, total_frames) {
            drop(encoder);
            let _ = std::fs::remove_file(&partial);
            return Err(RenderError::Cancelled);
        }
        let t = frame_time(n, options.fps_num, options.fps_den);
        if let Err(e) = composer.frame(t, &mut frame) {
            drop(encoder);
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
        let sound = mixer.mix(sample_of(n), sample_of(n + 1));
        let written = encoder.video(&frame).and_then(|()| encoder.audio(&sound));
        if let Err(e) = written {
            drop(encoder);
            let _ = std::fs::remove_file(&partial);
            return Err(RenderError::Media(e));
        }
    }
    encoder.finish().map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        RenderError::Media(e)
    })?;
    std::fs::rename(&partial, dest).map_err(|e| RenderError::Io {
        path: dest.display().to_string(),
        source: e,
    })?;
    progress(total_frames, total_frames);

    let mut warnings = composer.warnings;
    warnings.extend(mixer.warnings);
    warnings.dedup();
    Ok(RenderReport {
        path: dest.to_path_buf(),
        frames: total_frames,
        duration: layout.total,
        width: options.width,
        height: options.height,
        warnings,
    })
}

/// The piece at one moment, exactly as a render draws it: `width` x
/// `height` RGBA. What the editor shows when the user stops on a frame.
pub fn frame(
    dsl: &EditingDsl,
    graph: &FootageGraph,
    media: &dyn MediaBackend,
    at: Micros,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    if width == 0 || height == 0 {
        return Err(RenderError::BadOptions(format!("{width}x{height}")));
    }
    let layout = layers::layout(dsl, graph)?;
    let mut composer = compose::Composer::one_frame(media, &layout, width, height);
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let at = if layout.total > Micros::ZERO {
        Micros(at.0.min(layout.total.0 - 1))
    } else {
        at
    };
    composer.frame(at, &mut out)?;
    Ok(out)
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the timeline references slot {0}, which is not in the DSL")]
    MissingSlot(String),

    #[error("slot {slot_id} references source {source_id}, which is not in the footage graph")]
    MissingSource { slot_id: String, source_id: String },

    #[error("{0} is not where the footage graph says it is")]
    MissingFile(String),

    #[error("the size of {0} is not known, so it cannot be placed in the frame")]
    UnknownSize(String),

    #[error("the cut is empty")]
    Empty,

    #[error("cannot render at {0}")]
    BadOptions(String),

    #[error("this build cannot write {0} video")]
    NoEncoder(&'static str),

    #[error("stopped")]
    Cancelled,

    #[error(transparent)]
    Media(#[from] vb_media::MediaError),

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Blake3Hash;
    use vb_contracts::editing_dsl::{Slot, SourceRef, Transition, TransitionType};
    use vb_contracts::footage_graph::Source;

    fn graph(path: &str) -> FootageGraph {
        let mut g = FootageGraph::new("t");
        g.sources.push(Source {
            id: "src".into(),
            path: path.into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros::from_secs(60),
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
        });
        g
    }

    fn clip(id: &str, order: u32, secs: u64) -> Slot {
        Slot {
            id: id.into(),
            order,
            kind: SlotKind::Clip,
            duration_us: Micros::from_secs(secs),
            source_ref: Some(SourceRef {
                source_id: "src".into(),
                shot_id: None,
                in_us: Micros::ZERO,
                out_us: Micros::from_secs(secs),
            }),
            speed: None,
            reason: None,
            score: None,
        }
    }

    fn dsl(slots: Vec<Slot>) -> EditingDsl {
        let mut d = EditingDsl::new("t");
        d.slots = slots;
        d
    }

    #[test]
    fn segment_positions_come_from_the_resolved_timeline() {
        // The whole point: with a 1s crossfade the second clip starts at 4s,
        // not 5s. A renderer that summed durations would place it wrongly.
        let mut d = dsl(vec![clip("a", 0, 5), clip("b", 1, 3)]);
        d.transitions.push(Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Xfade,
            duration_us: Micros::from_secs(1),
        });
        let p = plan(&d, &graph("/tmp/a.mp4")).unwrap();
        assert_eq!(p.segments[1].timeline_start, Micros::from_secs(4));
        assert_eq!(p.total_duration, Micros::from_secs(7));
    }

    #[test]
    fn the_plan_agrees_with_the_dsls_own_resolution() {
        // Two independent derivations of time is how drift starts. There is
        // only one, and this asserts the renderer uses it.
        let d = dsl(vec![clip("a", 0, 5), clip("b", 1, 3), clip("c", 2, 2)]);
        let p = plan(&d, &graph("/tmp/a.mp4")).unwrap();
        let tl = d.resolve_timeline();
        for (segment, resolved) in p.segments.iter().zip(tl.slots.iter()) {
            assert_eq!(segment.timeline_start, resolved.start_us);
            assert_eq!(segment.timeline_end, resolved.end_us);
        }
        assert_eq!(p.total_duration, tl.total_duration_us);
    }

    #[test]
    fn a_title_card_plans_with_no_source() {
        let mut d = dsl(vec![clip("a", 1, 5)]);
        d.slots.push(Slot {
            id: "t".into(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: Micros::from_secs(2),
            source_ref: None,
            speed: None,
            reason: None,
            score: None,
        });
        let p = plan(&d, &graph("/tmp/a.mp4")).unwrap();
        assert!(p.segments[0].is_title_card);
        assert!(p.segments[0].source_path.is_none());
        assert_eq!(p.segments[1].timeline_start, Micros::from_secs(2));
    }

    #[test]
    fn a_missing_source_is_an_error_not_a_black_frame() {
        let mut d = dsl(vec![clip("a", 0, 5)]);
        d.slots[0].source_ref.as_mut().unwrap().source_id = "gone".into();
        assert!(matches!(
            plan(&d, &graph("/tmp/a.mp4")).unwrap_err(),
            RenderError::MissingSource { .. }
        ));
    }

    #[test]
    fn the_frame_follows_the_aspect_ratio_with_even_sides() {
        let mut d = dsl(vec![clip("a", 0, 5)]);
        let g = graph("/tmp/a.mp4");
        assert_eq!(frame_size(&d, &g, 1080), (1920, 1080));
        d.meta.aspect_ratio = Some("9:16".into());
        assert_eq!(frame_size(&d, &g, 1080), (1080, 1920));
        d.meta.aspect_ratio = Some("4:3".into());
        assert_eq!(frame_size(&d, &g, 720), (960, 720));
        d.meta.aspect_ratio = Some("1:1".into());
        assert_eq!(frame_size(&d, &g, 721), (722, 722));
    }

    #[test]
    fn frames_count_up_to_the_end_of_the_piece() {
        assert_eq!(frame_count(Micros::from_secs(2), 30, 1), 60);
        assert_eq!(frame_count(Micros::from_millis(2_010), 30, 1), 61);
        assert_eq!(frame_count(Micros::from_secs(10), 30_000, 1_001), 300);
        assert_eq!(frame_time(30, 30, 1), Micros::from_secs(1));
    }
}
