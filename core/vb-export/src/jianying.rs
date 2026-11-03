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

//! Draft export for 剪映专业版 and CapCut, which share one draft format.
//!
//! This is the export the product is built around: the engine decides *how to
//! cut*, and the editor the user already has does the work of cutting. So the
//! draft references the original footage with in and out points -- never a
//! pre-cut copy -- and every decision stays a thing the user can drag, trim
//! or delete in the editor.
//!
//! # What this writer will not do
//!
//! The draft format is undocumented and moves between releases, so everything
//! here is either copied from a draft the editor itself wrote or left out.
//! Transitions are the clearest case: each one is a resource in the editor's
//! own library, keyed by an id that cannot be guessed. The dissolve below was
//! read out of a real draft; for the others this writer emits a hard cut and
//! reports it, because a draft that names a resource the editor does not have
//! is a draft that opens broken.
//!
//! # Where drafts go
//!
//! Into the editor's real draft folder, found from `root_meta_info.json`.
//! A draft written anywhere else is invisible to the editor, and one written
//! while the editor is running appears only after it restarts -- the editor
//! reads that folder at startup.

use crate::{DraftWriter, ExportError};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use vb_contracts::editing_dsl::{AudioKind, SlotKind, TextRole, Transform, TransitionType};
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros};

/// Which editor a draft is for. The format is the same; the draft folder and
/// the name the user knows it by are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    /// 剪映专业版, the mainland Chinese release.
    Jianying,
    /// CapCut, the international release.
    CapCut,
}

impl Editor {
    pub const ALL: [Editor; 2] = [Editor::Jianying, Editor::CapCut];

    /// The name to show a user, in the language they see in the editor.
    pub fn display_name(self) -> &'static str {
        match self {
            Editor::Jianying => "剪映专业版",
            Editor::CapCut => "CapCut",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Editor::Jianying => "jianying",
            Editor::CapCut => "capcut",
        }
    }

    /// The name of the phone app, which is not the desktop product's: on a
    /// phone 剪映 is just 剪映.
    pub fn phone_name(self) -> &'static str {
        match self {
            Editor::Jianying => "剪映",
            Editor::CapCut => "CapCut",
        }
    }

    /// The Android package, which is how the host finds the phone app
    /// installed. Read from the app stores' own listings.
    pub fn android_package(self) -> &'static str {
        match self {
            Editor::Jianying => "com.lemon.lv",
            Editor::CapCut => "com.lemon.lvoverseas",
        }
    }

    /// Where the editor keeps its drafts, per platform.
    ///
    /// The folder is the same on both releases apart from the product
    /// directory; `com.lveditor.draft` is the editor's own name for it.
    fn draft_roots(self, home: &Path) -> Vec<PathBuf> {
        let product = match self {
            Editor::Jianying => "JianyingPro",
            Editor::CapCut => "CapCut",
        };
        if cfg!(target_os = "macos") {
            vec![
                home.join("Movies")
                    .join(product)
                    .join("User Data/Projects/com.lveditor.draft"),
            ]
        } else if cfg!(target_os = "windows") {
            // %LOCALAPPDATA%, which is where the Windows build installs.
            vec![
                home.join("AppData/Local")
                    .join(product)
                    .join("User Data/Projects/com.lveditor.draft"),
            ]
        } else {
            Vec::new()
        }
    }

    /// Where the application itself lives, for reporting whether it is
    /// installed and for opening it.
    fn app_paths(self) -> Vec<PathBuf> {
        if !cfg!(target_os = "macos") {
            return Vec::new();
        }
        match self {
            // The macOS bundle is named after the internal product, not after
            // what the user sees in the Dock.
            Editor::Jianying => vec![
                PathBuf::from("/Applications/VideoFusion-macOS.app"),
                PathBuf::from("/Applications/JianyingPro.app"),
            ],
            Editor::CapCut => vec![PathBuf::from("/Applications/CapCut.app")],
        }
    }
}

impl std::fmt::Display for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// What was found of an editor on this machine.
#[derive(Debug, Clone)]
pub struct EditorInstall {
    pub editor: Editor,
    pub app_path: Option<PathBuf>,
    pub version: Option<String>,
    /// The draft folder the editor actually reads. Absent when the editor has
    /// never run, since it creates the folder on first launch.
    pub draft_root: Option<PathBuf>,
    /// Whether the editor is running now. A draft written while it is appears
    /// only after it restarts, and a user who is not told that reads it as
    /// the export having failed.
    pub running: bool,
    /// Looked for as a phone app. A phone editor keeps its drafts in its own
    /// app storage, which Android lets no other app write to, so it is
    /// reported installed but never takes a draft from here.
    pub phone: bool,
}

impl EditorInstall {
    pub fn is_installed(&self) -> bool {
        self.app_path.is_some()
    }

    /// Can a draft be written for this editor right now?
    pub fn can_export(&self) -> bool {
        self.draft_root.is_some()
    }
}

/// Look for both editors.
pub fn detect_editors() -> Vec<EditorInstall> {
    // An iOS app sees another app only through URL schemes it declares in
    // advance, and the editors' are not documented; and a cut reaches a
    // phone editor as shared clips, which the iOS host does not do yet. So
    // none is listed, rather than both reported missing when they may not be.
    if cfg!(target_os = "ios") {
        return Vec::new();
    }
    if cfg!(target_os = "android") {
        let apps = phone_apps();
        return Editor::ALL
            .into_iter()
            .map(|editor| on_phone(editor, &apps))
            .collect();
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    Editor::ALL
        .into_iter()
        .map(|editor| detect_editor(editor, &home))
        .collect()
}

pub fn detect_editor(editor: Editor, home: &Path) -> EditorInstall {
    let app_path = editor.app_paths().into_iter().find(|p| p.exists());
    let draft_root = editor
        .draft_roots(home)
        .into_iter()
        .find(|root| root.is_dir());
    let version = app_path.as_deref().and_then(app_version);
    let running = app_path.as_deref().map(is_running).unwrap_or(false);

    EditorInstall {
        editor,
        app_path,
        version,
        draft_root,
        running,
        phone: false,
    }
}

/// The file the Android host lists installed apps in, in the directory it
/// names with `VB_WORKDIR`: one `package=version` a line. The engine cannot
/// ask Android's package manager itself, and the host rewrites the file
/// whenever the app comes to the front, so an editor installed meanwhile is
/// seen on return. A file, not an environment variable, because changing
/// the environment while engine threads read it is not safe.
pub const INSTALLED_APPS: &str = "installed-apps";

fn phone_apps() -> String {
    std::env::var_os("VB_WORKDIR")
        .map(|dir| PathBuf::from(dir).join(INSTALLED_APPS))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

/// An editor on a phone, from the host's list of installed apps.
pub fn on_phone(editor: Editor, installed_apps: &str) -> EditorInstall {
    let found = installed_apps
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(package, _)| package.trim() == editor.android_package());
    EditorInstall {
        editor,
        app_path: found.map(|(package, _)| PathBuf::from(package.trim())),
        version: found
            .map(|(_, version)| version.trim().to_string())
            .filter(|v| !v.is_empty()),
        draft_root: None,
        running: false,
        phone: true,
    }
}

/// The version out of an app bundle's Info.plist.
///
/// Through `plutil`, because the plist may be binary, and this only ever runs
/// on a desktop where spawning a process is fine.
fn app_version(app: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("/usr/bin/plutil")
        .arg("-extract")
        .arg("CFBundleShortVersionString")
        .arg("raw")
        .arg("-o")
        .arg("-")
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !version.is_empty()).then_some(version)
}

fn is_running(app: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    std::process::Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(app.join("Contents/MacOS").to_string_lossy().to_string())
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

/// Where the editor is allowed to read media from.
///
/// 剪映 and CapCut on macOS are sandboxed App Store apps. Their entitlements
/// grant `~/Movies` and files the user picked in the editor itself, and
/// nothing else, so a draft that points at footage on the Desktop, in
/// Downloads or on an external drive opens with every clip marked "no access
/// permission" (暂无访问权限) -- the paths are right and the editor may not
/// read them. The Windows builds are not sandboxed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaAccess {
    Anywhere,
    Within(Vec<PathBuf>),
}

impl MediaAccess {
    fn allows(&self, path: &Path) -> bool {
        match self {
            MediaAccess::Anywhere => true,
            MediaAccess::Within(roots) => {
                // Compared canonically: /tmp is /private/tmp, and a path
                // through a symlink is where the symlink points, which is
                // what the sandbox checks.
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                roots.iter().any(|root| {
                    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
                    path.starts_with(root)
                })
            }
        }
    }
}

impl Editor {
    /// What this editor may read on this platform, for a user whose home is
    /// `home`.
    pub fn media_access(self, home: &Path) -> MediaAccess {
        if cfg!(target_os = "macos") {
            MediaAccess::Within(vec![home.join("Movies")])
        } else {
            MediaAccess::Anywhere
        }
    }
}

