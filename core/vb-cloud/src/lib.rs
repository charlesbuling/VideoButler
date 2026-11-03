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

//! The one crate that touches the network.
//!
//! Two things need it, and nothing else in the engine may have it: fetching a
//! model the user asked to download, and talking to a model endpoint the user
//! configured. Keeping both here means the offline guarantee is structural
//! rather than a promise -- no other crate can reach the network, because no
//! other crate depends on an HTTP client.
//!
//! Everything here is optional to a run. A cloud adapter that cannot be
//! reached falls back to the on-device path with a warning; it never stops a
//! cut from being made.

pub mod download;
pub mod openai;

pub use download::{Download, fetch_to_file};
pub use openai::{CloudInference, CloudVlm, Endpoint};

/// How long to wait for a model endpoint.
///
/// Generous, because a large prompt to a slow model legitimately takes a
/// while, and bounded, because an app that hangs on a typo'd URL is worse
/// than one that says it could not connect.
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("could not reach {url}: {detail}")]
    Unreachable { url: String, detail: String },

    #[error("{url} returned HTTP {status}: {body}")]
    Status {
        url: String,
        status: u16,
        body: String,
    },

    #[error("{url} returned something unexpected: {detail}")]
    BadResponse { url: String, detail: String },

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "the download did not match its checksum, so it was discarded: expected {expected}, got {actual}"
    )]
    Checksum { expected: String, actual: String },

    #[error("the download was cancelled")]
    Cancelled,

    #[error("no model endpoint is configured")]
    NotConfigured,
}
