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

//! The desktop media backend: shells out to the `ffmpeg` and `ffprobe` CLIs.
//!
//! **Desktop only.** iOS does not let an app spawn a process, so this backend
//! cannot ship on mobile. That is exactly why [`MediaBackend`] exists: mobile
//! gets a linked-libav backend behind the same trait, and no caller changes.
//!
//! The Homebrew ffmpeg this is developed against is a GPL build. Fine locally,
//! not shippable -- a release must point at an LGPL build.

use crate::MediaError;
use crate::backend::{
    AudioSamples, CopyPart, EncodeSettings, Encoder, FrameReader, GrayFrame, MediaBackend,
    MediaInfo, SWEEP_SLACK, SampleReader, SampledFrame, SceneCut, VideoCodec,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use vb_contracts::Micros;

mod copy;
mod stream;

/// Runs the ffmpeg and ffprobe binaries found on PATH, or at explicit paths.
#[derive(Debug, Clone)]
pub struct FfmpegCli {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl Default for FfmpegCli {
    fn default() -> Self {
        FfmpegCli {
            ffmpeg: resolve("ffmpeg"),
            ffprobe: resolve("ffprobe"),
        }
    }
}

/// Where to look when PATH does not have the binaries.
///
/// A macOS app launched from Finder inherits launchd's PATH --
/// `/usr/bin:/bin:/usr/sbin:/sbin` -- so a Homebrew ffmpeg is invisible to it
/// while sitting on the developer's PATH in a terminal. The app would report
/// ffmpeg missing and refuse to analyse anything, on a machine where the CLI
/// works. A Linux desktop launcher has the same problem. These are the
/// directories the package managers actually install to.
const EXTRA_DIRS: [&str; 4] = [
    "/opt/homebrew/bin", // Homebrew on Apple Silicon
    "/usr/local/bin",    // Homebrew on Intel, and most manual installs
    "/opt/local/bin",    // MacPorts
    "/snap/bin",         // Snap on Linux
];

fn resolve(program: &str) -> PathBuf {
    resolve_in(program, std::env::var_os("PATH").as_deref(), &EXTRA_DIRS)
}

/// Prefer whatever PATH resolves to, then the usual install locations.
///
/// Returning the bare name when PATH already has it keeps the lookup the
/// operating system's job, so a user who put a specific build first on their
/// PATH still gets that one.
fn resolve_in(program: &str, path: Option<&OsStr>, extra: &[&str]) -> PathBuf {
    if let Some(path) = path {
        for dir in std::env::split_paths(path) {
            if is_executable(&dir.join(program)) {
                return PathBuf::from(program);
            }
        }
    }
    for dir in extra {
        let candidate = Path::new(dir).join(program);
        if is_executable(&candidate) {
            return candidate;
        }
    }
    // Found nowhere: keep the bare name, so the failure names the program
    // rather than a path that was only ever a guess.
    PathBuf::from(program)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

impl FfmpegCli {
    pub fn new(ffmpeg: impl Into<PathBuf>, ffprobe: impl Into<PathBuf>) -> Self {
        FfmpegCli {
            ffmpeg: ffmpeg.into(),
            ffprobe: ffprobe.into(),
        }
    }

    /// What `ffmpeg -version` prints, or `None` when it cannot run.
    fn version_output(&self) -> Option<String> {
        let out = Command::new(&self.ffmpeg).arg("-version").output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn run(&self, program: &Path, args: &[String]) -> Result<Vec<u8>, MediaError> {
        tracing::debug!(program = %program.display(), ?args, "running media command");
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| MediaError::Spawn {
                program: program.display().to_string(),
                source: e,
            })?;

        if !output.status.success() {
            return Err(MediaError::CommandFailed {
                program: program.display().to_string(),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
        Ok(output.stdout)
    }

    /// A photograph's size and codec, from ffprobe, and its orientation and
    /// capture time from its EXIF. A HEIC is a grid of tiles in ffmpeg's
    /// eyes, each a stream of its own; the picture is the grid's size.
    fn probe_still(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let args = vec![
            "-v".into(),
            "error".into(),
            "-print_format".into(),
            "json".into(),
            "-show_streams".into(),
            "-show_stream_groups".into(),
            path.display().to_string(),
        ];
        let stdout = self.run(&self.ffprobe.clone(), &args)?;
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).map_err(|e| MediaError::Parse {
                what: "ffprobe JSON",
                detail: e.to_string(),
            })?;
        let video = json["streams"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
            .ok_or(MediaError::NoVideoStream)?;
        let grid = json["stream_groups"]
            .as_array()
            .and_then(|g| g.iter().find(|g| g["type"] == "Tile Grid"))
            .and_then(|g| g["components"].get(0));
        let size = |v: &serde_json::Value| {
            (
                v["width"].as_u64().unwrap_or(0) as u32,
                v["height"].as_u64().unwrap_or(0) as u32,
            )
        };
        let (width, height) = grid.map(size).unwrap_or_else(|| size(video));
        let meta = crate::still::photo_meta(path);
        Ok(MediaInfo {
            duration: Micros::ZERO,
            fps_num: 30,
            fps_den: 1,
            width,
            height,
            video_codec: video["codec_name"].as_str().map(str::to_string),
            audio_codec: None,
            has_audio: false,
            rotation: meta.rotation,
            created_at: meta.taken_at.or_else(|| crate::name_time::from_name(path)),
            still: true,
            location: meta.location,
        })
    }

    /// A photograph decoded upright at `width` x `height`, in `pix_fmt`.
    ///
    /// ffmpeg's own autorotate is off: it reads orientation differently
    /// for each format, and the turn here is the EXIF one both backends
    /// share. Turned first, then scaled, as a video frame is.
    fn picture(
        &self,
        path: &Path,
        rotation: u32,
        width: u32,
        height: u32,
        pix_fmt: &str,
    ) -> Result<Vec<u8>, MediaError> {
        let mut args = vec![
            "-v".into(),
            "error".into(),
            "-noautorotate".into(),
            "-i".into(),
            path.display().to_string(),
        ];
        args.extend(self.picture_filter(path, still_filter(rotation, width, height)));
        args.extend([
            "-frames:v".into(),
            "1".into(),
            "-pix_fmt".into(),
            pix_fmt.into(),
            "-f".into(),
            "rawvideo".into(),
            "-".into(),
        ]);
        self.run(&self.ffmpeg.clone(), &args)
    }

    /// The arguments that run `filter` over a photograph's picture. For a
    /// HEIF stored as a tile grid that is the assembled grid, which ffmpeg
    /// joins in a filtergraph of its own and exposes as the stream group --
    /// `-vf` would apply to one tile. JPEG, PNG and WebP are never grids, so
    /// only a HEIF is asked.
    fn picture_filter(&self, path: &Path, filter: String) -> Vec<String> {
        let heif = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif"));
        if !heif {
            return vec!["-vf".into(), filter];
        }
        let args = vec![
            "-v".into(),
            "error".into(),
            "-print_format".into(),
            "json".into(),
            "-show_stream_groups".into(),
            path.display().to_string(),
        ];
        let grid = self
            .run(&self.ffprobe.clone(), &args)
            .ok()
            .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out).ok())
            .and_then(|json| {
                json["stream_groups"]
                    .as_array()
                    .map(|g| g.iter().any(|g| g["type"] == "Tile Grid"))
            })
            .unwrap_or(false);
        if grid {
            vec![
                "-filter_complex".into(),
                format!("[0:g:0]{filter}[picture]"),
                "-map".into(),
                "[picture]".into(),
            ]
        } else {
            vec!["-vf".into(), filter]
        }
    }
}

/// The filter that shows a photograph upright at `width` x `height`: the
/// EXIF turn as exact transposes, then the scale.
fn still_filter(rotation: u32, width: u32, height: u32) -> String {
    let turn = match rotation {
        90 => "transpose=clock,",
        180 => "hflip,vflip,",
        270 => "transpose=cclock,",
        _ => "",
    };
    format!("{turn}scale={width}:{height}")
}

/// A raw frame of exactly `expected` bytes from ffmpeg's output, or why not.
fn raw_frame(mut bytes: Vec<u8>, expected: usize, at: Micros) -> Result<Vec<u8>, MediaError> {
    if bytes.len() < expected {
        return Err(MediaError::Parse {
            what: "raw frame",
            detail: format!("expected {expected} bytes at {at}, got {}", bytes.len()),
        });
    }
    bytes.truncate(expected);
    Ok(bytes)
}

/// Parse `"30000/1001"` into its numerator and denominator.
///
/// Kept exact rather than collapsed to a float, because 30000/1001 is not
/// representable and rounding it is how frame-accurate edits stop being
/// frame-accurate.
fn parse_rational(s: &str) -> Option<(u32, u32)> {
    let (num, den) = s.split_once('/')?;
    let num: u32 = num.trim().parse().ok()?;
    let den: u32 = den.trim().parse().ok()?;
    if num == 0 || den == 0 {
        return None;
    }
    Some((num, den))
}

/// The license an ffmpeg build is under, from its configure flags.
///
/// The same rules ffmpeg's own configure applies to produce the string
/// `avcodec_license()` returns, so both backends report a build identically.
fn license_from_configuration(version_output: &str) -> Option<String> {
    let line = version_output
        .lines()
        .find(|l| l.trim_start().starts_with("configuration:"))?;
    let flag = |f: &str| line.split_whitespace().any(|w| w == f);
    let license = if flag("--enable-nonfree") {
        "nonfree and unredistributable"
    } else if flag("--enable-gpl") && flag("--enable-version3") {
        "GPL version 3 or later"
    } else if flag("--enable-gpl") {
        "GPL version 2 or later"
    } else if flag("--enable-version3") {
        "LGPL version 3 or later"
    } else {
        "LGPL version 2.1 or later"
    };
    Some(license.to_string())
}

/// The clockwise turn that shows a stream upright, from ffprobe's display
/// matrix side data. The same arithmetic as ffmpeg's own get_rotation, which
/// is what its autorotate applies: `rotation: -90` means a quarter turn
/// clockwise.
fn rotation_of(stream: &serde_json::Value) -> u32 {
    let Some(reported) = stream["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|entry| entry["rotation"].as_f64()))
    else {
        return 0;
    };
    let mut theta = -reported.round();
    theta -= 360.0 * (theta / 360.0 + 0.9 / 360.0).floor();
    [90u32, 180, 270]
        .into_iter()
        .find(|q| (theta - *q as f64).abs() < 1.0)
        .unwrap_or(0)
}

/// The time showinfo reports for a frame, from a line such as
/// `[Parsed_showinfo_1 @ 0x..] n:   3 pts:  23040 pts_time:1.5 duration: ..`.
///
/// Only the frame's own line counts: showinfo writes a second line per
/// frame, about colour, which carries no `n:`.
fn parse_showinfo_time(line: &str) -> Option<Micros> {
    if !line.contains("Parsed_showinfo") || !line.contains(" n:") {
        return None;
    }
    let rest = &line[line.find("pts_time:")? + "pts_time:".len()..];
    let value: f64 = rest.split_whitespace().next()?.parse().ok()?;
    // A frame stamped before the first is clamped to it, as a negative time
    // is not one the footage has.
    Some(Micros::from_secs_f64(value.max(0.0)))
}

/// Pull scene cuts out of `metadata=print` output.
///
/// The filter emits pairs of lines: a `frame:` line carrying `pts_time`, then
/// a `lavfi.scene_score` line. Timestamps are parsed as float seconds because
/// that is the only form ffmpeg offers, then immediately converted to integer
/// microseconds so the float does not travel any further.
fn parse_scene_output(text: &str) -> Vec<SceneCut> {
    let mut cuts = Vec::new();
    let mut pending: Option<Micros> = None;

    for line in text.lines() {
        let line = line.trim();
        if let Some(idx) = line.find("pts_time:") {
            let rest = &line[idx + "pts_time:".len()..];
            let value = rest.split_whitespace().next().unwrap_or("");
            pending = value.parse::<f64>().ok().map(Micros::from_secs_f64);
        } else if let Some(rest) = line.strip_prefix("lavfi.scene_score=") {
            if let (Some(at), Ok(score)) = (pending.take(), rest.trim().parse::<f64>()) {
                cuts.push(SceneCut {
                    at,
                    score: score.clamp(0.0, 1.0),
                });
            }
        }
    }
    cuts
}

impl MediaBackend for FfmpegCli {
    fn name(&self) -> &'static str {
        "FfmpegCli"
    }

    fn is_available(&self) -> bool {
        self.version_output().is_some()
    }

    fn spawns_processes(&self) -> bool {
        true
    }

    fn version(&self) -> Option<String> {
        self.version_output()?
            .lines()
            .next()?
            .split_whitespace()
            .nth(2)
            .map(str::to_string)
    }

    fn license(&self) -> Option<String> {
        license_from_configuration(&self.version_output()?)
    }

    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        if crate::still::is_still_path(path) {
            return self.probe_still(path);
        }
        let args = vec![
            "-v".into(),
            "error".into(),
            "-print_format".into(),
            "json".into(),
            "-show_format".into(),
            "-show_streams".into(),
            path.display().to_string(),
        ];
        let stdout = self.run(&self.ffprobe.clone(), &args)?;
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).map_err(|e| MediaError::Parse {
                what: "ffprobe JSON",
                detail: e.to_string(),
            })?;

        let streams = json["streams"].as_array().cloned().unwrap_or_default();
        let video = streams
            .iter()
            .find(|s| s["codec_type"] == "video")
            .ok_or(MediaError::NoVideoStream)?;
        let audio = streams.iter().find(|s| s["codec_type"] == "audio");

        // Prefer the container duration: a stream's own duration is often
        // absent or wrong in fragmented files.
        let duration = json["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| video["duration"].as_str().and_then(|s| s.parse().ok()))
            .map(Micros::from_secs_f64)
            .unwrap_or(Micros::ZERO);

        let (fps_num, fps_den) = video["r_frame_rate"]
            .as_str()
            .and_then(parse_rational)
            .unwrap_or((30, 1));

        Ok(MediaInfo {
            duration,
            fps_num,
            fps_den,
            width: video["width"].as_u64().unwrap_or(0) as u32,
            height: video["height"].as_u64().unwrap_or(0) as u32,
            video_codec: video["codec_name"].as_str().map(str::to_string),
            audio_codec: audio.and_then(|a| a["codec_name"].as_str().map(str::to_string)),
            has_audio: audio.is_some(),
            rotation: rotation_of(video),
            created_at: json["format"]["tags"]["creation_time"]
                .as_str()
                .map(str::to_string)
                .or_else(|| crate::name_time::from_name(path)),
            still: false,
            location: crate::location::from_tags(|tag| json["format"]["tags"][tag].as_str()),
        })
    }

    fn detect_scenes(&self, path: &Path, threshold: f64) -> Result<Vec<SceneCut>, MediaError> {
        let threshold = threshold.clamp(0.0, 1.0);
        let args = vec![
            "-v".into(),
            "error".into(),
            "-i".into(),
            path.display().to_string(),
            "-filter:v".into(),
            format!("select='gt(scene,{threshold})',metadata=print:file=-"),
            "-an".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ];
        let stdout = self.run(&self.ffmpeg.clone(), &args)?;
        Ok(parse_scene_output(&String::from_utf8_lossy(&stdout)))
    }

    fn gray_frame(
        &self,
        path: &Path,
        at: Micros,
        width: u32,
        height: u32,
    ) -> Result<GrayFrame, MediaError> {
        if crate::still::is_still_path(path) {
            let rotation = crate::still::photo_meta(path).rotation;
            let pixels = self.picture(path, rotation, width, height, "gray")?;
            return raw_frame(pixels, (width * height) as usize, at).map(|pixels| GrayFrame {
                width,
                height,
                pixels,
            });
        }
        // -ss before -i seeks by keyframe, which is inexact but fast. For
        // scoring, near enough is genuinely near enough, and the alternative
        // costs a full decode from the start of the file per sample.
        let args = vec![
            "-v".into(),
            "error".into(),
            "-ss".into(),
            format!("{:.6}", at.as_secs_f64()),
            "-i".into(),
            path.display().to_string(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            format!("scale={width}:{height}"),
            "-pix_fmt".into(),
            "gray".into(),
            "-f".into(),
            "rawvideo".into(),
            "-".into(),
        ];
        let pixels = self.run(&self.ffmpeg.clone(), &args)?;
        let expected = (width * height) as usize;
        if pixels.len() < expected {
            return Err(MediaError::Parse {
                what: "raw greyscale frame",
                detail: format!(
                    "expected {expected} bytes at {at}, got {}; the seek may be past the end \
                     of the file",
                    pixels.len()
                ),
            });
        }
        Ok(GrayFrame {
            width,
            height,
            pixels: pixels[..expected].to_vec(),
        })
    }

    fn rgb_frame(
        &self,
        path: &Path,
        at: Micros,
        width: u32,
        height: u32,
    ) -> Result<SampledFrame, MediaError> {
        if crate::still::is_still_path(path) {
            // A photograph is the same picture at every moment.
            let rotation = crate::still::photo_meta(path).rotation;
            let rgb = self.picture(path, rotation, width, height, "rgb24")?;
            return raw_frame(rgb, (width * height * 3) as usize, at).map(|rgb| SampledFrame {
                at,
                width,
                height,
                rgb,
            });
        }
        // The same decode as gray_frame, in colour. The input seek decodes
        // from the preceding keyframe up to `at`, and autorotate turns the
        // picture before the scale, as it does for the sweep.
        let args = vec![
            "-v".into(),
            "error".into(),
            "-ss".into(),
            format!("{:.6}", at.as_secs_f64()),
            "-i".into(),
            path.display().to_string(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            format!("scale={width}:{height}"),
            "-pix_fmt".into(),
            "rgb24".into(),
            "-f".into(),
            "rawvideo".into(),
            "-".into(),
        ];
        let rgb = self.run(&self.ffmpeg.clone(), &args)?;
        let expected = (width * height * 3) as usize;
        if rgb.len() < expected {
            return Err(MediaError::Parse {
                what: "raw colour frame",
                detail: format!(
                    "expected {expected} bytes at {at}, got {}; the seek may be past the end \
                     of the file",
                    rgb.len()
                ),
            });
        }
        Ok(SampledFrame {
            at,
            width,
            height,
            rgb: rgb[..expected].to_vec(),
        })
    }

    fn decode_audio(
        &self,
        path: &Path,
        sample_rate: u32,
        max: Option<Micros>,
    ) -> Result<AudioSamples, MediaError> {
        if sample_rate == 0 {
            return Err(MediaError::Parse {
                what: "audio sample rate",
                detail: "a sample rate of zero decodes nothing".into(),
            });
        }
        let mut args = vec![
            "-v".to_string(),
            "error".into(),
            "-i".into(),
            path.display().to_string(),
            "-vn".into(),
        ];
        if let Some(max) = max {
            args.push("-t".into());
            args.push(format!("{:.6}", max.as_secs_f64()));
        }
        args.extend([
            "-ac".into(),
            "1".into(),
            "-ar".into(),
            sample_rate.to_string(),
            "-f".into(),
            "f32le".into(),
            "-".into(),
        ]);

        // Asked first, for two things the decode cannot say. Whether there is
        // audio at all: ffmpeg fails outright on a file with none, and "no
        // audio" is an answer rather than an error. And the container's
        // duration, which the decode may cut short and which the video probe
        // cannot supply for a music file, because it has no video stream.
        let probed = self.run(
            &self.ffprobe.clone(),
            &[
                "-v".into(),
                "error".into(),
                "-show_entries".into(),
                "format=duration:stream=codec_type".into(),
                "-of".into(),
                "json".into(),
                path.display().to_string(),
            ],
        )?;
        let json: serde_json::Value =
            serde_json::from_slice(&probed).map_err(|e| MediaError::Parse {
                what: "ffprobe JSON",
                detail: e.to_string(),
            })?;
        let has_audio = json["streams"]
            .as_array()
            .is_some_and(|streams| streams.iter().any(|s| s["codec_type"] == "audio"));
        if !has_audio {
            return Err(MediaError::NoAudioStream);
        }
        let total_duration = json["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| *d > 0.0)
            .map(Micros::from_secs_f64);

        let raw = self.run(&self.ffmpeg.clone(), &args)?;
        if raw.is_empty() {
            return Err(MediaError::NoAudioStream);
        }
        let samples = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(AudioSamples {
            sample_rate,
            // The CLI resamples from the start of the output, which it has
            // already shifted to zero.
            start: Micros::ZERO,
            total_duration,
            samples,
        })
    }

    fn sample_frames(
        &self,
        path: &Path,
        every: Micros,
        width: u32,
        height: u32,
        sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<usize, MediaError> {
        use std::io::{BufRead, BufReader, Read};
        use std::process::Stdio;

        if width == 0 || height == 0 || every == Micros::ZERO {
            return Err(MediaError::Parse {
                what: "sweep settings",
                detail: format!("{width}x{height} every {every} samples nothing"),
            });
        }
        // The rule in SWEEP_SLACK, as a select expression: the first frame,
        // then any frame far enough after the last one taken. showinfo
        // prints each taken frame's time to stderr, because raw video on
        // stdout carries none.
        let gap = every.0.saturating_sub(SWEEP_SLACK.0) as f64 / 1_000_000.0;
        let args = vec![
            "-hide_banner".to_string(),
            "-nostats".into(),
            "-v".into(),
            "info".into(),
            "-i".into(),
            path.display().to_string(),
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-vf".into(),
            format!(
                "select='isnan(prev_selected_t)+gte(t-prev_selected_t,{gap:.6})',showinfo,\
                 scale={width}:{height}"
            ),
            "-fps_mode".into(),
            "passthrough".into(),
            "-pix_fmt".into(),
            "rgb24".into(),
            "-f".into(),
            "rawvideo".into(),
            "-".into(),
        ];
        tracing::debug!(?args, "sweeping frames");
        let mut child = Command::new(&self.ffmpeg)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source: e,
            })?;

        // stderr is read on its own thread, both because it carries the
        // timestamps and because a full stderr pipe would stall ffmpeg while
        // this side waits on stdout.
        let stderr = child.stderr.take().expect("stderr was piped");
        let (times, arrivals) = std::sync::mpsc::channel::<Micros>();
        let reader = std::thread::spawn(move || {
            let mut tail: Vec<String> = Vec::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                match parse_showinfo_time(&line) {
                    Some(at) => {
                        let _ = times.send(at);
                    }
                    None if !line.contains("Parsed_showinfo") => {
                        tail.push(line);
                        if tail.len() > 5 {
                            tail.remove(0);
                        }
                    }
                    None => {}
                }
            }
            tail.join("; ")
        });

        let mut stdout = child.stdout.take().expect("stdout was piped");
        let mut buf = vec![0u8; (width * height * 3) as usize];
        let mut handed = 0usize;
        let mut stopped = false;
        loop {
            match stdout.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MediaError::Io {
                        path: path.display().to_string(),
                        source: e,
                    });
                }
            }
            // Every frame on stdout passed showinfo first, so its time is
            // either waiting or on its way.
            let Ok(at) = arrivals.recv() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MediaError::Parse {
                    what: "sweep timestamps",
                    detail: "ffmpeg wrote a frame without reporting its time".into(),
                });
            };
            handed += 1;
            let frame = SampledFrame {
                at,
                width,
                height,
                rgb: buf.clone(),
            };
            if !sink(frame) {
                stopped = true;
                let _ = child.kill();
                break;
            }
        }
        drop(stdout);
        let status = child.wait().map_err(|e| MediaError::Spawn {
            program: self.ffmpeg.display().to_string(),
            source: e,
        })?;
        let tail = reader.join().unwrap_or_default();
        if !stopped && !status.success() {
            return Err(MediaError::CommandFailed {
                program: self.ffmpeg.display().to_string(),
                code: status.code(),
                stderr: tail,
            });
        }
        Ok(handed)
    }

    fn extract_still(&self, path: &Path, at: Micros, dest: &Path) -> Result<PathBuf, MediaError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MediaError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        if crate::still::is_still_path(path) {
            // Upright and no larger than the models need.
            let info = self.probe_still(path)?;
            let (width, height) = info.still_size(crate::still::STILL_MAX_SIDE);
            let args = vec![
                "-v".into(),
                "error".into(),
                "-y".into(),
                "-noautorotate".into(),
                "-i".into(),
                path.display().to_string(),
            ];
            let mut args = args;
            args.extend(self.picture_filter(path, still_filter(info.rotation, width, height)));
            args.extend([
                "-frames:v".into(),
                "1".into(),
                "-q:v".into(),
                "3".into(),
                dest.display().to_string(),
            ]);
            self.run(&self.ffmpeg.clone(), &args)?;
            return Ok(dest.to_path_buf());
        }
        let args = vec![
            "-v".into(),
            "error".into(),
            "-y".into(),
            "-ss".into(),
            format!("{:.6}", at.as_secs_f64()),
            "-i".into(),
            path.display().to_string(),
            "-frames:v".into(),
            "1".into(),
            "-q:v".into(),
            "3".into(),
            dest.display().to_string(),
        ];
        self.run(&self.ffmpeg.clone(), &args)?;
        Ok(dest.to_path_buf())
    }

    fn read_frames(
        &self,
        path: &Path,
        from: Micros,
        width: u32,
        height: u32,
    ) -> Result<Box<dyn FrameReader>, MediaError> {
        self.open_frames(path, from, width, height)
    }

    fn read_samples(
        &self,
        path: &Path,
        from: Micros,
        to: Option<Micros>,
        sample_rate: u32,
        channels: u32,
        tempo: f64,
    ) -> Result<Box<dyn SampleReader>, MediaError> {
        self.open_samples(path, from, to, sample_rate, channels, tempo)
    }

    fn can_encode(&self, codec: VideoCodec) -> bool {
        self.has_encoder(codec)
    }

    fn encode(
        &self,
        dest: &Path,
        settings: &EncodeSettings,
    ) -> Result<Box<dyn Encoder>, MediaError> {
        self.open_encoder(dest, settings)
    }

    fn keyframes(&self, path: &Path, from: Micros, to: Micros) -> Result<Vec<Micros>, MediaError> {
        self.read_keyframes(path, from, to)
    }

    fn copy_fingerprint(&self, path: &Path) -> Result<String, MediaError> {
        self.fingerprint(path)
    }

    fn can_copy(&self) -> bool {
        self.is_available()
    }

    fn copy_end(&self, path: &Path, from: Micros, to: Micros) -> Result<Micros, MediaError> {
        self.measure_copy(path, from, to)
    }

    fn copy_parts(
        &self,
        parts: &[CopyPart],
        dest: &Path,
        audio: bool,
        progress: &mut dyn FnMut(f64) -> bool,
    ) -> Result<(), MediaError> {
        self.join(parts, dest, audio, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn stub(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn a_binary_on_path_is_left_to_the_operating_system() {
        let dir = std::env::temp_dir().join("vb-resolve-path");
        let _ = std::fs::remove_dir_all(&dir);
        stub(&dir, "ffmpeg");
        let path = std::ffi::OsString::from(dir.to_string_lossy().to_string());
        // A bare name, so a user who put a particular build first on their
        // PATH keeps getting that one.
        assert_eq!(
            resolve_in("ffmpeg", Some(&path), &[]),
            PathBuf::from("ffmpeg")
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_binary_off_path_is_found_where_package_managers_put_it() {
        // The Finder case: the app's PATH has no Homebrew in it.
        let dir = std::env::temp_dir().join("vb-resolve-extra");
        let _ = std::fs::remove_dir_all(&dir);
        let expected = stub(&dir, "ffmpeg");
        let extra = [dir.to_string_lossy().to_string()];
        let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
        assert_eq!(
            resolve_in(
                "ffmpeg",
                Some(std::ffi::OsStr::new("/usr/bin:/bin")),
                &extra
            ),
            expected
        );
    }

    #[test]
    fn a_binary_found_nowhere_keeps_its_bare_name() {
        // So the error says "could not run ffmpeg", not a path nobody chose.
        assert_eq!(
            resolve_in("definitely-not-installed", None, &[]),
            PathBuf::from("definitely-not-installed")
        );
    }

    #[test]
    fn rational_frame_rates_parse_exactly() {
        assert_eq!(parse_rational("30000/1001"), Some((30_000, 1_001)));
        assert_eq!(parse_rational("25/1"), Some((25, 1)));
        assert_eq!(parse_rational(" 24 / 1 "), Some((24, 1)));
    }

    #[test]
    fn a_degenerate_frame_rate_is_rejected() {
        // ffprobe reports 0/0 for streams with no meaningful rate; accepting
        // it would put a division by zero into every later conversion.
        assert_eq!(parse_rational("0/0"), None);
        assert_eq!(parse_rational("30"), None);
        assert_eq!(parse_rational("abc/1"), None);
    }

    #[test]
    fn scene_output_parses_into_cuts() {
        let text = "\
frame:0    pts:123456  pts_time:4.116
lavfi.scene_score=0.512000
frame:1    pts:223456  pts_time:9.250
lavfi.scene_score=0.876000
";
        let cuts = parse_scene_output(text);
        assert_eq!(cuts.len(), 2);
        assert_eq!(cuts[0].at, Micros(4_116_000));
        assert!((cuts[0].score - 0.512).abs() < 1e-6);
        assert_eq!(cuts[1].at, Micros(9_250_000));
    }

    #[test]
    fn a_score_line_with_no_timestamp_is_skipped() {
        // Truncated output must not produce a cut at an invented time.
        let cuts = parse_scene_output("lavfi.scene_score=0.9\n");
        assert!(cuts.is_empty());
    }

    #[test]
    fn the_license_follows_the_configure_flags() {
        let with = |flags: &str| {
            license_from_configuration(&format!(
                "ffmpeg version 9.0.1\n  built with clang\n  configuration: --prefix=/x {flags}\n"
            ))
            .unwrap()
        };
        // Homebrew's build, which is what development runs against.
        assert_eq!(
            with("--enable-shared --enable-gpl --enable-libx264"),
            "GPL version 2 or later"
        );
        assert_eq!(
            with("--enable-gpl --enable-version3"),
            "GPL version 3 or later"
        );
        assert_eq!(with("--enable-version3"), "LGPL version 3 or later");
        assert_eq!(with("--enable-shared"), "LGPL version 2.1 or later");
        assert_eq!(
            with("--enable-gpl --enable-nonfree"),
            "nonfree and unredistributable"
        );
        // A prefix that merely contains the text is not the flag.
        assert_eq!(
            with("--prefix=/opt/--enable-gpl-tools"),
            "LGPL version 2.1 or later"
        );
    }

    #[test]
    fn no_configuration_line_means_an_unknown_license() {
        assert_eq!(license_from_configuration("ffmpeg version 9.0.1\n"), None);
    }

    #[test]
    fn a_phone_held_upright_reads_as_a_quarter_turn() {
        let with = |r: f64| serde_json::json!({ "side_data_list": [{ "rotation": r }] });
        assert_eq!(rotation_of(&with(-90.0)), 90);
        assert_eq!(rotation_of(&with(90.0)), 270);
        assert_eq!(rotation_of(&with(-180.0)), 180);
        assert_eq!(rotation_of(&with(180.0)), 180);
        assert_eq!(rotation_of(&with(0.0)), 0);
        assert_eq!(rotation_of(&serde_json::json!({})), 0);
    }

    #[test]
    fn showinfo_lines_give_the_frame_time() {
        let line = "[Parsed_showinfo_1 @ 0x6000] n:   3 pts:  23040 pts_time:1.5     \
                    duration:    512 duration_time:0.0333333 fmt:yuv420p";
        assert_eq!(parse_showinfo_time(line), Some(Micros::from_millis(1500)));
        // The colour line that follows each frame is not a frame.
        assert_eq!(
            parse_showinfo_time("[Parsed_showinfo_1 @ 0x6000]  color_range:tv color_space:bt709"),
            None
        );
        assert_eq!(parse_showinfo_time("Stream mapping: pts_time:3"), None);
    }

    #[test]
    fn empty_output_yields_no_cuts() {
        assert!(parse_scene_output("").is_empty());
    }
}
