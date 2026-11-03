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

//! The media seam.
//!
//! Everything the pipeline needs from ffmpeg goes through [`MediaBackend`].
//! There are two reasons this is a trait rather than a module of functions.
//!
//! The first is licensing: the shipped build must link an LGPL ffmpeg, and
//! keeping every call behind one interface means swapping the binary is a
//! configuration change, not a refactor.
//!
//! The second is iOS, which does not allow a process to spawn another process.
//! The CLI backend here is desktop-only. Mobile needs a backend that calls
//! linked libav directly, and it has to satisfy this same trait so that
//! nothing upstream notices the difference.

use std::path::{Path, PathBuf};
use vb_contracts::Micros;

/// What `ffprobe` tells us about a file.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration: Micros,
    pub fps_num: u32,
    pub fps_den: u32,
    pub width: u32,
    pub height: u32,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub has_audio: bool,
    /// The clockwise turn, in degrees, that shows the picture upright: 0,
    /// 90, 180 or 270. A phone held upright records its video on its side
    /// and says so here; `width` and `height` are as stored, before the turn.
    pub rotation: u32,
    /// The container's `creation_time` tag, verbatim, when it has one. For
    /// a photograph, when its EXIF says it was taken, with its zone. Else
    /// when a camera app's file name says it filmed it ([`crate::name_time`]).
    pub created_at: Option<String>,
    /// A photograph rather than a video: one picture, no running time
    /// (`duration` is zero) and no sound. `rotation` then comes from its
    /// EXIF orientation.
    pub still: bool,
    /// Where it was taken, when the camera wrote a GPS fix: a video's
    /// ISO 6709 location tag, a photo's EXIF.
    pub location: Option<crate::GeoPoint>,
}

impl MediaInfo {
    /// Width and height of a still written from this picture: upright, the
    /// longest side at most `max_side`, both sides even.
    pub fn still_size(&self, max_side: u32) -> (u32, u32) {
        let (w, h) = self.display_size();
        let scale = (max_side as f64 / w.max(h).max(1) as f64).min(1.0);
        let even = |v: u32| ((v as f64 * scale / 2.0).round() as u32).max(1) * 2;
        (even(w), even(h))
    }

    /// Width and height as the picture is shown, after [`Self::rotation`].
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

/// A detected shot boundary, with the scene score that triggered it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneCut {
    pub at: Micros,
    /// ffmpeg's `lavfi.scene_score` for the frame, 0.0 to 1.0.
    pub score: f64,
}

/// A decoded greyscale frame, used for classical scoring.
///
/// Deliberately small and 8-bit: the metrics we compute (Laplacian variance,
/// exposure) do not benefit from full resolution, and downscaling first is
/// what keeps scoring cheap enough to run over hours of 4K footage.
#[derive(Debug, Clone)]
pub struct GrayFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayFrame {
    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * self.width + x) as usize]
    }
}

/// One frame of a sweep through a whole video: small, in colour, and timed.
///
/// A sweep is how perception follows what happens across a take -- where
/// the camera settles, where it pans away, where the scene changes -- which
/// a handful of frames picked in advance cannot show. Colour because telling
/// a lake from a desert is most of telling two scenes apart.
#[derive(Debug, Clone)]
pub struct SampledFrame {
    /// Where the frame sits in the file, from its first frame.
    pub at: Micros,
    pub width: u32,
    pub height: u32,
    /// 8-bit RGB, full range, row-major with no padding: `width * height * 3`.
    pub rgb: Vec<u8>,
}

