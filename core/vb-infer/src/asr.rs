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

//! The speech-recognition seam.
//!
//! Transcripts are what align dialogue to picture, so ASR output carries
//! timestamps on the same microsecond timebase as everything else.

use crate::{Adapter, InferError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use vb_contracts::Micros;

/// One transcribed span of speech.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utterance {
    pub start: Micros,
    pub end: Micros,
    pub text: String,
    pub speaker: Option<String>,
    pub language: Option<String>,
    pub confidence: Option<f64>,
}

pub trait AsrAdapter: Adapter + Send + Sync {
    /// Transcribe a media file. Implementations extract audio themselves,
    /// since the formats they accept differ.
    fn transcribe(&self, path: &Path) -> Result<Vec<Utterance>, InferError>;

    fn requires_network(&self) -> bool {
        false
    }
}

/// Produces no utterances at all.
///
/// Returning an empty transcript rather than invented speech is deliberate:
/// fabricated dialogue would flow into subtitles and into the narrative
/// layer's sense of which shots carry a spoken beat, quietly corrupting the
/// cut. Silence is the honest mock.
#[derive(Debug, Default, Clone)]
pub struct MockAsr;

impl Adapter for MockAsr {
    fn name(&self) -> &'static str {
        "MockAsr"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl AsrAdapter for MockAsr {
    fn transcribe(&self, _path: &Path) -> Result<Vec<Utterance>, InferError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_returns_silence_rather_than_invented_speech() {
        let out = MockAsr.transcribe(Path::new("/nonexistent.mp4")).unwrap();
        assert!(out.is_empty());
        assert!(MockAsr.is_mock());
    }
}
