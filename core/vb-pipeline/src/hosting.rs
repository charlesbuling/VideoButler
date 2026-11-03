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

//! The video butler: every video and photo on the device, filed by what it
//! shows.
//!
//! One tap hands the butler the user's footage. It walks the folders videos
//! and photos live in -- or takes the list a caller already holds, a file
//! server's index of its disk, rather than walking it again -- looks at each
//! video it has not seen in three frames and each photo once -- the scene
//! labels and the faces in them -- and files it: by face, person by person,
//! into the album; and into categories, study to animals. Then it reads the
//! whole collection for memories (`memories.rs`) and makes each one a
//! project, cut in the background like any other.
//!
//! # What is kept
//!
//! A catalogue in the library, one entry per video or photo, keyed by its
//! path, size and modification time, so a second pass looks only at what is
//! new or changed. Each entry keeps what was *seen* -- every frame's labels
//! with their confidence, every face's size and frames -- not only the
//! categories drawn from it, so a category the user removes is dropped from
//! every video at once and comes back the same way, and a change to the
//! rules re-files the collection without decoding a frame.
//!
//! # What is not claimed
//!
//! Categories are rules over labels (see `vb_perception::categories`), faces
//! are grouped by likeness, not recognised, and a video looked at by a mock
//! is filed under nothing rather than under a guess.

use crate::library::Library;
use crate::progress::Observer;
use crate::{Lang, entities};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_infer::tracker::TrackWindow;
use vb_infer::{ScoredLabel, TrackerAdapter, VlmAdapter, VlmRequest};
use vb_media::{GeoPoint, MediaBackend};
use vb_perception::categories::{self, Category, ClipLook, FaceSeen, Vocabulary};

/// The rules entries are filed by. Raised when they change, so the
/// collection is filed again from what was seen.
const RULES: u32 = 3;

/// What is looked at: the frames and what reads them, and when the probe
/// says a video was filmed. Raised when that changes, so every video is
/// looked at again.
const LOOK: u32 = 4;

/// Labels kept per frame. Vision's tail is near-zero noise.
const LABELS_KEPT: usize = 15;

/// Shorter than this is a misfire, not footage.
const SHORTEST: u64 = 1_000_000;

/// A picture narrower than this on its short side is an icon, a sticker or
/// a thumbnail, not a photo someone took: a camera phone of 2008 took
/// 640 x 480.
const SMALLEST_SIDE: u32 = 480;

/// The butler lists someone among the people once they are the subject of
/// this many videos -- a day of photos counting as one, since a burst of
/// twenty photos of a stranger at a viewpoint is one sighting, not twenty.
const PERSON_VIDEOS: usize = 2;

/// What the user chose for the butler.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HostingSettings {
    /// The user switched hosting on: the app looks again each time it
    /// opens.
    pub enabled: bool,
    /// Folders to keep. Empty means [`default_roots`].
    pub roots: Vec<String>,
    /// Categories the user removed: not filed, not shown.
    pub removed: Vec<String>,
}

impl HostingSettings {
    pub fn is_default(&self) -> bool {
        *self == HostingSettings::default()
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        if self.roots.is_empty() {
            default_roots()
        } else {
            self.roots.iter().map(PathBuf::from).collect()
        }
    }

    fn removed(&self) -> HashSet<String> {
        self.removed.iter().cloned().collect()
    }
}

/// Where a device keeps its videos: on a desktop the user's own media and
/// document folders, on Android its shared storage's camera and download
/// folders. Only those that exist.
pub fn default_roots() -> Vec<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "android") {
        let base = PathBuf::from("/storage/emulated/0");
        ["DCIM", "Movies", "Pictures", "Download"]
            .iter()
            .map(|d| base.join(d))
            .collect()
    } else {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        [
            "Movies",
            "Videos",
            "Pictures",
            "Desktop",
            "Documents",
            "Downloads",
        ]
        .iter()
        .map(|d| home.join(d))
        .collect()
    };
    candidates.into_iter().filter(|p| p.is_dir()).collect()
}

/// One face, as one video showed it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeenFace {
    /// The album subject it was filed under.
    pub subject: Option<String>,
    /// The largest it was, as a share of the picture.
    pub area: f64,
    /// Which of the frames it is in.
    pub frames: Vec<usize>,
    /// The face as it was cut out of this video, when the tracker kept one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
}

/// What was seen of a video, kept so it can be filed again without being
/// looked at again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Look {
    /// The adapter that labelled the frames; none when nothing real did.
    pub labeller: Option<String>,
    /// The tracker that found the faces; none when nothing real did.
    pub tracker: Option<String>,
    pub frames: Vec<Vec<ScoredLabel>>,
    pub faces: Vec<SeenFace>,
    /// Its name is a screen recorder's, or for a photo a screenshot's.
    pub screen: bool,
    /// A photograph, looked at once: one picture and no running time.
    pub still: bool,
}

impl Look {
    fn clip(&self) -> ClipLook {
        ClipLook {
            frames: self.frames.clone(),
            vocabulary: self.labeller.as_deref().and_then(Vocabulary::of_adapter),
            faces: self.tracker.as_ref().map(|_| {
                self.faces
                    .iter()
                    .map(|f| FaceSeen {
                        area: f.area,
                        frames: f.frames.clone(),
                    })
                    .collect()
            }),
            screen_recording: self.screen,
            still: self.still,
        }
    }
}

/// One video or photo the butler keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    /// The file's modification time, in seconds.
    pub modified: u64,
    /// Its size and its first and last 256 KB, hashed: two copies of one
    /// video are one video.
    pub fingerprint: String,
    /// Zero for a photo.
    pub duration_us: u64,
    /// As shown, after rotation.
    pub width: u32,
    pub height: u32,
    /// When the camera says it filmed, as it wrote it.
    pub captured_at: Option<String>,
    /// When the file says it was made, for a video whose camera says
    /// nothing. A copy carries the day it was copied.
    pub file_time: Option<String>,
    pub location: Option<GeoPoint>,
    pub look: Look,
    /// What it is filed under, `people` included, removed categories not.
    pub categories: Vec<String>,
    pub rules: u32,
    pub looked: u32,
    pub error: Option<String>,
    pub at: String,
}

impl Entry {
    /// The album subjects in it who are its subjects, not passers-by.
    pub fn subjects(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .look
            .faces
            .iter()
            .filter(|f| f.area >= categories::SUBJECT_FACE)
            .filter_map(|f| f.subject.as_deref())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub fn name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }

    pub fn is_photo(&self) -> bool {
        self.look.still
    }