impl SampledFrame {
    /// The frame's brightness as greyscale, for the classical scores.
    ///
    /// BT.601 weights over the RGB the backend produced, so both backends
    /// derive it the same way from the same pixels.
    pub fn gray(&self) -> GrayFrame {
        let pixels = self
            .rgb
            .chunks_exact(3)
            .map(|p| {
                let y = 299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32;
                ((y + 500) / 1000).min(255) as u8
            })
            .collect();
        GrayFrame {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

/// How a sweep picks its frames.
///
/// The first decoded frame, then each frame at least `every` after the last
/// one taken. Both backends apply this one rule to the same timestamps, which
/// is what keeps their sweeps interchangeable. A millisecond of slack stops a
/// frame exactly `every` later from being skipped over a rounding error.
pub const SWEEP_SLACK: Micros = Micros(1_000);

/// What [`MediaBackend::scan`] found: the cuts, and how the sweep went.
///
/// The sweep's outcome is kept apart because it may fail on its own -- a
/// backend that cannot sweep still finds cuts -- and perception falls back
/// to picking keyframes rather than losing the shots.
#[derive(Debug)]
pub struct Scan {
    pub cuts: Vec<SceneCut>,
    /// How many frames the sweep handed over, or why it could not.
    pub frames: Result<usize, crate::MediaError>,
}

/// Decoded audio: one channel, at a rate the caller asked for.
///
/// Mono on purpose. Everything that listens to audio here -- beat detection,
/// and speech recognition when a model is installed -- wants one channel at a
/// low rate, and downmixing in the backend means neither has to know how many
/// channels the file had.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSamples {
    pub sample_rate: u32,
    /// Where the first sample sits in the file's timeline. Rarely zero for a
    /// stream with encoder priming, and a beat grid placed without it would
    /// be that far out.
    pub start: Micros,
    /// How long the audio runs in the file, which is not the same as how much
    /// was decoded: a caller that stops after a minute of a song still needs
    /// the song's length to lay it under a cut.
    pub total_duration: Option<Micros>,
    pub samples: Vec<f32>,
}

impl AudioSamples {
    pub fn duration(&self) -> Micros {
        if self.sample_rate == 0 {
            return Micros::ZERO;
        }
        Micros(self.samples.len() as u64 * 1_000_000 / self.sample_rate as u64)
    }
}

/// One frame of a video read in order, for the renderer: upright, in colour
/// with an alpha channel, at the size the reader was opened with.
///
/// RGBA rather than RGB because the renderer composes in RGBA -- text and
/// transitions are blended over the picture -- and a frame that arrives in
/// that layout is copied, not converted.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Where the frame sits in the file, from its first frame.
    pub at: Micros,
    pub width: u32,
    pub height: u32,
    /// 8-bit RGBA, full range, opaque, row-major with no padding:
    /// `width * height * 4`.
    pub rgba: Vec<u8>,
}

/// A video's frames in order from a moment, one at a time.
///
/// The renderer holds one open per clip on screen and pulls as the finished
/// piece's clock moves, which is how a slot is played without seeking for
/// every frame.
pub trait FrameReader: Send {
    /// The next frame, or `None` once the file has no more.
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, crate::MediaError>;
}

/// A file's sound in order from a moment, as interleaved 32-bit floats.
pub trait SampleReader: Send {
    /// Up to `max_frames` frames (a frame is one sample per channel);
    /// empty once there is no more.
    fn read(&mut self, max_frames: usize) -> Result<Vec<f32>, crate::MediaError>;
}

/// The video codec a finished piece is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    Hevc,
}

impl VideoCodec {
    pub fn id(self) -> &'static str {
        match self {
            VideoCodec::H264 => "h264",
            VideoCodec::Hevc => "hevc",
        }
    }

    pub fn from_id(id: &str) -> Option<VideoCodec> {
        match id {
            "h264" => Some(VideoCodec::H264),
            "hevc" | "h265" => Some(VideoCodec::Hevc),
            _ => None,
        }
    }
}

/// How a finished piece is encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeSettings {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub codec: VideoCodec,
    /// Bits a second for the picture.
    pub video_bitrate: u64,
    pub sample_rate: u32,
    pub channels: u32,
    /// Bits a second for the sound.
    pub audio_bitrate: u64,
}

/// Where a finished piece is written, frame by frame and sound alongside.
///
/// Frames arrive in order at the settings' rate, `width * height * 4` bytes
/// of RGBA each; sound arrives in order as interleaved floats. Nothing is
/// complete until [`Encoder::finish`]: an encoder dropped without it leaves
/// no playable file behind, which is what a cancelled export should do.
pub trait Encoder: Send {
    fn video(&mut self, rgba: &[u8]) -> Result<(), crate::MediaError>;
    fn audio(&mut self, interleaved: &[f32]) -> Result<(), crate::MediaError>;
    fn finish(self: Box<Self>) -> Result<(), crate::MediaError>;
}

