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

//! lavfi path escaping, and knowing when to give up.
//!
//! ffmpeg's filtergraph syntax uses `:` to separate options and `'` to quote,
//! and the escaping rules differ between the filtergraph parser and the shell.
//! A single quote in a filename cannot be escaped reliably at every level at
//! once. The project's recorded position on this is unambiguous: **raise an
//! error and let the caller fall back to fixed-length segmentation**, rather
//! than emit a filtergraph that silently addresses the wrong file.

use crate::MediaError;
use std::path::Path;

/// Escape a path for use inside a lavfi filtergraph argument.
///
/// On Windows a drive letter's backslash form has to become
/// forward-slash-plus-escaped-colon (`C:\a\b` becomes `C\:/a/b`), because the
/// filtergraph parser treats the backslash as an escape character and the
/// colon as an option separator.
///
/// # Errors
///
/// Returns [`MediaError::UnescapablePath`] for a path containing a single
/// quote. This is not a limitation worth working around; it is a case where
/// any output we produced would be a guess.
pub fn escape_for_lavfi(path: &Path) -> Result<String, MediaError> {
    let raw = path.to_str().ok_or_else(|| MediaError::UnescapablePath {
        path: path.display().to_string(),
        reason: "path is not valid UTF-8".into(),
    })?;

    if raw.contains('\'') {
        return Err(MediaError::UnescapablePath {
            path: raw.to_string(),
            reason: "contains a single quote, which lavfi cannot escape reliably at every \
                     parsing level; fall back to fixed-length segmentation"
                .into(),
        });
    }

    let mut out = String::with_capacity(raw.len() + 8);
    for ch in raw.chars() {
        match ch {
            '\\' => out.push('/'),
            ':' => out.push_str("\\:"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            _ => out.push(ch),
        }
    }
    Ok(out)
}

/// Can this path be addressed inside a filtergraph at all?
///
/// Callers use this to decide up front whether to plan a filtergraph pass or a
/// segmentation fallback, rather than discovering the problem mid-render.
pub fn is_lavfi_safe(path: &Path) -> bool {
    escape_for_lavfi(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_plain_posix_path_passes_through() {
        let p = PathBuf::from("/Users/me/Movies/hike.mp4");
        assert_eq!(escape_for_lavfi(&p).unwrap(), "/Users/me/Movies/hike.mp4");
    }

    #[test]
    fn a_windows_path_becomes_forward_slashes_with_an_escaped_colon() {
        let p = PathBuf::from(r"C:\Users\me\clip.mp4");
        assert_eq!(escape_for_lavfi(&p).unwrap(), r"C\:/Users/me/clip.mp4");
    }

    #[test]
    fn a_single_quote_is_refused_rather_than_guessed_at() {
        // The whole point: we do not emit a filtergraph we cannot trust.
        let p = PathBuf::from("/Users/me/Movies/my friend's hike.mp4");
        let err = escape_for_lavfi(&p).unwrap_err();
        assert!(matches!(err, MediaError::UnescapablePath { .. }));
        assert!(!is_lavfi_safe(&p));
    }

    #[test]
    fn filtergraph_metacharacters_are_escaped() {
        let p = PathBuf::from("/tmp/a,b[c];d.mp4");
        assert_eq!(escape_for_lavfi(&p).unwrap(), r"/tmp/a\,b\[c\]\;d.mp4");
    }

    #[test]
    fn spaces_and_unicode_are_fine() {
        let p = PathBuf::from("/tmp/山 行 2026.mp4");
        assert_eq!(escape_for_lavfi(&p).unwrap(), "/tmp/山 行 2026.mp4");
        assert!(is_lavfi_safe(&p));
    }
}
