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

//! The operation set every front end calls.
//!
//! The CLI, the C ABI and the local HTTP server all go through here, so there
//! is one implementation of "what the app can do" rather than three that drift
//! apart. Everything returns `serde_json::Value`, because every transport out
//! of this crate is JSON anyway.
//!
//! Front ends are expected to differ in *transport*, not in behaviour. If a
//! capability exists on one and not another, it belongs here first.

use crate::library::Library;
use crate::models;
use crate::progress::Observer;
use crate::projects::{ModelChoice, Project, StyleChoice};
use crate::{DirectorModels, Lang, Pipeline, RunOptions, Workspace, entities, jobs};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use vb_export::Editor;
use vb_infer::{
    AsrAdapter, DepthAdapter, FaceDetector, MockAsr, MockDepth, MockFaces, MockPeople, MockPose,
    MockTracker, MockVlm, PersonSegmenter, PoseEstimator, TrackerAdapter, VlmAdapter,
};
use vb_media::MediaBackend;

/// File extensions the browser offers as video to edit. Photographs are
/// footage too; their extensions are `vb_media::STILL_EXTENSIONS`. `3gp` is
/// what phones recorded into before MP4, and is MP4 inside.
const VIDEO_EXTENSIONS: [&str; 9] = [
    "mp4", "mov", "m4v", "mkv", "avi", "webm", "mts", "m2ts", "3gp",
];

/// File extensions the browser offers as music to cut to.
const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff"];

/// What a listing is looking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseKind {
    Video,
    Audio,
}

impl BrowseKind {
    pub fn parse(kind: Option<&str>) -> BrowseKind {
        match kind {
            Some("audio") => BrowseKind::Audio,
            _ => BrowseKind::Video,
        }
    }

    /// What a file is to this listing, or `None` when it is not offered:
    /// footage is a video or a photo, music is audio.
    fn kind_of(self, path: &Path) -> Option<&'static str> {
        match self {
            BrowseKind::Video => footage_kind(path),
            BrowseKind::Audio => path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                .then_some("audio"),
        }
    }
}

/// Engine version and the contract versions it speaks.
pub fn version() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "footageGraph": vb_contracts::footage_graph::SCHEMA_VERSION,
        "editingDsl": vb_contracts::editing_dsl::SCHEMA_VERSION,
    })
}

/// What is available, without running anything.
///
/// `mediaAvailable` is the one that decides whether a real run is possible.
/// With linked libav compiled in it is always true. With the CLI backend it
/// means the ffmpeg binary is on PATH, which a mobile platform can never
/// satisfy, since it cannot spawn a process.
///
/// `mediaShippable` answers the licensing question for the ffmpeg underneath:
/// only an LGPL build can ship. It is null when the license is unknown.
pub fn doctor() -> Value {
    let media = vb_media::default_backend();
    let available = media.is_available();
    let license = media.license();

    let library = Library::open();
    json!({
        "mediaBackend": media.name(),
        "mediaAvailable": available,
        "mediaVersion": media.version(),
        "mediaLicense": license,
        "mediaShippable": license.as_deref().map(vb_media::is_shippable_license),
        "mediaNote": media_note(media.as_ref(), available),
        // What a local run would use on this machine now, not a fixed list:
        // on a Mac, Vision is real from the start and speech becomes real the
        // moment a model is downloaded, and the interface has to say which.
        "adapters": adapter_report(&library, ModelChoice::Local),
        "requiresNetwork": false,
        "canAnalyse": available,
        "editors": editors(),
        "library": library_report(&library),
        "vision": vision_report(&library),
    })
}

/// The editors that can take a draft, and whether they can take one now.
pub fn editors() -> Value {
    json!(
        vb_export::detect_editors()
            .into_iter()
            .map(|install| json!({
                "id": install.editor.id(),
                "name": editor_name(&install),
                "installed": install.is_installed(),
                // A phone app: installed or not, it takes no draft from here.
                "phone": install.phone,
                "appPath": install.app_path,
                "version": install.version,
                "draftRoot": install.draft_root,
                "canExport": install.can_export(),
                // A draft written while the editor is open appears only after
                // it restarts. Users read that as the export having failed,
                // so it is said before the export rather than after.
                "running": install.running,
            }))
            .collect::<Vec<_>>()
    )
}

/// What the user calls the editor where they have it: 剪映专业版 on a
/// desktop, 剪映 on a phone.
fn editor_name(install: &vb_export::EditorInstall) -> &'static str {
    if install.phone {
        install.editor.phone_name()
    } else {
        install.editor.display_name()
    }
}

fn library_report(library: &Library) -> Value {
    let writable = library.writable();
    json!({
        "path": library.root(),
        "writable": writable.is_ok(),
        "error": writable.err(),
        "models": models::listing(library)["models"],
        "cloudConfigured": library.settings().cloud.is_configured(),
    })
}

/// On-device perception that needs nothing downloaded.
fn vision_report(library: &Library) -> Value {
    if vb_apple::is_available() {
        return json!({
            "available": true,
            "name": "Apple Vision",
            "provides": ["scene labels", "faces"],
            "note": "Scene labels and faces are read on this device, with nothing to download. \
                     Faces are grouped by how alike they look, which is not recognition.",
        });
    }
    let labels = models::installed_for(library, models::Role::Labels);
    json!({
        "available": labels.is_some(),
        "name": labels.map_or("none", |m| m.name),
        "provides": if labels.is_some() { vec!["scene labels"] } else { vec![] },
        "note": if labels.is_some() {
            "Scene labels are read on this device by SigLIP 2. There is no face grouping \
             on this platform."
        } else {
            "This platform has no built-in vision framework, so scene labels need the \
             SigLIP 2 model, or a cloud model."
        },
    })
}

/// One sentence on the media backend's state, written for the app to show.
fn media_note(media: &dyn MediaBackend, available: bool) -> &'static str {
    match (available, media.spawns_processes()) {
        (true, false) => "Linked libav is built in; video is analysed in-process.",
        (true, true) => "ffmpeg found on PATH.",
        (false, _) if cfg!(target_os = "android") || cfg!(target_os = "ios") => {
            "This platform cannot spawn an ffmpeg process, and this build was \
             made without the linked-libav backend that would replace it."
        }
        (false, _) => "ffmpeg was not found on PATH. Install it to analyse video.",
    }
}

/// Every template in the prior store.
pub fn templates() -> Value {
    json!(crate::templates())
}

/// List directories and video files under `dir`.
///
/// The engine owns this rather than the UI because the UI runs in a browser
/// on some platforms, where it cannot see the filesystem at all. Keeping it
/// here means one file picker works everywhere.
pub fn browse(dir: Option<&str>) -> Value {
    browse_for(dir, BrowseKind::Video)
}

/// List directories and files of one kind under `dir`: footage, or music to
/// cut to.
pub fn browse_for(dir: Option<&str>, kind: BrowseKind) -> Value {
    let path = match dir {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        // Music is kept in Music, and a picker that opens there saves the
        // user a click every time.
        _ if kind == BrowseKind::Audio && home_dir().join("Music").is_dir() => {
            home_dir().join("Music")
        }
        _ => home_dir(),
    };

    let path = dunce::canonicalize(&path).unwrap_or(path);
    if !path.is_dir() {
        return json!({
            "path": path.to_string_lossy(),
            "error": format!("{} is not a directory", path.display()),
            "parent": path.parent().map(|p| p.to_string_lossy().to_string()),
            "entries": [],
        });
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();

    // A directory that cannot be read is not an empty directory. Saying so is
    // the difference between "your videos are elsewhere" and "this app has no
    // permission to look here", which is the usual cause on Android.
    let read = match std::fs::read_dir(&path) {
        Ok(read) => read,
        Err(e) => {
            return json!({
                "path": path.to_string_lossy(),
                "error": unreadable(&path, &e, Library::open().settings().lang()),
                "parent": path.parent().map(|p| p.to_string_lossy().to_string()),
                "entries": [],
                "places": places_for(kind),
            });
        }
    };

    {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Hidden entries are noise in a file picker, and on macOS most of
            // them are not things a user would ever choose.
            if name.starts_with('.') {
                continue;
            }
            let entry_path = entry.path();
            if entry_path.is_dir() {
                directories.push(json!({
                    "name": name,
                    "path": entry_path.to_string_lossy(),
                    "kind": "dir",
                }));
            } else if let Some(file_kind) = kind.kind_of(&entry_path) {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push(json!({
                    "name": name,
                    "path": entry_path.to_string_lossy(),
                    "kind": file_kind,
                    "sizeBytes": size,
                }));
            }
        }
    }

    directories.sort_by_key(sort_key);
    files.sort_by_key(sort_key);
    directories.extend(files);

    json!({
        "path": path.to_string_lossy(),
        "parent": path.parent().map(|p| p.to_string_lossy().to_string()),
        "entries": directories,
        "places": places_for(kind),
    })
}

fn places_for(kind: BrowseKind) -> Vec<Value> {
    let android = cfg!(target_os = "android");
    match kind {
        BrowseKind::Audio => music_places(&home_dir(), android),
        BrowseKind::Video => footage_places(&home_dir(), android, &mounted_volumes()),
    }
}

/// Why a folder cannot be listed, as the user can act on it. On a Mac that
/// is usually privacy: a disk plugged in, the desktop, documents and
/// downloads each need the user's leave, and macOS says no without asking
/// again once the prompt was dismissed. Its refusal is EPERM, "operation not
/// permitted"; a folder's own permissions refuse with EACCES.
pub(crate) fn unreadable(path: &Path, error: &std::io::Error, lang: Lang) -> String {
    const EPERM: i32 = 1;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    if cfg!(target_os = "macos") && error.raw_os_error() == Some(EPERM) {
        return match lang {
            Lang::Zh => format!(
                "macOS 没有允许视频管家读取「{name}」。到「系统设置 → 隐私与安全性 → 文件与文件夹」，\
                 在 Video Butler 下打开对应的开关（移动硬盘是「可移除的宗卷」），\
                 或者把它加进「完全磁盘访问权限」，再回来打开这个文件夹。"
            ),
            Lang::En => format!(
                "macOS has not let Video Butler read {name}. In System Settings > Privacy & \
                 Security > Files and Folders, turn on the switch under Video Butler (Removable \
                 Volumes, for an external disk), or add it to Full Disk Access, then open this \
                 folder again."
            ),
        };
    }
    match lang {
        Lang::Zh => format!("无法读取 {}：{error}", path.display()),
        Lang::En => format!("cannot read {}: {error}", path.display()),
    }
}

/// The disks mounted beside the system's own -- an external drive, an SD
/// card, a network share -- by name. Nothing else tells a user where a
/// plugged-in drive is: on a Mac it is `/Volumes`, three levels up from home
/// among the system's folders, which is how a drive full of videos could not
/// be found to hand to the video butler.
pub fn mounted_volumes() -> Vec<(String, PathBuf)> {
    if cfg!(target_os = "macos") {
        volumes_in(Path::new("/Volumes"))
    } else if cfg!(target_os = "android") {
        // Shared storage is `emulated`; the rest is a card or a USB disk.
        volumes_in(Path::new("/storage"))
            .into_iter()
            .filter(|(name, _)| name != "emulated" && name != "self")
            .collect()
    } else if cfg!(target_os = "windows") {
        ('A'..='Z')
            .map(|letter| (format!("{letter}:"), PathBuf::from(format!("{letter}:\\"))))
            .filter(|(_, root)| root.is_dir())
            .collect()
    } else if cfg!(target_os = "linux") {
        let user = std::env::var("USER").unwrap_or_default();
        [
            format!("/media/{user}"),
            format!("/run/media/{user}"),
            "/mnt".to_string(),
        ]
        .iter()
        .flat_map(|base| volumes_in(Path::new(base)))
        .collect()
    } else {
        Vec::new()
    }
}

/// The volumes mounted in `dir`: its folders, but not hidden ones nor the
/// system's own -- a Mac's startup disk is there as a link back to `/`, and
/// Time Machine's snapshots as `com.apple.…`.
fn volumes_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut volumes: Vec<(String, PathBuf)> = read
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = entry.file_type().ok()?;
            let system = name.starts_with('.') || name.starts_with("com.apple.");
            (!system && kind.is_dir() && !kind.is_symlink()).then(|| (name, entry.path()))
        })
        .collect();
    volumes.sort();
    volumes
}

/// Where footage usually is, for a row of shortcuts above the footage
/// picker: the usual folders that exist here, then every disk mounted, by
/// its name.
fn footage_places(home: &Path, android: bool, volumes: &[(String, PathBuf)]) -> Vec<Value> {
    let candidates: &[(&str, &str)] = if android {
        &[
            ("dcim", "DCIM"),
            ("movies", "Movies"),
            ("pictures", "Pictures"),
            ("downloads", "Download"),
        ]
    } else {
        &[
            ("home", ""),
            ("desktop", "Desktop"),
            ("documents", "Documents"),
            ("movies", "Movies"),
            ("pictures", "Pictures"),
            ("downloads", "Downloads"),
        ]
    };
    candidates
        .iter()
        // Home itself, not home joined with "", which ends in a slash and
        // never matches the folder listed.
        .map(|(id, rel)| {
            let dir = if rel.is_empty() {
                home.to_path_buf()
            } else {
                home.join(rel)
            };
            (id, dir)
        })
        .filter(|(_, dir)| dir.is_dir())
        .map(|(id, dir)| json!({ "id": id, "path": dir.to_string_lossy() }))
        .chain(volumes.iter().map(
            |(name, path)| json!({ "id": "volume", "name": name, "path": path.to_string_lossy() }),
        ))
        .collect()
}

