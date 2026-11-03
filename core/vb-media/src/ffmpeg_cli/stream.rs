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

//! The CLI backend's streaming half: frames and sound read in order, and a
//! finished piece written, each through an ffmpeg process with a pipe.
//!
//! Desktop only, like the rest of this backend. The renderer reads a clip
//! through one process per clip on screen and writes the piece through one
//! more, so a cut of forty clips spawns forty-odd processes in turn; on a
//! desktop that costs a few milliseconds each.

use super::{FfmpegCli, parse_showinfo_time};
use crate::MediaError;
use crate::backend::{EncodeSettings, Encoder, FrameReader, SampleReader, VideoCodec, VideoFrame};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, OnceLock};
use vb_contracts::Micros;

/// Frames from one ffmpeg process: raw RGB24 on stdout, given their alpha
/// here, each frame's time on stderr from showinfo, as the sweep reads them.
///
/// RGB24 because every other frame either backend reads is RGB24, and on
/// x86 swscale's RGBA comes out up to two levels from it: the linked
/// backend's reader does the same, and the two stay alike.
pub(super) struct CliFrames {
    program: String,
    child: Child,
    stdout: ChildStdout,
    times: Receiver<Micros>,
    tail: Option<std::thread::JoinHandle<String>>,
    from: Micros,
    width: u32,
    height: u32,
    finished: bool,
}

impl FrameReader for CliFrames {
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaError> {
        if self.finished {
            return Ok(None);
        }
        let mut rgb = vec![0u8; (self.width * self.height * 3) as usize];
        match self.stdout.read_exact(&mut rgb) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.finished = true;
                let status = self.child.wait().map_err(|e| MediaError::Spawn {
                    program: self.program.clone(),
                    source: e,
                })?;
                let tail = self
                    .tail
                    .take()
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                if !status.success() {
                    return Err(MediaError::CommandFailed {
                        program: self.program.clone(),
                        code: status.code(),
                        stderr: tail,
                    });
                }
                return Ok(None);
            }
            Err(e) => {
                return Err(MediaError::Io {
                    path: self.program.clone(),
                    source: e,
                });
            }
        }
        // Every frame on stdout passed showinfo first, so its time is either
        // waiting or on its way. The input seek put time zero at `from`.
        let at = self.times.recv().map_err(|_| MediaError::Parse {
            what: "frame timestamps",
            detail: "ffmpeg wrote a frame without reporting its time".into(),
        })?;
        let rgba = rgb
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        Ok(Some(VideoFrame {
            at: self.from + at,
            width: self.width,
            height: self.height,
            rgba,
        }))
    }
}

impl Drop for CliFrames {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A photograph read as a video: the one picture, once.
struct OnePicture(Option<VideoFrame>);

impl FrameReader for OnePicture {
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaError> {
        Ok(self.0.take())
    }
}

/// Sound from one ffmpeg process, interleaved floats on stdout.
pub(super) struct CliSamples {
    program: String,
    child: Child,
    stdout: ChildStdout,
    channels: u32,
    finished: bool,
    tail: Option<std::thread::JoinHandle<String>>,
}

impl SampleReader for CliSamples {
    fn read(&mut self, max_frames: usize) -> Result<Vec<f32>, MediaError> {
        if self.finished || max_frames == 0 {
            return Ok(Vec::new());
        }
        let want = max_frames * self.channels as usize * 4;
        let mut bytes = vec![0u8; want];
        let mut filled = 0;
        while filled < want {
            match self.stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    return Err(MediaError::Io {
                        path: self.program.clone(),
                        source: e,
                    });
                }
            }
        }
        if filled < want {
            self.finished = true;
            let status = self.child.wait().map_err(|e| MediaError::Spawn {
                program: self.program.clone(),
                source: e,
            })?;
            let tail = self
                .tail
                .take()
                .and_then(|t| t.join().ok())
                .unwrap_or_default();
            if !status.success() && filled == 0 {
                return Err(MediaError::CommandFailed {
                    program: self.program.clone(),
                    code: status.code(),
                    stderr: tail,
                });
            }
        }
        // Whole frames only: a frame split across reads is kept for the next.
        let frame_bytes = self.channels as usize * 4;
        let whole = filled / frame_bytes * frame_bytes;
        Ok(bytes[..whole]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    }
}

