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

//! The identity-tracking seam.
//!
//! Cross-shot identity answers "is this the same person two shots later",
//! which the narrative layer cannot work without. Today this is faces grouped
//! by Apple Vision's feature prints. Phase 2's 主体衔接 (subject continuity)
//! follows subjects through each clip in 2D behind adapters of its own; this
//! interface stays as it is.
//!
//! One implementation is explicitly barred: CoTracker3 is CC-BY-NC and cannot
//! ship. TAPIR, BootsTAPIR and LocoTrack are the Apache-2.0 alternatives.

use crate::{Adapter, InferError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use vb_contracts::Micros;
use vb_contracts::footage_graph::EntityKind;

/// A subject followed across shots under one stable identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedEntity {
    pub id: String,
    pub kind: EntityKind,
    pub label: Option<String>,
    pub first_seen: Micros,
    pub last_seen: Micros,
    /// Shots this subject appears in, by shot id.
    pub shot_ids: Vec<String>,
    pub confidence: Option<f64>,
    /// A still of the subject, for showing it to a user.
    #[serde(default)]
    pub thumbnail: Option<std::path::PathBuf>,
    /// An appearance embedding, for grouping the same subject across clips.
    #[serde(default)]
    pub signature: Vec<f32>,
    /// The largest its face was seen, as a share of the picture's area: what
    /// tells someone the camera was on from a passer-by behind them. `None`
    /// from a tracker that does not measure faces.
    #[serde(default)]
    pub face_area: Option<f64>,
}

/// A time span the tracker should search, usually one shot.
#[derive(Debug, Clone)]
pub struct TrackWindow {
    pub start: Micros,
    pub end: Micros,
    /// Stills already extracted from this span, absolute. A tracker that works
    /// on images rather than video reads these instead of decoding again.
    pub stills: Vec<std::path::PathBuf>,
}

pub trait TrackerAdapter: Adapter + Send + Sync {
    fn track(
        &self,
        path: &Path,
        windows: &[(String, TrackWindow)],
    ) -> Result<Vec<TrackedEntity>, InferError>;

    fn requires_network(&self) -> bool {
        false
    }
}

/// Finds nothing.
///
/// As with the ASR mock, inventing entities would be worse than finding none:
/// the narrative layer treats a recurring entity as a reason to cut between
/// shots, so fabricated identities would produce confidently wrong edits.
#[derive(Debug, Default, Clone)]
pub struct MockTracker;

impl Adapter for MockTracker {
    fn name(&self) -> &'static str {
        "MockTracker"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl TrackerAdapter for MockTracker {
    fn track(
        &self,
        _path: &Path,
        _windows: &[(String, TrackWindow)],
    ) -> Result<Vec<TrackedEntity>, InferError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_finds_no_entities_rather_than_inventing_them() {
        let out = MockTracker
            .track(
                Path::new("/nonexistent.mp4"),
                &[(
                    "shot-01".into(),
                    TrackWindow {
                        start: Micros::ZERO,
                        end: Micros::from_secs(5),
                        stills: Vec::new(),
                    },
                )],
            )
            .unwrap();
        assert!(out.is_empty());
    }
}