/// One stretch of a file, copied into a finished piece without being
/// decoded or encoded again: lossless export, as LosslessCut does it.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyPart {
    pub path: PathBuf,
    /// Where the stretch starts: one of the video's keyframes, since a
    /// copy can only start where a picture is whole.
    pub from: Micros,
    pub to: Micros,
}

/// Media operations the pipeline depends on.
pub trait MediaBackend: Send + Sync {
    /// Name recorded in provenance, so a graph says which backend produced it.
    fn name(&self) -> &'static str;

    /// Can this backend run here? Checked before a run, so a missing
    /// dependency is one clear message rather than a mid-pipeline failure.
    fn is_available(&self) -> bool {
        true
    }

    /// Does this backend run other programs? iOS forbids it outright and
    /// Android makes it impractical, so a backend that does is desktop-only.
    fn spawns_processes(&self) -> bool {
        false
    }

    /// The ffmpeg version underneath, e.g. `9.0.1`.
    fn version(&self) -> Option<String> {
        None
    }

    /// The license the underlying ffmpeg was built under, in ffmpeg's own
    /// words, e.g. `LGPL version 2.1 or later`. See [`is_shippable_license`].
    fn license(&self) -> Option<String> {
        None
    }

    fn probe(&self, path: &Path) -> Result<MediaInfo, crate::MediaError>;

    /// Scene-change detection. `threshold` is ffmpeg's scene score cutoff;
    /// 0.3 to 0.4 is the usual working range.
    fn detect_scenes(
        &self,
        path: &Path,
        threshold: f64,
    ) -> Result<Vec<SceneCut>, crate::MediaError>;

    /// Decode one frame as greyscale at a reduced size, for scoring.
    fn gray_frame(
        &self,
        path: &Path,
        at: Micros,
        width: u32,
        height: u32,
    ) -> Result<GrayFrame, crate::MediaError>;