    /// What it counts as when deciding who to list: a video once, a photo
    /// once per local day it was taken on.
    fn sighting(&self) -> String {
        if !self.is_photo() {
            return self.path.clone();
        }
        let day = self
            .captured_at
            .as_deref()
            .or(self.file_time.as_deref())
            .and_then(|t| t.get(..10))
            .unwrap_or("undated");
        format!("photos {day}")
    }

    fn file(&self) -> FileKey {
        FileKey {
            size: self.size,
            modified: self.modified,
        }
    }

    fn refile(&mut self, removed: &HashSet<String>) {
        self.categories = categories::classify(&self.look.clip())
            .into_iter()
            .map(Category::id)
            .filter(|id| !removed.contains(*id))
            .map(str::to_string)
            .collect();
        self.rules = RULES;
    }

    pub fn is_in(&self, category: &str) -> bool {
        self.categories.iter().any(|c| c == category)
    }

    /// Take the faces of a video recorded off a screen out of the album's
    /// people (see [`subjects_only`]): filed before that rule, they are
    /// filed under no one now. Nothing is decoded for it. Whether any was.
    fn unfile_screen_faces(&mut self) -> bool {
        if !self.look.clip().off_a_screen() {
            return false;
        }
        let mut any = false;
        for face in &mut self.look.faces {
            any |= face.subject.take().is_some();
        }
        any
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileKey {
    size: u64,
    modified: u64,
}

fn file_key(path: &Path) -> Option<FileKey> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Some(FileKey {
        size: meta.len(),
        modified,
    })
}

/// Everything the butler keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Catalogue {
    pub entries: Vec<Entry>,
    /// When the last look through every folder finished.
    pub scanned_at: Option<String>,
    /// What the last look used, for the page to say.
    pub labeller: Option<String>,
    pub tracker: Option<String>,
}

impl Catalogue {
    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

pub fn dir(library: &Library) -> PathBuf {
    library.root().join("hosting")
}

fn catalogue_path(library: &Library) -> PathBuf {
    dir(library).join("catalogue.json")
}

pub fn load(library: &Library) -> Catalogue {
    std::fs::read_to_string(catalogue_path(library))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(library: &Library, catalogue: &Catalogue) -> Result<(), String> {
    let path = catalogue_path(library);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string(catalogue).map_err(|e| e.to_string())?;
    let part = path.with_extension("part");
    std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Finding the videos and photos
// ---------------------------------------------------------------------------

/// A video or a photo the butler looks at, by its name.
fn is_footage(path: &Path) -> bool {
    crate::api::is_video_path(path) || vb_media::is_still_path(path)
}

/// Directories no video of the user's is in, or that hold copies of them:
/// build output, app bundles, photo-library packages (which macOS guards
/// and which hold the originals of what the Photos app shows), and the
/// drafts editors write, whose `Resources/` link the footage again.
fn skipped_dir(name: &str) -> bool {
    const NAMES: [&str; 12] = [
        "node_modules",
        "target",
        "build",
        "Library",
        "JianyingPro",
        "CapCut",
        "_vbcache",
        "_vbwork",
        "vb-project",
        "$RECYCLE.BIN",
        "Android",
        "thumbnails",
    ];
    const BUNDLES: [&str; 8] = [
        ".app",
        ".photoslibrary",
        ".fcpbundle",
        ".imovielibrary",
        ".tvlibrary",
        ".musiclibrary",
        ".lrlibrary",
        ".bundle",
    ];
    name.starts_with('.')
        || NAMES.contains(&name)
        || BUNDLES.iter().any(|b| name.to_lowercase().ends_with(b))
}

/// A folder an editor wrote as a draft.
fn is_draft(dir: &Path) -> bool {
    dir.join("draft_content.json").exists() || dir.join("draft_meta_info.json").exists()
}

/// Whether a path is inside one of the folders.
fn under_any(path: &str, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| Path::new(path).starts_with(root))
}

/// What the page says of a folder kept that cannot be looked through now.
fn unreachable_note(root: &Path, lang: Lang) -> Option<String> {
    let error = std::fs::read_dir(root).err()?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string());
    if !root.exists() {
        return Some(match lang {
            Lang::Zh => format!(
                "「{name}」现在没有接上（{}）。之前从它整理出的视频先保留着，接上后会接着整理。",
                root.display()
            ),
            Lang::En => format!(
                "{name} is not connected now ({}). What was filed from it is kept, and filing \
                 carries on when it is back.",
                root.display()
            ),
        });
    }
    Some(crate::api::unreadable(root, &error, lang))
}

/// Every video and photo under the roots, once each: a hard link or a
/// second root over the same folder is one file.
pub fn find_footage(roots: &[PathBuf], skip: &Path) -> Vec<PathBuf> {
    let skip = dunce::canonicalize(skip).unwrap_or_else(|_| skip.to_path_buf());
    let skip = skip.as_path();
    let mut out = Vec::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        walk(root, skip, &mut out, &mut seen, &mut visited);
    }
    out.sort();
    out
}

fn walk(
    dir: &Path,
    skip: &Path,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<(u64, u64)>,
    visited: &mut HashSet<PathBuf>,
) {
    let canonical = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if canonical.starts_with(skip) || !visited.insert(canonical) || is_draft(dir) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Links are not followed: a link into a folder already walked, or
        // up the tree, is how a walk never ends.
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            if !skipped_dir(&name) {
                walk(&path, skip, out, seen, visited);
            }
        } else if kind.is_file() && !name.starts_with('.') && is_footage(&path) {
            if let Some(id) = inode(&path)
                && !seen.insert(id)
            {
                continue;
            }
            out.push(path);
        }
    }
}

/// The footage of a list the caller already holds, in place of a walk --
/// a file server that has indexed its disk need not have it walked twice.
/// Kept as [`find_footage`] would keep it: a video or a photo by its name,
/// absolute, a file that is there now and not a link, not hidden, not
/// inside the library, and a hard link once.
pub fn given_footage(files: &[PathBuf], skip: &Path) -> Vec<PathBuf> {
    let skip = dunce::canonicalize(skip).unwrap_or_else(|_| skip.to_path_buf());
    let mut listed: HashSet<&Path> = HashSet::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut out = Vec::new();
    for path in files {
        let hidden = path
            .file_name()
            .is_none_or(|n| n.to_string_lossy().starts_with('.'));
        if !path.is_absolute() || hidden || !is_footage(path) || !listed.insert(path) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.clone());
        if canonical.starts_with(&skip) {
            continue;
        }
        if let Some(id) = inode(path)
            && !seen.insert(id)
        {
            continue;
        }
        out.push(path.clone());
    }
    out.sort();
    out
}

