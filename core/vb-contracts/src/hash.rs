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

//! Content addressing.
//!
//! Every cache key in the pipeline is
//! `blake3(source_content_hash + stage_name + stage_params_hash)`. Incremental
//! processing with on-disk reuse is the load-bearing performance lever for the
//! "two hours of 4K, finished the same evening" target, so these hashes are
//! part of the contract rather than an implementation detail.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::Path;

/// A blake3 digest, serialised as `blake3:<64 hex chars>` to match the schema
/// pattern and to stay self-describing if the algorithm ever changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Blake3Hash(pub [u8; 32]);

impl Blake3Hash {
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Blake3Hash(*blake3::hash(bytes).as_bytes())
    }

    pub fn of_str(s: &str) -> Self {
        Self::of_bytes(s.as_bytes())
    }

    /// Hash a file, mapped rather than read into memory so a 4K source does
    /// not have to fit in it, and across every core: a trip is a hundred
    /// gigabytes, and hashing it one 8 KB read at a time took minutes.
    pub fn of_file(path: &Path) -> std::io::Result<Self> {
        let mut hasher = blake3::Hasher::new();
        hasher.update_mmap_rayon(path)?;
        Ok(Blake3Hash(*hasher.finalize().as_bytes()))
    }

    /// Combine components into a cache key. Each part is length-prefixed so
    /// that `["ab", "c"]` and `["a", "bc"]` cannot collide.
    pub fn of_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for part in parts {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        Blake3Hash(*hasher.finalize().as_bytes())
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix("blake3:")?;
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(Blake3Hash(out))
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blake3:{}", self.to_hex())
    }
}

impl Serialize for Blake3Hash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Blake3Hash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Blake3Hash::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("expected blake3:<64 hex chars>, got {s:?}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_round_trip_through_their_string_form() {
        let h = Blake3Hash::of_str("hello");
        assert_eq!(Blake3Hash::parse(&h.to_string()), Some(h));
    }

    #[test]
    fn a_malformed_hash_is_rejected_not_silently_accepted() {
        assert!(Blake3Hash::parse("sha256:abc").is_none());
        assert!(Blake3Hash::parse("blake3:tooshort").is_none());
        assert!(Blake3Hash::parse("blake3:zz00").is_none());
    }

    #[test]
    fn length_prefixing_stops_cache_key_collisions() {
        // Without length prefixes these two would hash identically, and two
        // different pipeline stages would share a cache entry.
        let a = Blake3Hash::of_parts(&[b"ab", b"c"]);
        let b = Blake3Hash::of_parts(&[b"a", b"bc"]);
        assert_ne!(a, b);
    }

    #[test]
    fn serde_uses_the_prefixed_string_form() {
        let h = Blake3Hash::of_str("x");
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.starts_with("\"blake3:"));
        let back: Blake3Hash = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }
}