    /// Decode the audio as mono at `sample_rate`, optionally stopping early.
    ///
    /// Optional: a backend that cannot decode audio says so rather than
    /// returning silence, because silence is a valid answer for a quiet file
    /// and "this build has no audio decoder" is not the same thing.
    fn decode_audio(
        &self,
        _path: &Path,
        _sample_rate: u32,
        _max: Option<Micros>,
    ) -> Result<AudioSamples, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "decoding audio",
            backend: self.name(),
        })
    }

    /// Decode the whole video once, handing `sink` a small colour frame
    /// roughly every `every` (see [`SWEEP_SLACK`] for the exact rule).
    ///
    /// Streamed rather than returned, because an hour of footage is several
    /// hundred megabytes of frames even at this size. `sink` returns false to
    /// stop early, which is how a cancelled analysis stops decoding. Returns
    /// how many frames were handed over.
    ///
    /// Optional, like audio: a backend that cannot sweep says so, and
    /// perception falls back to scoring frames picked in advance.
    fn sample_frames(
        &self,
        _path: &Path,
        _every: Micros,
        _width: u32,
        _height: u32,
        _sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<usize, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "sweeping through the frames",
            backend: self.name(),
        })
    }

    /// Scene detection and the sweep together: the cuts
    /// [`Self::detect_scenes`] finds and the frames [`Self::sample_frames`]
    /// hands over, from as few decodes as the backend can manage.
    ///
    /// Decoding is nearly all of what either costs on 4K footage, so a
    /// backend that can do both from one decode should. This default runs
    /// them one after the other.
    fn scan(
        &self,
        path: &Path,
        threshold: f64,
        every: Micros,
        width: u32,
        height: u32,
        sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<Scan, crate::MediaError> {
        let cuts = self.detect_scenes(path, threshold)?;
        let frames = self.sample_frames(path, every, width, height, sink);
        Ok(Scan { cuts, frames })
    }

    /// The sweep's rule over one stretch of the file, `from` to `to`: an
    /// upright colour frame at `width` x `height` for the first frame at or
    /// after `from`, then roughly every `every`. For reading a few seconds
    /// densely -- body poses where a dance cut may join -- without a seek per
    /// frame. `sink` returns false to stop. The default seeks once per frame.
    #[allow(clippy::too_many_arguments)]
    fn sample_range(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
        every: Micros,
        width: u32,
        height: u32,
        sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<usize, crate::MediaError> {
        let step = every.max(Micros(1_000));
        let mut at = from;
        let mut count = 0;
        while at <= to {
            let frame = self.rgb_frame(path, at, width, height)?;
            count += 1;
            if !sink(SampledFrame { at, ..frame }) {
                break;
            }
            at = at + step;
        }
        Ok(count)
    }

    /// Decode one frame in colour at `width` x `height`, upright.
    ///
    /// For the models that look at a chosen moment rather than the sweep:
    /// depth, and later faces and pose. The size is the upright one -- a
    /// portrait clip asks for a tall frame -- because both backends turn the
    /// picture before scaling it, as the sweep does.
    ///
    /// Optional for now: a backend without it says so, and the director
    /// scores that moment without a depth reading.
    fn rgb_frame(
        &self,
        _path: &Path,
        _at: Micros,
        _width: u32,
        _height: u32,
    ) -> Result<SampledFrame, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "decoding a colour frame",
            backend: self.name(),
        })
    }

    /// Write a single frame to disk as a still.
    fn extract_still(
        &self,
        path: &Path,
        at: Micros,
        dest: &Path,
    ) -> Result<PathBuf, crate::MediaError>;

    /// Every frame of a video from `from` on, in order, upright and scaled
    /// to `width` x `height` RGBA: the renderer's way of playing a clip.
    ///
    /// The first frame is the one on screen at `from` or the first after
    /// it, as an exact seek finds it; frames before are decoded and
    /// dropped, never handed out.
    ///
    /// Optional, like the sweep: a backend without it says so, and nothing
    /// can be rendered through it.
    fn read_frames(
        &self,
        _path: &Path,
        _from: Micros,
        _width: u32,
        _height: u32,
    ) -> Result<Box<dyn FrameReader>, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "reading frames in order",
            backend: self.name(),
        })
    }

    /// The sound of a file from `from` to `to` (or its end), at
    /// `sample_rate` with `channels` interleaved, played `tempo` times as
    /// fast with its pitch kept: a clip's own sound at its speed, or the
    /// music under a cut.
    #[allow(clippy::too_many_arguments)]
    fn read_samples(
        &self,
        _path: &Path,
        _from: Micros,
        _to: Option<Micros>,
        _sample_rate: u32,
        _channels: u32,
        _tempo: f64,
    ) -> Result<Box<dyn SampleReader>, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "reading sound in order",
            backend: self.name(),
        })
    }

    /// Whether this backend can write a finished piece in `codec`.
    fn can_encode(&self, _codec: VideoCodec) -> bool {
        false
    }

    /// Start writing a finished piece to `dest`, an `.mp4` or `.mov`.
    fn encode(
        &self,
        _dest: &Path,
        _settings: &EncodeSettings,
    ) -> Result<Box<dyn Encoder>, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "writing a finished video",
            backend: self.name(),
        })
    }

    /// The video's keyframes from `from` to `to`, both included, read from
    /// the packets' flags without decoding a frame: where a lossless copy
    /// can start.
    fn keyframes(
        &self,
        _path: &Path,
        _from: Micros,
        _to: Micros,
    ) -> Result<Vec<Micros>, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "reading keyframes",
            backend: self.name(),
        })
    }

    /// A file's streams as far as copying goes -- codec, size, pixel
    /// format, profile, colour, turn, the codec's own header, and its
    /// sound's: two files with the same fingerprint can be joined without
    /// re-encoding. Meaningful only between files of one backend.
    fn copy_fingerprint(&self, _path: &Path) -> Result<String, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "fingerprinting streams",
            backend: self.name(),
        })
    }

    /// Whether this backend can join stretches of files by copying them.
    fn can_copy(&self) -> bool {
        false
    }

    /// Where a copy of `path` from the keyframe at `from` to `to` ends, in
    /// the file's own time: at `to`, or where the frames are whole after it
    /// when `to` falls inside a group of B-frames, a few frames on. What
    /// [`MediaBackend::copy_parts`] keeps of a part.
    fn copy_end(
        &self,
        _path: &Path,
        _from: Micros,
        _to: Micros,
    ) -> Result<Micros, crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "measuring a copy",
            backend: self.name(),
        })
    }

    /// Join `parts` end to end into `dest` (an `.mp4`) by copying their
    /// packets: the video, and the first sound when `audio`. Nothing is
    /// decoded or encoded, so it takes seconds and loses nothing; the parts
    /// must share a [`MediaBackend::copy_fingerprint`] and start on
    /// keyframes. Each part runs to its [`MediaBackend::copy_end`], every
    /// frame in it shown, and its sound under it; the next starts after
    /// both. `progress` is told the share done and stops the copy by
    /// returning false, leaving nothing at `dest`.
    fn copy_parts(
        &self,
        _parts: &[CopyPart],
        _dest: &Path,
        _audio: bool,
        _progress: &mut dyn FnMut(f64) -> bool,
    ) -> Result<(), crate::MediaError> {
        Err(crate::MediaError::Unsupported {
            what: "joining files without re-encoding",
            backend: self.name(),
        })
    }
}

