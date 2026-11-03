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

//! Schema validation.
//!
//! The JSON Schema in `contracts/` is the source of truth; these Rust types
//! follow it. Validation runs against the schema text embedded at compile
//! time, so it works with no filesystem and no network -- which matters on
//! mobile, where neither is a given.

use std::sync::OnceLock;

pub const FOOTAGE_GRAPH_SCHEMA: &str =
    include_str!("../../../contracts/footage-graph/0.1/schema.json");
pub const EDITING_DSL_SCHEMA: &str = include_str!("../../../contracts/editing-dsl/0.1/schema.json");

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("the {contract} schema itself is invalid: {message}")]
    BadSchema {
        contract: &'static str,
        message: String,
    },
    #[error("document does not satisfy {contract}:\n{}", .errors.join("\n"))]
    Invalid {
        contract: &'static str,
        errors: Vec<String>,
    },
    #[error("could not serialise the document for validation: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn compiled(
    cell: &'static OnceLock<Result<jsonschema::Validator, String>>,
    text: &str,
) -> &'static Result<jsonschema::Validator, String> {
    cell.get_or_init(|| {
        let schema: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("schema is not valid JSON: {e}"))?;
        jsonschema::validator_for(&schema).map_err(|e| e.to_string())
    })
}

fn validate_against(
    contract: &'static str,
    cell: &'static OnceLock<Result<jsonschema::Validator, String>>,
    text: &str,
    instance: &serde_json::Value,
) -> Result<(), ValidationError> {
    let validator = match compiled(cell, text) {
        Ok(v) => v,
        Err(message) => {
            return Err(ValidationError::BadSchema {
                contract,
                message: message.clone(),
            });
        }
    };

    // Collect every error rather than stopping at the first. A contract
    // violation is usually several related mistakes, and reporting one at a
    // time turns a schema change into a guessing game.
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("  at {}: {}", e.instance_path, e))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::Invalid { contract, errors })
    }
}

static FOOTAGE_GRAPH_VALIDATOR: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
static EDITING_DSL_VALIDATOR: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();

pub fn validate_footage_graph_value(value: &serde_json::Value) -> Result<(), ValidationError> {
    validate_against(
        "footage-graph/0.1",
        &FOOTAGE_GRAPH_VALIDATOR,
        FOOTAGE_GRAPH_SCHEMA,
        value,
    )
}

pub fn validate_editing_dsl_value(value: &serde_json::Value) -> Result<(), ValidationError> {
    validate_against(
        "editing-dsl/0.1",
        &EDITING_DSL_VALIDATOR,
        EDITING_DSL_SCHEMA,
        value,
    )
}

pub fn validate_footage_graph(
    graph: &crate::footage_graph::FootageGraph,
) -> Result<(), ValidationError> {
    validate_footage_graph_value(&serde_json::to_value(graph)?)
}

pub fn validate_editing_dsl(dsl: &crate::editing_dsl::EditingDsl) -> Result<(), ValidationError> {
    validate_editing_dsl_value(&serde_json::to_value(dsl)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_schemas_compile() {
        assert!(
            compiled(&FOOTAGE_GRAPH_VALIDATOR, FOOTAGE_GRAPH_SCHEMA).is_ok(),
            "footage-graph schema failed to compile"
        );
        assert!(
            compiled(&EDITING_DSL_VALIDATOR, EDITING_DSL_SCHEMA).is_ok(),
            "editing-dsl schema failed to compile"
        );
    }

    #[test]
    fn a_document_missing_required_fields_is_rejected() {
        let empty = serde_json::json!({});
        assert!(validate_footage_graph_value(&empty).is_err());
        assert!(validate_editing_dsl_value(&empty).is_err());
    }

    #[test]
    fn the_wrong_schema_version_is_rejected() {
        // Version pinning is how a 0.2 document is stopped from being read as
        // a 0.1 one by a build that predates it.
        let graph = serde_json::json!({
            "schemaVersion": "footage-graph/0.2",
            "generatedAt": "2026-09-19T00:00:00Z",
            "sources": [{
                "id": "s",
                "path": "/x.mp4",
                "contentHash": "blake3:0000000000000000000000000000000000000000000000000000000000000000",
                "durationUs": 1000
            }],
            "shots": []
        });
        assert!(validate_footage_graph_value(&graph).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_so_typos_surface() {
        let graph = serde_json::json!({
            "schemaVersion": "footage-graph/0.1",
            "generatedAt": "2026-09-19T00:00:00Z",
            "sources": [],
            "shots": [],
            "definitelyNotAField": 1
        });
        assert!(validate_footage_graph_value(&graph).is_err());
    }
}
