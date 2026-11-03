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

//! An OpenAI-compatible endpoint, behind the adapters the engine already has.
//!
//! Compatible rather than a named provider, because the same request shape
//! reaches a hosted API and a model server running on the user's own machine.
//! "Cloud model" in the interface is really "a model somewhere else", and
//! pointing it at `http://localhost:11434/v1` is a supported answer.
//!
//! Both adapters here report `requires_network`, which is what lets a run
//! refuse them when the user asked for an offline result, and neither is ever
//! a mock: if this cannot reach the endpoint it fails, and the caller falls
//! back to the on-device path and says so.

use crate::{CloudError, REQUEST_TIMEOUT, download::from_ureq};
use serde_json::{Value, json};
use std::path::Path;
use vb_infer::{Adapter, InferError, InferenceAdapter, InferenceRequest, VlmAdapter, VlmRequest};

/// Where a model lives and how to address it.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// Up to and including the version segment, e.g. `https://host/v1`.
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

impl Endpoint {
    pub fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Send a chat completion and return the first choice's text.
    fn chat(&self, body: Value) -> Result<String, CloudError> {
        if !self.is_configured() {
            return Err(CloudError::NotConfigured);
        }
        let url = self.chat_url();
        let mut request = ureq::post(&url).timeout(REQUEST_TIMEOUT);
        if !self.api_key.trim().is_empty() {
            request = request.set("Authorization", &format!("Bearer {}", self.api_key.trim()));
        }
        let response = request.send_json(body).map_err(|e| from_ureq(&url, e))?;
        let value: Value = response.into_json().map_err(|e| CloudError::BadResponse {
            url: url.clone(),
            detail: e.to_string(),
        })?;

        value["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| CloudError::BadResponse {
                url,
                detail: format!(
                    "no message content in the reply: {}",
                    value.to_string().chars().take(300).collect::<String>()
                ),
            })
    }

    /// Ask for the model list, which is the cheapest way to tell a working
    /// endpoint from a typo before a long run depends on it.
    pub fn check(&self) -> Result<Vec<String>, CloudError> {
        if !self.is_configured() {
            return Err(CloudError::NotConfigured);
        }
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut request = ureq::get(&url).timeout(std::time::Duration::from_secs(20));
        if !self.api_key.trim().is_empty() {
            request = request.set("Authorization", &format!("Bearer {}", self.api_key.trim()));
        }
        let value: Value = request
            .call()
            .map_err(|e| from_ureq(&url, e))?
            .into_json()
            .map_err(|e| CloudError::BadResponse {
                url: url.clone(),
                detail: e.to_string(),
            })?;
        Ok(value["data"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn to_infer_error(adapter: &'static str, error: CloudError) -> InferError {
    match error {
        CloudError::NotConfigured => InferError::Unavailable {
            adapter,
            reason: "no endpoint is configured".into(),
        },
        CloudError::Unreachable { url, detail } => InferError::Unavailable {
            adapter,
            reason: format!("could not reach {url}: {detail}"),
        },
        CloudError::BadResponse { detail, .. } => InferError::BadOutput { adapter, detail },
        other => InferError::Failed {
            adapter,
            detail: other.to_string(),
        },
    }
}

/// Reasoning over an endpoint: the orchestrator's model when the user picks
/// the cloud.
#[derive(Debug, Clone, Default)]
pub struct CloudInference {
    pub endpoint: Endpoint,
}

impl Adapter for CloudInference {
    fn name(&self) -> &'static str {
        "CloudInference"
    }
}

impl InferenceAdapter for CloudInference {
    fn complete(&self, request: &InferenceRequest) -> Result<String, InferError> {
        let mut body = json!({
            "model": self.endpoint.model,
            "messages": [
                { "role": "system", "content": request.system },
                { "role": "user", "content": request.user }
            ],
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
        });
        // Ask for JSON when a schema was requested. Endpoints differ in how
        // far they honour this, so the caller still has to parse defensively.
        if request.json_schema.is_some() {
            body["response_format"] = json!({ "type": "json_object" });
        }
        self.endpoint
            .chat(body)
            .map_err(|e| to_infer_error("CloudInference", e))
    }

    fn requires_network(&self) -> bool {
        true
    }
}

/// Description of a still over an endpoint.
#[derive(Debug, Clone, Default)]
pub struct CloudVlm {
    pub endpoint: Endpoint,
}

impl Adapter for CloudVlm {
    fn name(&self) -> &'static str {
        "CloudVlm"
    }
}

impl VlmAdapter for CloudVlm {
    fn describe(&self, request: &VlmRequest) -> Result<String, InferError> {
        let image = data_url(&request.image_path).map_err(|detail| InferError::Failed {
            adapter: "CloudVlm",
            detail,
        })?;
        let body = json!({
            "model": self.endpoint.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": request.prompt },
                    { "type": "image_url", "image_url": { "url": image } }
                ]
            }],
            "max_tokens": request.max_tokens,
        });
        self.endpoint
            .chat(body)
            .map(|text| text.trim().to_string())
            .map_err(|e| to_infer_error("CloudVlm", e))
    }

    fn requires_network(&self) -> bool {
        true
    }
}

/// A still as a `data:` URL, which is how an image reaches a chat endpoint
/// without being uploaded anywhere first.
fn data_url(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let kind = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    Ok(format!("data:{kind};base64,{}", base64(&bytes)))
}

/// Standard base64.
///
/// Written out rather than pulled in: it is fifteen lines, and this crate is
/// already the one place the engine reaches the network, so its dependency
/// list is the one worth keeping shortest.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_still_becomes_a_data_url_with_the_right_media_type() {
        let dir = std::env::temp_dir().join("vb-cloud-dataurl");
        let _ = std::fs::create_dir_all(&dir);
        let jpg = dir.join("frame.jpg");
        std::fs::write(&jpg, b"foobar").unwrap();
        assert_eq!(data_url(&jpg).unwrap(), "data:image/jpeg;base64,Zm9vYmFy");

        let png = dir.join("frame.png");
        std::fs::write(&png, b"foobar").unwrap();
        assert!(
            data_url(&png)
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unconfigured_endpoint_is_unavailable_rather_than_a_failed_request() {
        // The difference matters upstream: unavailable falls back to the
        // on-device path, failed is worth showing the user.
        let adapter = CloudInference::default();
        let err = adapter
            .complete(&InferenceRequest::new("s", "u"))
            .unwrap_err();
        assert!(matches!(err, InferError::Unavailable { .. }), "got {err}");
        assert!(adapter.requires_network());
    }

    #[test]
    fn the_chat_url_is_built_whether_or_not_the_base_ends_in_a_slash() {
        for base in ["https://host/v1", "https://host/v1/"] {
            let endpoint = Endpoint {
                base_url: base.into(),
                model: "m".into(),
                api_key: String::new(),
            };
            assert_eq!(endpoint.chat_url(), "https://host/v1/chat/completions");
        }
    }

    #[test]
    fn an_unreachable_endpoint_reports_where_it_tried() {
        let adapter = CloudInference {
            endpoint: Endpoint {
                base_url: "http://192.0.2.1:9/v1".into(),
                model: "m".into(),
                api_key: String::new(),
            },
        };
        let err = adapter
            .complete(&InferenceRequest::new("s", "u"))
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("192.0.2.1"), "got {message}");
    }
}