/// How a draft points at footage the editor may not read where it is.
///
/// Nothing is ever copied: the user's footage is theirs, and a draft that
/// quietly doubled a 60 GB trip would fill the disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DraftMedia {
    /// A hard link inside the draft's `Resources/`: the same file under a
    /// second name, which costs no space and which the sandboxed editor may
    /// read. Footage on another disk cannot be linked and is referenced where
    /// it is.
    #[default]
    Link,
    /// Every clip referenced at its original path, nothing in the draft. The
    /// sandboxed editor marks footage outside `~/Movies` as not readable
    /// until the user relinks it there.
    Reference,
}

impl DraftMedia {
    pub fn id(self) -> &'static str {
        match self {
            DraftMedia::Link => "link",
            DraftMedia::Reference => "reference",
        }
    }

    pub fn parse(id: &str) -> Option<DraftMedia> {
        match id {
            "link" => Some(DraftMedia::Link),
            "reference" => Some(DraftMedia::Reference),
            _ => None,
        }
    }
}

/// Writes a draft folder the editor can open.
#[derive(Debug, Clone)]
pub struct JianyingWriter {
    pub editor: Editor,
    /// Media outside this is placed inside the draft, where the editor can
    /// read it, unless `media` says to reference it where it is.
    pub media_access: MediaAccess,
    pub media: DraftMedia,
}

impl JianyingWriter {
    /// A writer for `editor` as it is installed for this user.
    pub fn for_editor(editor: Editor) -> JianyingWriter {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        JianyingWriter {
            editor,
            media_access: editor.media_access(&home),
            media: DraftMedia::default(),
        }
    }
}

impl Default for JianyingWriter {
    fn default() -> Self {
        JianyingWriter::for_editor(Editor::Jianying)
    }
}

/// How media the editor could not have read was made readable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaPlacement {
    /// Hard-linked into the draft: same disk, so no copy and no extra space.
    pub linked: usize,
    /// Referenced where it is although the editor may not read it there:
    /// because the user asked for references, or because it is on another
    /// disk and a link cannot cross disks. The editor asks to relink these.
    pub unreadable: usize,
}

/// The editor's own dissolve, read out of a draft it wrote. Guessing these
/// ids is not possible and inventing them produces a draft that opens with a
/// missing resource, so this is the only transition written.
const DISSOLVE_EFFECT_ID: &str = "322577";
const DISSOLVE_RESOURCE_ID: &str = "6724845717472416269";
const DISSOLVE_NAME: &str = "叠化";

/// What a written draft came to, for telling the user.
#[derive(Debug, Clone)]
pub struct DraftSummary {
    pub editor: Editor,
    pub path: PathBuf,
    /// Media placed inside the draft so the editor may read it.
    pub media: MediaPlacement,
    pub name: String,
    pub clips: usize,
    pub texts: usize,
    pub has_music: bool,
    /// Transitions the DSL asked for that this writer had to emit as hard
    /// cuts, because the editor keys them by a resource id that cannot be
    /// guessed.
    pub transitions_dropped: usize,
}

impl DraftWriter for JianyingWriter {
    fn name(&self) -> &'static str {
        match self.editor {
            Editor::Jianying => "JianyingDraft",
            Editor::CapCut => "CapCutDraft",
        }
    }

    /// A draft is a folder, so there is no extension. The caller names the
    /// folder; `dest` is that folder, not a file inside it.
    fn extension(&self) -> &'static str {
        ""
    }

    fn write(
        &self,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        dest: &Path,
    ) -> Result<PathBuf, ExportError> {
        self.write_draft(dsl, graph, dest).map(|s| s.path)
    }
}

impl JianyingWriter {
    /// Write the draft folder, and say what went into it.
    pub fn write_draft(
        &self,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        dest: &Path,
    ) -> Result<DraftSummary, ExportError> {
        std::fs::create_dir_all(dest).map_err(|e| ExportError::Io {
            path: dest.display().to_string(),
            source: e,
        })?;

        // Only what the cut uses: a draft of twelve clips from a folder of a
        // hundred has no business linking the other eighty-eight.
        let used: std::collections::HashSet<&str> = dsl
            .slots
            .iter()
            .filter_map(|s| s.source_ref.as_ref().map(|r| r.source_id.as_str()))
            .collect();
        let mut paths: Vec<&str> = graph
            .sources
            .iter()
            .filter(|s| used.contains(s.id.as_str()))
            .map(|s| s.path.as_str())
            .collect();
        paths.extend(
            dsl.tracks
                .audio
                .iter()
                .filter_map(|a| a.asset_path.as_deref()),
        );
        let (relocated, media) = self.place_media(&paths, dest)?;

        let mut draft = self.build(dsl, graph, dest, &relocated)?;
        draft.summary.media = media;

        let content =
            serde_json::to_string(&draft.content).map_err(|source| ExportError::Serialize {
                format: "CapCut draft",
                source,
            })?;
        // Both names, with the same bytes. The editor has read
        // `draft_content.json` for years and `draft_info.json` is what its
        // own index points at; which one a given release opens is not
        // something to find out from a user's bug report.
        for name in ["draft_content.json", "draft_info.json"] {
            write_file(&dest.join(name), &content)?;
        }
        let meta = serde_json::to_string(&draft.meta).map_err(|source| ExportError::Serialize {
            format: "CapCut draft",
            source,
        })?;
        write_file(&dest.join("draft_meta_info.json"), &meta)?;

        Ok(draft.summary)
    }

