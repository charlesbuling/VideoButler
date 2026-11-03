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

//! Fetching a model file.
//!
//! Three things make this more than a GET. A model is hundreds of megabytes,
//! so it streams to disk and reports progress. A half-finished download must
//! never look like an installed model, so it lands under a temporary name and
//! is renamed only once it is complete and verified. And what arrives is
//! checked against a digest, because a truncated model file fails later, in
//! the middle of an analysis, in a way that looks like a bug in the engine.

use crate::CloudError;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// A finished download.
#[derive(Debug, Clone)]
pub struct Download {
    pub path: PathBuf,
    pub bytes: u64,
    /// What the file actually hashes to, recorded whether or not an expected
    /// digest was given.
    pub sha256: String,
}

/// How many times a dropped connection is picked up again before giving up.
///
/// Connections drop. On some networks a transfer of a few hundred megabytes
/// rarely completes in one go, and starting from zero each time means it
/// never completes at all -- so each attempt resumes where the last stopped.
const ATTEMPTS: u32 = 10;

/// Fetch `url` into `dest`, resuming across dropped connections.
///
/// `expected_sha256` is checked when given; when it is not, the server's own
/// content digest is used if it offers one. `on_progress` is called with the
/// bytes so far and the total when the server declared one, and returning
/// false from it cancels the download.
///
/// A partial download survives a failure, under a name no reader mistakes
/// for a model, so asking again carries on rather than starting over.
pub fn fetch_to_file(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    on_progress: &dyn Fn(u64, Option<u64>) -> bool,
) -> Result<Download, CloudError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CloudError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }

    // Written beside the destination so the rename at the end cannot cross a
    // filesystem.
    let partial = dest.with_extension("part");
    let mut advertised: Option<String> = None;
    let mut last_error: Option<CloudError> = None;
    let mut finished = false;

    let sources = sources(url);
    let mut current = 0usize;
    let mut refused = vec![false; sources.len()];
    for attempt in 0..ATTEMPTS {
        let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        let source = &sources[current];
        match fetch_from(source, &partial, have, &mut advertised, on_progress) {
            Ok(()) => {
                finished = true;
                break;
            }
            Err(CloudError::Cancelled) => {
                let _ = std::fs::remove_file(&partial);
                return Err(CloudError::Cancelled);
            }
            // A server that says the file is not there, or not for us, will
            // say the same on the next attempt -- but another source may not.
            Err(e @ CloudError::Status { status, .. })
                if (400..500).contains(&status) && status != 408 && status != 429 =>
            {
                refused[current] = true;
                match (0..sources.len()).find(|i| !refused[*i]) {
                    Some(next) => {
                        tracing::warn!(source = %source, error = %e, "refused; trying another source");
                        current = next;
                        last_error = Some(e);
                    }
                    None => return Err(e),
                }
            }
            Err(e) => {
                tracing::warn!(attempt, source = %source, error = %e, "download interrupted; resuming");
                last_error = Some(e);
                // The next attempt goes to the other source, if there is one
                // that has not refused: a host that cannot be reached from
                // here is usually a whole network's problem, not a moment's.
                if let Some(next) = (1..=sources.len())
                    .map(|k| (current + k) % sources.len())
                    .find(|i| !refused[*i])
                {
                    current = next;
                }
                // Backing off a little each time, so a network that is down
                // for a moment is not hammered through all the attempts.
                std::thread::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1)));
            }
        }
    }
    if !finished {
        return Err(last_error.unwrap_or(CloudError::Unreachable {
            url: url.to_string(),
            detail: "the download did not complete".into(),
        }));
    }

    // Hashed from the file rather than as it streamed, because a resumed
    // download arrived in pieces across several connections.
    let actual = digest_of(&partial)?;
    let written = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
    if let Some(expected) = expected_sha256.or(advertised.as_deref()) {
        if !expected.eq_ignore_ascii_case(&actual) {
            // Kept off disk entirely: a file that fails its digest is either
            // truncated or not the file that was asked for, and either way
            // installing it turns into a confusing failure much later.
            let _ = std::fs::remove_file(&partial);
            return Err(CloudError::Checksum {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    std::fs::rename(&partial, dest).map_err(|source| CloudError::Io {
        path: dest.display().to_string(),
        source,
    })?;

    Ok(Download {
        path: dest.to_path_buf(),
        bytes: written,
        sha256: actual,
    })
}

/// One connection's worth of a download: from `have` bytes on, appended to
/// `partial`, or from the start when the server will not resume.
/// Everywhere a file can be fetched from, the one named first.
///
/// Hugging Face is slow or unreachable from some networks, and hf-mirror.com
/// serves the same files byte for byte. What arrives is checked against the
/// catalogue's SHA-256 either way, so a mirror can make a download fail but
/// never install the wrong file.
pub fn sources(url: &str) -> Vec<String> {
    let mut out = vec![url.to_string()];
    if let Some(rest) = url.strip_prefix("https://huggingface.co/") {
        out.push(format!("https://hf-mirror.com/{rest}"));
    }
    out
}

fn fetch_from(
    url: &str,
    partial: &Path,
    have: u64,
    advertised: &mut Option<String>,
    on_progress: &dyn Fn(u64, Option<u64>) -> bool,
) -> Result<(), CloudError> {
    // A host that does not answer is given up on in seconds rather than the
    // minute and more the system allows, so the other source gets its turn.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .build();
    let mut request = agent
        .get(url)
        .timeout(std::time::Duration::from_secs(60 * 60));
    if have > 0 {
        request = request.set("Range", &format!("bytes={have}-"));
    }
    let response = match request.call() {
        Ok(response) => response,
        // Asked to resume past the end: what is on disk is already the whole
        // file, or is not this file at all. The digest check decides which.
        Err(ureq::Error::Status(416, _)) if have > 0 => return Ok(()),
        Err(e) => return Err(from_ureq(url, e)),
    };

    // Hugging Face puts the file's own sha256 here for storage-backed files,
    // which is a digest of the content rather than of whatever a proxy chose
    // to send.
    if advertised.is_none() {
        *advertised = response
            .header("X-Linked-Etag")
            .or_else(|| response.header("ETag"))
            .map(|tag| tag.trim_matches('"').to_string())
            .filter(|tag| tag.len() == 64 && tag.chars().all(|c| c.is_ascii_hexdigit()));
    }

    let resuming = response.status() == 206;
    let start = if resuming { have } else { 0 };
    let total: Option<u64> = if resuming {
        // "bytes 1000-1999/2000": the whole file's length is after the slash.
        response
            .header("Content-Range")
            .and_then(|range| range.rsplit('/').next())
            .and_then(|total| total.parse().ok())
    } else {
        response
            .header("Content-Length")
            .and_then(|v| v.parse().ok())
    };

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resuming)
        .truncate(!resuming)
        .open(partial)
        .map_err(|source| CloudError::Io {
            path: partial.display().to_string(),
            source,
        })?;

    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; 1 << 16];
    let mut written = start;
    loop {
        let read = reader.read(&mut buffer).map_err(|source| CloudError::Io {
            path: url.to_string(),
            source,
        })?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|source| CloudError::Io {
                path: partial.display().to_string(),
                source,
            })?;
        written += read as u64;
        if !on_progress(written, total) {
            return Err(CloudError::Cancelled);
        }
    }
    file.flush().map_err(|source| CloudError::Io {
        path: partial.display().to_string(),
        source,
    })?;

    // A connection that closed cleanly but early is still a short file.
    if let Some(total) = total {
        if written < total {
            return Err(CloudError::Unreachable {
                url: url.to_string(),
                detail: format!("the connection closed after {written} of {total} bytes"),
            });
        }
    }
    Ok(())
}