/// Where music usually is, for a row of shortcuts above the music picker:
/// only the folders that exist here. On a phone that includes where the
/// music apps put what they download, which few people could find by
/// walking the folders. By id, so the app words each in its language.
fn music_places(home: &Path, android: bool) -> Vec<Value> {
    let candidates: &[(&str, &str)] = if android {
        &[
            ("music", "Music"),
            ("downloads", "Download"),
            ("netease", "netease/cloudmusic/Music"),
            ("qqmusic", "qqmusic/song"),
            ("kugou", "kgmusic/download"),
            ("kuwo", "KuwoMusic/music"),
        ]
    } else {
        &[("music", "Music"), ("downloads", "Downloads")]
    };
    candidates
        .iter()
        .map(|(id, rel)| (id, home.join(rel)))
        .filter(|(_, dir)| dir.is_dir())
        .map(|(id, dir)| json!({ "id": id, "path": dir.to_string_lossy() }))
        .collect()
}

/// Case-insensitive name, so a picker reads alphabetically regardless of
/// how each file happens to be capitalised.
fn sort_key(v: &Value) -> String {
    v["name"].as_str().unwrap_or_default().to_lowercase()
}

/// `video` or `image` for a file the pipeline reads as footage, by its name.
/// Is this file a video the pipeline reads, by its name?
pub(crate) fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn footage_kind(path: &Path) -> Option<&'static str> {
    if vb_media::is_still_path(path) {
        return Some("image");
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .then_some("video")
}

/// Where the file picker opens.
///
/// `$HOME` on desktop. Android sets no HOME and forbids an app from reading
/// the filesystem root, so falling through to `/` there opens the picker on an
/// empty listing; shared storage is where a user's footage actually is.
fn home_dir() -> PathBuf {
    if cfg!(target_os = "android") {
        return PathBuf::from("/storage/emulated/0");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Where a run writes when the caller names no project directory.
///
/// Beside the footage, which is what a desktop user expects. `VB_WORKDIR`
/// overrides it for hosts whose footage lives somewhere they may not write:
/// on Android the picker browses shared storage, where an app can read a
/// video but cannot create a project folder next to it, so the app sets this
/// to a directory it owns.
fn default_project_dir(input: &Path, workdir: Option<PathBuf>) -> PathBuf {
    let base = workdir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf());
    base.join("vb-project")
}

/// Every video and photo a caller named, with any directory among them
/// expanded.
///
/// A project is often a folder, and asking a user to pick forty clips one by
/// one is not a feature. Directories are walked recursively; anything that is
/// not footage the pipeline reads is skipped, as are hidden entries and the
/// project's own output, so re-running over a folder does not try to analyse
/// the stills and drafts of the last run.
///
/// Order is capture time where the filesystem knows it, then path. A holiday
/// told in the order a directory listing happened to come back in is not the
/// same story.
pub fn expand_inputs(inputs: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut found: Vec<PathBuf> = Vec::new();
    for raw in inputs {
        let path = PathBuf::from(raw);
        if !path.exists() {
            return Err(format!("{raw} does not exist"));
        }
        if path.is_dir() {
            collect_footage(&path, &mut found);
        } else {
            found.push(path);
        }
    }

    found.sort();
    found.dedup();
    found.sort_by_key(|path| {
        let taken = std::fs::metadata(path)
            .and_then(|m| m.created())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        (taken, path.clone())
    });
    Ok(found)
}

// ---------------------------------------------------------------------------
// What each clip is, for choosing among them
// ---------------------------------------------------------------------------

/// When each video or photo was taken, how it is framed, and how long a
/// video runs.
///
/// For the picker, which lets a user choose a trip's clips by day, by shape
/// and by kind. Probed on several threads at once, because a folder is usually a
/// hundred clips and a picker that sits still for seconds reads as broken;
/// a clip that cannot be probed says so instead of vanishing from the list.
pub fn media_inspect(args: &Value) -> Result<Value, String> {
    let paths: Vec<String> = args["paths"]
        .as_array()
        .ok_or("which files?")?
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect();
    let media = vb_media::default_backend();
    if !media.is_available() {
        return Err("ffmpeg was not found, so this footage cannot be read".to_string());
    }
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let chunk = paths.len().div_ceil(workers).max(1);
    let mut items: Vec<Value> = Vec::with_capacity(paths.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|part| {
                let media = media.as_ref();
                scope.spawn(move || {
                    part.iter()
                        .map(|path| inspect_one(media, Path::new(path)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            items.extend(handle.join().unwrap_or_default());
        }
    });
    Ok(json!({ "items": items }))
}

/// A small upright picture of each video or photo, for the footage picker
/// to show before anything is chosen: from a second in (the middle of a clip
/// shorter than two; a photo is its one picture), `width` wide, PNG in
/// base64. Made on several threads, and kept
/// in the library by the file's size and time, so a folder opened again
/// shows at once.
pub fn media_thumbnails(args: &Value) -> Result<Value, String> {
    let paths: Vec<String> = args["paths"]
        .as_array()
        .ok_or("which files?")?
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect();
    let width = args["width"].as_u64().unwrap_or(200).clamp(64, 640) as u32;
    let media = vb_media::default_backend();
    if !media.is_available() {
        return Err("ffmpeg was not found, so this footage cannot be read".to_string());
    }
    let dir = Library::open().cache_dir().join("thumbs");
    let _ = std::fs::create_dir_all(&dir);
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let chunk = paths.len().div_ceil(workers).max(1);
    let mut items: Vec<Value> = Vec::with_capacity(paths.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|part| {
                let media = media.as_ref();
                let dir = dir.as_path();
                scope.spawn(move || {
                    part.iter()
                        .map(|path| thumbnail_one(media, Path::new(path), width, dir))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            items.extend(handle.join().unwrap_or_default());
        }
    });
    Ok(json!({ "items": items }))
}

fn thumbnail_one(media: &dyn MediaBackend, path: &Path, width: u32, dir: &Path) -> Value {
    use base64::Engine as _;
    let failed = |e: String| json!({ "path": path.to_string_lossy(), "error": e });
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) => return failed(e.to_string()),
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_micros());
    let key = vb_contracts::Blake3Hash::of_str(&format!(
        "thumbnail/1|{}|{}|{modified}|{width}",
        path.to_string_lossy(),
        meta.len()
    ));
    let cached = dir.join(format!("{}.png", key.to_hex()));
    let png = match std::fs::read(&cached) {
        Ok(bytes) => bytes,
        Err(_) => {
            let made = (|| -> Result<Vec<u8>, String> {
                let info = media.probe(path).map_err(|e| e.to_string())?;
                let (w, h) = info.display_size();
                if w == 0 || h == 0 {
                    return Err("no picture".into());
                }
                let height = ((width as u64 * h as u64 / w as u64) as u32).max(2) & !1;
                let at = if info.duration >= vb_contracts::Micros::from_secs(2) {
                    vb_contracts::Micros::from_secs(1)
                } else {
                    vb_contracts::Micros(info.duration.0 / 2)
                };
                let frame = media
                    .rgb_frame(path, at, width, height)
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.set_compression(png::Compression::Fast);
                let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
                writer
                    .write_image_data(&frame.rgb)
                    .map_err(|e| e.to_string())?;
                writer.finish().map_err(|e| e.to_string())?;
                Ok(out)
            })();
            match made {
                Ok(bytes) => {
                    let part = cached.with_extension("part");
                    if std::fs::write(&part, &bytes).is_ok() {
                        let _ = std::fs::rename(&part, &cached);
                    }
                    bytes
                }
                Err(e) => return failed(e),
            }
        }
    };
    json!({
        "path": path.to_string_lossy(),
        "png": base64::engine::general_purpose::STANDARD.encode(&png),
    })
}

fn inspect_one(media: &dyn MediaBackend, path: &Path) -> Value {
    let info = match media.probe(path) {
        Ok(info) => info,
        Err(e) => return json!({ "path": path.to_string_lossy(), "error": e.to_string() }),
    };
    let (width, height) = info.display_size();
    // The camera's own record of when it filmed -- a video's creation time,
    // a photo's EXIF -- which survives copying; the file's date is the
    // fallback, and is marked as one, because a copy made at home a week
    // later carries the day of the copy.
    let (captured_at, captured_from) = match info.created_at.as_deref().filter(|t| !t.is_empty()) {
        Some(tag) => (Some(tag.to_string()), "camera"),
        None => (file_time(path), "file"),
    };
    json!({
        "path": path.to_string_lossy(),
        "kind": if info.still { "image" } else { "video" },
        "capturedAt": captured_at,
        "capturedFrom": captured_from,
        "durationUs": info.duration.0,
        "width": width,
        "height": height,
        "orientation": orientation(width, height),
        "ratio": ratio_label(width, height),
    })
}

/// When the file was made, or last written when the filesystem keeps no
/// birth time, as RFC 3339.
fn file_time(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let when = meta.created().or_else(|_| meta.modified()).ok()?;
    time::OffsetDateTime::from(when)
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

/// Landscape, portrait or square, as the picture is shown. Square within a
/// tenth either way, so a 1080 x 1080 crop and a 1080 x 1100 one agree.
fn orientation(width: u32, height: u32) -> &'static str {
    if width == 0 || height == 0 {
        return "unknown";
    }
    let r = width as f64 / height as f64;
    if r > 1.1 {
        "landscape"
    } else if r < 1.0 / 1.1 {
        "portrait"
    } else {
        "square"
    }
}

/// The frame's shape as people name it: the nearest common ratio within 3%,
/// or the reduced fraction when it is none of them.
fn ratio_label(width: u32, height: u32) -> String {
    if width == 0 || height == 0 {
        return "?".to_string();
    }
    const COMMON: [(u32, u32); 11] = [
        (16, 9),
        (9, 16),
        (4, 3),
        (3, 4),
        (1, 1),
        (3, 2),
        (2, 3),
        (21, 9),
        (9, 21),
        (2, 1),
        (1, 2),
    ];
    let r = width as f64 / height as f64;
    if let Some((w, h)) = COMMON
        .iter()
        .find(|(w, h)| (r / (*w as f64 / *h as f64) - 1.0).abs() < 0.03)
    {
        return format!("{w}:{h}");
    }
    let (mut a, mut b) = (width, height);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    format!("{}:{}", width / a, height / a)
}

/// Directories a run writes into, which must never be read back as footage.
const OUTPUT_DIRS: [&str; 3] = ["_vbcache", "_vbwork", "vb-project"];

fn collect_footage(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if !OUTPUT_DIRS.contains(&name.as_str()) {
                collect_footage(&path, out);
            }
        } else if footage_kind(&path).is_some() {
            out.push(path);
        }
    }
}

/// Analyse a project's videos and photos and write one draft across all of
/// them.
///
/// Returns the shot list alongside the summary, so a front end can show what
/// was found and why each clip was chosen without reading the graph file back
/// off disk.
pub fn run(
    inputs: &[String],
    project_dir: Option<&str>,
    template: Option<&str>,
) -> Result<Value, String> {
    let library = Library::open();
    let request = RunRequest {
        project: None,
        inputs: inputs.to_vec(),
        project_dir: project_dir.map(str::to_string),
        template_id: template.map(str::to_string),
        style: StyleChoice::default(),
        model: ModelChoice::Local,
        editors: Vec::new(),
        title: None,
        fresh: false,
    };
    run_with(&library, request, &crate::Silent)
}

/// A run's result, as every front end receives it.
///
/// Returns the shot list alongside the summary, so a front end can show what
/// was found and why each clip was chosen without reading the graph file back
/// off disk.
/// The cut as a phone editor can take it. 剪映 on a phone accepts no draft
/// from another app, but opens videos shared with it as a new project, in
/// the order given and whole (checked on a real phone). So the cut goes as
/// its clips, each with the span the host trims it to -- the same in point
/// and length the desktop draft uses -- and what cannot go with them is
/// counted, so the user is told rather than left to find out.
fn handoff(result: &crate::RunResult) -> Value {
    use vb_contracts::editing_dsl::{AudioKind, SlotKind, TransitionType};
    let sources: std::collections::HashMap<&str, &vb_contracts::Source> = result
        .graph
        .sources
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    let slots = result.dsl.ordered_slots();
    let clips: Vec<Value> = slots
        .iter()
        .filter(|slot| slot.kind == SlotKind::Clip)
        .filter_map(|slot| {
            let source = slot.source_ref.as_ref()?;
            let file = sources.get(source.source_id.as_str())?;
            let (width, height) = file.display_size().unzip();
            Some(json!({
                "path": file.path,
                "inUs": source.in_us.0,
                "outUs": source.in_us.0 + slot.duration_us.0,
                "width": width,
                "height": height,
                // A photo: the host makes a clip of it that holds the
                // picture for the span, since the editor takes videos.
                "still": file.is_still(),
            }))
        })
        .collect();
    let music = result
        .dsl
        .tracks
        .audio
        .iter()
        .find(|a| a.kind == AudioKind::Bgm && a.asset_path.is_some());
    // Where the music is when the first clip handed over starts: a title
    // card before it is left behind, so the music the user adds in the
    // editor starts that much further in, and the cuts stay on its beats.
    let timeline = result.dsl.resolve_timeline();
    let first_clip = slots
        .iter()
        .find(|s| s.kind == SlotKind::Clip && s.source_ref.is_some())
        .and_then(|s| timeline.slot(&s.id))
        .map(|r| r.start_us.0);
    let music_in = music.and_then(|m| {
        let starts = timeline.resolve_anchor(&m.anchor)?.0;
        let into = first_clip?.saturating_sub(starts);
        Some(m.asset_in_us.map_or(0, |a| a.0) + into)
    });
    // The editor takes its frame from the first clip, so when that one is
    // the other way up from the cut the host fits it into this frame first.
    let frame = vb_narrative::orchestrator::MontageOrchestrator::frame(&result.graph);
    json!({
        "clips": clips,
        "frame": frame.map(|(width, height)| json!({ "width": width, "height": height })),
        "titleCards": slots.iter().filter(|s| s.kind == SlotKind::TitleCard).count(),
        "texts": result.dsl.tracks.text.len(),
        "transitions": result
            .dsl
            .transitions
            .iter()
            .filter(|t| t.kind != TransitionType::Cut)
            .count(),
        "music": music.and_then(|m| m.asset_path.clone()),
        "musicInUs": music_in,
    })
}

fn summarise(result: &crate::RunResult, workspace: &Workspace) -> Value {
    let timeline = result.dsl.resolve_timeline();

    // Only the slots that made the cut, in spine order, each with the reason
    // the orchestrator gave. This is what makes the result reviewable rather
    // than a black box.
    let slots: Vec<Value> = result
        .dsl
        .ordered_slots()
        .iter()
        .map(|slot| {
            let resolved = timeline.slot(&slot.id);
            json!({
                "id": slot.id,
                "kind": format!("{:?}", slot.kind).to_lowercase(),
                "durationUs": slot.duration_us.0,
                "timelineStartUs": resolved.map(|r| r.start_us.0),
                "shotId": slot.source_ref.as_ref().and_then(|r| r.shot_id.clone()),
                "sourceId": slot.source_ref.as_ref().map(|r| r.source_id.clone()),
                "sourceInUs": slot.source_ref.as_ref().map(|r| r.in_us.0),
                "sourceOutUs": slot.source_ref.as_ref().map(|r| r.out_us.0),
                "reason": slot.reason,
                "score": slot.score,
            })
        })
        .collect();

    // Every detected shot, including the ones not used, so the user can see
    // what was rejected and how close it came.
    let used: std::collections::HashSet<&str> = result
        .dsl
        .slots
        .iter()
        .filter_map(|s| s.source_ref.as_ref().and_then(|r| r.shot_id.as_deref()))
        .collect();

    let shots: Vec<Value> = result
        .graph
        .shots
        .iter()
        .map(|shot| {
            json!({
                "id": shot.id,
                "sourceId": shot.source_id,
                "startUs": shot.start_us.0,
                "endUs": shot.end_us.0,
                "durationUs": shot.duration().0,
                "score": shot.score(),
                "keyframes": shot.keyframes.len(),
                "used": used.contains(shot.id.as_str()),
                "description": shot.description.as_ref().map(|d| d.text.clone()),
            })
        })
        .collect();

    let beat = result
        .graph
        .priors
        .as_ref()
        .and_then(|p| p.beat_grid.as_ref());
    json!({
        "handoff": handoff(result),
        "draftPath": result.draft_path,
        "footageGraphPath": result.graph_path,
        "editingDslPath": result.dsl_path,
        "projectDir": workspace.project_dir,
        "templateId": result.template_id,
        "templateName": result.template_name,
        "editorDrafts": result.drafts.iter().map(|d| json!({
            "editor": d.editor.id(),
            "editorName": d.editor.display_name(),
            "name": d.name,
            "path": d.path,
            "clips": d.clips,
            "texts": d.texts,
            "hasMusic": d.has_music,
            "transitionsDropped": d.transitions_dropped,
            "mediaLinked": d.media.linked,
            "mediaUnreadable": d.media.unreadable,
        })).collect::<Vec<_>>(),
        "beat": beat.map(|b| json!({
            "bpm": (b.bpm * 10.0).round() / 10.0,
            "origin": b.origin,
            "confidence": b.confidence,
            "music": b.asset_path,
        })),
        "subjects": result.graph.entities.len(),
        "durationUs": timeline.total_duration_us.0,
        "shotsDetected": result.graph.shots.len(),
        "clipsUsed": used.len(),
        "verified": result.grounding.verified.len(),
        "rejected": result.grounding.rejected.len(),
        "rejections": result.grounding.rejected.iter().map(|r| json!({
            "slotId": r.slot_id,
            "reason": r.reason,
        })).collect::<Vec<_>>(),
        "reusedSources": result.reused_sources,
        "totalSources": result.total_sources,
        "sources": result.graph.sources.iter().map(|s| json!({
            "id": s.id,
            "path": s.path,
            "kind": if s.is_still() { "image" } else { "video" },
            "durationUs": s.duration_us.0,
        })).collect::<Vec<_>>(),
        "photosUsed": result.dsl.slots.iter()
            .filter_map(|s| s.source_ref.as_ref())
            .filter(|r| result.graph.source(&r.source_id).is_some_and(|s| s.is_still()))
            .count(),
        "warnings": result.warnings,
        "shots": shots,
        "slots": slots,
    })
}

// ---------------------------------------------------------------------------
// Adapters
// ---------------------------------------------------------------------------

/// The adapters a run uses, chosen from what this device has and what the
/// user asked for.
///
/// The rule the whole product rests on: the on-device path must produce a
/// complete result with no network. So the local set is always available, and
/// the cloud set is layered on top of it -- never in place of it. A cloud
/// adapter that cannot be reached leaves the local one doing the work, with a
/// note saying so, rather than failing the run.
pub struct Adapters {
    pub asr: Box<dyn AsrAdapter>,
    pub vlm: Box<dyn VlmAdapter>,
    pub tracker: Box<dyn TrackerAdapter>,
    /// The director's models: depth, faces and people.
    pub depth: Box<dyn DepthAdapter>,
    pub faces: Box<dyn FaceDetector>,
    pub people: Box<dyn PersonSegmenter>,
    /// Body poses: who is dancing, how many, and where two dance clips join.
    pub pose: Box<dyn PoseEstimator>,
    /// The composition judge, when one is installed.
    pub judge: Option<Box<dyn VlmAdapter>>,
    /// The general vision-language model that writes captions, when one is
    /// installed.
    pub captioner: Option<Box<dyn VlmAdapter>>,
    /// What the user should know about this choice.
    pub notes: Vec<String>,
}

impl Adapters {
    /// The director's models, as the pipeline takes them.
    pub fn director(&self) -> DirectorModels<'_> {
        DirectorModels {
            depth: self.depth.as_ref(),
            faces: self.faces.as_ref(),
            people: self.people.as_ref(),
            pose: self.pose.as_ref(),
        }
    }
}

