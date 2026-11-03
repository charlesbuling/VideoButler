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

//! Model adapters.
//!
//! Every model the pipeline calls sits behind a trait here, which is what
//! makes the inference engine an implementation detail rather than a product
//! decision. llama.cpp, MLC LLM and a cloud endpoint all satisfy
//! [`InferenceAdapter`]; swapping between them per device tier changes
//! configuration, not callers. The same holds for the vision models the
//! model host runs on ONNX Runtime, such as [`DepthAdapter`].
//!
//! # Mocks are first-class
//!
//! Every trait has a mock implementation, and every trait reports
//! [`Adapter::is_mock`]. This is not a testing convenience. The pipeline must
//! produce a complete result with no network and no model weights, and the
//! output of such a run has to be clearly distinguishable from a real one, all
//! the way into the provenance record. A montage built from mock descriptions
//! is a structural smoke test, not a usable cut.

pub mod asr;
pub mod depth;
pub mod faces;
pub mod inference;
pub mod people;
pub mod pose;
pub mod tracker;
pub mod vlm;

pub use asr::{AsrAdapter, MockAsr, Utterance};
pub use depth::{DepthAdapter, DepthMap, MockDepth, RgbImage};
pub use faces::{FaceBox, FaceDetector, MockFaces};
pub use inference::{InferenceAdapter, InferenceRequest, MockInference};
pub use people::{MockPeople, PersonMask, PersonSegmenter};
pub use pose::{BodyPose, MockPose, PoseEstimator};
pub use tracker::{MockTracker, TrackedEntity, TrackerAdapter};
pub use vlm::{MockVlm, ScoredLabel, VlmAdapter, VlmRequest};

/// Shared identity for every adapter, so provenance can always record which
/// implementation actually ran.
pub trait Adapter {
    /// Recorded in the graph's provenance, e.g. `WhisperCpp` or `MockAsr`.
    fn name(&self) -> &'static str;

    /// Does this produce synthetic output? Anything true here must be
    /// surfaced to the user rather than presented as a real result.
    fn is_mock(&self) -> bool {
        false
    }

    /// What cached output from this adapter is keyed by: its name, and for
    /// one whose answers depend on more than its name -- which model file,
    /// which table -- that as well, so changing either misses the cache
    /// rather than serving answers another model gave.
    fn cache_identity(&self) -> String {
        self.name().to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InferError {
    #[error("{adapter} is not available: {reason}")]
    Unavailable {
        adapter: &'static str,
        reason: String,
    },

    #[error("{adapter} failed: {detail}")]
    Failed {
        adapter: &'static str,
        detail: String,
    },

    #[error("{adapter} returned output that could not be parsed: {detail}")]
    BadOutput {
        adapter: &'static str,
        detail: String,
    },

    #[error("{adapter} requires a network connection, which is unavailable")]
    NetworkRequired { adapter: &'static str },
}