/// The sha256 of a file already on disk, for checking an install.
pub fn digest_of(path: &Path) -> Result<String, CloudError> {
    let mut file = std::fs::File::open(path).map_err(|source| CloudError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(|source| CloudError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn from_ureq(url: &str, error: ureq::Error) -> CloudError {
    match error {
        ureq::Error::Status(status, response) => {
            let body = response
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(400)
                .collect();
            CloudError::Status {
                url: url.to_string(),
                status,
                body,
            }
        }
        ureq::Error::Transport(transport) => CloudError::Unreachable {
            url: url.to_string(),
            detail: transport.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hugging_face_file_can_also_come_from_the_mirror() {
        assert_eq!(
            sources("https://huggingface.co/a/b/resolve/main/m.gguf"),
            vec![
                "https://huggingface.co/a/b/resolve/main/m.gguf".to_string(),
                "https://hf-mirror.com/a/b/resolve/main/m.gguf".to_string(),
            ]
        );
        assert_eq!(sources("https://example.com/m.onnx").len(), 1);
    }

    #[test]
    fn a_digest_matches_a_known_value() {
        // The sha256 of "abc", which is the standard test vector.
        let dir = std::env::temp_dir().join("vb-download-digest");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("abc.txt");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            digest_of(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreachable_host_is_an_error_rather_than_a_hang() {
        // Reserved for documentation, so nothing can be listening.
        let dir = std::env::temp_dir().join("vb-download-unreachable");
        let err = fetch_to_file(
            "http://192.0.2.1:9/model.bin",
            &dir.join("model.bin"),
            None,
            &|_, _| true,
        )
        .unwrap_err();
        assert!(matches!(err, CloudError::Unreachable { .. }), "got {err}");
        assert!(!dir.join("model.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
