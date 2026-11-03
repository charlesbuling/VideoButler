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

//! The linked backend's lossless half: keyframes from packet flags, a
//! fingerprint of the streams, and stretches joined by copying their
//! packets, in the shim (`vb_av_join`).

use super::{ERR_LEN, LibAv, c_buf, c_path, c_time, check};
use crate::MediaError;
use crate::backend::CopyPart;
use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;
use vb_contracts::Micros;

const VB_AV_ERR_STOPPED: c_int = -9;

type ShareCallback = extern "C" fn(ctx: *mut c_void, share: f64) -> c_int;

/// Mirrors `vb_av_part` in `csrc/vb_av.h`, field for field.
#[repr(C)]
struct VbAvPart {
    path: *const c_char,
    from_us: i64,
    to_us: i64,
}

unsafe extern "C" {
    fn vb_av_keyframes(
        path: *const c_char,
        from_us: i64,
        to_us: i64,
        out: *mut *mut i64,
        count: *mut usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_free_times(times: *mut i64);
    fn vb_av_copy_fingerprint(
        path: *const c_char,
        out: *mut c_char,
        out_len: usize,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    fn vb_av_can_copy() -> c_int;
    fn vb_av_copy_end(
        path: *const c_char,
        from_us: i64,
        to_us: i64,
        end_us: *mut i64,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
    #[allow(clippy::too_many_arguments)]
    fn vb_av_join(
        parts: *const VbAvPart,
        count: usize,
        dest: *const c_char,
        audio: i32,
        cb: ShareCallback,
        ctx: *mut c_void,
        err: *mut c_char,
        len: usize,
    ) -> c_int;
}

/// What `vb_av_join` calls back into.
struct Share<'a> {
    progress: &'a mut dyn FnMut(f64) -> bool,
    /// A panic in the callback, held until the shim has returned: unwinding
    /// through C is undefined behaviour.
    panic: Option<Box<dyn std::any::Any + Send>>,
}

extern "C" fn take_share(ctx: *mut c_void, share: f64) -> c_int {
    // SAFETY: `ctx` is the `&mut Share` passed to vb_av_join below, which
    // outlives the call; the shim calls back on the calling thread only.
    let share_ctx = unsafe { &mut *(ctx as *mut Share) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (share_ctx.progress)(share))) {
        Ok(more) => c_int::from(more),
        Err(payload) => {
            share_ctx.panic = Some(payload);
            0
        }
    }
}

impl LibAv {
    pub(super) fn read_keyframes(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
    ) -> Result<Vec<Micros>, MediaError> {
        let c = c_path(path)?;
        let (from_us, to_us) = (c_time("keyframes", from)?, c_time("keyframes", to)?);
        let mut times: *mut i64 = std::ptr::null_mut();
        let mut count: usize = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: every pointer is valid for the call; the shim hands over an
        // allocation of `count` times, or none.
        let code = unsafe {
            vb_av_keyframes(
                c.as_ptr(),
                from_us,
                to_us,
                &mut times,
                &mut count,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("reading keyframes", code, &err)?;
        if times.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: the shim guarantees `count` initialised times at `times`,
        // allocated by libav, which takes them back.
        let out = unsafe { std::slice::from_raw_parts(times, count) }
            .iter()
            .map(|t| Micros(u64::try_from(*t).unwrap_or(0)))
            .collect();
        unsafe { vb_av_free_times(times) };
        Ok(out)
    }

    pub(super) fn fingerprint(&self, path: &Path) -> Result<String, MediaError> {
        let c = c_path(path)?;
        let mut out = [0 as c_char; 512];
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: both buffers are as long as said, and outlive the call.
        let code = unsafe {
            vb_av_copy_fingerprint(
                c.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("fingerprinting streams", code, &err)?;
        Ok(c_buf(&out))
    }

    pub(super) fn measure_copy(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
    ) -> Result<Micros, MediaError> {
        let c = c_path(path)?;
        let (from_us, to_us) = (c_time("a copy", from)?, c_time("a copy", to)?);
        let mut end: i64 = 0;
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: every pointer is valid for the call.
        let code = unsafe {
            vb_av_copy_end(
                c.as_ptr(),
                from_us,
                to_us,
                &mut end,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        check("measuring a copy", code, &err)?;
        Ok(Micros(u64::try_from(end).unwrap_or(0)))
    }

    pub(super) fn copy_supported(&self) -> bool {
        // SAFETY: no arguments; asks the linked library what it carries.
        unsafe { vb_av_can_copy() != 0 }
    }

    pub(super) fn join(
        &self,
        parts: &[CopyPart],
        dest: &Path,
        audio: bool,
        progress: &mut dyn FnMut(f64) -> bool,
    ) -> Result<(), MediaError> {
        if parts.is_empty() {
            return Err(MediaError::Parse {
                what: "parts to join",
                detail: "there are none".into(),
            });
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MediaError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        // The paths live as long as the call that reads them.
        let paths: Vec<CString> = parts
            .iter()
            .map(|p| c_path(&p.path))
            .collect::<Result<_, _>>()?;
        let mut raw = Vec::with_capacity(parts.len());
        for (part, path) in parts.iter().zip(&paths) {
            raw.push(VbAvPart {
                path: path.as_ptr(),
                from_us: c_time("joining", part.from)?,
                to_us: c_time("joining", part.to)?,
            });
        }
        let dest_c = c_path(dest)?;
        let mut share = Share {
            progress,
            panic: None,
        };
        let mut err = [0 as c_char; ERR_LEN];
        // SAFETY: the parts, their paths, the callback context and the error
        // buffer all outlive the call; the shim calls back on this thread.
        let code = unsafe {
            vb_av_join(
                raw.as_ptr(),
                raw.len(),
                dest_c.as_ptr(),
                i32::from(audio),
                take_share,
                &mut share as *mut Share as *mut c_void,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        if let Some(payload) = share.panic.take() {
            let _ = std::fs::remove_file(dest);
            std::panic::resume_unwind(payload);
        }
        if code == VB_AV_ERR_STOPPED {
            let _ = std::fs::remove_file(dest);
            return Err(MediaError::Parse {
                what: "join",
                detail: "stopped".into(),
            });
        }
        if let Err(e) = check("joining files", code, &err) {
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        Ok(())
    }
}