#[cfg(unix)]
fn inode(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn inode(_path: &Path) -> Option<(u64, u64)> {
    None
}

fn fingerprint(path: &Path, size: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    const PART: u64 = 256 * 1024;
    let mut hasher: Vec<u8> = Vec::with_capacity(2 * PART as usize + 8);
    hasher.extend_from_slice(&size.to_le_bytes());
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut head = vec![0u8; PART.min(size) as usize];
        if file.read_exact(&mut head).is_ok() {
            hasher.extend_from_slice(&head);
        }
        if size > 2 * PART && file.seek(SeekFrom::End(-(PART as i64))).is_ok() {
            let mut tail = vec![0u8; PART as usize];
            if file.read_exact(&mut tail).is_ok() {
                hasher.extend_from_slice(&tail);
            }
        }
    }
    vb_contracts::Blake3Hash::of_parts(&[&hasher]).to_hex()
}

// ---------------------------------------------------------------------------
// Looking at a video or a photo
// ---------------------------------------------------------------------------

/// What looks at the frames.
pub struct Eyes<'a> {
    pub media: &'a dyn MediaBackend,
    pub labels: &'a dyn VlmAdapter,
    pub faces: &'a dyn TrackerAdapter,
}

/// Where in a video its frames are taken: a fifth, a half and four fifths
/// of the way through, clear of the camera coming up and going down; the
/// middle of anything shorter than three seconds.
fn frame_times(duration_us: u64) -> Vec<u64> {
    if duration_us < 3_000_000 {
        vec![duration_us / 2]
    } else {
        [0.2, 0.5, 0.8]
            .iter()
            .map(|f| (duration_us as f64 * f) as u64)
            .collect()
    }
}

/// Look at one video or photo: probe it, take its frames -- a photo is its
/// one picture -- label them and find the faces. Its stills are removed
/// afterwards; the faces cropped from them stay, for the album.
fn look_at(
    eyes: &Eyes,
    path: &Path,
    work: &Path,
    key: FileKey,
) -> (Entry, Vec<vb_infer::TrackedEntity>) {
    let mut entry = Entry {
        path: path.to_string_lossy().to_string(),
        size: key.size,
        modified: key.modified,
        looked: LOOK,
        at: vb_contracts::now_rfc3339(),
        ..Entry::default()
    };
    entry.file_time = file_time(path);
    entry.fingerprint = fingerprint(path, key.size);
    let info = match eyes.media.probe(path) {
        Ok(info) => info,
        Err(e) => {
            entry.error = Some(e.to_string());
            return (entry, Vec::new());
        }
    };
    entry.duration_us = info.duration.0;
    let (w, h) = info.display_size();
    (entry.width, entry.height) = (w, h);
    entry.captured_at = info.created_at.clone().filter(|t| !t.is_empty());
    entry.location = info.location;
    entry.look.still = info.still;
    let name = entry.name();
    // A screenshot is a picture of a screen, as a screen recording is a
    // video of one: whoever is on it is not one of the user's people.
    entry.look.screen = categories::named_as_screen_recording(&name)
        || (info.still && categories::named_as_screenshot(&name));
    if info.still {
        if w.min(h) < SMALLEST_SIDE {
            entry.error = Some("too small to be a photo".into());
            return (entry, Vec::new());
        }
    } else if info.duration.0 < SHORTEST {
        entry.error = Some("shorter than a second".into());
        return (entry, Vec::new());
    }

    let times = if info.still {
        vec![0]
    } else {
        frame_times(info.duration.0)
    };
    let stills_dir = work.join("frames").join(entry_dir(&entry));
    let mut windows: Vec<(String, TrackWindow)> = Vec::new();
    for (i, at) in times.into_iter().enumerate() {
        let dest = stills_dir.join(format!("f{i}.jpg"));
        match eyes
            .media
            .extract_still(path, vb_contracts::Micros(at), &dest)
        {
            Ok(still) => windows.push((
                format!("f{i}"),
                TrackWindow {
                    start: vb_contracts::Micros(at),
                    end: vb_contracts::Micros(at),
                    stills: vec![still],
                },
            )),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "a frame could not be read")
            }
        }
    }
    if windows.is_empty() {
        entry.error = Some("no frame could be read".into());
        let _ = std::fs::remove_dir_all(&stills_dir);
        return (entry, Vec::new());
    }

    if !eyes.labels.is_mock() {
        let mut frames = Vec::new();
        let mut ok = false;
        for (_, window) in &windows {
            match eyes.labels.labels(&VlmRequest::describe(&window.stills[0])) {
                Ok(mut labels) => {
                    labels.truncate(LABELS_KEPT);
                    for l in labels.iter_mut() {
                        l.confidence = (l.confidence * 1000.0).round() / 1000.0;
                    }
                    frames.push(labels);
                    ok = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "labelling a frame failed");
                    frames.push(Vec::new());
                }
            }
        }
        if ok {
            entry.look.labeller = Some(eyes.labels.name().to_string());
            entry.look.frames = frames;
        }
    }

    let mut found = Vec::new();
    if !eyes.faces.is_mock() {
        match eyes.faces.track(path, &windows) {
            Ok(entities) => {
                entry.look.tracker = Some(eyes.faces.name().to_string());
                entry.look.faces = entities
                    .iter()
                    .map(|e| SeenFace {
                        subject: None,
                        picture: e
                            .thumbnail
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        area: e.face_area.unwrap_or(0.0),
                        frames: e
                            .shot_ids
                            .iter()
                            .filter_map(|id| id.strip_prefix('f')?.parse().ok())
                            .collect(),
                    })
                    .collect();
                found = entities;
            }
            Err(e) => tracing::warn!(error = %e, "finding faces failed"),
        }
    }
    let _ = std::fs::remove_dir_all(&stills_dir);
    (entry, found)
}

/// Only the faces large enough to be a video's subjects are filed in the
/// album: a passer-by is counted in the frame, never named in a list of
/// people. Nor is anyone in a video recorded off a screen
/// ([`ClipLook::off_a_screen`]): a live stream's host, someone in a film
/// or on a call is someone on a screen, not one of the user's people --
/// and the face model is least sure of exactly the faces a family films
/// most. A Taobao live-stream host recorded off a phone in 2020 was filed
/// with a baby born five years later, 0.615 from one of the faces the
/// baby's group was kept by, under the 0.62 a face is matched at, and
/// opened the baby's growing-up memory. The others keep their place, with
/// nothing to be matched by.
fn subjects_only(
    mut faces: Vec<vb_infer::TrackedEntity>,
    off_a_screen: bool,
) -> Vec<vb_infer::TrackedEntity> {
    for face in faces.iter_mut() {
        if off_a_screen || face.face_area.unwrap_or(0.0) < categories::SUBJECT_FACE {
            face.signature.clear();
        }
    }
    faces
}

