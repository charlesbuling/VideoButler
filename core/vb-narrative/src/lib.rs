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

//! The narrative layer: Footage Graph in, Editing DSL out.
//!
//! This is the part of the system designed to be iterated on for years, so it
//! is deliberately insulated from everything that changes underneath it. It
//! knows nothing about ffmpeg, about how shots were detected, about how a
//! draft is written, or about which platform it is running on. It reads one
//! contract and writes another.
//!
//! - [`prior`] -- Subsystem A, the Prior Knowledge Base.
//! - [`orchestrator`] -- Subsystem B, the Reasoning Orchestrator.
//! - [`ground`] -- the gate every decision passes before it can leave.
//! - [`captions`] -- which clips of a cut carry a caption.
//! - [`dance`] -- which moments are dancing, and how two of them join.
//! - [`continuity`] -- 主体衔接: one action carried across every cut.

pub mod captions;
pub mod continuity;
pub mod dance;
pub mod ground;
pub mod orchestrator;
pub mod prior;

pub use captions::{CaptionPlace, caption_plan};
pub use ground::{GroundingReport, Rejection, ground};
pub use orchestrator::{
    Composition, MontageOrchestrator, Orchestrator, compose_continuity_grounded, compose_grounded,
};
pub use prior::{
    Mood, PriorStore, Style, Template, TemplateKind, TemplateParams, Topic, mine_shot_priors,
};

#[derive(Debug, thiserror::Error)]
pub enum NarrativeError {
    #[error("the footage graph contains no shots")]
    NoFootage,

    #[error("none of the {shots} shots is long enough to use (minimum is {min_slot_us}us)")]
    NoUsableShots { shots: usize, min_slot_us: u64 },

    #[error("a 主体衔接 cut was asked for, and no subject acts in this footage")]
    NoAction,

    #[error("template {0:?} is not in the prior store")]
    UnknownTemplate(String),

    #[error(transparent)]
    Prior(#[from] prior::PriorError),

    #[error("inference failed: {0}")]
    Inference(#[from] vb_infer::InferError),
}

#[cfg(test)]
mod module_wall {
    /// The narrative layer's dependency set is closed.
    ///
    /// It may reach `vb-contracts` and `vb-infer`, and nothing else in the
    /// workspace. This is the mechanical form of the architectural rule: the
    /// part that gets iterated on for years must not acquire a path to the
    /// parts that churn. If narrative code needs something perception knows,
    /// that belongs in the Footage Graph schema, not in a new dependency.
    ///
    /// The test reads the manifest rather than trusting a comment in it,
    /// because a comment cannot fail a build.
    #[test]
    fn narrative_depends_only_on_contracts_and_infer() {
        const ALLOWED: [&str; 2] = ["vb-contracts", "vb-infer"];
        const FORBIDDEN: [&str; 5] = [
            "vb-media",
            "vb-perception",
            "vb-render",
            "vb-export",
            "vb-pipeline",
        ];

        let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .expect("vb-narrative must have a manifest");

        // Only look below the [dependencies] header, so the explanatory
        // comment above it does not register as a dependency.
        let deps = manifest
            .split_once("[dependencies]")
            .expect("manifest must declare [dependencies]")
            .1;

        let declared: Vec<&str> = deps
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter_map(|l| l.split(['=', '.']).next())
            .map(str::trim)
            .filter(|name| name.starts_with("vb-"))
            .collect();

        for forbidden in FORBIDDEN {
            assert!(
                !declared.contains(&forbidden),
                "vb-narrative declares a dependency on {forbidden}. The narrative layer must \
                 stay insulated from the platform and runtime layers; if it needs something \
                 {forbidden} knows, put that in the Footage Graph contract instead."
            );
        }

        for name in &declared {
            assert!(
                ALLOWED.contains(name),
                "vb-narrative declares an unexpected workspace dependency {name}. Widening \
                 this set needs a deliberate decision, not an incidental import."
            );
        }
    }
}