pub fn adapters_for(
    library: &Library,
    choice: ModelChoice,
    work_dir: &Path,
    lang: Lang,
) -> Adapters {
    let mut notes = Vec::new();

    // --- speech ---------------------------------------------------------
    // Local only. A speech model is a download the user makes once, and
    // sending hours of audio to an endpoint is not something to do because a
    // menu said "cloud".
    let asr: Box<dyn AsrAdapter> = match models::installed_for(library, models::Role::Speech) {
        #[cfg(feature = "whisper")]
        Some(model) => {
            notes.push(match lang {
                Lang::En => format!("Speech: {} on this device.", model.name),
                Lang::Zh => format!("语音：{}，在本机运行。", model.name_zh),
            });
            Box::new(vb_whisper::WhisperAsr::new(models::path_of(library, model)))
        }
        #[cfg(not(feature = "whisper"))]
        Some(_) => {
            notes.push(
                lang.t(
                    "A speech model is installed, but this build was made without the speech \
                     runtime, so nothing was transcribed.",
                    "已经下载了语音模型，但这个版本没有编进语音识别，所以没有转写字幕。",
                )
                .to_string(),
            );
            Box::new(MockAsr)
        }
        None => {
            notes.push(
                lang.t(
                    "Speech: nothing transcribed. Only interview styles use speech; captions \
                     come from the picture.",
                    "语音：没有转写。只有访谈类风格用到语音；字幕按画面内容写。",
                )
                .to_string(),
            );
            Box::new(MockAsr)
        }
    };

    // --- vision ---------------------------------------------------------
    let cloud = library.settings().cloud;
    let vlm: Box<dyn VlmAdapter> = if choice == ModelChoice::Cloud && cloud.is_configured() {
        let model = if cloud.vision_model.trim().is_empty() {
            cloud.model.clone()
        } else {
            cloud.vision_model.clone()
        };
        notes.push(match lang {
            Lang::En => format!("Vision: {model} over your endpoint. Stills are sent to it."),
            Lang::Zh => format!("画面：通过你设置的接口用 {model} 识别，关键帧截图会发送给它。"),
        });
        Box::new(vb_cloud::CloudVlm {
            endpoint: vb_cloud::Endpoint {
                base_url: cloud.base_url.clone(),
                model,
                api_key: cloud.api_key.clone(),
            },
        })
    } else {
        if choice == ModelChoice::Cloud {
            notes.push(
                lang.t(
                    "No endpoint is configured, so this ran entirely on the device.",
                    "还没有设置云端接口，所以这次完全在本机运行。",
                )
                .to_string(),
            );
        }
        // Apple Vision where the platform has it; elsewhere SigLIP 2, a
        // download on a desktop and in the APK on Android. Loaded, not just
        // found, as for framing.
        let labels = models::installed_for(library, models::Role::Labels)
            .filter(|_| vb_onnx::runtime::ensure().is_ok());
        if vb_apple::is_available() {
            notes.push(
                lang.t(
                    "Vision: Apple Vision on this device, as scene labels.",
                    "画面：用本机的 Apple Vision 识别场景标签。",
                )
                .to_string(),
            );
            Box::new(vb_apple::VisionVlm::default())
        } else if let Some(model) = labels {
            notes.push(
                lang.t(
                    "Vision: SigLIP 2 on this device names each shot's place, people and \
                     activity, as labels.",
                    "画面：SigLIP 2 在本机给每个镜头标出地点、人物和在做什么。",
                )
                .to_string(),
            );
            Box::new(vb_onnx::SiglipLabels::new(models::path_of(library, model)))
        } else {
            notes.push(
                lang.t(
                    "Vision: nothing looked at the footage in this build.",
                    "画面：这个版本没有识别画面内容。",
                )
                .to_string(),
            );
            Box::new(MockVlm)
        }
    };

    // --- subjects -------------------------------------------------------
    // AuraFace where it is installed, with YuNet to find the faces: whose
    // face a face is, on every platform. Apple Vision's feature prints
    // otherwise, which say how alike two pictures of faces look.
    let face_id = models::installed_for(library, models::Role::FaceId)
        .zip(models::installed_for(library, models::Role::Faces))
        .filter(|_| vb_onnx::runtime::ensure().is_ok());
    let tracker: Box<dyn TrackerAdapter> = if let Some((embedder, detector)) = face_id {
        notes.push(
            lang.t(
                "Subjects: faces found by YuNet and grouped by AuraFace by whose face they \
                 are, on this device. Grouping, not knowing who anyone is.",
                "人物：在本机用 YuNet 找人脸，用 AuraFace 按是谁分组。只分组，不知道是谁。",
            )
            .to_string(),
        );
        // A face is only said to be someone's when it is clearly a face:
        // the director's 0.5 let a ceiling's pattern and a shop sign in.
        let mut detector = vb_onnx::YuNet::new(models::path_of(library, detector));
        detector.min_confidence = 0.75;
        let mut tracker =
            vb_onnx::AuraFace::new(models::path_of(library, embedder), Box::new(detector));
        tracker.thumbnail_dir = Some(work_dir.join("faces"));
        Box::new(tracker)
    } else if vb_apple::is_available() {
        notes.push(
            lang.t(
                "Subjects: faces found on this device and grouped by how alike they look, \
                 which is not recognition.",
                "人物：在本机找出人脸，并按长相相似程度分组。这是分组，不是人脸识别。",
            )
            .to_string(),
        );
        Box::new(vb_apple::VisionTracker {
            thumbnail_dir: Some(work_dir.join("faces")),
            ..Default::default()
        })
    } else {
        Box::new(MockTracker)
    };

    // --- framing --------------------------------------------------------
    // Local only, like speech: three small models on ONNX Runtime. Depth is
    // what makes the reading; faces and people refine it, and without them
    // the director is stricter rather than blind.
    let installed =
        |role| models::installed_for(library, role).map(|m| models::path_of(library, m));
    // Loaded, not just found: the prebuilt ONNX Runtime needs a newer macOS
    // than the app does, and a library that is there but will not load must
    // read as "not checked", not fail on every frame.
    let runtime = vb_onnx::runtime::ensure();
    let (depth, faces, people): (
        Box<dyn DepthAdapter>,
        Box<dyn FaceDetector>,
        Box<dyn PersonSegmenter>,
    ) = match (installed(models::Role::Depth), runtime.clone()) {
        (Some(depth), Ok(_)) => {
            notes.push(
                lang.t(
                    "Framing: depth, faces and people read on this device; shots blocked by \
                     a door, a dashboard or a sleeve go to the back.",
                    "构图：景深、人脸、人像三个模型在本机运行，被车门、仪表台、衣袖挡住的画面会排到后面。",
                )
                .to_string(),
            );
            (
                Box::new(vb_onnx::DepthAnything::new(depth)),
                match installed(models::Role::Faces) {
                    Some(path) => Box::new(vb_onnx::YuNet::new(path)),
                    None => Box::new(MockFaces),
                },
                match installed(models::Role::People) {
                    Some(path) => Box::new(vb_onnx::PpHumanSeg::new(path)),
                    None => Box::new(MockPeople),
                },
            )
        }
        (Some(_), Err(reason)) => {
            tracing::warn!(%reason, "ONNX Runtime is unavailable");
            notes.push(
                lang.t(
                    "Framing: the models are installed, but ONNX Runtime could not be loaded \
                     on this device, so framing was not checked.",
                    "构图：模型已经下载，但这台设备上加载不了 ONNX Runtime，所以没有检查构图。",
                )
                .to_string(),
            );
            (
                Box::new(MockDepth),
                Box::new(MockFaces),
                Box::new(MockPeople),
            )
        }
        (None, _) => {
            notes.push(
                lang.t(
                    "Framing: not checked. Download the framing models to keep shots blocked \
                     by a door or a dashboard out of the cut.",
                    "构图：没有检查。下载构图模型后，被车门、仪表台挡住的画面就不会进成片。",
                )
                .to_string(),
            );
            (
                Box::new(MockDepth),
                Box::new(MockFaces),
                Box::new(MockPeople),
            )
        }
    };

    // --- body pose ----------------------------------------------------------
    // On ONNX Runtime like the framing models, and needed only for dance, so
    // its absence is said only when it matters: in a dance project's notes.
    let pose: Box<dyn PoseEstimator> = match (installed(models::Role::Pose), &runtime) {
        (Some(path), Ok(_)) => Box::new(vb_onnx::RtmoPose::new(path)),
        _ => Box::new(MockPose),
    };

    // --- the composition judge --------------------------------------------
    // Only when its files are in the library. With none, the cut is made
    // once, on the readings, as it always was -- nothing to report.
    let judge: Option<Box<dyn VlmAdapter>> = models::local_for(library, models::Role::Composition)
        .map(|m| {
            let [model, mmproj] = models::local_paths(library, m);
            notes.push(match lang {
                Lang::En => format!(
                    "Composition: {} rates the moments the cut might use, on this device.",
                    m.name
                ),
                Lang::Zh => format!(
                    "构图评分：{} 在本机给候选画面打分，再按分数选片。",
                    m.name_zh
                ),
            });
            Box::new(vb_llama::LlamaVlm::new(m.adapter, model, mmproj)) as Box<dyn VlmAdapter>
        });

    // --- captions ---------------------------------------------------------
    // From the picture, by the general VLM, when it is downloaded. Never
    // from speech: what was said in holiday footage is rarely what a viewer
    // should read, and recognising it is often wrong.
    let captioner: Option<Box<dyn VlmAdapter>> = match models::installed_for(
        library,
        models::Role::Describe,
    ) {
        Some(m) => {
            notes.push(match lang {
                Lang::En => format!(
                    "Captions: {} looks at the clips and writes what each shows, on this \
                         device. Speech is not turned into subtitles.",
                    m.name
                ),
                Lang::Zh => format!(
                    "字幕：{} 在本机看画面，按画面内容写字幕，不用语音转文字。",
                    m.name_zh
                ),
            });
            let projector =
                models::projector_path(library, m).unwrap_or_else(|| models::path_of(library, m));
            Some(Box::new(vb_llama::LlamaVlm::new(
                m.adapter,
                models::path_of(library, m),
                projector,
            )) as Box<dyn VlmAdapter>)
        }
        None => {
            // The largest this device can hold, not the one a Mac would
            // take: a phone with 8 GB runs the 4B.
            let model = models::CATALOGUE
                .iter()
                .filter(|m| m.role == models::Role::Describe && models::fits(m))
                .max_by_key(|m| m.rank)
                .map_or("Qwen3-VL", |m| m.name);
            notes.push(match lang {
                Lang::En => format!(
                    "Captions: none. Install {model} and each clip gets a caption saying what \
                     the picture shows; speech is never turned into subtitles."
                ),
                Lang::Zh => format!(
                    "字幕：这一版没有字幕。装上 {model} 后，会按画面内容给片段配字幕；不会用语音转文字。"
                ),
            });
            None
        }
    };

    Adapters {
        asr,
        vlm,
        tracker,
        depth,
        faces,
        people,
        pose,
        judge,
        captioner,
        notes,
    }
}

