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
///
/// On Windows a path is a drive or a share and backslashes (RFC 8089):
/// `C:\a b.mp4` is `file:///C:/a%20b.mp4` and `\\host\share\a.mp4` is
/// `file://host/share/a.mp4`. The verbatim prefix `canonicalize` puts on a
/// path (`\\?\C:\`, `\\?\UNC\host\`) is no part of either.
pub fn file_url(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let (mut out, rest) = if cfg!(windows) {
        windows_url_parts(&raw)
    } else {
        ("file://".to_string(), raw.into_owned())
    };
    for byte in rest.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A Windows path as the start of its URL -- scheme, authority and drive,
/// written as they are -- and the rest, slashed, to be escaped.
fn windows_url_parts(raw: &str) -> (String, String) {
    let slashed = raw.replace('\\', "/");
    let plain = match slashed.strip_prefix("//?/UNC/") {
        Some(share) => format!("//{share}"),
        None => slashed
            .strip_prefix("//?/")
            .map(str::to_string)
            .unwrap_or(slashed),
    };
    if let Some(share) = plain.strip_prefix("//") {
        // The host is the URL's authority.
        let (host, rest) = share.split_at(share.find('/').unwrap_or(share.len()));
        return (format!("file://{host}"), rest.to_string());
    }
    let drive = plain.as_bytes();
    if drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':' {
        return (format!("file:///{}", &plain[..2]), plain[2..].to_string());
    }
    ("file://".to_string(), plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_becomes_a_file_url() {
        assert_eq!(
            file_url(Path::new("/Users/zhou/a.mp4")),
            "file:///Users/zhou/a.mp4"
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

    #[test]
    fn windows_drives_and_shares_are_written_as_rfc_8089_says() {
        // Not behind cfg(windows): the parts are plain string work, and a Mac
        // build should catch a change to them too.
        let url = |p: &str| {
            let (start, rest) = windows_url_parts(p);
            format!("{start}{rest}")
        };
        assert_eq!(url(r"C:\Users\zhou\a.mp4"), "file:///C:/Users/zhou/a.mp4");
        assert_eq!(url(r"\\?\K:\Videos\a.mp4"), "file:///K:/Videos/a.mp4");
        assert_eq!(url(r"\\nas\share\a.mp4"), "file://nas/share/a.mp4");
        assert_eq!(url(r"\\?\UNC\nas\share\a.mp4"), "file://nas/share/a.mp4");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_with_a_space_becomes_a_file_url() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\zhou\my clip #2.mp4")),
            "file:///C:/Users/zhou/my%20clip%20%232.mp4"
        );
        assert_eq!(
            file_url(Path::new(r"\\?\K:\旅行\a.mp4")),
            "file:///K:/%E6%97%85%E8%A1%8C/a.mp4"
        );
    }
}