fn entry_dir(entry: &Entry) -> String {
    vb_contracts::Blake3Hash::of_str(&format!("{}|{}|{}", entry.path, entry.size, entry.modified))
        .to_hex()[..16]
        .to_string()
}

fn file_time(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let when = meta.created().or_else(|_| meta.modified()).ok()?;
    time::OffsetDateTime::from(when)
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

// ---------------------------------------------------------------------------
// A pass through every folder
// ---------------------------------------------------------------------------

/// What one pass found.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pass {
    pub found: usize,
    pub looked: usize,
    pub kept: usize,
    pub gone: usize,
    pub failed: usize,
}

/// Look through every folder: file what is new or changed, drop what is
/// gone, fold the faces into the album. Saves as it goes, so a stopped pass
/// keeps what it did, and the next one carries on from there. `report` gets
/// the share done (0 to 1), the file and how many of how many.
///
/// With `files`, they are the footage, in place of a walk through the
/// folders ([`given_footage`]): what is not among them leaves the
/// catalogue as a file gone from a folder does -- except what was filed
/// from a kept folder that cannot be read now, as with a walk.
pub fn pass(
    library: &Library,
    eyes: &Eyes,
    files: Option<&[PathBuf]>,
    observer: &dyn Observer,
    report: &(dyn Fn(f64, &str, usize, usize) + Sync),
) -> Result<Pass, String> {
    let settings = library.settings().hosting;
    let removed = settings.removed();
    let work = dir(library);
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;

    report(0.0, "", 0, 0);
    let found = match files {
        Some(files) => given_footage(files, library.root()),
        None => find_footage(&settings.roots(), library.root()),
    };
    let mut catalogue = load(library);
    let mut stats = Pass {
        found: found.len(),
        ..Pass::default()
    };

    // What is gone, or outside the folders now kept, leaves -- but not
    // what was filed from a folder that cannot be looked through now: a
    // drive left unplugged has not had its videos deleted, and looking at
    // them all again when it is back would take the time it took first.
    let here: HashSet<String> = found
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let away: Vec<PathBuf> = settings
        .roots()
        .into_iter()
        .filter(|root| std::fs::read_dir(root).is_err())
        .collect();
    let before = catalogue.entries.len();
    catalogue
        .entries
        .retain(|e| here.contains(&e.path) || under_any(&e.path, &away));
    stats.gone = before - catalogue.entries.len();

    let known: HashMap<String, usize> = catalogue
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.clone(), i))
        .collect();
    // Looked at again: what is new or changed, and what was looked at by
    // other eyes than the ones this device has now -- a face model
    // installed since files every face again.
    let labeller = (!eyes.labels.is_mock()).then(|| eyes.labels.name());
    let tracker = (!eyes.faces.is_mock()).then(|| eyes.faces.name());
    let current = |e: &Entry| {
        e.looked == LOOK
            && (e.error.is_some()
                || (e.look.labeller.as_deref() == labeller && e.look.tracker.as_deref() == tracker))
    };
    let mut todo: Vec<(PathBuf, FileKey)> = Vec::new();
    for path in &found {
        let Some(key) = file_key(path) else { continue };
        let name = path.to_string_lossy().to_string();
        match known.get(&name) {
            Some(&i) if catalogue.entries[i].file() == key && current(&catalogue.entries[i]) => {
                stats.kept += 1;
            }
            _ => todo.push((path.clone(), key)),
        }
    }
    // Filed again from what was seen when the rules or the removed
    // categories changed; nothing is decoded for it. Faces filed from a
    // video recorded off a screen before those were left out are taken out
    // of the album with it.
    let mut off_screen: HashSet<String> = HashSet::new();
    for entry in catalogue.entries.iter_mut() {
        entry.refile(&removed);
        if entry.unfile_screen_faces() {
            off_screen.insert(entry.path.clone());
        }
    }

    let mut album = entities::load(library);
    let paths: HashSet<String> = catalogue.entries.iter().map(|e| e.path.clone()).collect();
    entities::drop_hosted(&mut album, &|p| {
        paths.contains(p)
            && !off_screen.contains(p)
            && !todo.iter().any(|(t, _)| t.to_string_lossy() == p)
    });

    let total = todo.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let queue = Mutex::new(todo.into_iter());
    let shared = Mutex::new((catalogue, album, 0usize, HashSet::<String>::new()));
    let workers = crate::analysis_workers().min(total.max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                vb_apple::power::user_initiated_thread();
                loop {
                    if observer.cancelled() {
                        break;
                    }
                    let next = queue.lock().ok().and_then(|mut q| q.next());
                    let Some((path, key)) = next else { break };
                    let n = done.load(std::sync::atomic::Ordering::Relaxed);
                    report(
                        n as f64 / total.max(1) as f64,
                        &path
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        n,
                        total,
                    );
                    let (mut entry, faces) = look_at(eyes, &path, &work, key);
                    let faces = subjects_only(faces, entry.look.clip().off_a_screen());
                    let Ok(mut guard) = shared.lock() else { break };
                    let (catalogue, album, since_save, touched) = &mut *guard;
                    let filed = entities::fold_hosted(album, &entry.path, &faces);
                    touched.extend(filed.iter().flatten().cloned());
                    for (face, subject) in entry.look.faces.iter_mut().zip(filed) {
                        face.subject = subject;
                    }
                    entry.refile(&removed);
                    catalogue.entries.retain(|e| e.path != entry.path);
                    catalogue.entries.push(entry);
                    *since_save += 1;
                    if *since_save >= 20 {
                        *since_save = 0;
                        let _ = save(library, catalogue);
                        let _ = entities::save(library, album);
                    }
                    done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
    });
    let (mut catalogue, mut album, _, touched) = shared
        .into_inner()
        .map_err(|_| "the catalogue was poisoned".to_string())?;
    if !touched.is_empty() {
        join_one_face(&mut catalogue, &mut album, &touched);
    }
    stats.looked = done.into_inner();
    stats.failed = catalogue
        .entries
        .iter()
        .filter(|e| e.error.is_some())
        .count();
    catalogue.entries.sort_by(|a, b| a.path.cmp(&b.path));
    if !eyes.labels.is_mock() {
        catalogue.labeller = Some(eyes.labels.name().to_string());
    }
    if !eyes.faces.is_mock() {
        catalogue.tracker = Some(eyes.faces.name().to_string());
    }
    if !observer.cancelled() {
        catalogue.scanned_at = Some(vb_contracts::now_rfc3339());
    }
    entities::sort(&mut album);
    save(library, &catalogue)?;
    entities::save(library, &album)?;
    report(1.0, "", total, total);
    Ok(stats)
}