    /// Put every file the editor may not read where it may: inside the
    /// draft, as a hard link, which costs nothing and is not a copy.
    ///
    /// This is not pre-cutting. The whole original is linked, untouched, and
    /// the draft still cuts it by in and out points. It only changes which
    /// path the editor is handed. A file that cannot be linked -- another
    /// disk -- or a user who asked for plain references keeps its own path,
    /// and is counted so the run can say the editor will ask to relink it.
    fn place_media(
        &self,
        paths: &[&str],
        dest: &Path,
    ) -> Result<(std::collections::HashMap<String, String>, MediaPlacement), ExportError> {
        let mut relocated = std::collections::HashMap::new();
        let mut placement = MediaPlacement::default();
        let mut seen = std::collections::HashSet::new();
        let resources = dest.join("Resources");

        for path in paths {
            if !seen.insert(*path) || self.media_access.allows(Path::new(path)) {
                continue;
            }
            if self.media == DraftMedia::Reference {
                placement.unreadable += 1;
                continue;
            }
            let source = Path::new(path);
            let file_name = source
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "media".into());
            std::fs::create_dir_all(&resources).map_err(|e| ExportError::Io {
                path: resources.display().to_string(),
                source: e,
            })?;
            // Numbered, so two clips both called IMG_0001.MOV from two cards
            // do not land on each other.
            let target = resources.join(format!("{:03}-{file_name}", relocated.len() + 1));

            if std::fs::hard_link(source, &target).is_ok() {
                placement.linked += 1;
                relocated.insert(path.to_string(), target.to_string_lossy().to_string());
            } else {
                placement.unreadable += 1;
            }
        }
        Ok((relocated, placement))
    }

    fn build(
        &self,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        dest: &Path,
        relocated: &std::collections::HashMap<String, String>,
    ) -> Result<BuiltDraft, ExportError> {
        let media_path = |path: &str| -> String {
            relocated
                .get(path)
                .cloned()
                .unwrap_or_else(|| path.to_string())
        };
        let name = dest
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "vb-draft".to_string());

        // The editor lays clips end to end and overlaps nothing, so the
        // timeline is resolved with transitions at zero rather than
        // recomputed here. Absolute positions come from one place in this
        // codebase, and this is how a second place is avoided.
        let mut flat = dsl.clone();
        for transition in &mut flat.transitions {
            transition.duration_us = Micros::ZERO;
        }
        let timeline = flat.resolve_timeline();

        let ordered = dsl.ordered_slots();
        let mut videos: Vec<Value> = Vec::new();
        let mut video_segments: Vec<Value> = Vec::new();
        let mut transitions: Vec<Value> = Vec::new();
        let mut texts: Vec<Value> = Vec::new();
        let mut text_segments: Vec<Value> = Vec::new();
        let mut audios: Vec<Value> = Vec::new();
        let mut audio_segments: Vec<Value> = Vec::new();
        let mut speeds: Vec<Value> = Vec::new();
        let mut dropped = 0usize;

        for (index, slot) in ordered.iter().enumerate() {
            let Some(placed) = timeline.slot(&slot.id) else {
                continue;
            };
            if slot.kind != SlotKind::Clip {
                // A title card is a gap on the video track with text over it.
                // The text is written below, from the DSL's own text track.
                continue;
            }
            let source_ref =
                slot.source_ref
                    .as_ref()
                    .ok_or_else(|| ExportError::DanglingSource {
                        slot_id: slot.id.clone(),
                        source_id: "<none>".into(),
                    })?;
            let source =
                graph
                    .source(&source_ref.source_id)
                    .ok_or_else(|| ExportError::DanglingSource {
                        slot_id: slot.id.clone(),
                        source_id: source_ref.source_id.clone(),
                    })?;

            let material_id = id_for(&format!("{name}/video/{}", slot.id));
            videos.push(video_material(
                &material_id,
                source,
                &media_path(&source.path),
            ));

            // A transition belongs to the clip it leads out of, which is how
            // the editor stores it.
            let mut refs: Vec<Value> = Vec::new();
            if let Some(next) = ordered.get(index + 1) {
                if let Some(t) = dsl
                    .transitions
                    .iter()
                    .find(|t| t.from_slot_id == slot.id && t.to_slot_id == next.id)
                {
                    match transition_material(
                        &id_for(&format!("{name}/xition/{}", slot.id)),
                        t.kind,
                        t.duration_us,
                    ) {
                        Some(material) => {
                            refs.push(json!(material["id"]));
                            transitions.push(material);
                        }
                        None if t.kind != TransitionType::Cut => dropped += 1,
                        None => {}
                    }
                }
            }

            // A clip played faster or slower takes a speed material of its
            // own, referenced like a transition.
            let speed = slot
                .speed
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(1.0);
            if (speed - 1.0).abs() > 1e-9 {
                let id = id_for(&format!("{name}/speed/{}", slot.id));
                speeds.push(speed_material(&id, speed));
                refs.insert(0, json!(id));
            }
            let transform = dsl
                .tracks
                .video
                .iter()
                .find(|v| v.slot_id == slot.id)
                .and_then(|v| v.transform.as_ref());
            video_segments.push(video_segment(&VideoSegment {
                id: &id_for(&format!("{name}/seg/{}", slot.id)),
                material_id: &material_id,
                source_in: source_ref.in_us,
                source_duration: source_ref.duration(),
                timeline_at: placed.start_us,
                duration: slot.duration_us,
                speed,
                transform,
                keyframe_seed: &format!("{name}/kf/{}", slot.id),
                extra_material_refs: refs,
            }));
        }

        for item in &dsl.tracks.text {
            let Some(at) = timeline.resolve_anchor(&item.anchor) else {
                continue;
            };
            let is_title = item.role == Some(TextRole::Title);
            let material_id = id_for(&format!("{name}/text/{}", item.id));
            texts.push(text_material(&material_id, &item.content, is_title));
            text_segments.push(text_segment(
                &id_for(&format!("{name}/textseg/{}", item.id)),
                &material_id,
                at,
                item.duration_us,
                is_title,
            ));
        }

        for item in dsl
            .tracks
            .audio
            .iter()
            .filter(|a| a.kind != AudioKind::Source)
        {
            let (Some(path), Some(duration)) = (item.asset_path.as_ref(), item.duration_us) else {
                continue;
            };
            let Some(at) = timeline.resolve_anchor(&item.anchor) else {
                continue;
            };
            let material_id = id_for(&format!("{name}/audio/{}", item.id));
            // The whole file's length, so the editor knows how much of the
            // track is there to drag out; the DSL only says how much was used.
            let file_duration = graph
                .priors
                .as_ref()
                .and_then(|p| p.beat_grid.as_ref())
                .filter(|g| g.asset_path.as_deref() == Some(path.as_str()))
                .and_then(|g| g.asset_duration_us)
                .unwrap_or(Micros(
                    item.asset_in_us.unwrap_or(Micros::ZERO).0 + duration.0,
                ));

            audios.push(audio_material(
                &material_id,
                &media_path(path),
                file_duration,
            ));
            audio_segments.push(audio_segment(
                &id_for(&format!("{name}/audioseg/{}", item.id)),
                &material_id,
                item.asset_in_us.unwrap_or(Micros::ZERO),
                at,
                duration,
            ));
        }

        let mut tracks = vec![track(
            "video",
            &id_for(&format!("{name}/track/video")),
            video_segments.clone(),
        )];
        if !text_segments.is_empty() {
            tracks.push(track(
                "text",
                &id_for(&format!("{name}/track/text")),
                text_segments.clone(),
            ));
        }
        if !audio_segments.is_empty() {
            tracks.push(track(
                "audio",
                &id_for(&format!("{name}/track/audio")),
                audio_segments.clone(),
            ));
        }

        // The first video's frame and rate: a photo's is its sensor's, often
        // twelve megapixels, and it has no rate at all.
        let first = graph
            .sources
            .iter()
            .find(|s| !s.is_still())
            .or(graph.sources.first());
        let (width, height) = first
            .and_then(|s| s.display_size())
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| fit_canvas(w, h))
            .unwrap_or((1920, 1080));
        let rate = first.map(|s| s.frame_rate()).unwrap_or_default();

        let draft_id = id_for(&format!("{name}/draft")).to_uppercase();
        let now_us = unix_micros();
        let folder = dest.to_string_lossy().to_string();
        let root = dest
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let content = json!({
            "canvas_config": { "width": width, "height": height, "ratio": "original", "background": null },
            "color_space": -1,
            "config": {
                "adjust_max_index": 1,
                "attachment_info": [],
                "combination_max_index": 1,
                "export_range": null,
                "extract_audio_last_index": 1,
                "lyrics_recognition_id": "",
                "lyrics_sync": true,
                "lyrics_taskinfo": [],
                // Off, because the cut may open on a title card, which is a
                // gap on the main track. With this on the editor pulls the
                // first clip back to zero and the title lands over it.
                "maintrack_adsorb": false,
                "material_save_mode": 0,
                "multi_language_current": "none",
                "multi_language_list": [],
                "original_sound_last_index": 1,
                "record_audio_last_index": 1,
                "sticker_max_index": 1,
                "subtitle_keywords_config": null,
                "subtitle_recognition_id": "",
                "subtitle_sync": true,
                "system_font_list": [],
                "video_mute": false,
                "zoom_info_params": null
            },
            "cover": null,
            "create_time": 0,
            "duration": timeline.total_duration_us.0,
            "extra_info": null,
            "fps": rate.as_f64(),
            "free_render_index_mode_on": false,
            "group_container": null,
            "id": id_for(&format!("{name}/id")).to_uppercase(),
            "is_drop_frame_timecode": false,
            "keyframe_graph_list": [],
            "keyframes": null,
            "lyrics_effects": [],
            "materials": materials(videos, texts, audios, transitions, speeds),
            "mutable_config": null,
            "name": name,
            "new_version": "120.0.0",
            "path": "",
            "relationships": [],
            "render_index_track_mode_on": false,
            "retouch_cover": null,
            "source": 0,
            "static_cover_image_path": "",
            "time_marks": null,
            "tracks": tracks,
            "update_time": 0,
            "version": 360000,
            "platform": "mac",
            "last_modified_platform": "mac"
        });

        let meta = json!({
            "cloud_draft_cover": false,
            "cloud_draft_sync": false,
            "draft_cloud_last_action_download": false,
            "draft_cloud_purchase_info": "",
            "draft_cloud_template_id": "",
            "draft_cloud_tutorial_info": "",
            "draft_cloud_videocut_purchase_info": "",
            "draft_cover": "draft_cover.jpg",
            "draft_deeplink_url": "",
            "draft_enterprise_info": {
                "draft_enterprise_extra": "",
                "draft_enterprise_id": "",
                "draft_enterprise_name": "",
                "enterprise_material": []
            },
            "draft_fold_path": folder,
            "draft_id": draft_id,
            "draft_is_ai_shorts": false,
            "draft_is_article_video_draft": false,
            "draft_is_from_deeplink": "false",
            "draft_is_invisible": false,
            "draft_materials": [],
            "draft_materials_copied_info": [],
            "draft_name": name,
            "draft_need_rename_folder": false,
            "draft_new_version": "",
            "draft_removable_storage_device": "",
            "draft_root_path": root,
            "draft_segment_extra_info": [],
            "draft_timeline_materials_size_": 0,
            "draft_type": "",
            "tm_draft_cloud_completed": "",
            "tm_draft_cloud_modified": 0,
            "tm_draft_create": now_us,
            "tm_draft_modified": now_us,
            "tm_draft_removed": 0,
            "tm_duration": timeline.total_duration_us.0
        });

        Ok(BuiltDraft {
            summary: DraftSummary {
                editor: self.editor,
                path: dest.to_path_buf(),
                media: MediaPlacement::default(),
                name: name.clone(),
                clips: video_segments.len(),
                texts: text_segments.len(),
                has_music: !audio_segments.is_empty(),
                transitions_dropped: dropped,
            },
            content,
            meta,
        })
    }
}