/// The tempo filters that play sound `tempo` times as fast with its pitch
/// kept, as one ffmpeg filter description: `atempo` takes 0.5 to 100 in one
/// instance, so a slower tempo is a chain of halvings. Both backends run
/// this same description, which is what keeps their sound alike.
pub fn tempo_filter(tempo: f64) -> Option<String> {
    if !tempo.is_finite() || tempo <= 0.0 || (tempo - 1.0).abs() < 1e-6 {
        return None;
    }
    let mut parts = Vec::new();
    let mut rest = tempo;
    while rest < 0.5 {
        parts.push("atempo=0.5".to_string());
        rest /= 0.5;
    }
    parts.push(format!("atempo={rest:.6}"));
    Some(parts.join(","))
}

/// Can a release ship linked against an ffmpeg with this license?
///
/// Only an LGPL build can. A GPL build is fine for development and would
/// force the whole app under the GPL; a nonfree build cannot be
/// redistributed at all. The strings are ffmpeg's own, from
/// `avcodec_license()` or the configure flags behind `ffmpeg -version`.
pub fn is_shippable_license(license: &str) -> bool {
    license.starts_with("LGPL")
}

// ---------------------------------------------------------------------------
// Classical scoring
// ---------------------------------------------------------------------------

/// Normalised Laplacian variance: the standard "is this frame in focus"
/// metric, and the one the project specifies for sharpness.
///
/// The 3x3 Laplacian responds to intensity discontinuities, so a sharp frame
/// has high response variance and a blurred one has low. The raw variance is
/// unbounded, so it is squashed into 0..1 with a saturating curve; the
/// divisor is the response variance of a typically sharp frame, found by
/// measurement, and only the ordering matters downstream.
pub fn laplacian_sharpness(frame: &GrayFrame) -> f64 {
    if frame.width < 3 || frame.height < 3 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut n = 0.0f64;

    for y in 1..frame.height - 1 {
        for x in 1..frame.width - 1 {
            let c = frame.at(x, y) as f64;
            let response = frame.at(x, y - 1) as f64
                + frame.at(x, y + 1) as f64
                + frame.at(x - 1, y) as f64
                + frame.at(x + 1, y) as f64
                - 4.0 * c;
            sum += response;
            sum_sq += response * response;
            n += 1.0;
        }
    }

    if n == 0.0 {
        return 0.0;
    }
    let mean = sum / n;
    let variance = (sum_sq / n) - (mean * mean);
    const TYPICAL_SHARP_VARIANCE: f64 = 500.0;
    (variance / TYPICAL_SHARP_VARIANCE).clamp(0.0, 1.0)
}