/// Join the groups of one face that this pass's sightings brought close
/// (`entities::consolidate`), and file their faces under the one kept.
fn join_one_face(
    catalogue: &mut Catalogue,
    album: &mut entities::Album,
    touched: &HashSet<String>,
) {
    let mut videos: HashMap<String, HashSet<String>> = HashMap::new();
    for entry in &catalogue.entries {
        for subject in entry.look.faces.iter().filter_map(|f| f.subject.as_ref()) {
            videos
                .entry(subject.clone())
                .or_default()
                .insert(entry.path.clone());
        }
    }
    let joined: HashMap<String, String> = entities::consolidate(album, &videos, touched)
        .into_iter()
        .collect();
    if joined.is_empty() {
        return;
    }
    tracing::info!(groups = joined.len(), "groups of one face joined");
    for face in catalogue
        .entries
        .iter_mut()
        .flat_map(|e| e.look.faces.iter_mut())
    {
        if let Some(into) = face.subject.as_ref().and_then(|s| joined.get(s)) {
            face.subject = Some(into.clone());
        }
    }
}

/// File the collection again under the removed categories now chosen.
pub fn refile(library: &Library) -> Result<(), String> {
    let removed = library.settings().hosting.removed();
    let mut catalogue = load(library);
    for entry in catalogue.entries.iter_mut() {
        entry.refile(&removed);
    }
    save(library, &catalogue)
}