/// What the adapters would be, without building them: for the self-test and
/// for the engine card in the interface.
fn adapter_report(library: &Library, choice: ModelChoice) -> Value {
    let work = library.work_dir();
    let adapters = adapters_for(library, choice, &work, Lang::En);
    let mut report = json!([
        { "role": "Speech", "name": adapters.asr.name(), "mock": adapters.asr.is_mock() },
        { "role": "Vision", "name": adapters.vlm.name(), "mock": adapters.vlm.is_mock() },
        { "role": "Tracking", "name": adapters.tracker.name(), "mock": adapters.tracker.is_mock() },
        {
            "role": "Framing",
            "name": adapters.director().name(),
            "mock": !adapters.director().reads(),
        },
    ]);
    // The judge is listed only when it is there: its absence is not a mock
    // standing in, just a cut made once instead of twice.
    if let (Some(judge), Some(list)) = (&adapters.judge, report.as_array_mut()) {
        list.push(json!({ "role": "Composition", "name": judge.name(), "mock": false }));
    }
    if let (Some(captioner), Some(list)) = (&adapters.captioner, report.as_array_mut()) {
        list.push(json!({ "role": "Captions", "name": captioner.name(), "mock": false }));
    }
    if let Some(list) = report.as_array_mut() {
        list.push(json!({ "role": "Reasoning", "name": "montage-v1", "mock": false }));
    }
    report
}

// ---------------------------------------------------------------------------
// The self-test
// ---------------------------------------------------------------------------

/// Everything that decides whether a real run will work, in one answer.
///
/// Shown when the app opens, because every one of these fails in a way that
/// looks like a bug in the app if it is discovered in the middle of a run:
/// no ffmpeg, no writable library, no draft folder for the editor.
///
/// Each check is `pass`, `warn` or `fail`. `fail` means a run cannot produce
/// a result; `warn` means it will, with something missing.
pub fn selftest() -> Value {
    selftest_in(Library::open().settings().lang())
}

pub fn selftest_in(lang: Lang) -> Value {
    let library = Library::open();
    let media = vb_media::default_backend();
    let mut checks = Vec::new();

    let available = media.is_available();
    checks.push(check(
        "media",
        lang.t("Video engine", "视频引擎"),
        if available { "pass" } else { "fail" },
        if available {
            format!(
                "{} · ffmpeg {}",
                media.name(),
                media.version().unwrap_or_else(|| "unknown".into())
            )
        } else {
            lang.t(
                media_note(media.as_ref(), available),
                "找不到 ffmpeg，无法分析视频。",
            )
            .to_string()
        },
    ));

    if let Some(license) = media.license() {
        let shippable = vb_media::is_shippable_license(&license);
        checks.push(check(
            "license",
            lang.t("Video engine licence", "视频引擎许可证"),
            if shippable { "pass" } else { "warn" },
            match (shippable, lang) {
                (true, Lang::En) => format!("{license} — can ship"),
                (true, Lang::Zh) => format!("{license}：可以随产品发布"),
                (false, Lang::En) => format!("{license} — fine to develop against, not to ship"),
                (false, Lang::Zh) => format!("{license}：开发可用，发布需换成 LGPL 版本"),
            },
        ));
    }

    let writable = library.writable();
    checks.push(check(
        "library",
        lang.t("Library folder", "资料库文件夹"),
        if writable.is_ok() { "pass" } else { "fail" },
        match &writable {
            Ok(()) => library.root().display().to_string(),
            Err(e) => match lang {
                Lang::En => format!("cannot write to {}: {e}", library.root().display()),
                Lang::Zh => format!("无法写入 {}：{e}", library.root().display()),
            },
        },
    ));

    let labels = models::installed_for(&library, models::Role::Labels);
    checks.push(check(
        "vision",
        lang.t("On-device vision", "本机画面识别"),
        if vb_apple::is_available() || labels.is_some() {
            "pass"
        } else {
            "warn"
        },
        if vb_apple::is_available() {
            lang.t(
                "Apple Vision: scene labels and faces, nothing to download",
                "Apple Vision：场景标签和人脸，无需下载",
            )
        } else if labels.is_some() {
            lang.t(
                "SigLIP 2: scene labels on this device; faces are not grouped here",
                "SigLIP 2：在本机识别场景标签；这里不做人脸分组",
            )
        } else {
            lang.t(
                "No built-in vision here; install the scene-label model, or use a cloud model",
                "此平台没有内置画面识别，请安装场景标签模型，或使用云端模型",
            )
        },
    ));

    let speech = models::installed_for(&library, models::Role::Speech);
    checks.push(check(
        "speech",
        lang.t("Speech model", "语音模型"),
        match speech {
            Some(_) if cfg!(feature = "whisper") => "pass",
            _ => "warn",
        },
        match (speech, lang) {
            (Some(model), Lang::En) if cfg!(feature = "whisper") => {
                format!("{} is installed", model.name)
            }
            (Some(model), Lang::Zh) if cfg!(feature = "whisper") => {
                format!("已安装 {}", model.name_zh)
            }
            (Some(model), Lang::En) => format!(
                "{} is installed, but this build has no speech runtime",
                model.name
            ),
            (Some(model), Lang::Zh) => {
                format!("已安装 {}，但这个版本没有语音识别", model.name_zh)
            }
            (None, _) => lang
                .t(
                    "None downloaded; only interview styles need one",
                    "还没有下载；只有访谈类风格需要它",
                )
                .to_string(),
        },
    ));

    // The director's models and the engine that runs them: all three
    // downloaded, and ONNX Runtime loadable on this OS.
    let framing: Vec<_> = [
        models::Role::Depth,
        models::Role::Faces,
        models::Role::People,
    ]
    .into_iter()
    .map(|role| models::installed_for(&library, role))
    .collect();
    let runtime = vb_onnx::runtime::ensure();
    let all_installed = framing.iter().all(Option::is_some);
    checks.push(check(
        "framing",
        lang.t("Framing models", "构图模型"),
        if all_installed && runtime.is_ok() {
            "pass"
        } else {
            "warn"
        },
        match (all_installed, &runtime) {
            (true, Ok(_)) => lang
                .t(
                    "Depth, faces and people are installed and run on this device",
                    "景深、人脸、人像都已安装，在本机运行",
                )
                .to_string(),
            (true, Err(_)) => lang
                .t(
                    "Installed, but ONNX Runtime cannot be loaded on this device",
                    "已安装，但这台设备上加载不了 ONNX Runtime",
                )
                .to_string(),
            (false, _) => lang
                .t(
                    "Not all downloaded, so shots blocked by a door or a dashboard are not caught",
                    "还没下载齐，所以被车门、仪表台挡住的画面挑不出来",
                )
                .to_string(),
        },
    ));

    for install in vb_export::detect_editors() {
        let status = match (install.is_installed(), install.can_export()) {
            (true, true) => "pass",
            (true, false) => "warn",
            // Not installed is not a failure: the portable draft opens in
            // other editors, and most users have one of the two.
            (false, _) => "warn",
        };
        let name = editor_name(&install);
        checks.push(check(
            &format!("editor.{}", install.editor.id()),
            &match lang {
                Lang::En => format!("{name} draft export"),
                Lang::Zh => format!("导出到{name}"),
            },
            status,
            match (&install.draft_root, install.is_installed()) {
                (Some(root), _) => {
                    let mut detail = match lang {
                        Lang::En => format!("drafts go to {}", root.display()),
                        Lang::Zh => format!("草稿写入 {}", root.display()),
                    };
                    if install.running {
                        detail.push_str(lang.t(
                            " — it is running now, so a new draft appears after it restarts",
                            "。它正在运行，新草稿要重启它之后才会出现",
                        ));
                    }
                    detail
                }
                (None, true) if install.phone => {
                    let version = install.version.as_deref();
                    match lang {
                        Lang::En => format!(
                            "installed on this device{}, but a draft cannot be put into it: \
                             the phone app keeps its drafts in its own storage, which Android \
                             lets no other app write to",
                            version.map(|v| format!(" ({v})")).unwrap_or_default()
                        ),
                        Lang::Zh => format!(
                            "这台设备上已安装{}，但不能直接把草稿放进去：手机版{name}把草稿存在它\
                             自己的存储里，安卓不允许其他 App 写入",
                            version.map(|v| format!("（{v}）")).unwrap_or_default()
                        ),
                    }
                }
                (None, true) => lang
                    .t(
                        "installed, but it has never created its draft folder; open it once",
                        "已安装，但还没有生成草稿文件夹，请先打开它一次",
                    )
                    .to_string(),
                (None, false) => lang
                    .t("not installed on this device", "这台设备上没有安装")
                    .to_string(),
            },
        ));
    }

    let cloud = library.settings().cloud;
    checks.push(check(
        "cloud",
        lang.t("Cloud model", "云端模型"),
        if cloud.is_configured() {
            "pass"
        } else {
            "warn"
        },
        if cloud.is_configured() {
            format!("{} @ {}", cloud.model, cloud.base_url)
        } else {
            lang.t(
                "Not configured. Everything runs on this device.",
                "未设置。所有处理都在本机完成。",
            )
            .to_string()
        },
    ));

    let worst = if checks.iter().any(|c| c["status"] == "fail") {
        "fail"
    } else if checks.iter().any(|c| c["status"] == "warn") {
        "warn"
    } else {
        "pass"
    };

    json!({
        "status": worst,
        "canAnalyse": available && writable.is_ok(),
        "checks": checks,
        "adapters": adapter_report(&library, ModelChoice::Local),
    })
}

