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

//! The linked backend's streaming half: frames and sound read in order, and
//! a finished piece written, each through a handle the shim keeps open
//! across calls. The CLI backend does the same through processes with
//! pipes; these are what a phone, which cannot spawn one, renders with.

use super::{ERR_LEN, LibAv, c_path, c_time, check};
use crate::MediaError;
use crate::backend::{EncodeSettings, Encoder, FrameReader, SampleReader, VideoCodec, VideoFrame};
use std::ffi::{c_char, c_int};
use std::path::{Path, PathBuf};
use vb_contracts::Micros;

#[repr(C)]
struct VbAvReader {
    _private: [u8; 0],
}

#[repr(C)]
struct VbAvSound {
    _private: [u8; 0],
}

#[repr(C)]
struct VbAvWriter {
    _private: [u8; 0],
}

/// Mirrors `vb_av_encode_settings` in `csrc/vb_av.h`, field for field.
#[repr(C)]
struct VbAvEncodeSettings {
    video_bitrate: u64,
    audio_bitrate: u64,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    sample_rate: u32,
    channels: u32,
    hevc: i32,
}

unsafe extern "C" {
    fn vb_av_reader_open(
        path: *const c_char,
        from_us: i64,
        width: u32,
        height: u32,
        out: *mut *mut VbAvReader,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_reader_next(
        reader: *mut VbAvReader,
        rgba: *mut u8,
        at_us: *mut i64,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_reader_close(reader: *mut VbAvReader);

    #[allow(clippy::too_many_arguments)]
    fn vb_av_sound_open(
        path: *const c_char,
        from_us: i64,
        to_us: i64,
        sample_rate: u32,
        channels: u32,
        filters: *const c_char,
        out: *mut *mut VbAvSound,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_sound_read(
        sound: *mut VbAvSound,
        out: *mut f32,
        max_frames: usize,
        got: *mut usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_sound_close(sound: *mut VbAvSound);

    fn vb_av_can_encode(hevc: i32) -> c_int;
    fn vb_av_writer_open(
        dest: *const c_char,
        settings: *const VbAvEncodeSettings,
        out: *mut *mut VbAvWriter,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_writer_video(
        writer: *mut VbAvWriter,
        rgba: *const u8,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_writer_audio(
        writer: *mut VbAvWriter,
        interleaved: *const f32,
        frames: usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_writer_finish(writer: *mut VbAvWriter, err: *mut c_char, len: usize) -> c_int;
    fn vb_av_writer_abort(writer: *mut VbAvWriter);
}

/// A clip's frames, from the shim's reader.
struct LibAvFrames {
    handle: *mut VbAvReader,
    width: u32,
    height: u32,
}

// SAFETY: the shim's reader holds no thread-local state between calls and
// is only ever used from one thread at a time, which `&mut self` ensures.
unsafe impl Send for LibAvFrames {}

impl FrameReader for LibAvFrames {
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaError> {
        if self.handle.is_null() {
            return Ok(None);
        }
        let mut rgba = vec![0u8; self.width as usize * self.height as usize * 4];
        let mut at_us: i64 = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `rgba` holds exactly the width * height * 4 bytes the shim
        // writes, and the handle is open until drop.
        let code = unsafe {
            vb_av_reader_next(
                self.handle,
                rgba.as_mut_ptr(),
                &mut at_us,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        match code {
            1 => Ok(Some(VideoFrame {
                at: Micros(u64::try_from(at_us).unwrap_or(0)),
                width: self.width,
                height: self.height,
                rgba,
            })),
            0 => Ok(None),
            code => check("reading frames", code, &err).map(|()| None),
        }
    }
}

impl Drop for LibAvFrames {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: opened by vb_av_reader_open and closed only here.
            unsafe { vb_av_reader_close(self.handle) };
            self.handle = std::ptr::null_mut();
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

/// A file's sound, from the shim's sound reader.
struct LibAvSamples {
    handle: *mut VbAvSound,
    channels: u32,
}

// SAFETY: as for `LibAvFrames`.
unsafe impl Send for LibAvSamples {}

impl SampleReader for LibAvSamples {
    fn read(&mut self, max_frames: usize) -> Result<Vec<f32>, MediaError> {
        if self.handle.is_null() || max_frames == 0 {
            return Ok(Vec::new());
        }
        let mut out = vec![0f32; max_frames * self.channels as usize];
        let mut got: usize = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `out` holds max_frames * channels floats, which is the
        // most the shim writes.
        let code = unsafe {
            vb_av_sound_read(
                self.handle,
                out.as_mut_ptr(),
                max_frames,
                &mut got,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("reading sound", code, &err)?;
        out.truncate(got.min(max_frames) * self.channels as usize);
        Ok(out)
    }
}

impl Drop for LibAvSamples {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: opened by vb_av_sound_open and closed only here.
            unsafe { vb_av_sound_close(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

/// A finished piece being written by the shim.
struct LibAvEncoder {
    handle: *mut VbAvWriter,
    dest: PathBuf,
    frame_bytes: usize,
    channels: usize,
}

// SAFETY: as for `LibAvFrames`.
unsafe impl Send for LibAvEncoder {}

impl Encoder for LibAvEncoder {
    fn video(&mut self, rgba: &[u8]) -> Result<(), MediaError> {
        if rgba.len() != self.frame_bytes {
            return Err(MediaError::Parse {
                what: "frame to encode",
                detail: format!("expected {} bytes, got {}", self.frame_bytes, rgba.len()),
            });
        }
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `rgba` holds the width * height * 4 bytes the shim reads.
        let code =
            unsafe { vb_av_writer_video(self.handle, rgba.as_ptr(), err.as_mut_ptr(), ERR_LEN) };
        check("writing the video", code, &err)
    }

    fn audio(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let frames = interleaved.len() / self.channels.max(1);
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: `interleaved` holds at least frames * channels floats.
        let code = unsafe {
            vb_av_writer_audio(
                self.handle,
                interleaved.as_ptr(),
                frames,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("writing the sound", code, &err)
    }

    fn finish(mut self: Box<Self>) -> Result<(), MediaError> {
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: the handle is open, and the shim frees it whatever it
        // returns; it is never used again.
        let code = unsafe { vb_av_writer_finish(handle, err.as_mut_ptr(), ERR_LEN) };
        let result = check("finishing the video", code, &err);
        if result.is_err() {
            let _ = std::fs::remove_file(&self.dest);
        }
        result
    }
}

impl Drop for LibAvEncoder {
    fn drop(&mut self) {
        // Dropped without finishing: a cancelled export leaves nothing.
        if !self.handle.is_null() {
            // SAFETY: opened by vb_av_writer_open and freed only once.
            unsafe { vb_av_writer_abort(self.handle) };
            self.handle = std::ptr::null_mut();
            let _ = std::fs::remove_file(&self.dest);
        }
    }
}

impl LibAv {
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
            let rgb = self.picture(path, width, height, false)?;
            let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
            for px in rgb.chunks_exact(3) {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            return Ok(Box::new(OnePicture(Some(VideoFrame {
                at: Micros::ZERO,
                width,
                height,
                rgba,
            }))));
        }
        let from_us = c_time("reading frames", from)?;
        let c = c_path(path)?;
        let mut handle: *mut VbAvReader = std::ptr::null_mut();
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: the path and error buffer outlive the call; the handle is
        // written only on success.
        let code = unsafe {
            vb_av_reader_open(
                c.as_ptr(),
                from_us,
                width,
                height,
                &mut handle,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("reading frames", code, &err)?;
        Ok(Box::new(LibAvFrames {
            handle,
            width,
            height,
        }))
    }

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
        let from_us = c_time("reading sound", from)?;
        let to_us = match to {
            Some(to) => c_time("reading sound", to)?,
            None => -1,
        };
        let c = c_path(path)?;
        // The same description the CLI is given, so both play a clip at a
        // speed alike.
        let filters = crate::backend::tempo_filter(tempo)
            .map(|f| std::ffi::CString::new(f).expect("a filter description has no NUL"));
        let mut handle: *mut VbAvSound = std::ptr::null_mut();
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: the strings and error buffer outlive the call; the handle
        // is written only on success.
        let code = unsafe {
            vb_av_sound_open(
                c.as_ptr(),
                from_us,
                to_us,
                sample_rate,
                channels,
                filters.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
                &mut handle,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("reading sound", code, &err)?;
        Ok(Box::new(LibAvSamples { handle, channels }))
    }

    pub(super) fn can_write(&self, codec: VideoCodec) -> bool {
        // SAFETY: no pointers; asks the linked library what it carries.
        unsafe { vb_av_can_encode(i32::from(codec == VideoCodec::Hevc)) != 0 }
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
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MediaError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        let c = c_path(dest)?;
        let raw = VbAvEncodeSettings {
            video_bitrate: s.video_bitrate,
            audio_bitrate: s.audio_bitrate,
            width: s.width,
            height: s.height,
            fps_num: s.fps_num,
            fps_den: s.fps_den,
            sample_rate: s.sample_rate,
            channels: s.channels,
            hevc: i32::from(s.codec == VideoCodec::Hevc),
        };
        let mut handle: *mut VbAvWriter = std::ptr::null_mut();
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: the path, settings and error buffer outlive the call; the
        // handle is written only on success.
        let code =
            unsafe { vb_av_writer_open(c.as_ptr(), &raw, &mut handle, err.as_mut_ptr(), ERR_LEN) };
        if let Err(e) = check("writing the video", code, &err) {
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        Ok(Box::new(LibAvEncoder {
            handle,
            dest: dest.to_path_buf(),
            frame_bytes: s.width as usize * s.height as usize * 4,
            channels: s.channels as usize,
        }))
    }
}
