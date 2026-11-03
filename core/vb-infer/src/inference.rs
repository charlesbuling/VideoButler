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

//! The LLM seam: the adapter the Reasoning Orchestrator talks to.
//!
//! This is the interface the project's two-phase plan turns on. Phase 1 puts
//! open models behind it; Phase 2 drops distilled, task-specialised models in
//! behind the same signature, and the narrative layer does not change.

use crate::{Adapter, InferError};
use serde::{Deserialize, Serialize};

/// A request for structured reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    /// Role and rules. Kept separate from the user turn so an adapter that
    /// has a real system-prompt slot can use it.
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// When set, the adapter should constrain output to this JSON Schema.
    /// llama.cpp supports this via GBNF grammars; adapters that cannot must
    /// still return parseable JSON or fail loudly.
    pub json_schema: Option<serde_json::Value>,
}

impl InferenceRequest {
    pub fn new(system: impl Into<String>, user: impl Into<String>) -> Self {
        InferenceRequest {
            system: system.into(),
            user: user.into(),
            max_tokens: 2048,
            temperature: 0.7,
            json_schema: None,
        }
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.json_schema = Some(schema);
        self
    }

    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = t;
        self
    }
}

/// Hosts a language model. Implemented by llama.cpp, MLC LLM, an
/// OpenAI-compatible endpoint, and the mock.
pub trait InferenceAdapter: Adapter + Send + Sync {
    fn complete(&self, request: &InferenceRequest) -> Result<String, InferError>;

    /// Does this adapter need the network? The offline guarantee means a
    /// pipeline run must be able to route around any adapter that does.
    fn requires_network(&self) -> bool {
        false
    }
}

/// Returns fixed, well-formed output without loading a model.
///
/// Lets the full pipeline run offline with no weights, which is how the
/// walking skeleton and CI exercise every contract boundary.
#[derive(Debug, Default, Clone)]
pub struct MockInference;

impl Adapter for MockInference {
    fn name(&self) -> &'static str {
        "MockInference"
    }
    fn is_mock(&self) -> bool {
        true
    }
}

impl InferenceAdapter for MockInference {
    fn complete(&self, request: &InferenceRequest) -> Result<String, InferError> {
        // When a schema was requested, return an empty object of the right
        // shape rather than prose. A caller that asked for JSON and got a
        // sentence would fail at parse time, hiding what actually happened.
        if request.json_schema.is_some() {
            Ok("{}".to_string())
        } else {
            Ok(String::from(
                "Mock inference output. No model was loaded and no decision was made.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mock_declares_itself_and_needs_no_network() {
        let m = MockInference;
        assert!(m.is_mock());
        assert!(!m.requires_network());
        assert_eq!(m.name(), "MockInference");
    }

    #[test]
    fn a_schema_request_gets_json_back_not_prose() {
        let m = MockInference;
        let req = InferenceRequest::new("s", "u").with_schema(serde_json::json!({}));
        let out = m.complete(&req).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok());
    }
}