fn check(id: &str, label: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({ "id": id, "label": label, "status": status, "detail": detail.into() })
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// What a front end asks for when it starts a run.
struct RunRequest {
    project: Option<Project>,
    inputs: Vec<String>,
    project_dir: Option<String>,
    template_id: Option<String>,
    style: StyleChoice,
    model: ModelChoice,
    editors: Vec<Editor>,
    title: Option<String>,
    /// Look at the footage again rather than reuse what was found before.
    fresh: bool,
}

fn editor_by_id(id: &str) -> Option<Editor> {
    Editor::ALL.into_iter().find(|e| e.id() == id)
}

fn parse_request(library: &Library, args: &Value) -> Result<RunRequest, String> {
    let project = match args["projectId"].as_str() {
        Some(id) => Some(
            crate::projects::get(library, id).ok_or_else(|| format!("there is no project {id}"))?,
        ),
        None => None,
    };

    // What the call says wins over what the project remembers, so a run can
    // try a style without saving it.
    let inputs: Vec<String> = match args["inputs"].as_array() {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => project
            .as_ref()
            .map(|p| p.inputs.clone())
            .unwrap_or_default(),
    };
    if inputs.is_empty() {
        return Err("a project needs at least one video or photo".to_string());
    }

    let style = match args.get("style") {
        Some(value) if !value.is_null() => serde_json::from_value(value.clone())
            .map_err(|e| format!("the style could not be read: {e}"))?,
        _ => project
            .as_ref()
            .map(|p| p.style.clone())
            .unwrap_or_default(),
    };

    let model = match args["model"].as_str() {
        Some("cloud") => ModelChoice::Cloud,
        Some("local") => ModelChoice::Local,
        Some(other) => return Err(format!("unknown model choice {other:?}")),
        None => project.as_ref().map(|p| p.model).unwrap_or_default(),
    };

    let editor_ids: Vec<String> = match args["editors"].as_array() {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => project
            .as_ref()
            .map(|p| p.editors.clone())
            .unwrap_or_else(|| library.settings().default_editors.clone()),
    };
    let mut editors = Vec::new();
    for id in &editor_ids {
        editors.push(editor_by_id(id).ok_or_else(|| format!("unknown editor {id:?}"))?);
    }

    Ok(RunRequest {
        title: args["title"]
            .as_str()
            .map(str::to_string)
            .or_else(|| project.as_ref().map(|p| p.name.clone())),
        template_id: args["templateId"]
            .as_str()
            .map(str::to_string)
            .or_else(|| project.as_ref().and_then(|p| p.template_id.clone())),
        project_dir: args["projectDir"].as_str().map(str::to_string),
        project,
        inputs,
        style,
        model,
        editors,
        fresh: args["fresh"].as_bool().unwrap_or(false),
    })
}

/// Start a run in the background and return its job id.
///
/// Analysis takes minutes, so this never blocks: the caller polls
/// `run.status` for the stage and the percentage, and may cancel.
pub fn run_start(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    // Parsed here rather than inside the job, so a bad request is an error
    // the caller sees immediately instead of a job that fails a second later.
    parse_request(&library, args)?;
    let args = args.clone();

    let id = jobs::start("run", move |job| {
        let library = Library::open();
        let request = parse_request(&library, &args)?;
        run_with(&library, request, job)
    });
    Ok(json!({ "jobId": id }))
}

/// The whole run, on whichever thread the caller is on.
fn run_with(
    library: &Library,
    request: RunRequest,
    observer: &dyn Observer,
) -> Result<Value, String> {
    let sources = expand_inputs(&request.inputs)?;
    let first = sources
        .first()
        .cloned()
        .ok_or("no videos or photos to analyse")?;

    let media = vb_media::default_backend();
    if !media.is_available() {
        return Err("ffmpeg was not found, so this footage cannot be analysed".to_string());
    }

    // A project keeps its outputs in the library and shares the analysis
    // cache with every other project. A run without one writes beside the
    // footage, which is what the CLI has always done.
    let named_dir = request.project_dir.as_deref().filter(|d| !d.is_empty());
    let workspace = match (&request.project, named_dir) {
        (_, Some(dir)) => Workspace::beside(PathBuf::from(dir)),
        (Some(project), None) => project.workspace(library),
        (None, None) => Workspace::beside(default_project_dir(
            &first,
            std::env::var_os("VB_WORKDIR").map(PathBuf::from),
        )),
    };
    std::fs::create_dir_all(&workspace.project_dir)
        .map_err(|e| format!("{}: {e}", workspace.project_dir.display()))?;

    let lang = library.settings().lang();
    let adapters = adapters_for(library, request.model, &workspace.work_dir, lang);
    let mut pipeline = Pipeline::new(
        media.as_ref(),
        adapters.asr.as_ref(),
        adapters.vlm.as_ref(),
        adapters.tracker.as_ref(),
    );
    pipeline.director = adapters.director();
    pipeline.judge = adapters.judge.as_deref();
    pipeline.captioner = adapters.captioner.as_deref();

    let options = RunOptions {
        template_id: request.template_id.clone(),
        style: request.style.style(),
        music: request
            .style
            .music
            .as_ref()
            .filter(|m| !m.is_empty())
            .map(PathBuf::from),
        title: request.title.clone(),
        draft_path: None,
        editors: request.editors.clone(),
        target_duration: request
            .style
            .target_duration_us
            .filter(|d| *d > 0)
            .map(vb_contracts::Micros),
        lang,
        fresh: request.fresh,
        music_loop: request.style.music_loop.unwrap_or(false),
        draft_media: library.settings().draft_media(),
        order: request.style.order.clone(),
        categories: {
            // A cut ordered by category needs every video filed: one the
            // butler has not looked at is looked at now.
            let sorted = request.style.order.as_deref() == Some("category")
                || (request.style.order.is_none()
                    && request.style.topic == Some(vb_narrative::Topic::Annual));
            let eyes = crate::hosting::Eyes {
                media: media.as_ref(),
                labels: adapters.vlm.as_ref(),
                faces: adapters.tracker.as_ref(),
            };
            crate::hosting::categories_for(library, &sources, sorted.then_some(&eyes))
        },
    };
    crate::thermal::set_enabled(library.settings().thermal_guard());

    let result = pipeline
        .run_project(&sources, &workspace, &options, observer)
        .map_err(|e| describe_error(&e, lang))?;

    // The album is the project's subjects folded into what the library
    // already knew, so the same people carry their names between projects.
    // Only a project's run: a run with no project has nowhere to be opened
    // from, and nothing would ever take its sightings out again.
    if let Some(project) = &request.project {
        entities::record_run(
            library,
            &project.id,
            &project.name,
            &result.graph,
            &workspace.work_dir,
        );
    }

    let mut value = summarise(&result, &workspace);
    value["adapterNotes"] = json!(adapters.notes);

    // Remember what was run, so reopening the project shows it.
    if let Some(mut project) = request.project {
        // The record was read when the run began, and the editor may have
        // written to it since -- a cut edited or exported while this one
        // ran. What the editor owns is taken from disk, not overwritten.
        if let Some(latest) = crate::projects::get(library, &project.id) {
            project.edited_at = latest.edited_at;
            project.exports = latest.exports;
        }
        project.inputs = request.inputs.clone();
        project.style = request.style.clone();
        project.model = request.model;
        project.editors = request.editors.iter().map(|e| e.id().to_string()).collect();
        project.template_id = request.template_id.clone();
        project.updated_at = vb_contracts::now_rfc3339();
        project.last_run = Some(crate::projects::LastRun {
            at: project.updated_at.clone(),
            draft_path: result.draft_path.display().to_string(),
            duration_us: value["durationUs"].as_u64().unwrap_or(0),
            clips_used: result.dsl.slots.len(),
            template_id: result.template_id.clone(),
            editor_drafts: result
                .drafts
                .iter()
                .map(|d| d.path.display().to_string())
                .collect(),
            dsl_path: Some(result.dsl_path.display().to_string()),
            graph_path: Some(result.graph_path.display().to_string()),
        });
        if let Err(e) = project.save(library) {
            tracing::warn!(error = %e, "could not save the project");
        }
        value["projectId"] = json!(project.id);
    }

    Ok(value)
}

/// A pipeline failure, as the user reads it.
///
/// The ones a user can cause and fix -- a moved file, no clip long enough, a
/// stop they asked for -- get a sentence in their language. The rest keep the
/// engine's own wording, which names the file and the library that failed and
/// is what a bug report needs.
fn describe_error(error: &crate::PipelineError, lang: Lang) -> String {
    use crate::PipelineError as E;
    if lang == Lang::En {
        return error.to_string();
    }
    match error {
        E::InputNotFound { path } => format!("找不到这个文件：{path}"),
        E::NoInputs => "项目里还没有视频或照片。".to_string(),
        E::EmptyCut => "没有能用的片段：所有剪辑决定都没有通过素材核验。".to_string(),
        E::NoDancing => {
            "选了舞蹈风格，但这些素材里没有找到跳舞的画面。换一种风格，或者加入跳舞的视频。"
                .to_string()
        }
        E::NoAction => "选了主体衔接，但这些素材里没有找到在做动作的主体（目前只认人）。换一种风格，或者加入有人在跳舞、运动的视频。"
            .to_string(),
        E::NeedsPoseModel => {
            "主体衔接要靠人体姿态模型跟住主体，这台设备还没有。先在「模型」页下载 RTMO-s（40 MB）。"
                .to_string()
        }
        E::Cancelled | E::Perception(vb_perception::PerceptionError::Cancelled) => {
            "已停止。".to_string()
        }
        E::NoDraftRoot { editor } => {
            format!("{editor}还没有草稿文件夹。先打开一次{editor}，它会自己创建。")
        }
        E::Music { path, detail } => format!("无法读取配乐 {path}：{detail}"),
        E::Narrative(vb_narrative::NarrativeError::NoUsableShots { shots, .. }) => {
            format!("找到了 {shots} 个镜头，但都太短，按当前风格用不上。换一种节奏更快的风格试试。")
        }
        E::Narrative(vb_narrative::NarrativeError::NoFootage) => {
            "没有在视频里找到任何镜头。".to_string()
        }
        other => format!("处理失败：{other}"),
    }
}

/// Run a saved project as it is, reporting to `observer`: for the video
/// butler, which makes a project of each memory and cuts it.
pub(crate) fn run_saved_project(
    library: &Library,
    project_id: &str,
    observer: &dyn Observer,
) -> Result<Value, String> {
    let request = parse_request(library, &json!({ "projectId": project_id }))?;
    run_with(library, request, observer)
}

// ---------------------------------------------------------------------------
// The video butler
// ---------------------------------------------------------------------------

/// The butler's pass running now, if one is: there is only ever one.
fn hosting_job() -> &'static std::sync::Mutex<Option<String>> {
    static JOB: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
    JOB.get_or_init(|| std::sync::Mutex::new(None))
}

fn running_hosting_job() -> Option<String> {
    let id = hosting_job().lock().ok()?.clone()?;
    let status = jobs::status(&id)?;
    (status["finished"] != true).then_some(id)
}

/// A memory's cut, reported as a share of the butler's pass.
struct MemoryRun<'a> {
    job: &'a jobs::Job,
    from: f64,
    share: f64,
    name: String,
    index: usize,
    total: usize,
}

impl Observer for MemoryRun<'_> {
    fn progress(&self, p: &crate::Progress) {
        self.job.update(&crate::Progress {
            stage: p.stage,
            percent: self.from + self.share * p.percent / 100.0,
            detail: format!("{} · {}", self.name, p.detail),
            step: "hosting.project".into(),
            file: Some(self.name.clone()),
            source_index: self.index,
            source_count: self.total,
            active: 0,
        });
    }

    fn cancelled(&self) -> bool {
        self.job.cancelled()
    }
}

/// One pass of the butler: look through the folders -- or at the files
/// given in their place -- find the memories, and cut each memory's project
/// that is new or whose footage changed.
fn hosting_pass(
    library: &Library,
    job: &jobs::Job,
    generate: bool,
    only: Option<Vec<String>>,
    files: Option<Vec<PathBuf>>,
) -> Result<Value, String> {
    let lang = library.settings().lang();
    let media = vb_media::default_backend();
    if !media.is_available() {
        return Err(lang
            .t(
                "ffmpeg was not found, so no video can be read",
                "找不到 ffmpeg，读不了视频",
            )
            .to_string());
    }
    crate::thermal::set_enabled(library.settings().thermal_guard());
    let look_share = if generate { 38.0 } else { 95.0 };
    let pass = if only.is_none() {
        let work = crate::hosting::dir(library);
        let adapters = adapters_for(library, ModelChoice::Local, &work, lang);
        let eyes = crate::hosting::Eyes {
            media: media.as_ref(),
            labels: adapters.vlm.as_ref(),
            faces: adapters.tracker.as_ref(),
        };
        let pass = crate::hosting::pass(
            library,
            &eyes,
            files.as_deref(),
            job,
            &|share, file, n, total| {
                job.update(&crate::Progress {
                    stage: crate::RunStage::Analyse,
                    percent: 2.0 + look_share * share,
                    detail: file.to_string(),
                    step: if total == 0 {
                        "hosting.find".into()
                    } else {
                        "hosting.look".into()
                    },
                    file: (!file.is_empty()).then(|| file.to_string()),
                    source_index: n,
                    source_count: total,
                    active: 0,
                })
            },
        )?;
        if job.is_cancelled() {
            return Err("cancelled".into());
        }
        Some(pass)
    } else {
        None
    };
    job.update(&crate::Progress {
        stage: crate::RunStage::Compose,
        percent: 2.0 + look_share,
        detail: String::new(),
        step: "hosting.memories".into(),
        file: None,
        source_index: 0,
        source_count: 0,
        active: 0,
    });
    let book = if only.is_none() {
        crate::memories::refresh(library)?
    } else {
        crate::memories::load(library)
    };
    let generated = if generate {
        let from = 3.0 + look_share;
        crate::memories::generate(
            library,
            only.as_deref(),
            &|| job.is_cancelled(),
            &|n, total, project_id, name| {
                let share = (99.0 - from) / total.max(1) as f64;
                let observer = MemoryRun {
                    job,
                    from: from + share * n as f64,
                    share,
                    name: name.to_string(),
                    index: n,
                    total,
                };
                observer.progress(&crate::Progress {
                    stage: crate::RunStage::Prepare,
                    percent: 0.0,
                    detail: String::new(),
                    step: String::new(),
                    file: None,
                    source_index: 0,
                    source_count: 0,
                    active: 0,
                });
                run_saved_project(library, project_id, &observer)
            },
        )?
    } else {
        Vec::new()
    };
    if job.is_cancelled() {
        return Err("cancelled".into());
    }
    Ok(json!({
        "pass": pass,
        "memories": book.memories.len(),
        "generated": generated,
    }))
}

fn start_hosting(generate: bool, only: Option<Vec<String>>, files: Option<Vec<PathBuf>>) -> Value {
    if let Some(id) = running_hosting_job() {
        return json!({ "jobId": id, "already": true });
    }
    let id = jobs::start("hosting", move |job| {
        let library = Library::open();
        hosting_pass(&library, job, generate, only, files)
    });
    if let Ok(mut current) = hosting_job().lock() {
        *current = Some(id.clone());
    }
    json!({ "jobId": id })
}

pub fn hosting_get() -> Value {
    let library = Library::open();
    let lang = library.settings().lang();
    let mut value = crate::hosting::overview(&library, lang);
    value["jobId"] = json!(running_hosting_job());
    value["memories"] = json!(crate::memories::load(&library).memories.len());
    value
}