impl Drop for CliSamples {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Writes the picture through one ffmpeg process as it comes, and the sound
/// to a file beside it; `finish` joins the two into the piece. The picture
/// cannot share the process with the sound, because a child has one stdin.
pub(super) struct CliEncoder {
    ffmpeg: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    tail: Option<std::thread::JoinHandle<String>>,
    picture: PathBuf,
    sound_path: PathBuf,
    sound: Option<std::io::BufWriter<std::fs::File>>,
    dest: PathBuf,
    settings: EncodeSettings,
}

impl Encoder for CliEncoder {
    fn video(&mut self, rgba: &[u8]) -> Result<(), MediaError> {
        let expected = (self.settings.width * self.settings.height * 4) as usize;
        if rgba.len() != expected {
            return Err(MediaError::Parse {
                what: "frame to encode",
                detail: format!("expected {expected} bytes, got {}", rgba.len()),
            });
        }
        let stdin = self.stdin.as_mut().ok_or_else(|| MediaError::Parse {
            what: "encoder",
            detail: "frames after the end".into(),
        })?;
        stdin.write_all(rgba).map_err(|e| self.failed(e))
    }

    fn audio(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let sound = self.sound.as_mut().ok_or_else(|| MediaError::Parse {
            what: "encoder",
            detail: "sound after the end".into(),
        })?;
        let mut bytes = Vec::with_capacity(interleaved.len() * 4);
        for s in interleaved {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        sound.write_all(&bytes).map_err(|e| MediaError::Io {
            path: self.sound_path.display().to_string(),
            source: e,
        })
    }

    fn finish(mut self: Box<Self>) -> Result<(), MediaError> {
        drop(self.stdin.take());
        if let Some(mut sound) = self.sound.take() {
            sound.flush().map_err(|e| MediaError::Io {
                path: self.sound_path.display().to_string(),
                source: e,
            })?;
        }
        let mut child = self
            .child
            .take()
            .expect("the picture's process runs until finish");
        let status = child.wait().map_err(|e| MediaError::Spawn {
            program: self.ffmpeg.display().to_string(),
            source: e,
        })?;
        let tail = self
            .tail
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        if !status.success() {
            return Err(MediaError::CommandFailed {
                program: self.ffmpeg.display().to_string(),
                code: status.code(),
                stderr: tail,
            });
        }
        let s = &self.settings;
        let args: Vec<String> = vec![
            "-hide_banner".into(),
            "-v".into(),
            "error".into(),
            "-y".into(),
            "-i".into(),
            self.picture.display().to_string(),
            "-f".into(),
            "f32le".into(),
            "-ar".into(),
            s.sample_rate.to_string(),
            "-ac".into(),
            s.channels.to_string(),
            "-i".into(),
            self.sound_path.display().to_string(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "1:a:0".into(),
            "-c:v".into(),
            "copy".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            s.audio_bitrate.to_string(),
            "-movflags".into(),
            "+faststart".into(),
            self.dest.display().to_string(),
        ];
        let output = Command::new(&self.ffmpeg)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source: e,
            })?;
        let _ = std::fs::remove_file(&self.picture);
        let _ = std::fs::remove_file(&self.sound_path);
        if !output.status.success() {
            return Err(MediaError::CommandFailed {
                program: self.ffmpeg.display().to_string(),
                code: output.status.code(),
                stderr: last_lines(&String::from_utf8_lossy(&output.stderr)),
            });
        }
        Ok(())
    }
}

impl CliEncoder {
    /// A write to the picture's process failed: almost always because the
    /// process died, whose stderr says why.
    fn failed(&mut self, e: std::io::Error) -> MediaError {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            drop(self.stdin.take());
            if let Some(mut child) = self.child.take() {
                let code = child.wait().ok().and_then(|s| s.code());
                let tail = self
                    .tail
                    .take()
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                return MediaError::CommandFailed {
                    program: self.ffmpeg.display().to_string(),
                    code,
                    stderr: tail,
                };
            }
        }
        MediaError::Io {
            path: self.ffmpeg.display().to_string(),
            source: e,
        }
    }
}

