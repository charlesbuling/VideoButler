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

//! Projects: what the user chose, kept so they can come back to it.
//!
//! A project is the footage, the style and the model choice, plus wherever
//! its outputs went. It is a small JSON file in the library, not a database:
//! the app has tens of these, a user may want to read or copy one, and a file
//! per project means a corrupt one costs that project rather than all of them.
//!
//! What a project does *not* hold is the analysis. That is content-addressed
//! in the library's cache, so two projects over the same holiday footage
//! analyse it once, and deleting a project never throws away work another one
//! is using.

use crate::library::Library;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vb_narrative::{Mood, Style, Topic};

/// Which models a run should use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelChoice {
    /// Everything on this device. Always available, needs no network.
    #[default]
    Local,
    /// An OpenAI-compatible endpoint the user configured, for description and
    /// for the cut itself. Falls back to local when it cannot be reached: the
    /// offline path is the guarantee, and the cloud is an accelerator.
    Cloud,
}

/// The style a project was set to, as the user chose it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleChoice {
    pub mood: Option<Mood>,
    pub topic: Option<Topic>,
    /// Cut on the beat.
    pub beat_sync: Option<bool>,
    /// Music to cut to and lay under the result.
    pub music: Option<String>,
    /// How long the finished piece should be. `None` lets the style decide.
    pub target_duration_us: Option<u64>,
    /// What to do when the cut runs longer than the music: play it again
    /// (`true`) or let the end fall silent (`false`). `None` until the user
    /// has been asked, which the app does before a run that needs it.
    pub music_loop: Option<bool>,
    /// How a cut that can be ordered two ways is: `time`, as filmed, or
    /// `category`, people then scenery then food then the rest, each as
    /// filmed. `None` is the style's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
}

impl StyleChoice {
    pub fn style(&self) -> Style {
        Style {
            mood: self.mood,
            topic: self.topic,
            beat_sync: self.beat_sync,
        }
    }
}

/// What the last run of a project came to, so reopening it shows the result
/// without re-running anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub at: String,
    pub draft_path: String,
    pub duration_us: u64,
    pub clips_used: usize,
    pub template_id: String,
    #[serde(default)]
    pub editor_drafts: Vec<String>,
    /// The cut and the analysis it was made from, which the built-in
    /// editor opens. Absent for a run before 0.2.0, whose files are found
    /// beside its draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsl_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_path: Option<String>,
}

/// Something the built-in editor wrote: a video, or a draft for another
/// editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecord {
    pub at: String,
    /// `video`, `jianying`, `capcut` or `otio`.
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    /// What the user picked: files, folders, or both. Folders are expanded at
    /// run time rather than here, so a folder that gains a clip is a project
    /// that gains a clip.
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub style: StyleChoice,
    #[serde(default)]
    pub model: ModelChoice,
    /// Editors to write a native draft for, by id.
    #[serde(default)]
    pub editors: Vec<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub last_run: Option<LastRun>,
    /// The memory the video butler made this project for, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// When a person last changed the cut in the built-in editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
    /// What the built-in editor has written, newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<ExportRecord>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Project {
        let now = vb_contracts::now_rfc3339();
        Project {
            id: new_id(&now),
            name: name.into(),
            inputs: Vec::new(),
            style: StyleChoice::default(),
            model: ModelChoice::default(),
            editors: Vec::new(),
            template_id: None,
            created_at: now.clone(),
            updated_at: now,
            last_run: None,
            memory: None,
            edited_at: None,
            exports: Vec::new(),
        }
    }

    /// Where this project's outputs go.
    pub fn dir(&self, library: &Library) -> PathBuf {
        library.projects_dir().join(&self.id)
    }

    pub fn workspace(&self, library: &Library) -> crate::Workspace {
        crate::Workspace::shared(self.dir(library), library.root())
    }

    pub fn save(&self, library: &Library) -> Result<(), String> {
        let dir = self.dir(library);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let path = dir.join("project.json");
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// An id that sorts by creation time and cannot collide within a run.
///
/// Time alone collides when two projects are made in the same second, which
/// is easy to do by accident; the counter closes that.
fn new_id(now: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let stamp: String = now.chars().filter(|c| c.is_ascii_digit()).collect();
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("p{stamp}-{:04x}{:04x}", seq & 0xffff, unique & 0xffff)
}

/// Every project, newest first.
pub fn list(library: &Library) -> Vec<Project> {
    let Ok(entries) = std::fs::read_dir(library.projects_dir()) else {
        return Vec::new();
    };
    let mut projects: Vec<Project> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path().join("project.json");
            let text = std::fs::read_to_string(&path).ok()?;
            match serde_json::from_str::<Project>(&text) {
                Ok(project) => Some(project),
                Err(e) => {
                    // One unreadable project must not hide the others.
                    tracing::warn!(path = %path.display(), error = %e, "skipping an unreadable project");
                    None
                }
            }
        })
        .collect();
    projects.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    projects
}