/// Hand the footage to the butler: switch hosting on, look through every
/// folder, find the memories and cut them, in the background. Returns the
/// job to follow; a pass already running is that pass.
///
/// `files`, when given, is the footage itself, in place of walking the
/// folders: a caller that already knows every file -- a file server with
/// its disk indexed -- hands the list over, and what is not on it leaves
/// the catalogue, except what came from a kept folder that cannot be read
/// now. Absolute paths; anything that is not a video or a photo is passed
/// over.
pub fn hosting_start(args: &Value) -> Result<Value, String> {
    let files = match &args["files"] {
        Value::Null => None,
        Value::Array(list) => Some(
            list.iter()
                .map(|f| f.as_str().map(PathBuf::from))
                .collect::<Option<Vec<PathBuf>>>()
                .ok_or("files must be a list of paths")?,
        ),
        _ => return Err("files must be a list of paths".into()),
    };
    let library = Library::open();
    let mut settings = library.settings();
    if let Some(roots) = args["roots"].as_array() {
        settings.hosting.roots = roots
            .iter()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
    }
    settings.hosting.enabled = true;
    library.save_settings(&settings)?;
    Ok(start_hosting(
        args["generate"].as_bool().unwrap_or(true),
        None,
        files,
    ))
}

pub fn hosting_update(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let mut settings = library.settings();
    if let Some(on) = args["enabled"].as_bool() {
        settings.hosting.enabled = on;
        if !on && let Some(id) = running_hosting_job() {
            jobs::cancel(&id);
        }
    }
    if let Some(roots) = args["roots"].as_array() {
        // A folder added must be there; one kept already may be on a disk
        // that is not plugged in now, and stays.
        for root in roots.iter().filter_map(|r| r.as_str()) {
            if !Path::new(root).is_dir() && !settings.hosting.roots.iter().any(|kept| kept == root)
            {
                return Err(format!("{root} is not a folder"));
            }
        }
        settings.hosting.roots = roots
            .iter()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
    }
    library.save_settings(&settings)?;
    Ok(hosting_get())
}

fn memory_kind(args: &Value) -> Result<Option<crate::memories::Kind>, String> {
    match args["kind"]
        .as_str()
        .filter(|k| !k.is_empty() && *k != "all")
    {
        Some(k) => crate::memories::Kind::parse(k)
            .map(Some)
            .ok_or_else(|| format!("unknown kind of memory {k:?}")),
        None => Ok(None),
    }
}

/// The world map every front end draws: countries and China's provinces,
/// parsed once.
pub fn geo_world() -> Value {
    static WORLD: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    WORLD
        .get_or_init(|| serde_json::from_str(vb_geo::world_json()).unwrap_or(Value::Null))
        .clone()
}

// ---------------------------------------------------------------------------
// Projects, settings, models, subjects
// ---------------------------------------------------------------------------

fn project_value(project: &Project, library: &Library) -> Value {
    let mut value = serde_json::to_value(project).unwrap_or(Value::Null);
    value["dir"] = json!(project.dir(library));
    value
}

pub fn projects_list() -> Value {
    let library = Library::open();
    json!({
        "projects": crate::projects::list(&library)
            .iter()
            .map(|p| project_value(p, &library))
            .collect::<Vec<_>>(),
    })
}

pub fn project_create(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    library.ensure().map_err(|e| e.to_string())?;
    let name = args["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or("a project needs a name")?;

    let mut project = Project::new(name);
    apply_project_fields(&mut project, args)?;
    project.save(&library)?;
    Ok(project_value(&project, &library))
}

pub fn project_update(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let id = args["id"].as_str().ok_or("which project?")?;
    let mut project =
        crate::projects::get(&library, id).ok_or_else(|| format!("there is no project {id}"))?;
    if let Some(name) = args["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        project.name = name.to_string();
    }
    apply_project_fields(&mut project, args)?;
    project.updated_at = vb_contracts::now_rfc3339();
    project.save(&library)?;
    Ok(project_value(&project, &library))
}

fn apply_project_fields(project: &mut Project, args: &Value) -> Result<(), String> {
    if let Some(inputs) = args["inputs"].as_array() {
        project.inputs = inputs
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(style) = args.get("style").filter(|v| !v.is_null()) {
        project.style = serde_json::from_value(style.clone())
            .map_err(|e| format!("the style could not be read: {e}"))?;
    }
    match args["model"].as_str() {
        Some("cloud") => project.model = ModelChoice::Cloud,
        Some("local") => project.model = ModelChoice::Local,
        Some(other) => return Err(format!("unknown model choice {other:?}")),
        None => {}
    }
    if let Some(editors) = args["editors"].as_array() {
        let mut ids = Vec::new();
        for value in editors {
            let id = value.as_str().unwrap_or_default();
            editor_by_id(id).ok_or_else(|| format!("unknown editor {id:?}"))?;
            ids.push(id.to_string());
        }
        project.editors = ids;
    }
    if args.get("templateId").is_some() {
        project.template_id = args["templateId"].as_str().map(str::to_string);
    }
    Ok(())
}

pub fn project_get(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let id = args["id"].as_str().ok_or("which project?")?;
    crate::projects::get(&library, id)
        .map(|p| project_value(&p, &library))
        .ok_or_else(|| format!("there is no project {id}"))
}

pub fn project_delete(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let id = args["id"].as_str().ok_or("which project?")?;
    crate::projects::delete(&library, id)?;
    crate::editor::close(id);
    // Its people go with it, unless another project saw them too.
    entities::prune(&library);
    Ok(json!({ "deleted": id }))
}

pub fn settings_get() -> Value {
    let library = Library::open();
    let settings = library.settings();
    json!({
        "cloud": settings.cloud.redacted(),
        "defaultEditors": settings.default_editors,
        "language": settings.lang().id(),
        "draftMedia": settings.draft_media().id(),
        "thermalGuard": settings.thermal_guard(),
        "libraryPath": library.root(),
    })
}

pub fn settings_update(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let mut settings = library.settings();

    if let Some(cloud) = args.get("cloud").filter(|v| v.is_object()) {
        if let Some(url) = cloud["baseUrl"].as_str() {
            settings.cloud.base_url = url.trim().to_string();
        }
        if let Some(model) = cloud["model"].as_str() {
            settings.cloud.model = model.trim().to_string();
        }
        if let Some(model) = cloud["visionModel"].as_str() {
            settings.cloud.vision_model = model.trim().to_string();
        }
        // An absent key leaves the stored one alone, so a front end that
        // never receives the key cannot wipe it by sending the form back.
        if let Some(key) = cloud["apiKey"].as_str() {
            settings.cloud.api_key = key.trim().to_string();
        }
    }
    if let Some(language) = args["language"].as_str() {
        settings.language = Lang::parse(Some(language)).id().to_string();
    }
    if let Some(media) = args["draftMedia"].as_str() {
        let media = vb_export::DraftMedia::parse(media)
            .ok_or_else(|| format!("unknown draft media {media:?}"))?;
        settings.draft_media = media.id().to_string();
    }
    if let Some(on) = args["thermalGuard"].as_bool() {
        settings.thermal_guard = Some(on);
        // A run already going follows it from its next unit of work.
        crate::thermal::set_enabled(on);
    }
    if let Some(editors) = args["defaultEditors"].as_array() {
        let mut ids = Vec::new();
        for value in editors {
            let id = value.as_str().unwrap_or_default();
            editor_by_id(id).ok_or_else(|| format!("unknown editor {id:?}"))?;
            ids.push(id.to_string());
        }
        settings.default_editors = ids;
    }

    library.save_settings(&settings)?;
    Ok(settings_get())
}

/// Try the configured endpoint, so a typo is found here rather than in the
/// middle of a run.
pub fn cloud_check() -> Result<Value, String> {
    let cloud = Library::open().settings().cloud;
    let endpoint = vb_cloud::Endpoint {
        base_url: cloud.base_url.clone(),
        model: cloud.model.clone(),
        api_key: cloud.api_key.clone(),
    };
    match endpoint.check() {
        Ok(models) => Ok(json!({
            "reachable": true,
            "models": models.iter().take(50).collect::<Vec<_>>(),
            "hasChosenModel": models.contains(&cloud.model),
        })),
        Err(e) => Ok(json!({ "reachable": false, "error": e.to_string() })),
    }
}

pub fn models_list() -> Value {
    let library = Library::open();
    models::listing_in(&library, library.settings().lang())
}

/// What a piece of music is, before it is used: how long, and how fast.
///
/// Shown when the user picks music, so "120 BPM · 3:04" appears beside the
/// file straight away, and a track with no steady beat is found out here
/// rather than after a whole run.
pub fn music_inspect(args: &Value) -> Result<Value, String> {
    let path = args["path"].as_str().ok_or("which file?")?;
    let lang = Library::open().settings().lang();
    let media = vb_media::default_backend();
    let audio = media
        .decode_audio(
            Path::new(path),
            vb_perception::beats::ANALYSIS_RATE,
            Some(vb_contracts::Micros::from_secs(120)),
        )
        .map_err(|e| match lang {
            Lang::En => format!("{path} could not be read as audio: {e}"),
            Lang::Zh => format!("无法把 {path} 当作音频读取：{e}"),
        })?;
    let beat = vb_perception::detect_beats(&audio.samples, audio.sample_rate);
    Ok(json!({
        "path": path,
        "durationUs": audio.total_duration.map(|d| d.0),
        "bpm": beat.as_ref().map(|b| (b.bpm * 10.0).round() / 10.0),
        "confidence": beat.as_ref().map(|b| b.confidence),
        // Below this the tempo is a guess, and the UI says the music has no
        // clear beat rather than quoting a number.
        "steady": beat.as_ref().is_some_and(|b| b.confidence >= 0.35),
    }))
}

/// How much earlier analysis the library keeps.
///
/// The analysis of every clip ever run is kept, keyed by the clip's content,
/// so running it again costs nothing. What is shown here is what clearing it
/// would give back.
pub fn cache_info() -> Value {
    let library = Library::open();
    let cache = crate::Cache::new(library.cache_dir()).size();
    let stills = dir_size(&analysis_stills(&library));
    json!({
        "entries": cache.entries,
        "bytes": cache.bytes + stills,
        "path": library.cache_dir(),
    })
}

/// Forget every earlier analysis, so the next run of any clip looks at it
/// afresh. Projects, their drafts, the album and the models are kept; the
/// album's faces live apart from the stills removed here.
pub fn cache_clear() -> Result<Value, String> {
    if jobs::list()
        .iter()
        .any(|j| j["kind"] == "run" && j["finished"] == false)
    {
        return Err(Library::open()
            .settings()
            .lang()
            .t(
                "A run is using the analysis now; stop it or let it finish first.",
                "正在运行的项目在用这些分析结果，先停止或等它完成。",
            )
            .to_string());
    }
    let library = Library::open();
    let before = cache_info();
    crate::Cache::new(library.cache_dir())
        .clear()
        .map_err(|e| e.to_string())?;
    for dir in analysis_stills(&library).iter() {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    Ok(json!({ "cleared": before["entries"], "bytes": before["bytes"] }))
}

/// The stills a cached analysis points at: cleared with it, since a graph
/// without its stills and stills without a graph are both useless.
fn analysis_stills(library: &Library) -> Vec<PathBuf> {
    let work = library.work_dir();
    vec![work.join("keyframes"), work.join("judge")]
}

fn dir_size(dirs: &[PathBuf]) -> u64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => walk(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    }
    dirs.iter().map(|d| walk(d)).sum()
}

/// What a run is about to do that the user should decide first.
///
/// Today that is music shorter than the cut: the end of the cut would be
/// silent, and whether to play the song again is the user's call, asked
/// before the run rather than discovered in the editor.
pub fn run_preflight(args: &Value) -> Result<Value, String> {
    let library = Library::open();
    let request = parse_request(&library, args)?;
    let store = vb_narrative::PriorStore::with_builtins();
    let empty = vb_contracts::FootageGraph::new("preflight");
    let template = match &request.template_id {
        Some(id) => store.get(id).cloned(),
        None => store.template_for_style(&request.style.style(), &empty),
    };
    // With no length chosen a 主体衔接 cut is as long as its action, which is
    // unknown until analysis: there is no length to compare the music with.
    let continuity = template
        .as_ref()
        .is_some_and(|t| vb_narrative::continuity::is_continuity(&t.params.extra));
    let cut = match request.style.target_duration_us.filter(|d| *d > 0) {
        Some(d) => Some(d),
        None if continuity => None,
        None => template.as_ref().map(|t| t.params.target_duration_us.0),
    };
    let music = request
        .style
        .music
        .as_deref()
        .filter(|m| !m.is_empty())
        .and_then(|m| {
            vb_media::default_backend()
                .decode_audio(
                    Path::new(m),
                    vb_perception::beats::ANALYSIS_RATE,
                    Some(vb_contracts::Micros::from_secs(1)),
                )
                .ok()
                .and_then(|a| a.total_duration)
        })
        .map(|d| d.0);
    let short = matches!((cut, music), (Some(c), Some(m)) if m < c);
    Ok(json!({
        "cutDurationUs": cut,
        "musicDurationUs": music,
        "musicShort": short,
        "musicLoop": request.style.music_loop,
        // Asked only when it matters and the user has not already said.
        "askMusicLoop": short && request.style.music_loop.is_none(),
    }))
}

/// Download a model in the background. Returns a job id.
pub fn model_download(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which model?")?.to_string();
    let model = models::spec(&id).ok_or_else(|| format!("there is no model {id}"))?;
    let library = Library::open();
    library.ensure().map_err(|e| e.to_string())?;

    if models::is_installed(&library, model) {
        return Ok(json!({ "jobId": Value::Null, "alreadyInstalled": true }));
    }

    let job_id = jobs::start("download", move |job| {
        let library = Library::open();
        let model = models::spec(&id).ok_or("the model vanished from the catalogue")?;
        // The weights, then the projector when there is one, as one bar.
        let files = models::files_of(model);
        let total = models::total_bytes(model).max(1);
        let mut before = 0u64;
        let mut fetched = Vec::new();
        for file in &files {
            let dest = library.models_dir().join(file.file_name);
            let already = std::fs::metadata(&dest)
                .map(|m| m.len() == file.bytes)
                .unwrap_or(false);
            if !already {
                let outcome =
                    vb_cloud::fetch_to_file(file.url, &dest, Some(file.sha256), &|done, _| {
                        let done = before + done;
                        job.say(
                            crate::RunStage::Download,
                            (done as f64 / total as f64) * 100.0,
                            format!("{} of {}", megabytes(done), megabytes(total)),
                        );
                        !job.is_cancelled()
                    })
                    .map_err(|e| e.to_string())?;
                fetched.push(json!({ "path": outcome.path, "sha256": outcome.sha256 }));
            }
            before += file.bytes;
        }
        Ok(json!({
            "id": model.id,
            "path": models::path_of(&library, model),
            "bytes": total,
            "files": fetched,
        }))
    });
    Ok(json!({ "jobId": job_id }))
}

/// Bring models in from a folder: a model pack copied onto a machine with
/// no network. Every file the catalogue knows, in the folder or one level
/// down, is copied into the library and checked against its SHA-256 on the
/// way; anything else is left where it is. Returns a job id.
pub fn model_import(args: &Value) -> Result<Value, String> {
    let dir = PathBuf::from(args["dir"].as_str().ok_or("which folder?")?);
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let library = Library::open();
    library.ensure().map_err(|e| e.to_string())?;

    let job_id = jobs::start("download", move |job| {
        let library = Library::open();
        let found = models::found_in(&dir);
        let total: u64 = found.iter().map(|(_, f)| f.bytes).sum::<u64>().max(1);
        let mut done = 0u64;
        let (mut imported, mut already, mut failed) = (Vec::new(), Vec::new(), Vec::new());
        for (path, file) in &found {
            if job.is_cancelled() {
                return Err("cancelled".into());
            }
            let dest = library.models_dir().join(file.file_name);
            if std::fs::metadata(&dest).is_ok_and(|m| m.len() == file.bytes) {
                already.push(file.file_name);
                done += file.bytes;
                continue;
            }
            job.say(
                crate::RunStage::Download,
                done as f64 / total as f64 * 100.0,
                format!(
                    "{} · {} of {}",
                    file.file_name,
                    megabytes(done),
                    megabytes(total)
                ),
            );
            match models::copy_checked(path, &dest, file, &|copied| {
                job.say(
                    crate::RunStage::Download,
                    (done + copied) as f64 / total as f64 * 100.0,
                    format!(
                        "{} · {} of {}",
                        file.file_name,
                        megabytes(done + copied),
                        megabytes(total)
                    ),
                );
                !job.is_cancelled()
            }) {
                Ok(()) => imported.push(file.file_name),
                Err(e) => failed.push(json!({ "file": file.file_name, "error": e })),
            }
            done += file.bytes;
        }
        Ok(json!({
            "imported": imported,
            "alreadyInstalled": already,
            "failed": failed,
            "found": found.len(),
        }))
    });
    Ok(json!({ "jobId": job_id }))
}

pub fn model_delete(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which model?")?;
    models::remove(&Library::open(), id)?;
    Ok(json!({ "deleted": id }))
}

/// Run this installed model for its role from now on, rather than the most
/// capable one. Returns the listing, which marks it in use.
pub fn model_use(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which model?")?;
    let library = Library::open();
    models::choose(&library, id, library.settings().lang())?;
    Ok(models_list())
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}

pub fn entities_list() -> Value {
    let library = Library::open();
    entities::listing_in(&library, library.settings().lang())
}

pub fn entity_rename(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which subject?")?;
    entities::rename(&Library::open(), id, args["label"].as_str())
}

pub fn entity_forget(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which subject?")?;
    entities::forget(&Library::open(), id)?;
    Ok(json!({ "forgotten": id }))
}

/// Open a path in the desktop's own file manager, or an editor by id.
///
/// Desktop only, and a no-op elsewhere: a mobile app cannot hand a folder to
/// anything.
pub fn reveal(args: &Value) -> Result<Value, String> {
    let path = args["path"].as_str().ok_or("which path?")?;
    if !Path::new(path).exists() {
        return Err(format!("{path} is not there"));
    }
    open_with_os(path)
}

pub fn editor_open(args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("which editor?")?;
    let editor = editor_by_id(id).ok_or_else(|| format!("unknown editor {id:?}"))?;
    let install = vb_export::detect_editors()
        .into_iter()
        .find(|i| i.editor == editor)
        .ok_or("that editor was not found")?;
    let app = install
        .app_path
        .ok_or_else(|| format!("{} is not installed", editor.display_name()))?;
    let opened = open_with_os(&app.to_string_lossy())?;
    Ok(json!({
        "opened": opened["opened"],
        // Worth repeating at the moment it matters: the editor reads its
        // draft folder at startup.
        "restartNeeded": install.running,
    }))
}

fn open_with_os(target: &str) -> Result<Value, String> {
    if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("could not open {target}: {e}"))?;
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map_err(|e| format!("could not open {target}: {e}"))?;
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| format!("could not open {target}: {e}"))?;
    } else {
        return Err("this platform cannot open a folder for you".to_string());
    }
    Ok(json!({ "opened": target }))
}