/// How well-exposed a frame is, peaking at mid-grey.
///
/// Scores the mean intensity's distance from 128, so both a blown-out and a
/// crushed frame rank low. Crude, but it reliably rejects the black frames
/// that bracket a cut, which is most of what it is for.
pub fn exposure_quality(frame: &GrayFrame) -> f64 {
    if frame.pixels.is_empty() {
        return 0.0;
    }
    let sum: u64 = frame.pixels.iter().map(|p| *p as u64).sum();
    let mean = sum as f64 / frame.pixels.len() as f64;
    1.0 - ((mean - 128.0).abs() / 128.0).clamp(0.0, 1.0)
}

/// Mean absolute difference between two frames, 0..1.
///
/// Used as a cheap proxy for optical-flow jitter: comparing a frame with its
/// neighbour shows how much the picture is moving, and a high value at a
/// keyframe means motion blur or camera shake.
pub fn frame_difference(a: &GrayFrame, b: &GrayFrame) -> f64 {
    if a.pixels.len() != b.pixels.len() || a.pixels.is_empty() {
        return 0.0;
    }
    let total: u64 = a
        .pixels
        .iter()
        .zip(b.pixels.iter())
        .map(|(x, y)| x.abs_diff(*y) as u64)
        .sum();
    (total as f64 / a.pixels.len() as f64 / 255.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(value: u8, w: u32, h: u32) -> GrayFrame {
        GrayFrame {
            width: w,
            height: h,
            pixels: vec![value; (w * h) as usize],
        }
    }

    /// A checkerboard is the maximally high-frequency image, so it must score
    /// far above a flat field, which has no edges at all.
    fn checkerboard(w: u32, h: u32) -> GrayFrame {
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.push(if (x + y) % 2 == 0 { 0 } else { 255 });
            }
        }
        GrayFrame {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn a_flat_frame_has_no_sharpness() {
        assert_eq!(laplacian_sharpness(&flat(128, 16, 16)), 0.0);
    }

    #[test]
    fn a_detailed_frame_outscores_a_flat_one() {
        let sharp = laplacian_sharpness(&checkerboard(16, 16));
        let blurred = laplacian_sharpness(&flat(128, 16, 16));
        assert!(
            sharp > blurred,
            "checkerboard {sharp} should outscore flat {blurred}"
        );
        assert!((0.0..=1.0).contains(&sharp));
    }

    #[test]
    fn a_frame_too_small_to_convolve_scores_zero_rather_than_panicking() {
        assert_eq!(laplacian_sharpness(&flat(128, 2, 2)), 0.0);
        assert_eq!(laplacian_sharpness(&flat(128, 1, 1)), 0.0);
    }

    #[test]
    fn exposure_peaks_at_mid_grey_and_rejects_black_frames() {
        assert!(exposure_quality(&flat(128, 8, 8)) > 0.99);
        assert!(exposure_quality(&flat(0, 8, 8)) < 0.01);
        assert!(exposure_quality(&flat(255, 8, 8)) < 0.01);
    }

    #[test]
    fn identical_frames_show_no_motion() {
        let a = checkerboard(8, 8);
        assert_eq!(frame_difference(&a, &a), 0.0);
    }

    #[test]
    fn opposite_frames_show_maximum_motion() {
        let d = frame_difference(&flat(0, 8, 8), &flat(255, 8, 8));
        assert!((d - 1.0).abs() < 1e-9);
    }

    #[test]
    fn only_an_lgpl_build_is_shippable() {
        assert!(is_shippable_license("LGPL version 2.1 or later"));
        assert!(is_shippable_license("LGPL version 3 or later"));
        assert!(!is_shippable_license("GPL version 2 or later"));
        assert!(!is_shippable_license("nonfree and unredistributable"));
    }

    #[test]
    fn mismatched_frame_sizes_do_not_panic() {
        assert_eq!(frame_difference(&flat(0, 8, 8), &flat(0, 4, 4)), 0.0);
    }
}
