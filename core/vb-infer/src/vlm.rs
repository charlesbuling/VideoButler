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

//! The vision-language seam: dense descriptions of keyframes.
//!
//! Only keyframes reach a VLM, never the raw frame stream. That compression is
//! the whole reason a multi-hour source can be reasoned about at all.

use crate::{Adapter, InferError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlmRequest {
    pub image_path: PathBuf,
    pub prompt: String,
    pub max_tokens: u32,
}

impl VlmRequest {
    pub fn describe(image_path: impl Into<PathBuf>) -> Self {
        VlmRequest {
            image_path: image_path.into(),
            prompt: "Describe this frame: the subjects, what they are doing, the setting, \
                     and the shot type. Be concrete and brief."
                .into(),
            max_tokens: 256,
        }
    }
}

/// A label an adapter saw in a picture, with how sure it is, 0 to 1 on its
/// own scale: what one adapter's 0.5 means is not what another's does, so a
/// reader of these knows which adapter wrote them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredLabel {
    pub label: String,
    pub confidence: f64,
}

pub trait VlmAdapter: Adapter + Send + Sync {
    fn describe(&self, request: &VlmRequest) -> Result<String, InferError>;

    /// The labels an adapter that answers in labels sees, most certain
    /// first, with their confidence: what the video butler classifies
    /// footage by. An adapter that writes sentences has none to give.
    fn labels(&self, _request: &VlmRequest) -> Result<Vec<ScoredLabel>, InferError> {
        Err(InferError::Unavailable {
            adapter: self.name(),
            reason: "it describes a picture in sentences, not labels".into(),
        })
    }

    fn requires_network(&self) -> bool {
        false
    }

    /// Give back the memory a loaded model holds, when the caller is done
    /// with it for now. The next request loads it again. Two 7B models
    /// resident at once is more than a 24 GB machine should be asked for.
    fn release(&self) {}
}

/// Returns an explicit placeholder rather than a plausible description.
///
/// The text says so in words, because a mock description that read like a real
/// one could reach a user as though the system had actually looked at their
/// footage.
#[derive(Debug, Default, Clone)]
pub struct MockVlm;

impl Adapter for MockVlm {
    fn name(&self) -> &'static str {
        "MockVlm"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl VlmAdapter for MockVlm {
    /// Nothing, rather than labels nothing looked for: a classification
    /// from a mock would file footage under categories nobody measured.
    fn labels(&self, _request: &VlmRequest) -> Result<Vec<ScoredLabel>, InferError> {
        Ok(Vec::new())
    }

    fn describe(&self, request: &VlmRequest) -> Result<String, InferError> {
        let name = request
            .image_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "frame".into());
        Ok(format!(
            "[no vision model loaded] Keyframe {name} was not described."
        ))
    }
}

/// Convenience for adapters that take a still on disk.
pub fn describe_still<A: VlmAdapter>(adapter: &A, path: &Path) -> Result<String, InferError> {
    adapter.describe(&VlmRequest::describe(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_description_announces_itself() {
        let out = MockVlm
            .describe(&VlmRequest::describe("/tmp/kf-01.jpg"))
            .unwrap();
        assert!(
            out.contains("no vision model"),
            "mock output must be recognisable as such, got: {out}"
        );
        assert!(out.contains("kf-01.jpg"));
    }
}
