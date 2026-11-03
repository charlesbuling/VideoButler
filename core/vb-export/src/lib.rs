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

// The editor draft format has objects of fifty-odd keys, and `json!` expands
// one macro level per key.
#![recursion_limit = "512"]

//! Draft export.
//!
//! The DSL is a decision manifest, and a draft file is that manifest expressed
//! in a form some editor can open. Every writer sits behind [`DraftWriter`] so
//! a downstream format change stays contained: CapCut's draft format is
//! undocumented and moves, which is exactly why OTIO is the primary target and
//! CapCut is one implementation among several.

pub mod jianying;
pub mod otio;

pub use jianying::{
    DraftMedia, DraftSummary, Editor, EditorInstall, JianyingWriter, detect_editors,
};
pub use otio::OtioWriter;

use std::path::{Path, PathBuf};
use vb_contracts::{EditingDsl, FootageGraph};

/// Writes a DSL out as a project file some editor can open.
pub trait DraftWriter {
    fn name(&self) -> &'static str;

    /// Conventional file extension, without the dot.
    fn extension(&self) -> &'static str;

    fn write(
        &self,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        dest: &Path,
    ) -> Result<PathBuf, ExportError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not serialise the {format} draft: {source}")]
    Serialize {
        format: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("slot {slot_id} references source {source_id}, which is not in the footage graph")]
    DanglingSource { slot_id: String, source_id: String },

    #[error("{format} export is not implemented yet")]
    NotImplemented { format: &'static str },
}

/// Percent-encode a filesystem path into a `file://` URL.
///
/// Only the characters that would break a URL are escaped; a path with a space
/// or a hash in it is ordinary on every platform this ships to and must not
/// produce a draft that silently points at nothing.
pub fn file_url(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::from("file://");
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_becomes_a_file_url() {
        assert_eq!(
            file_url(Path::new("/Users/me/a.mp4")),
            "file:///Users/me/a.mp4"
        );
    }

    #[test]
    fn spaces_and_hashes_are_escaped_rather_than_breaking_the_url() {
        assert_eq!(
            file_url(Path::new("/tmp/my clip #2.mp4")),
            "file:///tmp/my%20clip%20%232.mp4"
        );
    }

    #[test]
    fn non_ascii_paths_survive_as_utf8_percent_escapes() {
        let url = file_url(Path::new("/tmp/山行.mp4"));
        assert!(url.starts_with("file:///tmp/"));
        assert!(url.contains('%'));
        assert!(url.ends_with(".mp4"));
    }
}
