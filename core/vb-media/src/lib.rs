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

//! Media I/O, behind one trait.
//!
//! See [`backend::MediaBackend`] for why this is a seam rather than a module
//! of free functions: licensing (the shipped build needs LGPL ffmpeg) and iOS
//! (no subprocesses, so mobile needs a linked-libav backend).
//!
//! Two backends satisfy it. [`FfmpegCli`] shells out to the binaries and is
//! desktop-only. `LibAv`, behind the `libav` feature, calls linked libav
//! in-process and runs everywhere. [`default_backend`] picks between them, so
//! front ends never have to.

pub mod backend;
pub mod escape;
pub mod ffmpeg_cli;
#[cfg(feature = "libav")]
pub mod libav;
pub mod location;
pub mod name_time;
pub mod still;

pub use backend::{
    AudioSamples, CopyPart, EncodeSettings, Encoder, FrameReader, GrayFrame, MediaBackend,
    MediaInfo, SWEEP_SLACK, SampleReader, SampledFrame, Scan, SceneCut, VideoCodec, VideoFrame,
    exposure_quality, frame_difference, is_shippable_license, laplacian_sharpness, tempo_filter,
};
pub use escape::{escape_for_lavfi, is_lavfi_safe};
pub use ffmpeg_cli::FfmpegCli;
#[cfg(feature = "libav")]
pub use libav::LibAv;
pub use location::{GeoPoint, parse_iso6709};
pub use still::{PhotoMeta, STILL_EXTENSIONS, STILL_MAX_SIDE, is_still_path, photo_meta};

/// The backend this build analyses video with.
///
/// Linked libav when it is compiled in: it is the only backend that works on
/// every platform, it spawns no processes, and it is what a release ships.
/// Otherwise the ffmpeg CLI, which needs the binaries on PATH.
///
/// Every front end goes through here rather than naming a backend, so the
/// CLI, the app and the HTTP server cannot disagree about which one ran.
pub fn default_backend() -> Box<dyn MediaBackend> {
    #[cfg(feature = "libav")]
    {
        Box::new(LibAv)
    }
    #[cfg(not(feature = "libav"))]
    {
        Box::new(FfmpegCli::default())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("could not run {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{program} exited with {code:?}: {stderr}")]
    CommandFailed {
        program: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("could not parse {what}: {detail}")]
    Parse { what: &'static str, detail: String },

    #[error("the file has no video stream")]
    NoVideoStream,

    #[error("the file has no audio stream")]
    NoAudioStream,

    #[error("{backend} does not support {what}")]
    Unsupported {
        what: &'static str,
        backend: &'static str,
    },

    /// The path cannot be addressed inside a filtergraph. The caller is
    /// expected to fall back to fixed-length segmentation rather than treat
    /// this as fatal.
    #[error("cannot address {path} in a filtergraph: {reason}")]
    UnescapablePath { path: String, reason: String },

    /// A linked-library call failed. `op` names the operation, `detail` is
    /// libav's own account of why.
    #[error("libav {op} failed: {detail}")]
    Library { op: &'static str, detail: String },

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl MediaError {
    /// Should the caller retry with fixed-length segmentation instead of
    /// giving up? Only the escaping failure is recoverable that way.
    pub fn suggests_segmentation_fallback(&self) -> bool {
        matches!(self, MediaError::UnescapablePath { .. })
    }
}