// ---------------------------------------------------------------------------
// The dispatcher
// ---------------------------------------------------------------------------

/// The project an editor call is about.
fn project_id(args: &Value) -> Result<&str, String> {
    args["projectId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "which project?".to_string())
}

/// Every operation, by name.
///
/// One entry point rather than one C function and one HTTP route per
/// operation: the transports stay fixed while the operation set grows, which
/// is what keeps the three front ends from drifting apart.
pub fn call(method: &str, args: &Value) -> Result<Value, String> {
    match method {
        "version" => Ok(version()),
        "doctor" => Ok(doctor()),
        "selftest" => Ok(match args["lang"].as_str() {
            Some(lang) => selftest_in(Lang::parse(Some(lang))),
            None => selftest(),
        }),
        "templates" => Ok(templates()),
        "browse" => Ok(browse_for(
            args["dir"].as_str(),
            BrowseKind::parse(args["kind"].as_str()),
        )),
        "media.inspect" => media_inspect(args),
        "media.thumbnails" => media_thumbnails(args),

        "projects.list" => Ok(projects_list()),
        "projects.create" => project_create(args),
        "projects.get" => project_get(args),
        "projects.update" => project_update(args),
        "projects.delete" => project_delete(args),

        "run.start" => run_start(args),
        "run.preflight" => run_preflight(args),
        "run.status" | "jobs.status" => {
            let id = args["jobId"].as_str().ok_or("which job?")?;
            jobs::status(id).ok_or_else(|| format!("there is no job {id}"))
        }
        "run.cancel" | "jobs.cancel" => {
            let id = args["jobId"].as_str().ok_or("which job?")?;
            if jobs::cancel(id) {
                Ok(json!({ "cancelling": id }))
            } else {
                Err(format!("there is no job {id}"))
            }
        }
        "jobs.list" => Ok(json!({ "jobs": jobs::list() })),

        "cache.info" => Ok(cache_info()),
        "cache.clear" => cache_clear(),

        "models.list" => Ok(models_list()),
        "music.inspect" => music_inspect(args),
        "models.download" => model_download(args),
        "models.delete" => model_delete(args),
        "models.import" => model_import(args),
        "models.use" => model_use(args),

        "settings.get" => Ok(settings_get()),
        "settings.update" => settings_update(args),
        "cloud.check" => cloud_check(),

        "entities.list" => Ok(entities_list()),
        "entities.rename" => entity_rename(args),
        "entities.forget" => entity_forget(args),
        "entities.merge" => {
            let from = args["id"].as_str().ok_or("which subject?")?;
            let into = args["into"].as_str().ok_or("merged into which subject?")?;
            crate::entities::merge(&Library::open(), from, into)
        }

        "hosting.get" => Ok(hosting_get()),
        "hosting.start" => hosting_start(args),
        "hosting.update" => hosting_update(args),
        "hosting.category" => {
            let library = Library::open();
            let id = args["id"].as_str().ok_or("which category?")?;
            crate::hosting::category(&library, id, library.settings().lang())
        }
        "hosting.person" => {
            let library = Library::open();
            let id = args["id"].as_str().ok_or("which person?")?;
            crate::hosting::person(&library, id, library.settings().lang())
        }
        "hosting.removeCategory" | "hosting.restoreCategory" => {
            let library = Library::open();
            let id = args["id"].as_str().ok_or("which category?")?;
            crate::hosting::set_removed(&library, id, method == "hosting.removeCategory")?;
            Ok(hosting_get())
        }
        "memories.list" => {
            let library = Library::open();
            let kind = memory_kind(args)?;
            Ok(crate::memories::listing(
                &library,
                kind,
                library.settings().lang(),
            ))
        }
        "memories.get" => {
            let library = Library::open();
            let id = args["id"].as_str().ok_or("which memory?")?;
            crate::memories::get(&library, id, library.settings().lang())
        }
        "memories.update" => {
            let library = Library::open();
            let id = args["id"].as_str().ok_or("which memory?")?;
            if let Some(order) = args["order"].as_str() {
                crate::memories::set_order(&library, id, order)?;
            }
            Ok(start_hosting(true, Some(vec![id.to_string()]), None))
        }
        "memories.generate" => {
            let id = args["id"].as_str().ok_or("which memory?")?;
            Ok(start_hosting(true, Some(vec![id.to_string()]), None))
        }
        "geo.world" => Ok(geo_world()),

        "editor.open" => {
            let library = Library::open();
            let id = project_id(args)?;
            crate::editor::open(
                &library,
                id,
                args["restart"].as_bool().unwrap_or(false),
                library.settings().lang(),
            )
        }
        "editor.state" => {
            let library = Library::open();
            crate::editor::state(&library, project_id(args)?, None, library.settings().lang())
        }
        "editor.apply" => {
            let library = Library::open();
            let op = args.get("op").ok_or("which edit?")?;
            crate::editor::apply(&library, project_id(args)?, op, library.settings().lang())
        }
        "editor.undo" | "editor.redo" => {
            let library = Library::open();
            crate::editor::undo(
                &library,
                project_id(args)?,
                method == "editor.redo",
                library.settings().lang(),
            )
        }
        "editor.revert" => {
            let library = Library::open();
            crate::editor::revert(&library, project_id(args)?, library.settings().lang())
        }
        "editor.frame" => {
            let library = Library::open();
            crate::editor::frame(
                &library,
                project_id(args)?,
                vb_contracts::Micros(args["atUs"].as_u64().unwrap_or(0)),
                args["maxSide"].as_u64().unwrap_or(640) as u32,
                library.settings().lang(),
            )
        }
        "editor.strip" => {
            let library = Library::open();
            let items: Vec<(String, vb_contracts::Micros)> = args["items"]
                .as_array()
                .ok_or("which pictures?")?
                .iter()
                .filter_map(|item| {
                    Some((
                        item["sourceId"].as_str()?.to_string(),
                        vb_contracts::Micros(item["atUs"].as_u64().unwrap_or(0)),
                    ))
                })
                .collect();
            crate::editor::strip(
                &library,
                project_id(args)?,
                &items,
                args["height"].as_u64().unwrap_or(64) as u32,
                library.settings().lang(),
            )
        }
        "editor.footage" => {
            let library = Library::open();
            crate::editor::footage(&library, project_id(args)?, library.settings().lang())
        }
        "editor.export" => {
            let library = Library::open();
            crate::editor::export(&library, project_id(args)?, args, library.settings().lang())
        }
        "editor.lossless" => {
            let library = Library::open();
            crate::editor::lossless(&library, project_id(args)?, library.settings().lang())
        }
        "editor.close" => {
            crate::editor::close(project_id(args)?);
            Ok(json!({ "closed": true }))
        }

        "editors.list" => Ok(editors()),
        "editors.open" => editor_open(args),
        "reveal" => reveal(args),

        other => Err(format!("unknown method {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_music_picker_offers_only_the_music_folders_that_exist() {
        let home = std::env::temp_dir().join(format!("vb-music-places-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Music")).unwrap();
        std::fs::create_dir_all(home.join("netease/cloudmusic/Music")).unwrap();
        std::fs::create_dir_all(home.join("Download")).unwrap();

        let ids = |places: Vec<Value>| -> Vec<String> {
            places
                .iter()
                .map(|p| p["id"].as_str().unwrap().to_string())
                .collect()
        };
        // A phone: its own folders and the music apps' that are there, in
        // order; QQ Music, Kugou and Kuwo are not installed.
        assert_eq!(
            ids(music_places(&home, true)),
            ["music", "downloads", "netease"]
        );
        let netease = &music_places(&home, true)[2];
        assert!(
            netease["path"]
                .as_str()
                .unwrap()
                .ends_with("netease/cloudmusic/Music")
        );
        // A desktop has Downloads, not Download.
        assert_eq!(ids(music_places(&home, false)), ["music"]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_plugged_in_disk_is_a_shortcut_by_its_name_and_the_startup_disk_is_not() {
        let volumes = std::env::temp_dir().join(format!("vb-volumes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&volumes);
        for dir in [
            "PIC",
            "PIC2",
            ".hidden",
            "com.apple.TimeMachine.localsnapshots",
        ] {
            std::fs::create_dir_all(volumes.join(dir)).unwrap();
        }
        std::fs::write(volumes.join("notes.txt"), b"x").unwrap();
        // As a Mac mounts its startup disk in /Volumes: a link back to /.
        #[cfg(unix)]
        std::os::unix::fs::symlink("/", volumes.join("Macintosh HD")).unwrap();

        let found = volumes_in(&volumes);
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["PIC", "PIC2"]);

        // The footage picker offers the usual folders that exist, then the
        // disks by name.
        let home = volumes.join("PIC");
        std::fs::create_dir_all(home.join("Movies")).unwrap();
        let places = footage_places(&home, false, &found);
        let shown: Vec<(&str, Option<&str>)> = places
            .iter()
            .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("home", None),
                ("movies", None),
                ("volume", Some("PIC")),
                ("volume", Some("PIC2")),
            ]
        );
        assert_eq!(
            places[3]["path"].as_str().unwrap(),
            volumes.join("PIC2").to_string_lossy()
        );
        assert_eq!(places[0]["path"].as_str().unwrap(), home.to_string_lossy());
        let _ = std::fs::remove_dir_all(&volumes);
    }

    #[test]
    fn a_folder_macos_will_not_let_us_read_says_where_to_allow_it() {
        // EPERM is macOS's privacy refusing; EACCES a folder's own modes.
        let denied = std::io::Error::from_raw_os_error(1);
        let said = unreadable(Path::new("/Volumes/Media"), &denied, Lang::Zh);
        if cfg!(target_os = "macos") {
            assert!(said.contains("可移除的宗卷"), "{said}");
            assert!(said.contains("「Media」"), "{said}");
        }
        let locked = std::io::Error::from_raw_os_error(13);
        let said = unreadable(Path::new("/x/y"), &locked, Lang::En);
        assert!(said.starts_with("cannot read /x/y"), "{said}");
    }

    #[test]
    fn version_reports_both_contracts() {
        let v = version();
        assert_eq!(v["footageGraph"], "footage-graph/0.1");
        assert_eq!(v["editingDsl"], "editing-dsl/0.1");
    }

    #[test]
    fn doctor_never_claims_a_network_is_needed() {
        assert_eq!(doctor()["requiresNetwork"], false);
    }

    #[test]
    fn doctor_reports_the_media_license_alongside_whether_it_can_ship() {
        let d = doctor();
        match d["mediaLicense"].as_str() {
            Some(license) => assert_eq!(
                d["mediaShippable"],
                vb_media::is_shippable_license(license),
                "shippability must follow from the license string"
            ),
            // No ffmpeg at all: nothing to judge, and nothing claimed.
            None => assert!(d["mediaShippable"].is_null()),
        }
    }

    #[test]
    fn doctor_names_the_backend_this_build_selects() {
        assert_eq!(
            doctor()["mediaBackend"],
            vb_media::default_backend().name(),
            "the app must see the backend that would actually run"
        );
    }

    #[test]
    fn doctor_lists_every_adapter_with_its_mock_status() {
        let d = doctor();
        let adapters = d["adapters"].as_array().unwrap();
        let roles: Vec<&str> = adapters
            .iter()
            .map(|a| a["role"].as_str().unwrap())
            .collect();
        // The composition judge is listed exactly when it is installed:
        // its absence is a cut made once, not a mock standing in.
        let library = Library::open();
        let judged = models::local_for(&library, models::Role::Composition).is_some();
        let captioned = models::installed_for(&library, models::Role::Describe).is_some();
        let mut expected = vec!["Speech", "Vision", "Tracking", "Framing"];
        if judged {
            expected.push("Composition");
        }
        if captioned {
            expected.push("Captions");
        }
        expected.push("Reasoning");
        assert_eq!(roles, expected);

        // Every entry says whether it is a mock, and says it truthfully: the
        // flag is the adapter's own, not a label written beside it. The UI
        // depends on this to keep a structural draft from passing as a cut.
        let real = adapters_for(&library, ModelChoice::Local, &library.work_dir(), Lang::En);
        assert_eq!(adapters[0]["mock"], real.asr.is_mock());
        assert_eq!(adapters[1]["mock"], real.vlm.is_mock());
        assert_eq!(adapters[2]["mock"], real.tracker.is_mock());
        assert_eq!(adapters[0]["name"], real.asr.name());
    }

    #[test]
    #[cfg(target_vendor = "apple")]
    fn on_apple_platforms_vision_is_real_from_the_start() {
        let d = doctor();
        assert_eq!(d["adapters"][1]["name"], "AppleVision");
        assert_eq!(d["adapters"][1]["mock"], false);
    }

    #[test]
    fn clips_are_named_by_their_shape() {
        assert_eq!(ratio_label(1920, 1080), "16:9");
        assert_eq!(ratio_label(1080, 1920), "9:16");
        assert_eq!(ratio_label(1440, 1080), "4:3");
        assert_eq!(ratio_label(1080, 1080), "1:1");
        // A phone's full-screen recording is none of the common shapes.
        assert_eq!(ratio_label(2400, 1080), "20:9");
        assert_eq!(orientation(1920, 1080), "landscape");
        assert_eq!(orientation(1080, 1920), "portrait");
        assert_eq!(orientation(1080, 1100), "square");
        assert_eq!(orientation(0, 0), "unknown");
    }

    #[test]
    fn inspecting_says_when_and_how_each_clip_was_filmed() {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
        if !sample.exists() || !vb_media::default_backend().is_available() {
            eprintln!("skipping: needs ffmpeg and test-media/sample.mp4");
            return;
        }
        let out = media_inspect(&json!({
            "paths": [sample.to_string_lossy(), "/nonexistent/clip.mp4"],
        }))
        .unwrap();
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "every file answers, readable or not");
        let clip = &items[0];
        assert_eq!(clip["orientation"], "landscape");
        assert_eq!(clip["ratio"], "16:9");
        assert!(clip["durationUs"].as_u64().unwrap() > 20_000_000);
        // The fixture is untagged, so its date is the file's, and says so.
        assert_eq!(clip["capturedFrom"], "file");
        assert!(clip["capturedAt"].as_str().is_some());
        assert!(items[1]["error"].as_str().is_some());
    }

    #[test]
    fn a_cut_can_be_handed_to_a_phone_editor_clip_by_clip() {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
        if !sample.exists() || !vb_media::default_backend().is_available() {
            eprintln!("skipping: needs ffmpeg and test-media/sample.mp4");
            return;
        }
        let dir = std::env::temp_dir().join(format!("vb-handoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = run(
            &[sample.to_string_lossy().to_string()],
            Some(&dir.to_string_lossy()),
            None,
        )
        .unwrap();
        let clips = out["handoff"]["clips"].as_array().unwrap();
        let slots: Vec<&Value> = out["slots"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind"] == "clip")
            .collect();
        // Every clip of the cut, in its order, with the span it plays.
        assert!(!clips.is_empty());
        assert_eq!(clips.len(), slots.len());
        for (clip, slot) in clips.iter().zip(&slots) {
            assert!(clip["path"].as_str().unwrap().ends_with("sample.mp4"));
            assert_eq!(clip["inUs"], slot["sourceInUs"]);
            assert_eq!(
                clip["outUs"].as_u64().unwrap() - clip["inUs"].as_u64().unwrap(),
                slot["durationUs"].as_u64().unwrap()
            );
        }
        // The frame the editor should take, and each clip's own.
        assert_eq!(out["handoff"]["frame"]["width"], 1280);
        assert_eq!(out["handoff"]["frame"]["height"], 720);
        assert_eq!(clips[0]["width"], 1280);
        // What cannot go with them is counted, not dropped silently.
        assert!(out["handoff"]["titleCards"].as_u64().is_some());
        assert!(out["handoff"]["texts"].as_u64().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_video_has_a_thumbnail_the_right_way_up_and_it_is_kept() {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
        if !sample.exists() || !vb_media::default_backend().is_available() {
            eprintln!("skipping: needs ffmpeg and test-media/sample.mp4");
            return;
        }
        use base64::Engine as _;
        let args = json!({
            "paths": [sample.to_string_lossy(), "/nonexistent/clip.mp4"],
            "width": 160,
        });
        let first = media_thumbnails(&args).unwrap();
        let items = first["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "every file answers");
        let png = base64::engine::general_purpose::STANDARD
            .decode(items[0]["png"].as_str().unwrap())
            .unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let info = decoder.read_info().unwrap().info().clone();
        // The fixture is 16:9, so 160 wide is 90 high.
        assert_eq!((info.width, info.height), (160, 90));
        assert!(items[1]["error"].as_str().is_some());
        // Asked again, the same picture comes from the library.
        let again = media_thumbnails(&args).unwrap();
        assert_eq!(again["items"][0]["png"], items[0]["png"]);
    }

    #[test]
    fn templates_come_back_as_a_list() {
        assert!(templates().as_array().unwrap().len() >= 9);
    }

    #[test]
    fn browsing_a_directory_lists_its_contents() {
        let dir = std::env::temp_dir().join("vb-api-browse");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"x").unwrap();
        std::fs::write(dir.join("IMG_0001.HEIC"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join(".hidden.mp4"), b"x").unwrap();

        let listing = browse(Some(&dir.to_string_lossy()));
        let kind = |name: &str| {
            listing["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == name)
                .map(|e| e["kind"].as_str().unwrap().to_string())
        };
        assert_eq!(kind("clip.mp4").as_deref(), Some("video"));
        assert_eq!(kind("IMG_0001.HEIC").as_deref(), Some("image"));
        let names: Vec<&str> = listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();

        assert!(names.contains(&"sub"), "directories should be listed");
        assert!(names.contains(&"clip.mp4"), "videos should be listed");
        assert!(names.contains(&"IMG_0001.HEIC"), "photos should be listed");
        assert!(
            !names.contains(&"notes.txt"),
            "files that are not footage are noise in a picker"
        );
        assert!(!names.contains(&".hidden.mp4"), "hidden entries are noise");
        // Directories sort before files so a picker reads top-down.
        assert_eq!(names[0], "sub");
    }

    #[test]
    fn browsing_a_file_reports_an_error_rather_than_an_empty_listing() {
        let file = std::env::temp_dir().join("vb-api-not-a-dir.mp4");
        std::fs::write(&file, b"x").unwrap();
        let listing = browse(Some(&file.to_string_lossy()));
        assert!(listing["error"].is_string());
        // A parent is still offered, so the picker is not a dead end.
        assert!(listing["parent"].is_string());
    }

    #[test]
    #[cfg(unix)]
    fn a_directory_that_cannot_be_read_says_so_rather_than_looking_empty() {
        use std::os::unix::fs::PermissionsExt;

        // The Android failure mode: the picker lists nothing, and without this
        // the UI cannot tell "no videos here" from "not allowed to look".
        let dir = std::env::temp_dir().join("vb-api-unreadable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut perms = std::fs::metadata(&dir).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&dir, perms).unwrap();

        let listing = browse(Some(&dir.to_string_lossy()));
        // Running as root defeats the permission bits, so only assert when the
        // directory really is unreadable.
        if std::fs::read_dir(&dir).is_err() {
            assert!(
                listing["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(&*dir.to_string_lossy()),
                "expected a reason, got {listing}"
            );
            assert!(
                listing["parent"].is_string(),
                "the picker must not dead-end"
            );
        }
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browsing_nothing_falls_back_to_a_real_directory() {
        let listing = browse(None);
        assert!(listing["path"].is_string());
        assert!(listing["error"].is_null());
    }

    #[test]
    fn a_folder_expands_into_the_videos_and_photos_inside_it() {
        let dir = std::env::temp_dir().join("vb-api-expand");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("day2")).unwrap();
        std::fs::create_dir_all(dir.join("_vbcache")).unwrap();
        for name in ["b.mp4", "a.mov", "p.jpg", "notes.txt", ".hidden.mp4"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::write(dir.join("day2/c.mp4"), b"x").unwrap();
        // A previous run's output must never be read back in as footage.
        std::fs::write(dir.join("_vbcache/cached.mp4"), b"x").unwrap();

        let found = expand_inputs(&[dir.to_string_lossy().to_string()]).unwrap();
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains(&"a.mov".to_string()));
        assert!(names.contains(&"b.mp4".to_string()));
        assert!(names.contains(&"c.mp4".to_string()), "subfolders count too");
        assert!(names.contains(&"p.jpg".to_string()), "photos are footage");
        assert!(!names.contains(&"notes.txt".to_string()));
        assert!(!names.contains(&"\u{2e}hidden.mp4".to_string()));
        assert!(
            !names.contains(&"cached.mp4".to_string()),
            "a run's own output is not footage"
        );
        assert_eq!(found.len(), 4);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_photo_is_inspected_by_its_exif_the_right_way_up() {
        let photo =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/photo-rotated.jpg");
        if !photo.exists() || !vb_media::default_backend().is_available() {
            eprintln!("skipping: needs ffmpeg and test-media/photo-rotated.jpg");
            return;
        }
        let args = json!({ "paths": [photo.to_string_lossy()] });
        let item = &media_inspect(&args).unwrap()["items"][0];
        assert_eq!(item["kind"], "image");
        assert_eq!(item["capturedAt"], "2023-09-29T18:31:43+08:00");
        assert_eq!(item["capturedFrom"], "camera");
        // Stored 800 x 600 on its side; shown 600 x 800.
        assert_eq!(item["width"], 600);
        assert_eq!(item["height"], 800);
        assert_eq!(item["orientation"], "portrait");
        assert_eq!(item["durationUs"], 0);
    }

    #[test]
    fn naming_a_file_twice_analyses_it_once() {
        let dir = std::env::temp_dir().join("vb-api-dedup");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("a.mp4");
        std::fs::write(&clip, b"x").unwrap();

        let named = clip.to_string_lossy().to_string();
        let found =
            expand_inputs(&[named.clone(), named, dir.to_string_lossy().to_string()]).unwrap();
        assert_eq!(found.len(), 1, "got {found:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_run_writes_beside_the_footage_unless_the_host_says_otherwise() {
        let input = Path::new("/movies/holiday.mp4");
        assert_eq!(
            default_project_dir(input, None),
            PathBuf::from("/movies/vb-project")
        );
        // Android: shared storage is readable but not writable by the app, so
        // the project has to land in a directory the app owns.
        assert_eq!(
            default_project_dir(input, Some(PathBuf::from("/data/data/app/files"))),
            PathBuf::from("/data/data/app/files/vb-project")
        );
    }

    #[test]
    fn running_a_missing_file_is_an_error_not_a_panic() {
        let err = run(&["/definitely/not/here.mp4".to_string()], None, None).unwrap_err();
        assert!(err.contains("does not exist"));
    }
}