pub fn get(library: &Library, id: &str) -> Option<Project> {
    // The id names a directory, so anything with a separator in it is either
    // a bug or an attempt to read elsewhere on disk.
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return None;
    }
    let path = library.projects_dir().join(id).join("project.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Delete a project and its outputs.
///
/// Its footage is untouched -- that is the user's and lives elsewhere -- and
/// so is the analysis cache, which other projects share.
pub fn delete(library: &Library, id: &str) -> Result<(), String> {
    let Some(project) = get(library, id) else {
        return Err(format!("there is no project {id}"));
    };
    let dir = project.dir(library);
    std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Library {
        let root = std::env::temp_dir().join(format!("vb-projects-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let library = Library::at(root);
        library.ensure().unwrap();
        library
    }

    #[test]
    fn a_project_survives_being_written_and_read_back() {
        let library = scratch("roundtrip");
        let mut project = Project::new("Kyoto in spring");
        project.inputs = vec!["/movies/kyoto".into()];
        project.style = StyleChoice {
            mood: Some(Mood::Happy),
            topic: Some(Topic::Travel),
            beat_sync: Some(true),
            music: Some("/music/track.mp3".into()),
            target_duration_us: Some(60_000_000),
            music_loop: Some(true),
            order: Some("time".into()),
        };
        project.model = ModelChoice::Cloud;
        project.editors = vec!["jianying".into()];
        project.save(&library).unwrap();

        let read = get(&library, &project.id).expect("the project is there");
        assert_eq!(read.name, "Kyoto in spring");
        assert_eq!(read.style.mood, Some(Mood::Happy));
        assert_eq!(read.style.beat_sync, Some(true));
        assert_eq!(read.style.target_duration_us, Some(60_000_000));
        assert_eq!(read.style.music_loop, Some(true));
        assert_eq!(read.model, ModelChoice::Cloud);
        assert_eq!(read.editors, vec!["jianying".to_string()]);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn projects_list_newest_first_and_ids_do_not_collide() {
        let library = scratch("list");
        let mut ids = Vec::new();
        for i in 0..5 {
            let mut project = Project::new(format!("project {i}"));
            // Written in order, so the newest is unambiguous whatever the
            // clock does within one second.
            project.updated_at = format!("2026-09-2{i}T00:00:00Z");
            project.save(&library).unwrap();
            ids.push(project.id);
        }
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "ids collided: {ids:?}");

        let listed = list(&library);
        assert_eq!(listed.len(), 5);
        assert_eq!(listed[0].name, "project 4");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn an_unreadable_project_does_not_hide_the_others() {
        let library = scratch("broken");
        Project::new("good").save(&library).unwrap();
        let broken = library.projects_dir().join("p-broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("project.json"), b"{ not json").unwrap();

        let listed = list(&library);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "good");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn deleting_a_project_leaves_the_shared_cache_alone() {
        // Another project may be relying on that analysis, and the footage
        // itself is the user's.
        let library = scratch("delete");
        let project = Project::new("one");
        project.save(&library).unwrap();
        std::fs::write(library.cache_dir().join("entry.json"), b"{}").unwrap();

        delete(&library, &project.id).unwrap();
        assert!(get(&library, &project.id).is_none());
        assert!(library.cache_dir().join("entry.json").exists());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn an_id_cannot_point_outside_the_projects_directory() {
        let library = scratch("traversal");
        assert!(get(&library, "../../etc").is_none());
        assert!(get(&library, "").is_none());
        assert!(delete(&library, "../../etc").is_err());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn a_projects_analysis_is_shared_and_its_outputs_are_not() {
        let library = scratch("workspace");
        let project = Project::new("one");
        let workspace = project.workspace(&library);
        assert_eq!(workspace.cache_dir, library.cache_dir());
        assert_eq!(workspace.work_dir, library.work_dir());
        assert!(workspace.project_dir.ends_with(&project.id));
        let _ = std::fs::remove_dir_all(library.root());
    }
}
