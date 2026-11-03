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

//! The Four Contracts.
//!
//! This crate is the root of the dependency graph and depends on no other
//! `vb-*` crate. Everything else in the workspace depends on it, which is what
//! lets the narrative layer talk to the perception and render layers without
//! ever referencing them.
//!
//! - [`footage_graph`] -- Input Contract, written by perception.
//! - [`editing_dsl`] -- Output Contract, written by the narrative layer.
//! - [`time`] -- the shared microsecond timebase.
//! - [`hash`] -- content addressing for incremental work.
//! - [`validate`] -- JSON Schema checking against the embedded contracts.

pub mod editing_dsl;
pub mod footage_graph;
pub mod hash;
pub mod time;
pub mod validate;

pub use editing_dsl::{EditingDsl, ResolvedSlot, Slot, SlotKind, Timeline};
pub use footage_graph::{FootageGraph, MediaKind, Shot, Source, Stage};
pub use hash::Blake3Hash;
pub use time::{FrameRate, Micros};

/// Current UTC time as an RFC 3339 string, which is what both schemas expect.
pub fn now_rfc3339() -> String {
    time::_now_rfc3339()
}

#[cfg(test)]
mod contract_fixture_tests {
    use super::*;
    use std::path::PathBuf;

    fn examples_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/examples")
    }

    fn read(name: &str) -> String {
        let path = examples_dir().join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
    }

    /// Parse, re-serialise, re-validate. This is the test that keeps the Rust
    /// types and the JSON Schema honest with each other: if a field is dropped
    /// or renamed on either side, the round trip stops matching.
    #[test]
    fn footage_graph_fixture_round_trips() {
        let text = read("footage-graph.example.json");
        let original: serde_json::Value = serde_json::from_str(&text).unwrap();
        validate::validate_footage_graph_value(&original).expect("fixture must satisfy the schema");

        let typed: FootageGraph = serde_json::from_str(&text).expect("fixture must parse");
        let reserialised = serde_json::to_value(&typed).unwrap();
        validate::validate_footage_graph_value(&reserialised)
            .expect("re-serialised graph must still satisfy the schema");
        assert_eq!(
            original, reserialised,
            "round trip changed the document; the Rust types and the schema have diverged"
        );
    }

    #[test]
    fn editing_dsl_fixture_round_trips() {
        let text = read("editing-dsl.example.json");
        let original: serde_json::Value = serde_json::from_str(&text).unwrap();
        validate::validate_editing_dsl_value(&original).expect("fixture must satisfy the schema");

        let typed: EditingDsl = serde_json::from_str(&text).expect("fixture must parse");
        let reserialised = serde_json::to_value(&typed).unwrap();
        validate::validate_editing_dsl_value(&reserialised)
            .expect("re-serialised DSL must still satisfy the schema");
        assert_eq!(
            original, reserialised,
            "round trip changed the document; the Rust types and the schema have diverged"
        );
    }

    /// The DSL must not acquire a field that stores absolute timeline
    /// position. Slots carry order and duration; position is derived. A
    /// stored position is the drift bug coming back.
    #[test]
    fn the_dsl_schema_has_no_absolute_timeline_field() {
        let schema = validate::EDITING_DSL_SCHEMA;
        for banned in [
            "timelineStartUs",
            "timelineEndUs",
            "absoluteStartUs",
            "startOnTimelineUs",
        ] {
            assert!(
                !schema.contains(banned),
                "editing-dsl schema defines {banned}: absolute timeline positions must be \
                 derived by resolve_timeline, never stored"
            );
        }
    }
}