#[derive(Debug)]
struct BuiltDraft {
    summary: DraftSummary,
    content: Value,
    meta: Value,
}

fn write_file(path: &Path, text: &str) -> Result<(), ExportError> {
    std::fs::write(path, text).map_err(|e| ExportError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// A UUID-shaped id, derived from the draft and the thing it names.
///
/// Derived rather than random so that rewriting the same draft produces the
/// same ids, which makes a diff between two runs readable. The editor only
/// requires that ids are unique within a draft.
fn id_for(seed: &str) -> String {
    let hex = Blake3Hash::of_str(seed).to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn unix_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// The editor's material lists. Every key it expects is present, because a
/// missing one is read as a malformed draft rather than as an empty list.
///
/// The empty ones are listed by name rather than written out as JSON: there
/// are fifty of them, they carry no information, and a table is easier to
/// keep in step with a new release than fifty literal keys.
const EMPTY_MATERIAL_LISTS: [&str; 46] = [
    "ai_translates",
    "audio_balances",
    "audio_effects",
    "audio_fades",
    "audio_track_indexes",
    "beats",
    "canvases",
    "chromas",
    "color_curves",
    "digital_humans",
    "drafts",
    "effects",
    "flowers",
    "green_screens",
    "handwrites",
    "hsl",
    "images",
    "log_color_wheels",
    "loudnesses",
    "manual_deformations",
    "masks",
    "material_animations",
    "material_colors",
    "multi_language_refs",
    "placeholder_infos",
    "placeholders",
    "plugin_effects",
    "primary_color_wheels",
    "realtime_denoises",
    "shapes",
    "smart_crops",
    "smart_relights",
    "sound_channel_mappings",
    "speeds",
    "stickers",
    "tail_leaders",
    "text_templates",
    "time_marks",
    "video_effects",
    "video_trackings",
    "vocal_beautifys",
    "vocal_separations",
    "audio_pitch_shifts",
    "common_mask",
    "video_radius",
    "video_strokes",
];

fn materials(
    videos: Vec<Value>,
    texts: Vec<Value>,
    audios: Vec<Value>,
    transitions: Vec<Value>,
    speeds: Vec<Value>,
) -> Value {
    let mut map = serde_json::Map::new();
    for name in EMPTY_MATERIAL_LISTS {
        map.insert(name.to_string(), json!([]));
    }
    map.insert("videos".into(), Value::Array(videos));
    map.insert("texts".into(), Value::Array(texts));
    map.insert("audios".into(), Value::Array(audios));
    map.insert("transitions".into(), Value::Array(transitions));
    map.insert("speeds".into(), Value::Array(speeds));
    Value::Object(map)
}

/// A constant change of speed, as the editor keeps one: a material the
/// segment references, beside the segment's own `speed`.
///
/// Its shape follows the open-source pyJianYingDraft's reading of the
/// editor's drafts, not a draft read here: once 剪映 has opened a draft it
/// writes the timeline back encrypted, so none of the drafts on the
/// development machine show one.
fn speed_material(id: &str, speed: f64) -> Value {
    json!({
        "curve_speed": null,
        "id": id,
        "mode": 0,
        "speed": speed,
        "type": "speed"
    })
}

/// One of a keyframe's numbers, as the editor's keyframe track for it holds
/// it.
type KeyedValue = fn(&vb_contracts::editing_dsl::TransformKeyframe) -> f64;

/// The editor's keyframe tracks for a clip whose placement changes over it:
/// one track a property, a straight line between keys. Position is in half
/// the canvas, up positive; scale is the editor's own, 1 fitted; rotation
/// clockwise in degrees -- as the DSL keys them, converted.
///
/// The shape, like the speed material's, follows pyJianYingDraft's reading
/// of the editor's drafts rather than a draft read here.
fn keyframes(seed: &str, transform: &Transform) -> Vec<Value> {
    if transform.keyframes.len() < 2 {
        return Vec::new();
    }
    let tracks: [(&str, KeyedValue); 5] = [
        ("KFTypePositionX", |k| 2.0 * k.x + 0.0),
        ("KFTypePositionY", |k| -2.0 * k.y + 0.0),
        ("KFTypeScaleX", |k| k.scale),
        ("KFTypeScaleY", |k| k.scale),
        ("KFTypeRotation", |k| k.rotation_deg),
    ];
    tracks
        .iter()
        .filter(|(_, value)| {
            let first = value(&transform.keyframes[0]);
            transform
                .keyframes
                .iter()
                .any(|k| (value(k) - first).abs() > 1e-9)
        })
        .map(|(property, value)| {
            let list: Vec<Value> = transform
                .keyframes
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    json!({
                        "curveType": "Line",
                        "graphID": "",
                        "id": id_for(&format!("{seed}/{property}/{i}")),
                        "left_control": { "x": 0.0, "y": 0.0 },
                        "right_control": { "x": 0.0, "y": 0.0 },
                        "time_offset": k.offset_us.0,
                        "values": [value(k)]
                    })
                })
                .collect();
            json!({
                "id": id_for(&format!("{seed}/{property}")),
                "keyframe_list": list,
                "material_id": "",
                "property_type": property
            })
        })
        .collect()
}

/// A canvas no larger than 4K on its long side, the shape kept: a project
/// of photos alone takes its frame from a photo.
fn fit_canvas(w: u32, h: u32) -> (u32, u32) {
    const LONGEST: u32 = 3840;
    if w.max(h) <= LONGEST {
        return (w, h);
    }
    let scale = LONGEST as f64 / w.max(h) as f64;
    let even = |v: u32| ((v as f64 * scale / 2.0).round() as u32).max(1) * 2;
    (even(w), even(h))
}

/// How long the editor treats a photograph as running: three hours, which is
/// what it gives a still imported by hand. The segment says how much of it
/// the cut shows.
const PHOTO_DURATION_US: u64 = 10_800_000_000;

fn video_material(id: &str, source: &vb_contracts::footage_graph::Source, path: &str) -> Value {
    let name = Path::new(&source.path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    json!({
        "aigc_type": "none",
        "audio_fade": null,
        "cartoon_path": "",
        "category_id": "",
        "category_name": "",
        "check_flag": 63487,
        "crop": {
            "lower_left_x": 0, "lower_left_y": 1, "lower_right_x": 1, "lower_right_y": 1,
            "upper_left_x": 0, "upper_left_y": 0, "upper_right_x": 1, "upper_right_y": 0
        },
        "crop_ratio": "free",
        "crop_scale": 1,
        "duration": if source.is_still() { PHOTO_DURATION_US } else { source.duration_us.0 },
        "extra_type_option": 0,
        "formula_id": "",
        "freeze": null,
        "has_audio": source.has_audio.unwrap_or(false),
        "height": source.height.unwrap_or(0),
        "id": id,
        "intensifies_audio_path": "",
        "intensifies_path": "",
        "is_ai_generate_content": false,
        "is_copyright": false,
        "is_text_edit_overdub": false,
        "is_unified_beauty_mode": false,
        "local_id": "",
        "local_material_id": "",
        "material_id": "",
        "material_name": name,
        "material_url": "",
        "matting": {
            "flag": 0, "has_use_quick_brush": false, "has_use_quick_eraser": false,
            "interactiveTime": [], "path": "", "strokes": []
        },
        "media_path": "",
        "object_locked": null,
        "origin_material_id": "",
        // The original file: where it already is, or a link to it inside
        // the draft when the editor may not read where it is. Never a
        // pre-cut copy: the in and out points below are the edit.
        "path": path,
        "picture_from": "none",
        "picture_set_category_id": "",
        "picture_set_category_name": "",
        "request_id": "",
        "reverse_intensifies_path": "",
        "reverse_path": "",
        "smart_motion": null,
        "source": 0,
        "source_platform": 0,
        "stable": { "matrix_path": "", "stable_level": 0, "time_range": { "duration": 0, "start": 0 } },
        "team_id": "",
        // A still is a material of its own kind, not a one-frame video.
        "type": if source.is_still() { "photo" } else { "video" },
        "video_algorithm": {
            "algorithms": [], "deflicker": null, "motion_blur_config": null,
            "noise_reduction": null, "path": "", "time_range": null
        },
        "width": source.width.unwrap_or(0)
    })
}

/// What a video segment is made of.
struct VideoSegment<'a> {
    id: &'a str,
    material_id: &'a str,
    source_in: Micros,
    /// How much of the source it plays: its duration times its speed.
    source_duration: Micros,
    timeline_at: Micros,
    duration: Micros,
    speed: f64,
    /// Where the picture sits, when the cut moved it.
    transform: Option<&'a Transform>,
    keyframe_seed: &'a str,
    extra_material_refs: Vec<Value>,
}

fn video_segment(segment: &VideoSegment) -> Value {
    let VideoSegment {
        id,
        material_id,
        source_in,
        source_duration,
        timeline_at,
        duration,
        speed,
        transform,
        keyframe_seed,
        ref extra_material_refs,
    } = *segment;
    // The placement it opens on; keyframes carry it from there.
    let first = transform.and_then(|t| t.keyframes.first());
    let scale = first.map_or(1.0, |k| k.scale);
    let (x, y) = first.map_or((0.0, 0.0), |k| (2.0 * k.x + 0.0, -2.0 * k.y + 0.0));
    let rotation = first.map_or(0.0, |k| k.rotation_deg);
    let common_keyframes = transform.map_or_else(Vec::new, |t| keyframes(keyframe_seed, t));
    // The editor's defaults written as it writes them, so a draft the cut
    // did not move or speed up is the draft that was checked in the editor.
    let number = |v: f64, default: i64| {
        if (v - default as f64).abs() < 1e-12 {
            json!(default)
        } else {
            json!(v)
        }
    };
    json!({
        "caption_info": null,
        "cartoon": false,
        "clip": {
            "alpha": 1,
            "flip": { "horizontal": false, "vertical": false },
            "rotation": number(rotation, 0),
            "scale": { "x": number(scale, 1), "y": number(scale, 1) },
            "transform": { "x": number(x, 0), "y": number(y, 0) }
        },
        "common_keyframes": common_keyframes,
        "enable_adjust": true,
        "enable_color_correct_adjust": false,
        "enable_color_curves": true,
        "enable_color_match_adjust": false,
        "enable_color_wheels": true,
        "enable_lut": true,
        "enable_smart_color_adjust": false,
        "extra_material_refs": extra_material_refs,
        "group_id": "",
        "hdr_settings": { "intensity": 1, "mode": 1, "nits": 1000 },
        "id": id,
        "intensifies_audio": false,
        "is_placeholder": false,
        "is_tone_modify": false,
        "keyframe_refs": [],
        "last_nonzero_volume": 1,
        "material_id": material_id,
        "render_index": 0,
        "responsive_layout": {
            "enable": false, "horizontal_pos_layout": 0, "size_layout": 0,
            "target_follow": "", "vertical_pos_layout": 0
        },
        "reverse": false,
        "source_timerange": { "duration": source_duration.0, "start": source_in.0 },
        "speed": number(speed, 1),
        "target_timerange": { "duration": duration.0, "start": timeline_at.0 },
        "template_id": "",
        "template_scene": "default",
        "track_attribute": 0,
        "track_render_index": 0,
        "uniform_scale": { "on": true, "value": 1 },
        "visible": true,
        "volume": 1
    })
}

/// The editor's transition resource for a DSL transition, or `None` when this
/// writer has no verified id for it.
fn transition_material(id: &str, kind: TransitionType, duration: Micros) -> Option<Value> {
    let (effect_id, resource_id, name) = match kind {
        TransitionType::Xfade | TransitionType::Dissolve => {
            (DISSOLVE_EFFECT_ID, DISSOLVE_RESOURCE_ID, DISSOLVE_NAME)
        }
        // Cut needs no resource, and the rest are resources whose ids are not
        // knowable from outside the editor.
        _ => return None,
    };
    Some(json!({
        "id": id,
        "effect_id": effect_id,
        "resource_id": resource_id,
        "name": name,
        "duration": duration.0,
        "type": "transition",
        "is_overlap": false,
        "category_id": "",
        "category_name": "",
        "platform": "all",
        "source_platform": 0,
        "request_id": "",
        "team_id": "",
        "path": "",
        "icon_url": "",
        "is_cloud_material": false,
        "material_url": "",
        "panel": "",
        "third_resource_id": 0
    }))
}

/// A text material.
///
/// The content is the editor's own rich-text JSON rather than a bare string:
/// that is what recent releases write, and a bare string loses the styling on
/// the first edit. The range is in UTF-16 units, as the editor counts them.
fn text_material(id: &str, content: &str, is_title: bool) -> Value {
    let units: usize = content.chars().map(char::len_utf16).sum();
    let size = if is_title { 12 } else { 6 };
    let rich = json!({
        "styles": [{
            "fill": {
                "alpha": 1,
                "content": { "render_type": "solid", "solid": { "alpha": 1, "color": [1, 1, 1] } }
            },
            "range": [0, units],
            "size": size,
            "font": { "id": "", "path": "" }
        }],
        "text": content
    });
    json!({
        "add_type": 0,
        "alignment": 1,
        "background_alpha": 1,
        "background_color": "",
        "background_height": 0.14,
        "background_horizontal_offset": 0,
        "background_round_radius": 0,
        "background_style": 0,
        "background_vertical_offset": 0,
        "background_width": 0.14,
        "base_content": "",
        "bold_width": 0,
        "border_alpha": 1,
        "border_color": "#000000",
        "border_width": 0.08,
        "caption_template_info": null,
        "check_flag": 1,
        "combo_info": null,
        "content": rich.to_string(),
        "fixed_height": -1,
        "fixed_width": -1,
        "font_category_id": "",
        "font_category_name": "",
        "font_id": "",
        "font_name": "",
        "font_path": "",
        "font_resource_id": "",
        "font_size": size,
        "font_source_platform": 0,
        "font_team_id": "",
        "font_title": "none",
        "font_url": "",
        "fonts": [],
        "force_apply_line_max_width": false,
        "global_alpha": 1,
        "group_id": "",
        "has_shadow": false,
        "id": id,
        "initial_scale": 1,
        "inner_padding": -1,
        "is_rich_text": false,
        "italic_degree": 0,
        "ktv_color": "",
        "language": "",
        "layer_weight": 1,
        "letter_spacing": 0,
        "line_feed": 1,
        "line_max_width": 0.82,
        "line_spacing": 0.02,
        "multi_language_current": "none",
        "name": "",
        "original_size": [],
        "preset_category": "",
        "preset_category_id": "",
        "preset_has_set_alignment": false,
        "preset_id": "",
        "preset_index": 0,
        "preset_name": "",
        "recognize_task_id": "",
        "recognize_type": 0,
        "relevance_segment": [],
        "shadow_alpha": 0.9,
        "shadow_angle": -45,
        "shadow_color": "",
        "shadow_distance": 5,
        "shadow_point": { "x": 0.6363961030728928, "y": -0.6363961030728928 },
        "shadow_smoothing": 0.45,
        "shape_clip_x": false,
        "shape_clip_y": false,
        "source_from": "",
        "style_name": "",
        "sub_type": 0,
        "subtitle_keywords": null,
        "subtitle_template_original_fontsize": 0,
        "text_alpha": 1,
        "text_color": "#FFFFFF",
        "text_curve": null,
        "text_preset_resource_id": "",
        "text_size": if is_title { 60 } else { 30 },
        "text_to_audio_ids": [],
        "tts_auto_update": false,
        "type": if is_title { "text" } else { "subtitle" },
        "typesetting": 0,
        "underline": false,
        "underline_offset": 0,
        "underline_width": 0.05,
        "use_effect_default_color": true,
        "words": []
    })
}

fn text_segment(
    id: &str,
    material_id: &str,
    timeline_at: Micros,
    duration: Micros,
    is_title: bool,
) -> Value {
    json!({
        "caption_info": null,
        "cartoon": false,
        "clip": {
            "alpha": 1,
            "flip": { "horizontal": false, "vertical": false },
            "rotation": 0,
            "scale": { "x": 1, "y": 1 },
            // A title sits in the middle of the frame; a subtitle sits near
            // the bottom, where the DSL's own style puts it.
            "transform": { "x": 0, "y": if is_title { 0.0 } else { -0.8 } }
        },
        "common_keyframes": [],
        "enable_adjust": false,
        "enable_color_correct_adjust": false,
        "enable_color_curves": true,
        "enable_color_match_adjust": false,
        "enable_color_wheels": true,
        "enable_lut": false,
        "enable_smart_color_adjust": false,
        "extra_material_refs": [],
        "group_id": "",
        "hdr_settings": null,
        "id": id,
        "intensifies_audio": false,
        "is_placeholder": false,
        "is_tone_modify": false,
        "keyframe_refs": [],
        "last_nonzero_volume": 1,
        "material_id": material_id,
        "render_index": 1,
        "responsive_layout": {
            "enable": false, "horizontal_pos_layout": 0, "size_layout": 0,
            "target_follow": "", "vertical_pos_layout": 0
        },
        "reverse": false,
        "source_timerange": { "duration": duration.0, "start": 0 },
        "speed": 1,
        "target_timerange": { "duration": duration.0, "start": timeline_at.0 },
        "template_id": "",
        "template_scene": "default",
        "track_attribute": 0,
        "track_render_index": 0,
        "uniform_scale": { "on": true, "value": 1 },
        "visible": true,
        "volume": 1
    })
}

fn audio_material(id: &str, path: &str, file_duration: Micros) -> Value {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    json!({
        "app_id": 0,
        "category_id": "",
        "category_name": "local",
        "check_flag": 1,
        "duration": file_duration.0,
        "effect_id": "",
        "formula_id": "",
        "id": id,
        "intensifies_path": "",
        "local_material_id": "",
        "music_id": id,
        "name": name,
        "path": path,
        "request_id": "",
        "resource_id": "",
        "source_platform": 0,
        "team_id": "",
        "text_id": "",
        "tone_category_id": "",
        "tone_category_name": "",
        "tone_effect_id": "",
        "tone_effect_name": "",
        "tone_speaker": "",
        "tone_type": "",
        "type": "extract_music",
        "video_id": "",
        "wave_points": []
    })
}

fn audio_segment(
    id: &str,
    material_id: &str,
    asset_in: Micros,
    timeline_at: Micros,
    duration: Micros,
) -> Value {
    json!({
        "caption_info": null,
        "cartoon": false,
        "clip": null,
        "common_keyframes": [],
        "enable_adjust": false,
        "enable_color_correct_adjust": false,
        "enable_color_curves": true,
        "enable_color_match_adjust": false,
        "enable_color_wheels": true,
        "enable_lut": false,
        "enable_smart_color_adjust": false,
        "extra_material_refs": [],
        "group_id": "",
        "hdr_settings": null,
        "id": id,
        "intensifies_audio": false,
        "is_placeholder": false,
        "is_tone_modify": false,
        "keyframe_refs": [],
        "last_nonzero_volume": 1,
        "material_id": material_id,
        "render_index": 0,
        "responsive_layout": {
            "enable": false, "horizontal_pos_layout": 0, "size_layout": 0,
            "target_follow": "", "vertical_pos_layout": 0
        },
        "reverse": false,
        "source_timerange": { "duration": duration.0, "start": asset_in.0 },
        "speed": 1,
        "target_timerange": { "duration": duration.0, "start": timeline_at.0 },
        "template_id": "",
        "template_scene": "default",
        "track_attribute": 0,
        "track_render_index": 0,
        "uniform_scale": null,
        "visible": true,
        "volume": 1
    })
}

fn track(kind: &str, id: &str, segments: Vec<Value>) -> Value {
    json!({
        "attribute": 0,
        "flag": 0,
        "id": id,
        "is_default_name": true,
        "name": "",
        "segments": segments,
        "type": kind
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_editor_is_found_by_its_package_and_takes_no_draft() {
        let apps = "com.tencent.mm=8.0.50\ncom.lemon.lv=21.6.1\n";
        let jianying = on_phone(Editor::Jianying, apps);
        assert!(jianying.is_installed());
        assert_eq!(jianying.version.as_deref(), Some("21.6.1"));
        assert!(jianying.phone);
        // Its drafts are its own: nothing is written for it.
        assert!(!jianying.can_export());

        let capcut = on_phone(Editor::CapCut, apps);
        assert!(!capcut.is_installed());
        assert!(capcut.phone);
        assert!(!on_phone(Editor::Jianying, "").is_installed());
    }
    use vb_contracts::editing_dsl::{
        Anchor, AudioItem, Slot, SourceRef, TextItem, TextStyle, Transition,
    };
    use vb_contracts::footage_graph::{BeatGrid, BeatOrigin, MinedPriors, Source};

    /// A writer for an editor that may read anything, so tests about the
    /// draft's structure are not about where their fixtures live.
    fn anywhere() -> JianyingWriter {
        JianyingWriter {
            editor: Editor::Jianying,
            media_access: MediaAccess::Anywhere,
            media: DraftMedia::Link,
        }
    }

    fn graph() -> FootageGraph {
        let mut g = FootageGraph::new("p");
        g.sources.push(Source {
            id: "src-0001".into(),
            path: "/movies/holiday.mp4".into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros::from_secs(60),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".into()),
            audio_codec: None,
            has_audio: Some(true),
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        });
        g
    }

    fn clip(id: &str, order: u32, from_s: u64, len_s: u64) -> Slot {
        Slot {
            id: id.into(),
            order,
            kind: SlotKind::Clip,
            duration_us: Micros::from_secs(len_s),
            source_ref: Some(SourceRef {
                source_id: "src-0001".into(),
                shot_id: Some(format!("shot-{order}")),
                in_us: Micros::from_secs(from_s),
                out_us: Micros::from_secs(from_s + len_s),
            }),
            speed: None,
            reason: None,
            score: None,
        }
    }

    fn dsl() -> EditingDsl {
        let mut dsl = EditingDsl::new("Holiday");
        dsl.slots.push(clip("slot-a", 0, 10, 3));
        dsl.slots.push(clip("slot-b", 1, 30, 4));
        dsl
    }

    fn built(dsl: &EditingDsl, graph: &FootageGraph) -> Value {
        anywhere()
            .build(
                dsl,
                graph,
                Path::new("/drafts/holiday-1"),
                &Default::default(),
            )
            .unwrap()
            .content
    }

    #[test]
    fn clips_reference_the_original_file_with_in_and_out_points() {
        // The whole point of this export: the engine says how to cut, the
        // editor does the cutting. A pre-cut copy would make every decision
        // final, which is the opposite of what the draft is for.
        let content = built(&dsl(), &graph());
        let videos = content["materials"]["videos"].as_array().unwrap();
        assert_eq!(videos.len(), 2);
        for material in videos {
            assert_eq!(material["path"], "/movies/holiday.mp4");
            assert_eq!(material["duration"], 60_000_000);
        }

        let segments = content["tracks"][0]["segments"].as_array().unwrap();
        assert_eq!(segments[0]["source_timerange"]["start"], 10_000_000);
        assert_eq!(segments[0]["source_timerange"]["duration"], 3_000_000);
        assert_eq!(segments[1]["source_timerange"]["start"], 30_000_000);
    }

    #[test]
    fn a_clip_left_alone_is_written_exactly_as_before() {
        // No speed, no framing: the editor's own defaults, in its own form,
        // so the drafts already checked in 剪映 are what is still written.
        let content = built(&dsl(), &graph());
        let segment = &content["tracks"][0]["segments"][0];
        assert_eq!(segment["speed"], json!(1));
        assert_eq!(segment["clip"]["scale"], json!({ "x": 1, "y": 1 }));
        assert_eq!(segment["clip"]["transform"], json!({ "x": 0, "y": 0 }));
        assert_eq!(segment["clip"]["rotation"], json!(0));
        assert_eq!(segment["common_keyframes"], json!([]));
        assert_eq!(content["materials"]["speeds"], json!([]));
    }

    #[test]
    fn a_sped_up_clip_plays_more_of_its_source_and_says_how_fast() {
        let mut d = dsl();
        // Four seconds on screen at 1.5x is six of the source.
        d.slots[1].speed = Some(1.5);
        d.slots[1].source_ref.as_mut().unwrap().out_us = Micros::from_secs(36);
        let content = built(&d, &graph());
        let segment = &content["tracks"][0]["segments"][1];
        assert_eq!(segment["speed"], json!(1.5));
        assert_eq!(segment["source_timerange"]["duration"], 6_000_000);
        assert_eq!(segment["target_timerange"]["duration"], 4_000_000);
        let speeds = content["materials"]["speeds"].as_array().unwrap();
        assert_eq!(speeds.len(), 1);
        assert_eq!(speeds[0]["speed"], json!(1.5));
        assert_eq!(speeds[0]["type"], "speed");
        assert_eq!(segment["extra_material_refs"][0], speeds[0]["id"]);
        // The clip after it still starts where it ends on the timeline.
        assert_eq!(segment["target_timerange"]["start"], 3_000_000);
    }

    #[test]
    fn a_moved_picture_opens_where_the_cut_put_it_and_eases_by_keyframes() {
        use vb_contracts::editing_dsl::{TransformKeyframe, VideoItem};
        let mut d = dsl();
        d.tracks.video.push(VideoItem {
            slot_id: "slot-b".into(),
            crop: None,
            filters: Vec::new(),
            transform: Some(Transform {
                keyframes: vec![
                    TransformKeyframe {
                        offset_us: Micros::ZERO,
                        scale: 1.25,
                        x: -0.05,
                        y: 0.1,
                        rotation_deg: 3.0,
                    },
                    TransformKeyframe::fitted(Micros::from_secs(4)),
                ],
            }),
            background: None,
        });
        let content = built(&d, &graph());
        let segment = &content["tracks"][0]["segments"][1];
        // Opens enlarged, a little left and below centre -- the editor
        // counts position in half the canvas, up positive -- and turned.
        assert_eq!(segment["clip"]["scale"]["x"], json!(1.25));
        assert_eq!(segment["clip"]["transform"]["x"], json!(-0.1));
        assert_eq!(segment["clip"]["transform"]["y"], json!(-0.2));
        assert_eq!(segment["clip"]["rotation"], json!(3.0));
        let tracks = segment["common_keyframes"].as_array().unwrap();
        let kinds: Vec<&str> = tracks
            .iter()
            .map(|t| t["property_type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "KFTypePositionX",
                "KFTypePositionY",
                "KFTypeScaleX",
                "KFTypeScaleY",
                "KFTypeRotation"
            ]
        );
        let scale = &tracks[2]["keyframe_list"];
        assert_eq!(scale[0]["time_offset"], 0);
        assert_eq!(scale[0]["values"], json!([1.25]));
        assert_eq!(scale[1]["time_offset"], 4_000_000);
        assert_eq!(scale[1]["values"], json!([1.0]));
        // The clip before it was left alone.
        let before = &content["tracks"][0]["segments"][0];
        assert_eq!(before["common_keyframes"], json!([]));
    }

    #[test]
    fn a_photo_is_a_photo_material_and_does_not_set_the_canvas() {
        // A 12 MP photo first in the project, then the video.
        let mut g = graph();
        g.sources.insert(
            0,
            Source {
                id: "src-0000".into(),
                path: "/movies/P30929-183143.jpg".into(),
                content_hash: Blake3Hash::of_str("photo"),
                media_kind: Some(vb_contracts::MediaKind::Image),
                duration_us: Micros::from_secs(10),
                fps_num: None,
                fps_den: None,
                width: Some(3060),
                height: Some(4080),
                video_codec: Some("mjpeg".into()),
                audio_codec: None,
                has_audio: Some(false),
                created_at: None,
                rotation: Some(0),
                categories: Vec::new(),
            },
        );
        let mut dsl = EditingDsl::new("mixed");
        dsl.slots = vec![
            Slot {
                source_ref: Some(SourceRef {
                    source_id: "src-0000".into(),
                    shot_id: None,
                    in_us: Micros::ZERO,
                    out_us: Micros::from_secs(3),
                }),
                ..clip("slot-photo", 0, 0, 3)
            },
            clip("slot-video", 1, 10, 4),
        ];
        let content = built(&dsl, &g);
        let materials = content["materials"]["videos"].as_array().unwrap();
        assert_eq!(materials[0]["type"], "photo");
        assert_eq!(materials[0]["duration"], PHOTO_DURATION_US);
        assert_eq!(materials[0]["has_audio"], false);
        assert_eq!(materials[1]["type"], "video");
        let segments = content["tracks"][0]["segments"].as_array().unwrap();
        assert_eq!(segments[0]["source_timerange"]["start"], 0);
        assert_eq!(segments[0]["source_timerange"]["duration"], 3_000_000);
        assert_eq!(segments[1]["target_timerange"]["start"], 3_000_000);
        // The frame and the rate are the video's.
        assert_eq!(content["canvas_config"]["width"], 1920);
        assert_eq!(content["canvas_config"]["height"], 1080);
    }

    #[test]
    fn a_project_of_photos_alone_gets_a_canvas_no_larger_than_4k() {
        assert_eq!(fit_canvas(3060, 4080), (2880, 3840));
        assert_eq!(fit_canvas(1920, 1080), (1920, 1080));
    }

    #[test]
    fn clips_are_laid_end_to_end_with_no_gap_and_no_overlap() {
        let content = built(&dsl(), &graph());
        let segments = content["tracks"][0]["segments"].as_array().unwrap();
        assert_eq!(segments[0]["target_timerange"]["start"], 0);
        assert_eq!(segments[1]["target_timerange"]["start"], 3_000_000);
        assert_eq!(content["duration"], 7_000_000);
    }

    #[test]
    fn a_title_card_leaves_a_gap_that_the_title_sits_over() {
        let mut dsl = dsl();
        dsl.slots.push(Slot {
            id: "slot-title".into(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: Micros::from_secs(2),
            source_ref: None,
            speed: None,
            reason: None,
            score: None,
        });
        for slot in dsl.slots.iter_mut().filter(|s| s.kind == SlotKind::Clip) {
            slot.order += 1;
        }
        dsl.tracks.text.push(TextItem {
            id: "txt-title".into(),
            anchor: Anchor::at("slot-title"),
            duration_us: Micros::from_secs(2),
            content: "Holiday".into(),
            role: Some(TextRole::Title),
            transcript_segment_id: None,
            style: Some(TextStyle::default()),
        });

        let content = built(&dsl, &graph());
        let segments = content["tracks"][0]["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2, "a title card is not a video segment");
        assert_eq!(
            segments[0]["target_timerange"]["start"], 2_000_000,
            "the first clip starts after the title card"
        );
        assert_eq!(
            content["config"]["maintrack_adsorb"], false,
            "with this on the editor closes the gap and the title lands over the first clip"
        );

        let text = content["tracks"][1]["segments"].as_array().unwrap();
        assert_eq!(text[0]["target_timerange"]["start"], 0);
        let material = &content["materials"]["texts"][0];
        let rich: Value = serde_json::from_str(material["content"].as_str().unwrap()).unwrap();
        assert_eq!(rich["text"], "Holiday");
        assert_eq!(rich["styles"][0]["range"][1], 7);
    }

    #[test]
    fn a_subtitle_carries_its_text_and_sits_low_in_the_frame() {
        let mut dsl = dsl();
        dsl.tracks.text.push(TextItem {
            id: "txt-1".into(),
            anchor: Anchor {
                slot_id: "slot-b".into(),
                offset_us: Some(500_000),
            },
            duration_us: Micros::from_secs(1),
            content: "你好 world".into(),
            role: Some(TextRole::Subtitle),
            transcript_segment_id: Some("tr-0001".into()),
            style: None,
        });
        let content = built(&dsl, &graph());
        let segment = &content["tracks"][1]["segments"][0];
        assert_eq!(segment["target_timerange"]["start"], 3_500_000);
        assert_eq!(segment["clip"]["transform"]["y"], -0.8);
        let rich: Value = serde_json::from_str(
            content["materials"]["texts"][0]["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(rich["text"], "你好 world");
        // UTF-16 units, which is how the editor counts a range.
        assert_eq!(rich["styles"][0]["range"][1], 8);
    }

    #[test]
    fn a_dissolve_is_written_and_an_unknown_transition_becomes_a_cut() {
        // Naming a resource the editor does not have produces a draft that
        // opens broken, so anything without a verified id is a hard cut and
        // the caller is told how many.
        let mut dsl = dsl();
        dsl.slots.push(clip("slot-c", 2, 40, 3));
        dsl.transitions.push(Transition {
            from_slot_id: "slot-a".into(),
            to_slot_id: "slot-b".into(),
            kind: TransitionType::Dissolve,
            duration_us: Micros::from_millis(600),
        });
        dsl.transitions.push(Transition {
            from_slot_id: "slot-b".into(),
            to_slot_id: "slot-c".into(),
            kind: TransitionType::WipeLeft,
            duration_us: Micros::from_millis(600),
        });

        let draft = anywhere()
            .build(
                &dsl,
                &graph(),
                Path::new("/drafts/holiday-2"),
                &Default::default(),
            )
            .unwrap();
        let transitions = draft.content["materials"]["transitions"]
            .as_array()
            .unwrap();
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0]["resource_id"], DISSOLVE_RESOURCE_ID);
        assert_eq!(transitions[0]["duration"], 600_000);
        assert_eq!(draft.summary.transitions_dropped, 1);

        let segments = draft.content["tracks"][0]["segments"].as_array().unwrap();
        assert_eq!(
            segments[0]["extra_material_refs"][0], transitions[0]["id"],
            "a transition belongs to the clip it leads out of"
        );
        assert!(
            segments[1]["extra_material_refs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn chosen_music_becomes_an_audio_track_starting_on_its_first_beat() {
        let mut graph = graph();
        graph.priors = Some(MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 120.0,
                first_beat_us: Some(Micros::from_millis(250)),
                confidence: Some(0.9),
                origin: Some(BeatOrigin::Music),
                asset_path: Some("/music/track.mp3".into()),
                asset_duration_us: Some(Micros::from_secs(200)),
            }),
            ..Default::default()
        });
        let mut dsl = dsl();
        dsl.tracks.audio.push(AudioItem {
            id: "aud-music".into(),
            anchor: Anchor::at("slot-a"),
            kind: AudioKind::Bgm,
            duration_us: Some(Micros::from_secs(7)),
            asset_path: Some("/music/track.mp3".into()),
            asset_in_us: Some(Micros::from_millis(250)),
            gain_db: None,
            muted: None,
            fade_in_us: None,
            fade_out_us: Some(Micros::from_millis(1500)),
            ducking: None,
        });

        let content = built(&dsl, &graph);
        let audios = content["materials"]["audios"].as_array().unwrap();
        assert_eq!(audios[0]["path"], "/music/track.mp3");
        // The file's own length, not the part used, so the track can be
        // dragged out in the editor.
        assert_eq!(audios[0]["duration"], 200_000_000);

        let audio_track = content["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["type"] == "audio")
            .expect("music becomes an audio track");
        let segment = &audio_track["segments"][0];
        assert_eq!(segment["source_timerange"]["start"], 250_000);
        assert_eq!(segment["target_timerange"]["start"], 0);
        assert_eq!(segment["target_timerange"]["duration"], 7_000_000);
    }

    #[test]
    fn a_slot_naming_a_source_the_graph_lacks_is_an_error_not_a_broken_draft() {
        let mut dsl = dsl();
        dsl.slots[0].source_ref.as_mut().unwrap().source_id = "src-9999".into();
        let err = anywhere()
            .build(&dsl, &graph(), Path::new("/drafts/x"), &Default::default())
            .unwrap_err();
        assert!(
            matches!(err, ExportError::DanglingSource { .. }),
            "got {err}"
        );
    }

    #[test]
    fn ids_are_unique_within_a_draft_and_stable_between_runs() {
        let dsl = dsl();
        let graph = graph();
        let first = built(&dsl, &graph);
        let second = built(&dsl, &graph);
        assert_eq!(first, second, "the same cut must produce the same draft");

        let mut ids = Vec::new();
        for material in first["materials"]["videos"].as_array().unwrap() {
            ids.push(material["id"].as_str().unwrap().to_string());
        }
        for segment in first["tracks"][0]["segments"].as_array().unwrap() {
            ids.push(segment["id"].as_str().unwrap().to_string());
        }
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate ids: {ids:?}");
        for id in &ids {
            assert_eq!(id.len(), 36, "{id} is not shaped like a UUID");
        }
    }

    #[test]
    fn the_canvas_follows_the_footage_including_portrait() {
        let mut portrait = graph();
        portrait.sources[0].width = Some(1080);
        portrait.sources[0].height = Some(1920);
        let content = built(&dsl(), &portrait);
        assert_eq!(content["canvas_config"]["width"], 1080);
        assert_eq!(content["canvas_config"]["height"], 1920);
    }

    #[test]
    fn a_draft_is_a_folder_of_the_three_files_the_editor_reads() {
        let dir = std::env::temp_dir().join("vb-jianying-write/holiday");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        let summary = anywhere().write_draft(&dsl(), &graph(), &dir).unwrap();

        for name in [
            "draft_content.json",
            "draft_info.json",
            "draft_meta_info.json",
        ] {
            let path = dir.join(name);
            assert!(path.is_file(), "{name} was not written");
            let text = std::fs::read_to_string(&path).unwrap();
            serde_json::from_str::<Value>(&text).expect("the editor reads plain JSON");
        }
        assert_eq!(summary.clips, 2);

        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("draft_meta_info.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["draft_fold_path"], dir.to_string_lossy().as_ref());
        assert_eq!(meta["draft_name"], "holiday");
        assert_eq!(meta["tm_duration"], 7_000_000);

        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_draft_root_is_only_reported_when_it_exists() {
        // The editor creates the folder on first launch, so its absence means
        // "never run here", which is a different answer from "not installed".
        let empty = std::env::temp_dir().join("vb-no-editor-here");
        let _ = std::fs::create_dir_all(&empty);
        let found = detect_editor(Editor::Jianying, &empty);
        assert!(found.draft_root.is_none());
        assert!(!found.can_export());
    }

    #[test]
    fn media_the_editor_may_not_read_is_linked_into_the_draft() {
        // The sandbox case: footage outside ~/Movies. The draft must point at
        // a copy of the path the editor may read, and cost no disk to do it.
        let base = std::env::temp_dir().join("vb-jianying-sandbox");
        let _ = std::fs::remove_dir_all(&base);
        let movies = base.join("home/Movies");
        let elsewhere = base.join("Desktop");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let outside = elsewhere.join("holiday.mp4");
        let inside = movies.join("already-fine.mp4");
        std::fs::write(&outside, b"outside footage").unwrap();
        std::fs::write(&inside, b"inside footage").unwrap();

        let mut graph = graph();
        graph.sources[0].path = outside.to_string_lossy().to_string();
        let mut second = graph.sources[0].clone();
        second.id = "src-0002".into();
        second.path = inside.to_string_lossy().to_string();
        graph.sources.push(second);
        let mut dsl = dsl();
        dsl.slots[1].source_ref.as_mut().unwrap().source_id = "src-0002".into();

        let writer = JianyingWriter {
            editor: Editor::Jianying,
            media_access: MediaAccess::Within(vec![movies.clone()]),
            media: DraftMedia::Link,
        };
        let draft_dir = movies.join("JianyingPro/drafts/holiday");
        let summary = writer.write_draft(&dsl, &graph, &draft_dir).unwrap();
        assert_eq!(summary.media.linked, 1, "{:?}", summary.media);
        assert_eq!(summary.media.unreadable, 0);

        let content: Value = serde_json::from_str(
            &std::fs::read_to_string(draft_dir.join("draft_content.json")).unwrap(),
        )
        .unwrap();
        let paths: Vec<&str> = content["materials"]["videos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["path"].as_str().unwrap())
            .collect();
        let linked = draft_dir.join("Resources/001-holiday.mp4");
        assert!(
            paths.contains(&linked.to_string_lossy().as_ref()),
            "{paths:?}"
        );
        assert!(
            paths.contains(&inside.to_string_lossy().as_ref()),
            "a file the editor can already read is referenced where it is: {paths:?}"
        );
        // A link, not a new file: the same bytes, and the original is intact.
        assert_eq!(std::fs::read(&linked).unwrap(), b"outside footage");
        assert!(outside.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn asked_for_references_the_draft_points_at_the_originals_and_holds_no_media() {
        let base = std::env::temp_dir().join("vb-jianying-reference");
        let _ = std::fs::remove_dir_all(&base);
        let movies = base.join("home/Movies");
        let elsewhere = base.join("Documents");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let outside = elsewhere.join("holiday.mp4");
        std::fs::write(&outside, b"outside footage").unwrap();

        let mut graph = graph();
        graph.sources[0].path = outside.to_string_lossy().to_string();
        let writer = JianyingWriter {
            editor: Editor::Jianying,
            media_access: MediaAccess::Within(vec![movies.clone()]),
            media: DraftMedia::Reference,
        };
        let draft_dir = movies.join("JianyingPro/drafts/holiday");
        let summary = writer.write_draft(&dsl(), &graph, &draft_dir).unwrap();
        assert_eq!(summary.media.linked, 0);
        assert_eq!(summary.media.unreadable, 1, "the run must say so");
        assert!(!draft_dir.join("Resources").exists());

        let content: Value = serde_json::from_str(
            &std::fs::read_to_string(draft_dir.join("draft_content.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            content["materials"]["videos"][0]["path"],
            outside.to_string_lossy().as_ref()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_the_footage_the_cut_uses_is_linked() {
        let base = std::env::temp_dir().join("vb-jianying-used-only");
        let _ = std::fs::remove_dir_all(&base);
        let movies = base.join("home/Movies");
        let elsewhere = base.join("Documents");
        std::fs::create_dir_all(&movies).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let used = elsewhere.join("used.mp4");
        let unused = elsewhere.join("unused.mp4");
        std::fs::write(&used, b"used").unwrap();
        std::fs::write(&unused, b"unused").unwrap();

        let mut graph = graph();
        graph.sources[0].path = used.to_string_lossy().to_string();
        let mut other = graph.sources[0].clone();
        other.id = "src-0009".into();
        other.path = unused.to_string_lossy().to_string();
        graph.sources.push(other);
        let writer = JianyingWriter {
            editor: Editor::Jianying,
            media_access: MediaAccess::Within(vec![movies.clone()]),
            media: DraftMedia::Link,
        };
        let draft_dir = movies.join("JianyingPro/drafts/used");
        let summary = writer.write_draft(&dsl(), &graph, &draft_dir).unwrap();
        assert_eq!(summary.media.linked, 1);
        let linked: Vec<_> = std::fs::read_dir(draft_dir.join("Resources"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(linked, vec!["001-used.mp4".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn media_access_compares_real_paths() {
        let access = MediaAccess::Within(vec![PathBuf::from("/Users/someone/Movies")]);
        assert!(access.allows(Path::new("/Users/someone/Movies/a/b.mp4")));
        assert!(!access.allows(Path::new("/Users/someone/Desktop/b.mp4")));
        assert!(!access.allows(Path::new("/Users/someone/MoviesElsewhere/b.mp4")));
        assert!(MediaAccess::Anywhere.allows(Path::new("/tmp/b.mp4")));
    }
}
