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

//! The in-process media backend: linked libav, through a small C shim.
//!
//! This is the backend that works everywhere, including iOS and Android,
//! where an app cannot spawn the ffmpeg binary. It exists behind the same
//! [`MediaBackend`] trait as the CLI backend, so nothing upstream can tell
//! them apart except through [`MediaBackend::name`], which is recorded in
//! provenance and in the cache key.
//!
//! Rust never sees a libav struct. `csrc/vb_av.c` owns every libav call and
//! exposes a handful of plain functions, so a new ffmpeg release that moves
//! struct fields changes nothing on this side. See `csrc/vb_av.h`.
//!
//! Whether a build is shippable depends on which libav it links. Homebrew's
//! is a GPL build; [`MediaBackend::license`] reports what was linked, and a
//! release needs it to say LGPL.

use crate::MediaError;
use crate::backend::{
    AudioSamples, CopyPart, EncodeSettings, Encoder, FrameReader, GrayFrame, MediaBackend,
    MediaInfo, SWEEP_SLACK, SampleReader, SampledFrame, Scan, SceneCut, VideoCodec,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use vb_contracts::Micros;

mod copy;
mod stream;

const VB_AV_OK: c_int = 0;
const VB_AV_ERR_NO_VIDEO: c_int = -2;
const VB_AV_ERR_NO_AUDIO: c_int = -8;

/// Room for the shim's error message; libav's own messages are short.
const ERR_LEN: usize = 512;

/// Mirrors `vb_av_info` in `csrc/vb_av.h`, field for field.
#[repr(C)]
struct VbAvInfo {
    duration_us: i64,
    fps_num: u32,
    fps_den: u32,
    width: u32,
    height: u32,
    has_audio: i32,
    video_codec: [c_char; 32],
    audio_codec: [c_char; 32],
    rotation: i32,
    created_at: [c_char; 48],
    location: [c_char; 64],
}

type SceneCallback = extern "C" fn(ctx: *mut c_void, at_us: i64, score: f64);
type FrameCallback = extern "C" fn(ctx: *mut c_void, at_us: i64, rgb: *const u8) -> c_int;

unsafe extern "C" {
    fn vb_av_probe(path: *const c_char, out: *mut VbAvInfo, err: *mut c_char, len: usize) -> c_int;
    fn vb_av_detect_scenes(
        path: *const c_char,
        threshold: f64,
        cb: SceneCallback,
        ctx: *mut c_void,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_gray_frame(
        path: *const c_char,
        at_us: i64,
        width: u32,
        height: u32,
        out: *mut u8,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_rgb_frame(
        path: *const c_char,
        at_us: i64,
        width: u32,
        height: u32,
        out: *mut u8,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_sample_frames(
        path: *const c_char,
        min_gap_us: i64,
        width: u32,
        height: u32,
        cb: FrameCallback,
        ctx: *mut c_void,
        count: *mut usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    #[allow(clippy::too_many_arguments)]
    fn vb_av_sample_range(
        path: *const c_char,
        from_us: i64,
        to_us: i64,
        min_gap_us: i64,
        width: u32,
        height: u32,
        cb: FrameCallback,
        ctx: *mut c_void,
        count: *mut usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    #[allow(clippy::too_many_arguments)]
    fn vb_av_scan(
        path: *const c_char,
        threshold: f64,
        scene_cb: SceneCallback,
        scene_ctx: *mut c_void,
        min_gap_us: i64,
        width: u32,
        height: u32,
        frame_cb: FrameCallback,
        frame_ctx: *mut c_void,
        count: *mut usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_extract_still(
        path: *const c_char,
        at_us: i64,
        dest: *const c_char,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_picture_info(
        path: *const c_char,
        width: *mut u32,
        height: *mut u32,
        codec: *mut c_char,
        codec_len: usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    #[allow(clippy::too_many_arguments)]
    fn vb_av_picture(
        path: *const c_char,
        rotation: i32,
        width: u32,
        height: u32,
        gray: i32,
        out: *mut u8,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_picture_still(
        path: *const c_char,
        rotation: i32,
        width: u32,
        height: u32,
        dest: *const c_char,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_decode_audio(
        path: *const c_char,
        sample_rate: u32,
        max_us: i64,
        out: *mut *mut f32,
        count: *mut usize,
        start_us: *mut i64,
        duration_us: *mut i64,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_free_samples(samples: *mut f32);
    fn vb_av_version() -> *const c_char;
    fn vb_av_license() -> *const c_char;
}

/// Calls linked libav in-process. Stateless: every call opens the file
/// afresh, so one value can be shared across threads freely.
#[derive(Debug, Default, Clone, Copy)]
pub struct LibAv;

/// Turn a path into the C string libav expects.
///
/// On Unix a path is bytes and passes through untouched. libav on Windows
/// expects UTF-8 and converts it to a wide path itself, so a path that is
/// not valid Unicode cannot be opened there and is reported as such.
fn c_path(path: &Path) -> Result<CString, MediaError> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path
        .to_str()
        .ok_or_else(|| MediaError::Library {
            op: "open",
            detail: format!("{} is not valid Unicode", path.display()),
        })?
        .as_bytes()
        .to_vec();

    CString::new(bytes).map_err(|_| MediaError::Library {
        op: "open",
        detail: format!("{} contains a NUL byte", path.display()),
    })
}

/// Read a fixed-size, NUL-terminated C buffer. A buffer with no NUL is
/// read in full rather than past its end.
fn c_buf(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn check(op: &'static str, code: c_int, err: &[c_char]) -> Result<(), MediaError> {
    match code {
        VB_AV_OK => Ok(()),
        VB_AV_ERR_NO_VIDEO => Err(MediaError::NoVideoStream),
        VB_AV_ERR_NO_AUDIO => Err(MediaError::NoAudioStream),
        _ => Err(MediaError::Library {
            op,
            detail: c_buf(err),
        }),
    }
}

/// Microseconds as the shim's signed timestamp. Anything past `i64::MAX` is
/// some 292,000 years into a clip, so it is a caller bug, not footage.
fn c_time(op: &'static str, at: Micros) -> Result<i64, MediaError> {
    i64::try_from(at.0).map_err(|_| MediaError::Library {
        op,
        detail: format!("{at} is out of range"),
    })
}

fn static_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the shim returns libav's static, NUL-terminated strings.
    let s = unsafe { CStr::from_ptr(ptr) };
    Some(s.to_string_lossy().into_owned())
}

extern "C" fn collect_cut(ctx: *mut c_void, at_us: i64, score: f64) {
    // SAFETY: `ctx` is the `&mut Vec<SceneCut>` passed to vb_av_detect_scenes
    // below, which outlives the call, and the shim calls back on the calling
    // thread only.
    let cuts = unsafe { &mut *(ctx as *mut Vec<SceneCut>) };
    // Negative only if a frame precedes the container's start time, which a
    // malformed file can do. A cut before the start of the footage is not a
    // cut in it.
    if let Ok(at) = u64::try_from(at_us) {
        cuts.push(SceneCut {
            at: Micros(at),
            score: score.clamp(0.0, 1.0),
        });
    }
}

/// What `vb_av_sample_frames` calls back into: the caller's sink, and the
/// size every frame arrives at.
struct Sweep<'a> {
    sink: &'a mut dyn FnMut(SampledFrame) -> bool,
    width: u32,
    height: u32,
    /// A panic in the sink, held until the shim has returned: unwinding
    /// through C is undefined behaviour.
    panic: Option<Box<dyn std::any::Any + Send>>,
}

extern "C" fn take_frame(ctx: *mut c_void, at_us: i64, rgb: *const u8) -> c_int {
    // SAFETY: `ctx` is the `&mut Sweep` passed to vb_av_sample_frames below,
    // which outlives the call; the shim calls back on the calling thread only.
    let sweep = unsafe { &mut *(ctx as *mut Sweep) };
    let len = (sweep.width * sweep.height * 3) as usize;
    // SAFETY: the shim hands over exactly width * height * 3 bytes, valid for
    // the duration of this call.
    let rgb = unsafe { std::slice::from_raw_parts(rgb, len) }.to_vec();
    let frame = SampledFrame {
        at: Micros(u64::try_from(at_us).unwrap_or(0)),
        width: sweep.width,
        height: sweep.height,
        rgb,
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (sweep.sink)(frame))) {
        Ok(more) => c_int::from(more),
        Err(payload) => {
            sweep.panic = Some(payload);
            0
        }
    }
}

impl LibAv {
    /// A photograph's size and codec from libav -- a HEIF grid's size, not
    /// a tile's -- and its orientation and capture time from its EXIF.
    fn probe_still(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let c = c_path(path)?;
        let (mut width, mut height) = (0u32, 0u32);
        let mut codec = [0 as c_char; 32];
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: every pointer is valid for the duration of the call, and
        // the lengths match the buffers.
        let code = unsafe {
            vb_av_picture_info(
                c.as_ptr(),
                &mut width,
                &mut height,
                codec.as_mut_ptr(),
                codec.len(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("probe", code, &err)?;
        let meta = crate::still::photo_meta(path);
        Ok(MediaInfo {
            duration: Micros::ZERO,
            fps_num: 30,
            fps_den: 1,
            width,
            height,
            video_codec: Some(c_buf(&codec)).filter(|s| !s.is_empty()),
            audio_codec: None,
            has_audio: false,
            rotation: meta.rotation,
            created_at: meta.taken_at.or_else(|| crate::name_time::from_name(path)),
            still: true,
            location: meta.location,
        })
    }

    /// A photograph upright at `width` x `height`: RGB, or grey when `gray`.
    fn picture(
        &self,
        path: &Path,
        width: u32,
        height: u32,
        gray: bool,
    ) -> Result<Vec<u8>, MediaError> {
        let op = "picture decode";
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(if gray { 1 } else { 3 }))
            .ok_or_else(|| MediaError::Library {
                op,
                detail: format!("a {width}x{height} picture is too large"),
            })?;
        let rotation = crate::still::photo_meta(path).rotation as i32;
        let c = c_path(path)?;
        let mut out = vec![0u8; len];
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `out` holds exactly the bytes the shim writes for this
        // size and format.
        let code = unsafe {
            vb_av_picture(
                c.as_ptr(),
                rotation,
                width,
                height,
                i32::from(gray),
                out.as_mut_ptr(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check(op, code, &err)?;
        Ok(out)
    }
}

impl MediaBackend for LibAv {
    fn name(&self) -> &'static str {
        "LibAv"
    }

    fn version(&self) -> Option<String> {
        // SAFETY: no arguments; returns a static string.
        static_str(unsafe { vb_av_version() })
    }

    fn license(&self) -> Option<String> {
        // SAFETY: no arguments; returns a static string.
        static_str(unsafe { vb_av_license() })
    }

    fn decode_audio(
        &self,
        path: &Path,
        sample_rate: u32,
        max: Option<Micros>,
    ) -> Result<AudioSamples, MediaError> {
        let c = c_path(path)?;
        let max_us = match max {
            Some(max) => c_time("decode_audio", max)?,
            None => 0,
        };
        let mut samples: *mut f32 = std::ptr::null_mut();
        let mut count: usize = 0;
        let mut start_us: i64 = 0;
        let mut duration_us: i64 = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: every pointer is valid for the call, and the shim writes
        // `count` floats into an allocation it hands over here.
        let code = unsafe {
            vb_av_decode_audio(
                c.as_ptr(),
                sample_rate,
                max_us,
                &mut samples,
                &mut count,
                &mut start_us,
                &mut duration_us,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("decode_audio", code, &err)?;
        if samples.is_null() {
            return Err(MediaError::Library {
                op: "decode_audio",
                detail: "the shim reported success but returned no samples".into(),
            });
        }
        // SAFETY: the shim guarantees `count` initialised floats at
        // `samples`. Copied out rather than adopted, because the allocation
        // came from libav's allocator and must go back to it.
        let copied = unsafe { std::slice::from_raw_parts(samples, count) }.to_vec();
        unsafe { vb_av_free_samples(samples) };

        Ok(AudioSamples {
            sample_rate,
            start: Micros(u64::try_from(start_us).unwrap_or(0)),
            total_duration: u64::try_from(duration_us)
                .ok()
                .filter(|d| *d > 0)
                .map(Micros),
            samples: copied,
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
        let c = c_path(path)?;
        let gap = c_time("sample_frames", every.saturating_sub(SWEEP_SLACK))?;
        let mut sweep = Sweep {
            sink,
            width,
            height,
            panic: None,
        };
        let mut count: usize = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `sweep` and `count` outlive the call, and `take_frame` is
        // the only thing that dereferences the context pointer.
        let code = unsafe {
            vb_av_sample_frames(
                c.as_ptr(),
                gap,
                width,
                height,
                take_frame,
                &mut sweep as *mut Sweep as *mut c_void,
                &mut count,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        if let Some(payload) = sweep.panic.take() {
            std::panic::resume_unwind(payload);
        }
        check("sample_frames", code, &err)?;
        Ok(count)
    }

    fn sample_range(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
        every: Micros,
        width: u32,
        height: u32,
        sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<usize, MediaError> {
        let c = c_path(path)?;
        let gap = c_time("sample_range", every.saturating_sub(SWEEP_SLACK))?;
        let (from_us, to_us) = (
            c_time("sample_range", from)?,
            c_time("sample_range", to.max(from))?,
        );
        let mut sweep = Sweep {
            sink,
            width,
            height,
            panic: None,
        };
        let mut count: usize = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `sweep` and `count` outlive the call, and `take_frame` is
        // the only thing that dereferences the context pointer.
        let code = unsafe {
            vb_av_sample_range(
                c.as_ptr(),
                from_us,
                to_us,
                gap,
                width,
                height,
                take_frame,
                &mut sweep as *mut Sweep as *mut c_void,
                &mut count,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        if let Some(payload) = sweep.panic.take() {
            std::panic::resume_unwind(payload);
        }
        check("sample_range", code, &err)?;
        Ok(count)
    }

    /// One decode for both, which halves the analysis of long 4K footage.
    fn scan(
        &self,
        path: &Path,
        threshold: f64,
        every: Micros,
        width: u32,
        height: u32,
        sink: &mut dyn FnMut(SampledFrame) -> bool,
    ) -> Result<Scan, MediaError> {
        let c = c_path(path)?;
        let gap = c_time("scan", every.saturating_sub(SWEEP_SLACK))?;
        let mut cuts: Vec<SceneCut> = Vec::new();
        let mut sweep = Sweep {
            sink,
            width,
            height,
            panic: None,
        };
        let mut count: usize = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `cuts`, `sweep` and `count` outlive the call; `collect_cut`
        // and `take_frame` are the only things that dereference the context
        // pointers, each its own.
        let code = unsafe {
            vb_av_scan(
                c.as_ptr(),
                threshold.clamp(0.0, 1.0),
                collect_cut,
                &mut cuts as *mut Vec<SceneCut> as *mut c_void,
                gap,
                width,
                height,
                take_frame,
                &mut sweep as *mut Sweep as *mut c_void,
                &mut count,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        if let Some(payload) = sweep.panic.take() {
            std::panic::resume_unwind(payload);
        }
        check("scan", code, &err)?;
        Ok(Scan {
            cuts,
            frames: Ok(count),
        })
    }

    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        if crate::still::is_still_path(path) {
            return self.probe_still(path);
        }
        let named = crate::name_time::from_name(path);
        let path = c_path(path)?;
        let mut info = VbAvInfo {
            duration_us: 0,
            fps_num: 0,
            fps_den: 0,
            width: 0,
            height: 0,
            has_audio: 0,
            video_codec: [0; 32],
            audio_codec: [0; 32],
            rotation: 0,
            created_at: [0; 48],
            location: [0; 64],
        };
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: every pointer is valid for the duration of the call, and
        // the lengths match the buffers.
        let code = unsafe { vb_av_probe(path.as_ptr(), &mut info, err.as_mut_ptr(), ERR_LEN) };
        check("probe", code, &err)?;

        let codec = |buf: &[c_char]| Some(c_buf(buf)).filter(|s| !s.is_empty());
        Ok(MediaInfo {
            duration: Micros(u64::try_from(info.duration_us).unwrap_or(0)),
            fps_num: info.fps_num,
            fps_den: info.fps_den,
            width: info.width,
            height: info.height,
            video_codec: codec(&info.video_codec),
            audio_codec: codec(&info.audio_codec),
            has_audio: info.has_audio != 0,
            rotation: u32::try_from(info.rotation).unwrap_or(0),
            created_at: Some(c_buf(&info.created_at))
                .filter(|s| !s.is_empty())
                .or(named),
            still: false,
            location: crate::location::parse_iso6709(&c_buf(&info.location)),
        })
    }

    fn detect_scenes(&self, path: &Path, threshold: f64) -> Result<Vec<SceneCut>, MediaError> {
        let path = c_path(path)?;
        let mut cuts: Vec<SceneCut> = Vec::new();
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `cuts` outlives the call, and `collect_cut` is the only
        // thing that dereferences the context pointer.
        let code = unsafe {
            vb_av_detect_scenes(
                path.as_ptr(),
                threshold.clamp(0.0, 1.0),
                collect_cut,
                &mut cuts as *mut Vec<SceneCut> as *mut c_void,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("scene detection", code, &err)?;
        Ok(cuts)
    }

    fn gray_frame(
        &self,
        path: &Path,
        at: Micros,
        width: u32,
        height: u32,
    ) -> Result<GrayFrame, MediaError> {
        if crate::still::is_still_path(path) {
            let pixels = self.picture(path, width, height, true)?;
            return Ok(GrayFrame {
                width,
                height,
                pixels,
            });
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| MediaError::Library {
                op: "frame decode",
                detail: format!("a {width}x{height} frame is too large"),
            })?;
        let at_us = c_time("frame decode", at)?;
        let path = c_path(path)?;
        let mut pixels = vec![0u8; len];
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `pixels` holds exactly width * height bytes, which is what
        // the shim writes.
        let code = unsafe {
            vb_av_gray_frame(
                path.as_ptr(),
                at_us,
                width,
                height,
                pixels.as_mut_ptr(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("frame decode", code, &err)?;
        Ok(GrayFrame {
            width,
            height,
            pixels,
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
            let rgb = self.picture(path, width, height, false)?;
            return Ok(SampledFrame {
                at,
                width,
                height,
                rgb,
            });
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(3))
            .ok_or_else(|| MediaError::Library {
                op: "colour frame decode",
                detail: format!("a {width}x{height} frame is too large"),
            })?;
        let at_us = c_time("colour frame decode", at)?;
        let path = c_path(path)?;
        let mut rgb = vec![0u8; len];
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `rgb` holds exactly width * height * 3 bytes, which is what
        // the shim writes.
        let code = unsafe {
            vb_av_rgb_frame(
                path.as_ptr(),
                at_us,
                width,
                height,
                rgb.as_mut_ptr(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("colour frame decode", code, &err)?;
        Ok(SampledFrame {
            at,
            width,
            height,
            rgb,
        })
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
            let source = c_path(path)?;
            let target = c_path(dest)?;
            let mut err = [0 as c_char; ERR_LEN];
            // SAFETY: both strings and the error buffer outlive the call.
            let code = unsafe {
                vb_av_picture_still(
                    source.as_ptr(),
                    info.rotation as i32,
                    width,
                    height,
                    target.as_ptr(),
                    err.as_mut_ptr(),
                    ERR_LEN,
                )
            };
            check("still extraction", code, &err)?;
            return Ok(dest.to_path_buf());
        }
        let at_us = c_time("still extraction", at)?;
        let source = c_path(path)?;
        let target = c_path(dest)?;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: both strings and the error buffer outlive the call.
        let code = unsafe {
            vb_av_extract_still(
                source.as_ptr(),
                at_us,
                target.as_ptr(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("still extraction", code, &err)?;
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
        self.can_write(codec)
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
        self.copy_supported()
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

    #[test]
    fn a_missing_file_is_an_error_not_a_crash() {
        let err = LibAv
            .probe(Path::new("/definitely/not/here.mp4"))
            .unwrap_err();
        assert!(
            err.to_string().contains("could not open"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_file_that_is_not_media_reports_why() {
        let path = std::env::temp_dir().join("vb-libav-not-media.mp4");
        std::fs::write(&path, b"this is not a video").unwrap();
        let err = LibAv.probe(&path).unwrap_err();
        assert!(matches!(err, MediaError::Library { .. }), "got {err:?}");
    }

    #[test]
    fn an_unknown_still_format_is_refused_before_any_decoding() {
        let err = LibAv
            .extract_still(
                Path::new("/definitely/not/here.mp4"),
                Micros::ZERO,
                &std::env::temp_dir().join("vb-libav-still.bmp"),
            )
            .unwrap_err();
        assert!(err.to_string().contains(".jpg or .png"), "got {err}");
    }

    #[test]
    fn a_path_with_a_nul_byte_is_rejected_on_this_side() {
        let err = c_path(Path::new("a\0b.mp4")).unwrap_err();
        assert!(err.to_string().contains("NUL"));
    }

    #[test]
    fn version_and_license_come_from_the_linked_library() {
        assert!(LibAv.version().is_some_and(|v| !v.is_empty()));
        let license = LibAv.license().unwrap();
        assert!(
            license.contains("GPL") || license.contains("nonfree"),
            "unexpected license string {license:?}"
        );
    }

    #[test]
    fn an_unterminated_c_buffer_is_read_without_overrunning() {
        let buf = [b'a' as c_char; 4];
        assert_eq!(c_buf(&buf), "aaaa");
    }
}
