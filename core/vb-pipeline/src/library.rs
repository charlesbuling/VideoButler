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

//! The library: what the app keeps between projects.
//!
//! One directory holds the analysis cache, the stills the cache points at,
//! the downloaded models, the projects and the settings. Everything in it is
//! shared, which is the point:
//!
//! - a model downloaded once is a model every later project can use;
//! - a clip analysed for one project is not analysed again for the next;
//! - a project can be reopened after the app is closed.
//!
//! The location is the platform's own place for application data, so the
//! library survives moving the footage, and a user who wants it elsewhere
//! sets `VB_LIBRARY_DIR`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the app keeps its shared state.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    /// The library this machine uses.
    pub fn open() -> Library {
        Library::at(default_root())
    }

    pub fn at(root: impl Into<PathBuf>) -> Library {
        Library { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Analysis, keyed by content hash, shared by every project.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Stills the cached analyses point at. Shared with the cache and for the
    /// same reason: a cached graph names its keyframes by a path relative to
    /// here, so the two directories move together or not at all.
    pub fn work_dir(&self) -> PathBuf {
        self.root.join("work")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Downloaded model weights. One copy, every project.
    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Create what is missing. Called before anything is written rather than
    /// at startup, so merely asking where the library is does not create one.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.root.clone(),
            self.cache_dir(),
            self.work_dir(),
            self.projects_dir(),
            self.models_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Is the library usable? A read-only or full disk is worth saying
    /// plainly at start-up rather than at the end of a long analysis.
    pub fn writable(&self) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        let probe = self.root.join(".writable");
        std::fs::write(&probe, b"ok").map_err(|e| format!("{}: {e}", self.root.display()))?;
        let _ = std::fs::remove_file(&probe);
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        std::fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save settings, readable only by this user.
    ///
    /// The file holds an API key when the user has set one, and a key in a
    /// world-readable file is a key leaked to every process on the machine.
    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        let path = self.settings_path();
        let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        restrict_to_owner(&path);
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) {}

/// Where the library lives when the host has not said otherwise.
pub fn default_root() -> PathBuf {
    if let Some(named) = std::env::var_os("VB_LIBRARY_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(named);
    }
    // Android and iOS hand an app a directory it owns and forbid it the rest
    // of the filesystem, so the host passes that in rather than this guessing.
    if let Some(work) = std::env::var_os("VB_WORKDIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(work).join("library");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if cfg!(target_vendor = "apple") {
        home.join("Library/Application Support/VideoButler")
    } else if cfg!(target_os = "windows") {
        home.join("AppData/Roaming/VideoButler")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("video-butler")
    }
}

/// Settings that outlive a project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub cloud: CloudSettings,
    /// Editors to write a draft for, by id, unless a project says otherwise.
    pub default_editors: Vec<String>,
    /// The language the app shows, `zh` or `en`. Empty until the user
    /// chooses; the app starts in Chinese.
    pub language: String,
    /// How an editor draft points at footage the editor may not read where
    /// it is: `link` (a hard link in the draft, no space used) or
    /// `reference` (the original path). Empty means `link`.
    pub draft_media: String,
    /// Slow a run down when the device gets hot. Absent means on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermal_guard: Option<bool>,
    /// The model the user chose for a role, by role id (`speech`,
    /// `describe`), in place of the most capable one installed. A role not
    /// named here uses that one.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, String>,
    /// The video butler: whether it keeps the footage, which folders, and
    /// the categories the user removed.
    #[serde(skip_serializing_if = "crate::hosting::HostingSettings::is_default")]
    pub hosting: crate::hosting::HostingSettings,
}

impl Settings {
    pub fn thermal_guard(&self) -> bool {
        self.thermal_guard.unwrap_or(true)
    }

    pub fn draft_media(&self) -> vb_export::DraftMedia {
        vb_export::DraftMedia::parse(&self.draft_media).unwrap_or_default()
    }

    /// The language engine sentences are written in for the app.
    pub fn lang(&self) -> crate::i18n::Lang {
        if self.language.is_empty() {
            crate::i18n::Lang::Zh
        } else {
            crate::i18n::Lang::parse(Some(&self.language))
        }
    }
}

/// An OpenAI-compatible endpoint, for the cloud model choice.
///
/// Compatible rather than a named provider on purpose: the same shape reaches
/// a hosted API and a model server running on this machine, so "local model"
/// and "cloud model" differ in where the user points it, not in what the
/// engine supports.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudSettings {
    /// e.g. `https://api.example.com/v1`, or `http://localhost:11434/v1`.
    pub base_url: String,
    /// The model that reasons about the cut.
    pub model: String,
    /// The model that looks at stills. Often the same one.
    pub vision_model: String,
    pub api_key: String,
}

impl CloudSettings {
    pub fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }

    /// The settings as a front end may see them: never the key itself, only
    /// whether one is set.
    pub fn redacted(&self) -> serde_json::Value {
        serde_json::json!({
            "baseUrl": self.base_url,
            "model": self.model,
            "visionModel": self.vision_model,
            "apiKeySet": !self.api_key.trim().is_empty(),
            "configured": self.is_configured(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Library {
        let root = std::env::temp_dir().join(format!("vb-library-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        Library::at(root)
    }

    #[test]
    fn a_library_creates_the_directories_it_needs() {
        let library = scratch("ensure");
        library.ensure().unwrap();
        for dir in [
            library.cache_dir(),
            library.work_dir(),
            library.projects_dir(),
            library.models_dir(),
        ] {
            assert!(dir.is_dir(), "{} was not created", dir.display());
        }
        assert!(library.writable().is_ok());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn asking_where_the_library_is_does_not_create_one() {
        let library = scratch("no-touch");
        let _ = library.root();
        let _ = library.models_dir();
        assert!(!library.root().exists());
    }

    #[test]
    fn settings_round_trip_and_the_key_stays_out_of_what_is_shown() {
        let library = scratch("settings");
        let mut settings = Settings::default();
        settings.cloud.base_url = "https://api.example.com/v1".into();
        settings.cloud.model = "some-model".into();
        settings.cloud.api_key = "sk-secret".into();
        library.save_settings(&settings).unwrap();

        let read = library.settings();
        assert_eq!(read.cloud.api_key, "sk-secret");
        assert!(read.cloud.is_configured());

        let shown = read.cloud.redacted().to_string();
        assert!(
            !shown.contains("sk-secret"),
            "the key must never be sent out: {shown}"
        );
        assert!(shown.contains("\"apiKeySet\":true"));
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    #[cfg(unix)]
    fn the_settings_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let library = scratch("perms");
        library.save_settings(&Settings::default()).unwrap();
        let mode = std::fs::metadata(library.settings_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "an API key lives here");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn the_default_location_is_the_platforms_own_place_for_app_data() {
        let root = default_root();
        assert!(root.is_absolute() || root.starts_with("."));
        if cfg!(target_vendor = "apple") && std::env::var_os("VB_LIBRARY_DIR").is_none() {
            assert!(
                root.to_string_lossy().contains("Application Support"),
                "got {}",
                root.display()
            );
        }
    }
}