impl Drop for CliEncoder {
    fn drop(&mut self) {
        // Dropped without finishing: a cancelled export leaves nothing.
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&self.dest);
        }
        drop(self.sound.take());
        let _ = std::fs::remove_file(&self.picture);
        let _ = std::fs::remove_file(&self.sound_path);
    }
}

fn last_lines(text: &str) -> String {
    text.lines().rev().take(5).collect::<Vec<_>>().join("; ")
}

/// Reads a child's stderr on its own thread, keeping its last lines -- a
/// full stderr pipe would otherwise stall the child -- and handing on the
/// frame times showinfo writes when `times` is given.
fn drain_stderr(
    stderr: std::process::ChildStderr,
    times: Option<std::sync::mpsc::Sender<Micros>>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(times) = &times
                && let Some(at) = parse_showinfo_time(&line)
            {
                let _ = times.send(at);
                continue;
            }
            if line.contains("Parsed_showinfo") {
                continue;
            }
            tail.push(line);
            if tail.len() > 5 {
                tail.remove(0);
            }
        }
        tail.join("; ")
    })
}

/// The encoders an ffmpeg has, from `ffmpeg -encoders`, asked once per
/// binary.
fn encoders(ffmpeg: &Path) -> Vec<String> {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, Vec<String>>>> = OnceLock::new();
    let mut known = KNOWN
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("the encoder list is never poisoned");
    known
        .entry(ffmpeg.to_path_buf())
        .or_insert_with(|| {
            Command::new(ffmpeg)
                .args(["-hide_banner", "-encoders"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter_map(|l| {
                            let mut parts = l.split_whitespace();
                            let flags = parts.next()?;
                            let name = parts.next()?;
                            (flags.len() == 6
                                && (flags.starts_with('V') || flags.starts_with('A'))
                                && name != "=")
                                .then(|| name.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .clone()
}

/// How this ffmpeg writes a codec: the encoder, and for Media Foundation's
/// whether it is the GPU's.
#[derive(Clone, Copy, Debug)]
pub(super) struct VideoEncoder {
    name: &'static str,
    /// `Some(true)` for the GPU's Media Foundation encoder, `Some(false)` for
    /// Microsoft's software one, `None` for any other encoder.
    mf_hardware: Option<bool>,
}

/// The encoder this ffmpeg writes `codec` with, best first: the platform's
/// own -- the Mac's media engine, Windows' Media Foundation, the same
/// encoders the linked backend uses there -- else x264 or x265.
pub(super) fn video_encoder(ffmpeg: &Path, codec: VideoCodec) -> Option<VideoEncoder> {
    let names: &[&'static str] = match codec {
        VideoCodec::H264 => &["h264_videotoolbox", "h264_mf", "libx264"],
        VideoCodec::Hevc => &["hevc_videotoolbox", "hevc_mf", "libx265"],
    };
    let have = encoders(ffmpeg);
    names
        .iter()
        .copied()
        .filter(|name| have.iter().any(|h| h == name))
        .find_map(|name| {
            if name.ends_with("_mf") {
                let hardware = mf_hardware(ffmpeg, name)?;
                Some(VideoEncoder {
                    name,
                    mf_hardware: Some(hardware),
                })
            } else {
                Some(VideoEncoder {
                    name,
                    mf_hardware: None,
                })
            }
        })
}

/// Whether Media Foundation's `encoder` writes through the GPU (`true`) or
/// through Microsoft's software encoder (`false`), found by encoding a few
/// frames each way, once per binary. `None` when neither opens: HEVC on a
/// machine with no GPU encoder, since Windows has no software one.
///
/// The linked backend makes the same choice by opening the encoder; a
/// process cannot try again, so this asks before the real one starts.
fn mf_hardware(ffmpeg: &Path, encoder: &'static str) -> Option<bool> {
    type Known = HashMap<(PathBuf, &'static str), Option<bool>>;
    static KNOWN: OnceLock<Mutex<Known>> = OnceLock::new();
    let mut known = KNOWN
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("the encoder list is never poisoned");
    *known
        .entry((ffmpeg.to_path_buf(), encoder))
        .or_insert_with(|| {
            const WIDTH: usize = 256;
            const HEIGHT: usize = 144;
            const FRAMES: usize = 5;
            [true, false].into_iter().find(|&hardware| {
                let Ok(mut child) = Command::new(ffmpeg)
                    .args(["-hide_banner", "-v", "error", "-f", "rawvideo"])
                    .args(["-pix_fmt", "nv12", "-s", &format!("{WIDTH}x{HEIGHT}")])
                    .args(["-i", "-", "-c:v", encoder, "-hw_encoding"])
                    .arg(if hardware { "1" } else { "0" })
                    .args(["-f", "null", "-"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    return false;
                };
                if let Some(mut stdin) = child.stdin.take() {
                    // Grey: luma and chroma at mid level. A refusal shows in
                    // the exit status, so a failed write is not looked at.
                    let _ = stdin.write_all(&vec![128u8; WIDTH * HEIGHT * 3 / 2 * FRAMES]);
                }
                child.wait().is_ok_and(|status| status.success())
            })
        })
}

impl FfmpegCli {
    pub(super) fn open_frames(
        &self,
        path: &Path,
        from: Micros,
        width: u32,
        height: u32,
    ) -> Result<Box<dyn FrameReader>, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::Parse {
                what: "frame size",
                detail: format!("{width}x{height} holds no picture"),
            });
        }
        if crate::still::is_still_path(path) {
            let rotation = crate::still::photo_meta(path).rotation;
            // RGB24 given its alpha, as the frames of a video are (CliFrames).
            let mut rgb = self.picture(path, rotation, width, height, "rgb24")?;
            rgb.truncate((width * height * 3) as usize);
            let rgba = rgb
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
            return Ok(Box::new(OnePicture(Some(VideoFrame {
                at: Micros::ZERO,
                width,
                height,
                rgba,
            }))));
        }
        let args: Vec<String> = vec![
            "-hide_banner".into(),
            "-nostats".into(),
            "-v".into(),
            "info".into(),
            "-ss".into(),
            format!("{:.6}", from.as_secs_f64()),
            "-i".into(),
            path.display().to_string(),
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-vf".into(),
            format!("showinfo,scale={width}:{height}"),
            "-fps_mode".into(),
            "passthrough".into(),
            "-pix_fmt".into(),
            "rgb24".into(),
            "-f".into(),
            "rawvideo".into(),
            "-".into(),
        ];
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
        let (sender, times) = std::sync::mpsc::channel();
        let tail = drain_stderr(child.stderr.take().expect("stderr was piped"), Some(sender));
        let stdout = child.stdout.take().expect("stdout was piped");
        Ok(Box::new(CliFrames {
            program: self.ffmpeg.display().to_string(),
            child,
            stdout,
            times,
            tail: Some(tail),
            from,
            width,
            height,
            finished: false,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_samples(
        &self,
        path: &Path,
        from: Micros,
        to: Option<Micros>,
        sample_rate: u32,
        channels: u32,
        tempo: f64,
    ) -> Result<Box<dyn SampleReader>, MediaError> {
        if sample_rate == 0 || channels == 0 {
            return Err(MediaError::Parse {
                what: "sound settings",
                detail: format!("{channels} channels at {sample_rate} Hz holds no sound"),
            });
        }
        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-nostats".into(),
            "-v".into(),
            "error".into(),
            "-ss".into(),
            format!("{:.6}", from.as_secs_f64()),
        ];
        if let Some(to) = to {
            args.push("-t".into());
            args.push(format!("{:.6}", to.saturating_sub(from).as_secs_f64()));
        }
        args.extend([
            "-i".into(),
            path.display().to_string(),
            "-vn".into(),
            "-sn".into(),
            "-dn".into(),
        ]);
        if let Some(filter) = crate::backend::tempo_filter(tempo) {
            args.push("-af".into());
            args.push(filter);
        }
        args.extend([
            "-ac".into(),
            channels.to_string(),
            "-ar".into(),
            sample_rate.to_string(),
            "-f".into(),
            "f32le".into(),
            "-".into(),
        ]);
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
        let tail = drain_stderr(child.stderr.take().expect("stderr was piped"), None);
        let stdout = child.stdout.take().expect("stdout was piped");
        Ok(Box::new(CliSamples {
            program: self.ffmpeg.display().to_string(),
            child,
            stdout,
            channels,
            finished: false,
            tail: Some(tail),
        }))
    }

    pub(super) fn open_encoder(
        &self,
        dest: &Path,
        settings: &EncodeSettings,
    ) -> Result<Box<dyn Encoder>, MediaError> {
        let s = settings;
        if s.width == 0 || s.height == 0 || s.width % 2 == 1 || s.height % 2 == 1 {
            return Err(MediaError::Parse {
                what: "output size",
                detail: format!("{}x{} must be even and not empty", s.width, s.height),
            });
        }
        let encoder = video_encoder(&self.ffmpeg, s.codec).ok_or(MediaError::Unsupported {
            what: "writing this codec",
            backend: "FfmpegCli",
        })?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MediaError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        // Beside the destination, so the final join is a rename-free copy on
        // the same disk, and hidden, so a folder being watched shows nothing
        // half-made.
        let stem = dest
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "render".into());
        let dir = dest.parent().unwrap_or(Path::new("."));
        let picture = dir.join(format!(".{stem}.picture.mp4"));
        let sound_path = dir.join(format!(".{stem}.sound.f32"));

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-nostats".into(),
            "-v".into(),
            "error".into(),
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-s".into(),
            format!("{}x{}", s.width, s.height),
            "-framerate".into(),
            format!("{}/{}", s.fps_num, s.fps_den),
            "-i".into(),
            "-".into(),
            // RGB to BT.709 limited range, as HD video is, and said so; NV12
            // for Media Foundation, whose GPU encoders take nothing else.
            "-vf".into(),
            format!(
                "scale=out_color_matrix=bt709:out_range=tv,format={}",
                if encoder.mf_hardware.is_some() {
                    "nv12"
                } else {
                    "yuv420p"
                }
            ),
            "-c:v".into(),
            encoder.name.into(),
            "-b:v".into(),
            s.video_bitrate.to_string(),
            "-colorspace".into(),
            "bt709".into(),
            "-color_primaries".into(),
            "bt709".into(),
            "-color_trc".into(),
            "bt709".into(),
            "-color_range".into(),
            "tv".into(),
        ];
        if s.codec == VideoCodec::Hevc {
            // QuickTime and the Photos app play HEVC in MP4 only as hvc1.
            args.extend(["-tag:v".into(), "hvc1".into()]);
        }
        if let Some(hardware) = encoder.mf_hardware {
            args.extend(["-hw_encoding".into(), u8::from(hardware).to_string()]);
            if s.codec == VideoCodec::H264 {
                // Left to itself Media Foundation writes Constrained
                // Baseline; High, as the linked backend asks for. By number:
                // h264_mf has no names for its profiles.
                args.extend(["-profile:v".into(), "100".into()]);
            }
        }
        args.extend(["-an".into(), picture.display().to_string()]);
        let mut child = Command::new(&self.ffmpeg)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::Spawn {
                program: self.ffmpeg.display().to_string(),
                source: e,
            })?;
        let tail = drain_stderr(child.stderr.take().expect("stderr was piped"), None);
        let stdin = child.stdin.take().expect("stdin was piped");
        let sound = std::fs::File::create(&sound_path).map_err(|e| MediaError::Io {
            path: sound_path.display().to_string(),
            source: e,
        })?;
        Ok(Box::new(CliEncoder {
            ffmpeg: self.ffmpeg.clone(),
            child: Some(child),
            stdin: Some(stdin),
            tail: Some(tail),
            picture,
            sound_path,
            sound: Some(std::io::BufWriter::new(sound)),
            dest: dest.to_path_buf(),
            settings: settings.clone(),
        }))
    }

    pub(super) fn has_encoder(&self, codec: VideoCodec) -> bool {
        video_encoder(&self.ffmpeg, codec).is_some()
    }
}