/// The categories of a project's videos, for a cut ordered by them: from
/// the catalogue where the butler has looked at a video, else looked at now
/// and kept, so a project of videos outside its folders sorts as well.
pub fn categories_for(
    library: &Library,
    paths: &[PathBuf],
    eyes: Option<&Eyes>,
) -> HashMap<String, Vec<String>> {
    let removed = library.settings().hosting.removed();
    let mut catalogue = load(library);
    let mut out = HashMap::new();
    let mut changed = false;
    let mut album = None;
    for path in paths {
        let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.clone());
        let name = canonical.to_string_lossy().to_string();
        let key = file_key(&canonical);
        let current = catalogue
            .entries
            .iter()
            .find(|e| e.path == name && Some(e.file()) == key && e.looked == LOOK);
        let categories = match (current, eyes, key) {
            (Some(entry), _, _) => entry.categories.clone(),
            (None, Some(eyes), Some(key)) => {
                let (mut entry, faces) = look_at(eyes, &canonical, &dir(library), key);
                let faces = subjects_only(faces, entry.look.clip().off_a_screen());
                let album = album.get_or_insert_with(|| entities::load(library));
                let filed = entities::fold_hosted(album, &entry.path, &faces);
                for (face, subject) in entry.look.faces.iter_mut().zip(filed) {
                    face.subject = subject;
                }
                entry.refile(&removed);
                let categories = entry.categories.clone();
                catalogue.entries.retain(|e| e.path != entry.path);
                catalogue.entries.push(entry);
                changed = true;
                categories
            }
            _ => Vec::new(),
        };
        out.insert(name, categories.clone());
        out.insert(path.to_string_lossy().to_string(), categories);
    }
    if changed {
        let _ = save(library, &catalogue);
        if let Some(album) = album {
            let _ = entities::save(library, &album);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// What the app shows
// ---------------------------------------------------------------------------

/// A video or a photo as a listing carries it.
pub fn video_value(entry: &Entry, lang: Lang) -> Value {
    let place = entry.location.map(|p| {
        let at = vb_geo::world().locate(p.lat, p.lon);
        let zh = lang == Lang::Zh;
        let name = at
            .city
            .map(|c| c.name(zh).to_string())
            .or_else(|| at.region().map(|r| r.name(zh).to_string()));
        json!({ "lat": p.lat, "lon": p.lon, "name": name, "region": at.region().map(|r| r.id.clone()) })
    });
    json!({
        "path": entry.path,
        "name": entry.name(),
        "mediaKind": if entry.is_photo() { "image" } else { "video" },
        "capturedAt": entry.captured_at.clone().or_else(|| entry.file_time.clone()),
        "capturedFrom": if entry.captured_at.is_some() { "camera" } else { "file" },
        "durationUs": entry.duration_us,
        "width": entry.width,
        "height": entry.height,
        "place": place,
        "categories": entry.categories,
        "subjects": entry.subjects(),
    })
}

fn by_time(a: &&Entry, b: &&Entry) -> std::cmp::Ordering {
    let t = |e: &Entry| {
        e.captured_at
            .as_deref()
            .or(e.file_time.as_deref())
            .and_then(vb_contracts::time::capture_seconds)
    };
    t(b).cmp(&t(a)).then_with(|| a.path.cmp(&b.path))
}

/// The butler's page: the categories with their counts and a few videos
/// each, the people, what was removed, and what looked at the footage.
/// Every count of videos has its count of photos beside it.
pub fn overview(library: &Library, lang: Lang) -> Value {
    let settings = library.settings().hosting;
    let catalogue = load(library);
    let removed = settings.removed();
    let entries: Vec<&Entry> = catalogue
        .entries
        .iter()
        .filter(|e| e.error.is_none())
        .collect();
    let photos_of = |items: &[&Entry]| items.iter().filter(|e| e.is_photo()).count();

    let categories: Vec<Value> = Category::SMART
        .iter()
        .filter(|c| !removed.contains(c.id()))
        .map(|c| {
            let mut items: Vec<&Entry> = entries
                .iter()
                .copied()
                .filter(|e| e.is_in(c.id()))
                .collect();
            items.sort_by(by_time);
            let photos = photos_of(&items);
            json!({
                "id": c.id(),
                "name": if lang == Lang::Zh { c.zh() } else { c.en() },
                "count": items.len(),
                "videos": items.len() - photos,
                "photos": photos,
                "samples": items.iter().take(4).map(|e| e.path.clone()).collect::<Vec<_>>(),
                "unverified": c.unverified(),
            })
        })
        .collect();
    let removed_list: Vec<Value> = Category::SMART
        .iter()
        .filter(|c| removed.contains(c.id()))
        .map(|c| json!({ "id": c.id(), "name": if lang == Lang::Zh { c.zh() } else { c.en() } }))
        .collect();

    let album = entities::load(library);
    // Per subject: videos, photos, and the sightings they add up to.
    let mut counts: HashMap<&str, (usize, usize, HashSet<String>)> = HashMap::new();
    for entry in &entries {
        for subject in entry.subjects() {
            let count = counts.entry(subject).or_default();
            if entry.is_photo() {
                count.1 += 1;
            } else {
                count.0 += 1;
            }
            count.2.insert(entry.sighting());
        }
    }
    // Someone seen in one video only is not listed unless the user named
    // them: on 2,555 real videos 613 of 723 faces were, mostly strangers in
    // saved clips, drawn faces and blurs. The album keeps them all.
    let mut people: Vec<(usize, Value)> = album
        .subjects
        .iter()
        .filter_map(|s| {
            let (videos, photos, sightings) = counts.get(s.id.as_str())?;
            (sightings.len() >= PERSON_VIDEOS || s.label.is_some()).then(|| {
                (
                    videos + photos,
                    json!({
                        "id": s.id,
                        "label": s.label,
                        "thumbnail": s.thumbnail,
                        "videos": videos,
                        "photos": photos,
                    }),
                )
            })
        })
        .collect();
    people.sort_by_key(|p| std::cmp::Reverse(p.0));
    let people: Vec<Value> = people.into_iter().map(|(_, p)| p).collect();

    let labeller = catalogue.labeller.clone();
    let tracker = catalogue.tracker.clone();
    // Shown whatever else is: a folder nothing can be read from is the
    // reason a first pass finds nothing.
    let warnings: Vec<String> = settings
        .roots()
        .iter()
        .filter_map(|root| unreachable_note(root, lang))
        .collect();
    let mut notes = Vec::new();
    notes.push(match labeller.as_deref() {
        Some("AppleVision") => lang.t(
            "Categories come from Apple Vision's scene labels in three frames of each video \
             and in each photo, sorted by rules -- not a model trained to sort videos.",
            "分类依据：每个视频取三帧、每张照片看一次，用 Apple Vision 的场景标签按规则归类，不是专门训练的分类模型。",
        ),
        Some(_) => lang.t(
            "Categories come from SigLIP 2's scene labels in three frames of each video and \
             in each photo, sorted by rules -- not a model trained to sort videos.",
            "分类依据：每个视频取三帧、每张照片看一次，用 SigLIP 2 的场景标签按规则归类，不是专门训练的分类模型。",
        ),
        None => lang.t(
            "Nothing on this device labels pictures, so videos and photos are not sorted into \
             categories.",
            "这台设备上没有识别画面的模型，所以视频和照片没有按类别归类。",
        ),
    }.to_string());
    notes.push(
        match tracker {
            Some(_) => lang.t(
                "Faces are grouped by how alike they look, not recognised; small faces in the \
             background are passers-by and are not counted, and someone in only one video, or \
             in photos of one day, is not listed. Nobody in a screen recording or a screenshot \
             is filed as one of your people.",
                "人脸按长相相似程度分组，不是人脸识别；背景里很小的人脸算路人，不计入；只在一个视频或同一天的照片里出现的人不列出。录屏和截图里的人不会归为你身边的人。",
            ),
            None => lang.t(
                "Faces are not grouped on this device.",
                "这台设备上不做人脸分组。",
            ),
        }
        .to_string(),
    );
    notes.push(
        lang.t(
            "Study and live-stream clips have no label of their own: study is books, documents \
             and whiteboards in the picture, a live-stream clip a screen recording with someone \
             on it. Neither has been checked against real footage yet.",
            "「学习」和「直播切片」没有对应的标签：学习是画面里有书、文档、白板；直播切片是有人出镜的录屏。这两类还没有用真实素材校验过。",
        )
        .to_string(),
    );

    json!({
        "enabled": settings.enabled,
        "roots": settings.roots().iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "customRoots": !settings.roots.is_empty(),
        "defaultRoots": default_roots().iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        // The disks plugged in, for adding one in a tap.
        "volumes": crate::api::mounted_volumes()
            .into_iter()
            .map(|(name, path)| json!({ "name": name, "path": path.to_string_lossy() }))
            .collect::<Vec<_>>(),
        "videos": entries.len() - photos_of(&entries),
        "photos": photos_of(&entries),
        "unreadable": catalogue.entries.len() - entries.len(),
        "scannedAt": catalogue.scanned_at,
        "labeller": labeller,
        "tracker": catalogue.tracker,
        "categories": categories,
        "removed": removed_list,
        "people": people,
        "notes": notes,
        "warnings": warnings,
    })
}

/// The videos and photos in one category, newest first.
pub fn category(library: &Library, id: &str, lang: Lang) -> Result<Value, String> {
    let category = Category::parse(id)
        .filter(|c| *c != Category::People)
        .ok_or_else(|| format!("there is no category {id}"))?;
    let catalogue = load(library);
    let mut videos: Vec<&Entry> = catalogue
        .entries
        .iter()
        .filter(|e| e.error.is_none() && e.is_in(id))
        .collect();
    videos.sort_by(by_time);
    Ok(json!({
        "id": id,
        "name": if lang == Lang::Zh { category.zh() } else { category.en() },
        "unverified": category.unverified(),
        "videos": videos.iter().map(|e| video_value(e, lang)).collect::<Vec<_>>(),
    }))
}

/// The videos and photos a person is a subject of, newest first.
pub fn person(library: &Library, subject: &str, lang: Lang) -> Result<Value, String> {
    let album = entities::load(library);
    let found = album
        .subjects
        .iter()
        .find(|s| s.id == subject)
        .ok_or_else(|| format!("there is no subject {subject}"))?;
    let catalogue = load(library);
    let mut videos: Vec<&Entry> = catalogue
        .entries
        .iter()
        .filter(|e| e.error.is_none() && e.subjects().contains(&subject))
        .collect();
    videos.sort_by(by_time);
    Ok(json!({
        "id": found.id,
        "label": found.label,
        "thumbnail": found.thumbnail,
        "videos": videos.iter().map(|e| video_value(e, lang)).collect::<Vec<_>>(),
    }))
}

/// Every face filed under one subject, filed under another: after the user
/// merged the two. The memories are found again, since who is in them changed.
pub fn rename_subject(library: &Library, from: &str, into: &str) -> Result<(), String> {
    let mut catalogue = load(library);
    let mut changed = false;
    for face in catalogue
        .entries
        .iter_mut()
        .flat_map(|e| e.look.faces.iter_mut())
    {
        if face.subject.as_deref() == Some(from) {
            face.subject = Some(into.to_string());
            changed = true;
        }
    }
    if changed {
        save(library, &catalogue)?;
        crate::memories::refresh(library)?;
    }
    Ok(())
}

/// Remove a category, or bring one back: the collection is filed again at
/// once from what was seen.
pub fn set_removed(library: &Library, id: &str, removed: bool) -> Result<(), String> {
    let category = Category::parse(id)
        .filter(|c| *c != Category::People)
        .ok_or_else(|| format!("there is no category {id}"))?;
    let mut settings = library.settings();
    let list: BTreeSet<String> = settings
        .hosting
        .removed
        .iter()
        .cloned()
        .chain(removed.then(|| category.id().to_string()))
        .filter(|c| removed || c != category.id())
        .collect();
    settings.hosting.removed = list.into_iter().collect();
    library.save_settings(&settings)?;
    refile(library)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vb-hosting-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_walk_finds_videos_and_photos_once_and_skips_drafts_bundles_and_the_library() {
        let root = scratch("walk");
        let touch = |p: &str| {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
        };
        touch("trip/V1.mp4");
        touch("trip/V2.MOV");
        touch("trip/P1.HEIC");
        touch("trip/notes.txt");
        touch("trip/.hidden.mp4");
        touch("Photos Library.photoslibrary/originals/a.mov");
        touch("Photos Library.photoslibrary/originals/b.heic");
        touch("draft/draft_content.json");
        touch("draft/Resources/V1.mp4");
        touch("node_modules/x/demo.mp4");
        touch("node_modules/x/logo.png");
        touch("library/hosting/frames/x.mp4");
        touch("library/hosting/frames/f0.jpg");
        #[cfg(unix)]
        std::fs::hard_link(root.join("trip/V1.mp4"), root.join("trip/copy-of-V1.mp4")).unwrap();

        let found = find_footage(&[root.clone(), root.join("trip")], &root.join("library"));
        let names: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().to_string())
            .collect();
        // One of the hard-linked pair, whichever the directory listed first.
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().any(|n| n == "trip/V2.MOV"), "{names:?}");
        assert!(names.iter().any(|n| n == "trip/P1.HEIC"), "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_list_given_is_kept_as_a_walk_would_keep_it() {
        let root = scratch("given");
        let touch = |p: &str| {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
            path
        };
        let video = touch("disk/视频/V1.mp4");
        let photo = touch("disk/图片/P1.jpg");
        let notes = touch("disk/文件/notes.pdf");
        let hidden = touch("disk/图片/._P1.jpg");
        let ours = touch("library/hosting/frames/f0.jpg");
        let folder = root.join("disk/clip.mp4");
        std::fs::create_dir_all(&folder).unwrap();
        #[cfg(unix)]
        let linked = {
            let link = root.join("disk/图片/P1-again.jpg");
            std::fs::hard_link(&photo, &link).unwrap();
            link
        };
        #[cfg(not(unix))]
        let linked = photo.clone();

        let given = vec![
            photo.clone(),
            video.clone(),
            video.clone(),
            notes,
            hidden,
            ours,
            folder,
            root.join("disk/gone.mp4"),
            PathBuf::from("relative/V2.mp4"),
            linked,
        ];
        let kept = given_footage(&given, &root.join("library"));
        assert_eq!(kept, vec![photo, video], "sorted, each file once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_burst_of_photos_of_someone_is_one_sighting() {
        let library = Library::at(scratch("bursts"));
        let subject = |id: &str| entities::Subject {
            id: id.into(),
            label: None,
            kind: vb_contracts::footage_graph::EntityKind::Person,
            thumbnail: None,
            signature: vec![],
            exemplars: vec![],
            appearances: vec![],
            first_seen_at: String::new(),
            updated_at: String::new(),
        };
        let album = entities::Album {
            subjects: vec![
                subject("sub-0001"),
                subject("sub-0002"),
                subject("sub-0003"),
            ],
        };
        entities::save(&library, &album).unwrap();
        let shown = |path: &str, at: &str, subject: &str, still: bool| Entry {
            path: path.into(),
            captured_at: Some(at.into()),
            look: Look {
                tracker: Some("AuraFace".into()),
                still,
                faces: vec![SeenFace {
                    subject: Some(subject.into()),
                    area: 0.05,
                    frames: vec![0],
                    picture: None,
                }],
                ..Look::default()
            },
            ..Entry::default()
        };
        let mut entries: Vec<Entry> = (0..5)
            .map(|n| {
                shown(
                    &format!("/view/P{n}.jpg"),
                    "2023-10-01T12:00:0{n}+08:00",
                    "sub-0001",
                    true,
                )
            })
            .collect();
        entries.push(shown(
            "/a/P1.jpg",
            "2023-10-01T09:00:00+08:00",
            "sub-0002",
            true,
        ));
        entries.push(shown(
            "/a/P2.jpg",
            "2023-10-02T09:00:00+08:00",
            "sub-0002",
            true,
        ));
        entries.push(shown(
            "/b/V1.mp4",
            "2023-10-01T09:00:00+08:00",
            "sub-0003",
            false,
        ));
        entries.push(shown(
            "/b/P1.jpg",
            "2023-10-01T09:05:00+08:00",
            "sub-0003",
            true,
        ));
        save(
            &library,
            &Catalogue {
                entries,
                ..Catalogue::default()
            },
        )
        .unwrap();

        let page = overview(&library, Lang::Zh);
        let listed: Vec<(&str, u64, u64)> = page["people"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["id"].as_str().unwrap(),
                    p["videos"].as_u64().unwrap(),
                    p["photos"].as_u64().unwrap(),
                )
            })
            .collect();
        // Five photos of one moment are one sighting; photos of two days
        // are two, and so are a video and a photo.
        assert_eq!(listed, [("sub-0002", 0, 2), ("sub-0003", 1, 1)]);
        assert_eq!(page["videos"], 1);
        assert_eq!(page["photos"], 8);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn frames_are_taken_clear_of_the_ends() {
        assert_eq!(
            frame_times(10_000_000),
            vec![2_000_000, 5_000_000, 8_000_000]
        );
        assert_eq!(frame_times(2_000_000), vec![1_000_000]);
    }

    fn entry_with(labels: &[(&str, f64)]) -> Entry {
        Entry {
            path: "/v.mp4".into(),
            look: Look {
                labeller: Some("AppleVision".into()),
                tracker: Some("AppleVisionFaces".into()),
                frames: vec![
                    labels
                        .iter()
                        .map(|(l, c)| ScoredLabel {
                            label: (*l).into(),
                            confidence: *c,
                        })
                        .collect();
                    3
                ],
                faces: vec![],
                screen: false,
                still: false,
            },
            ..Entry::default()
        }
    }

    #[test]
    fn a_removed_category_is_dropped_and_comes_back_from_what_was_seen() {
        let mut entry = entry_with(&[("food", 0.9), ("sky", 0.8)]);
        entry.refile(&HashSet::new());
        assert_eq!(entry.categories, ["food", "scenery"]);
        entry.refile(&["food".to_string()].into_iter().collect());
        assert_eq!(entry.categories, ["scenery"]);
        entry.refile(&HashSet::new());
        assert_eq!(entry.categories, ["food", "scenery"]);
    }

    #[test]
    fn a_video_nothing_real_looked_at_is_filed_under_nothing() {
        let mut entry = entry_with(&[("food", 0.9)]);
        entry.look.labeller = None;
        entry.look.tracker = None;
        entry.refile(&HashSet::new());
        assert!(entry.categories.is_empty());
    }

    #[test]
    fn a_disk_not_plugged_in_keeps_what_was_filed_from_it_and_the_page_says_so() {
        let library = Library::at(scratch("unplugged"));
        let disk = PathBuf::from("/Volumes/VB-NOT-PLUGGED-IN");
        let mut settings = library.settings();
        settings.hosting.roots = vec![disk.to_string_lossy().to_string()];
        library.save_settings(&settings).unwrap();

        // What the pass keeps of the catalogue while the disk is away.
        assert!(under_any(
            "/Volumes/VB-NOT-PLUGGED-IN/2023/a.mp4",
            std::slice::from_ref(&disk)
        ));
        assert!(!under_any(
            "/Volumes/VB-NOT-PLUGGED-IN-2/a.mp4",
            std::slice::from_ref(&disk)
        ));

        let page = overview(&library, Lang::Zh);
        let warnings: Vec<&str> = page["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w.as_str().unwrap())
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("「VB-NOT-PLUGGED-IN」现在没有接上"),
            "{warnings:?}"
        );
        assert!(page["volumes"].is_array());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn someone_in_one_video_is_listed_only_once_named() {
        let library = Library::at(scratch("people"));
        let subject = |id: &str, label: Option<&str>| entities::Subject {
            id: id.into(),
            label: label.map(str::to_string),
            kind: vb_contracts::footage_graph::EntityKind::Person,
            thumbnail: None,
            signature: vec![],
            exemplars: vec![],
            appearances: vec![],
            first_seen_at: String::new(),
            updated_at: String::new(),
        };
        let album = entities::Album {
            subjects: vec![
                subject("sub-0001", None),
                subject("sub-0002", None),
                subject("sub-0003", Some("外婆")),
            ],
        };
        entities::save(&library, &album).unwrap();
        let in_video = |path: &str, subjects: &[&str]| Entry {
            path: path.into(),
            look: Look {
                tracker: Some("AuraFace".into()),
                faces: subjects
                    .iter()
                    .map(|s| SeenFace {
                        subject: Some((*s).into()),
                        area: 0.05,
                        frames: vec![0],
                        picture: None,
                    })
                    .collect(),
                ..Look::default()
            },
            ..Entry::default()
        };
        let catalogue = Catalogue {
            entries: vec![
                in_video("/a.mp4", &["sub-0001", "sub-0002"]),
                in_video("/b.mp4", &["sub-0001"]),
                in_video("/c.mp4", &["sub-0003"]),
            ],
            ..Catalogue::default()
        };
        save(&library, &catalogue).unwrap();
        let listed: Vec<String> = overview(&library, Lang::Zh)["people"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(listed, ["sub-0001", "sub-0003"]);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn faces_recorded_off_a_screen_are_counted_and_never_filed() {
        // A live stream recorded off a phone: Vision reads its frames as a
        // screenshot, and its host's face had been filed with a baby.
        let mut entry = entry_with(&[("screenshot", 0.94), ("people", 0.85), ("adult", 0.85)]);
        entry.look.faces = vec![SeenFace {
            subject: Some("sub-0054".into()),
            area: 0.0052,
            frames: vec![1],
            picture: None,
        }];
        assert!(entry.unfile_screen_faces());
        assert!(entry.subjects().is_empty());
        assert!(!entry.unfile_screen_faces(), "nothing left to take out");
        // Still a live-stream clip with someone in it.
        entry.refile(&HashSet::new());
        assert!(
            entry.is_in("livestream") && entry.is_in("people"),
            "{:?}",
            entry.categories
        );

        // Looked at afresh, its faces never reach the album.
        let face = |area: f64| vb_infer::TrackedEntity {
            id: "f".into(),
            kind: vb_contracts::footage_graph::EntityKind::Person,
            label: None,
            first_seen: vb_contracts::Micros::ZERO,
            last_seen: vb_contracts::Micros::ZERO,
            shot_ids: vec!["f1".into()],
            confidence: None,
            thumbnail: None,
            signature: vec![0.1; 512],
            face_area: Some(area),
        };
        assert!(
            subjects_only(vec![face(0.05)], true)[0]
                .signature
                .is_empty()
        );
        assert_eq!(
            subjects_only(vec![face(0.05)], false)[0].signature.len(),
            512
        );
        assert!(
            subjects_only(vec![face(0.001)], false)[0]
                .signature
                .is_empty()
        );

        // A video filmed with a camera keeps its people.
        let mut filmed = entry_with(&[("people", 0.9), ("baby", 0.8)]);
        filmed.look.faces = vec![SeenFace {
            subject: Some("sub-0054".into()),
            area: 0.05,
            frames: vec![0],
            picture: None,
        }];
        assert!(!filmed.unfile_screen_faces());
        assert_eq!(filmed.subjects(), ["sub-0054"]);
    }

    #[test]
    fn only_faces_large_enough_are_a_videos_subjects() {
        let mut entry = entry_with(&[]);
        entry.look.faces = vec![
            SeenFace {
                subject: Some("sub-0002".into()),
                area: 0.05,
                frames: vec![0],
                picture: None,
            },
            SeenFace {
                subject: Some("sub-0001".into()),
                area: 0.0005,
                frames: vec![1],
                picture: None,
            },
        ];
        assert_eq!(entry.subjects(), ["sub-0002"]);
    }
}
