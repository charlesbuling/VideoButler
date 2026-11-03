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

//! The incremental cache.
//!
//! Getting a finished cut out of two hours of 4K in an evening depends far
//! more on not redoing work than on raw model speed. Every stage's output is
//! stored under a key derived from the source content, the stage, and the
//! stage's parameters, so changing one setting re-runs that stage and what
//! follows it, and nothing else.
//!
//! Keying on content rather than path means a renamed or moved file still
//! hits, and an edited file with the same name correctly misses.

use std::path::{Path, PathBuf};
use vb_contracts::{Blake3Hash, Stage};

/// A content-addressed store of stage outputs under a project directory.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
    /// False when the caller asked to look at the footage again: nothing is
    /// read back, and what the run finds replaces what was there.
    reads: bool,
}

/// How much a cache holds, for showing the user before they clear it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheSize {
    pub entries: usize,
    pub bytes: u64,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Cache {
            root: root.into(),
            reads: true,
        }
    }

    /// The same store, answering every lookup with a miss. A fresh analysis
    /// still writes, so the run after it reuses what this one found.
    pub fn write_only(mut self) -> Self {
        self.reads = false;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The key for one stage's output.
    ///
    /// The stage name is part of the key, so two stages that happen to be
    /// configured identically do not collide.
    pub fn key(source: &Blake3Hash, stage: Stage, params: &Blake3Hash) -> Blake3Hash {
        Blake3Hash::of_parts(&[
            source.to_hex().as_bytes(),
            stage.name().as_bytes(),
            params.to_hex().as_bytes(),
        ])
    }

    fn path_for(&self, key: &Blake3Hash) -> PathBuf {
        // Shard by the first byte so a long project does not put tens of
        // thousands of files in one directory.
        let hex = key.to_hex();
        self.root.join(&hex[0..2]).join(format!("{hex}.json"))
    }

    pub fn get(&self, key: &Blake3Hash) -> Option<String> {
        if !self.reads {
            return None;
        }
        std::fs::read_to_string(self.path_for(key)).ok()
    }

    pub fn put(&self, key: &Blake3Hash, contents: &str) -> Result<(), CacheError> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CacheError::Io {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        // Write to a temporary file and rename, so an interrupted run leaves
        // no half-written entry that a later run would read as complete.
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, contents).map_err(|e| CacheError::Io {
            path: temp.display().to_string(),
            source: e,
        })?;
        std::fs::rename(&temp, &path).map_err(|e| CacheError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Ok(())
    }

    pub fn contains(&self, key: &Blake3Hash) -> bool {
        self.reads && self.path_for(key).exists()
    }

    /// How many entries there are and how much they take on disk.
    pub fn size(&self) -> CacheSize {
        let mut size = CacheSize::default();
        let Ok(shards) = std::fs::read_dir(&self.root) else {
            return size;
        };
        for shard in shards.flatten() {
            let Ok(entries) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata()
                    && meta.is_file()
                {
                    size.entries += 1;
                    size.bytes += meta.len();
                }
            }
        }
        size
    }

    /// Drop a stage's entry and every stage after it.
    ///
    /// Stages form a chain, so re-running shot detection invalidates the
    /// keyframes drawn from those shots, the scores drawn from those
    /// keyframes, and so on. Returns how many entries were removed.
    pub fn invalidate_from(&self, source: &Blake3Hash, stage: Stage, params: &Blake3Hash) -> usize {
        let mut removed = 0;
        for downstream in stage.downstream() {
            let key = Self::key(source, downstream, params);
            if std::fs::remove_file(self.path_for(&key)).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Delete everything. Used when a project is reset.
    pub fn clear(&self) -> Result<(), CacheError> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).map_err(|e| CacheError::Io {
                path: self.root.display().to_string(),
                source: e,
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache(name: &str) -> Cache {
        let dir = std::env::temp_dir().join(format!("vb-cache-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        Cache::new(dir)
    }

    #[test]
    fn what_goes_in_comes_back_out() {
        let cache = temp_cache("roundtrip");
        let key = Blake3Hash::of_str("k");
        assert!(cache.get(&key).is_none());
        cache.put(&key, "{\"a\":1}").unwrap();
        assert_eq!(cache.get(&key).as_deref(), Some("{\"a\":1}"));
        assert!(cache.contains(&key));
    }

    #[test]
    fn the_same_inputs_produce_the_same_key() {
        let src = Blake3Hash::of_str("source");
        let params = Blake3Hash::of_str("params");
        assert_eq!(
            Cache::key(&src, Stage::ShotDetect, &params),
            Cache::key(&src, Stage::ShotDetect, &params)
        );
    }

    #[test]
    fn changing_the_parameters_misses_the_cache() {
        let src = Blake3Hash::of_str("source");
        assert_ne!(
            Cache::key(&src, Stage::ShotDetect, &Blake3Hash::of_str("t=0.3")),
            Cache::key(&src, Stage::ShotDetect, &Blake3Hash::of_str("t=0.4"))
        );
    }

    #[test]
    fn editing_the_source_misses_the_cache() {
        // Content addressing is what makes this work: same name, new bytes,
        // different key.
        let params = Blake3Hash::of_str("p");
        assert_ne!(
            Cache::key(&Blake3Hash::of_str("v1"), Stage::Probe, &params),
            Cache::key(&Blake3Hash::of_str("v2"), Stage::Probe, &params)
        );
    }

    #[test]
    fn different_stages_never_collide() {
        let src = Blake3Hash::of_str("s");
        let params = Blake3Hash::of_str("p");
        let mut keys: Vec<_> = Stage::ALL
            .iter()
            .map(|s| Cache::key(&src, *s, &params).to_hex())
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "two stages share a cache key");
    }

    #[test]
    fn invalidating_a_stage_also_clears_everything_downstream() {
        let cache = temp_cache("invalidate");
        let src = Blake3Hash::of_str("s");
        let params = Blake3Hash::of_str("p");
        for stage in Stage::ALL {
            cache.put(&Cache::key(&src, stage, &params), "{}").unwrap();
        }

        let removed = cache.invalidate_from(&src, Stage::Keyframes, &params);
        assert_eq!(
            removed, 10,
            "keyframes itself plus the nine stages after it"
        );

        // Earlier stages survive: re-running keyframe extraction does not
        // require probing the file again.
        assert!(cache.contains(&Cache::key(&src, Stage::Probe, &params)));
        assert!(cache.contains(&Cache::key(&src, Stage::ShotDetect, &params)));
        assert!(!cache.contains(&Cache::key(&src, Stage::Keyframes, &params)));
        // The director reads the keyframes' moments, so it goes with them.
        assert!(!cache.contains(&Cache::key(&src, Stage::Director, &params)));
        assert!(!cache.contains(&Cache::key(&src, Stage::MinePriors, &params)));
    }

    #[test]
    fn an_interrupted_write_leaves_no_readable_entry() {
        // put() writes to a temp file and renames, so a partial write cannot
        // be mistaken for a complete cache entry.
        let cache = temp_cache("atomic");
        let key = Blake3Hash::of_str("k");
        cache.put(&key, "complete").unwrap();
        let stray = cache.root().join("aa").join("partial.json.tmp");
        std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
        std::fs::write(&stray, "half").unwrap();
        assert_eq!(cache.get(&key).as_deref(), Some("complete"));
    }

    #[test]
    fn a_fresh_run_reads_nothing_back_but_still_writes() {
        let cache = temp_cache("write-only");
        let key = Blake3Hash::of_bytes(b"k");
        cache.put(&key, "old").unwrap();
        let fresh = cache.clone().write_only();
        assert_eq!(fresh.get(&key), None);
        assert!(!fresh.contains(&key));
        fresh.put(&key, "new").unwrap();
        assert_eq!(cache.get(&key).as_deref(), Some("new"));
        assert_eq!(cache.size().entries, 1);
        assert_eq!(cache.size().bytes, 3);
    }

    #[test]
    fn clearing_removes_the_whole_store() {
        let cache = temp_cache("clear");
        cache.put(&Blake3Hash::of_str("k"), "{}").unwrap();
        cache.clear().unwrap();
        assert!(!cache.contains(&Blake3Hash::of_str("k")));
        // Clearing an absent cache is not an error.
        cache.clear().unwrap();
    }
}
