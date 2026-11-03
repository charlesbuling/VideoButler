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

//! The built-in editor: a project's cut, opened for a person to finish.
//!
//! From 0.2.0 a cut no longer has to leave the app to be finished. The
//! orchestrator's cut opens here, a person changes it -- clips split,
//! trimmed, moved, slowed down, text and music laid on -- and it leaves as
//! a video, or as a draft for another editor, as before. Every change is an
//! operation of `vb-edit` on the Editing DSL, so the cut stays a decision
//! manifest from first to last: nothing is rendered until the person asks
//! for a video, and nothing they did cannot be undone.
//!
//! One session per project, held by the engine while the app has it open
//! and kept on disk after every change, under the project's `edit/`:
//!
//! - `original.editing-dsl.json`: the cut the orchestrator made, which the
//!   session started from -- what the change ratio is measured against and
//!   what going back to the AI's cut restores.
//! - `cut.editing-dsl.json`: the cut as the person has it now.
//! - `footage-graph.json`: the analysis both refer to, copied, because a
//!   new run of the project rewrites its own and may number its sources
//!   differently.
//! - `session.json`: which run it started from, so a newer run can be
//!   offered rather than silently replacing the person's work.
//!
//! Undo and redo are whole cuts, in memory, up to [`HISTORY`] steps: a cut
//! is small, an operation cheap, and a whole cut cannot be undone halfway.

use crate::i18n::Lang;
use crate::jobs;
use crate::library::Library;
use crate::progress::{Observer, Progress, RunStage};
use crate::projects::{self, ExportRecord, Project};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use vb_contracts::editing_dsl::{AudioKind, SlotKind};
use vb_contracts::{EditingDsl, FootageGraph, Micros};
use vb_edit::{EditError, Op, Selection};
use vb_export::{DraftWriter, Editor, OtioWriter};
use vb_media::{MediaBackend, VideoCodec};

/// Recorded in a cut's provenance once a person has changed it.
pub const EDITOR: &str = concat!("video-butler-editor@", env!("CARGO_PKG_VERSION"));

/// How many steps can be undone.
pub const HISTORY: usize = 200;

const ORIGINAL: &str = "original.editing-dsl.json";
const CUT: &str = "cut.editing-dsl.json";
const GRAPH: &str = "footage-graph.json";
const SESSION: &str = "session.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    /// The `at` of the run the session started from.
    based_on: String,
    created_at: String,
}

struct Step {
    cut: EditingDsl,
    label: String,
}

struct Session {
    project_id: String,
    dir: PathBuf,
    graph: Arc<FootageGraph>,
    original: EditingDsl,
    cut: EditingDsl,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Counts every change, so a front end can tell a frame it asked for
    /// before an edit from one after it.
    version: u64,
    based_on: String,
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Mutex<Session>>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<Session>>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A panic while editing must not take every later edit with it: the
    // data under the lock is a whole cut either way.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The directory a project's edit lives in.
pub fn edit_dir(library: &Library, project: &Project) -> PathBuf {
    project.dir(library).join("edit")
}

/// Whether a person has an edit of this project on disk.
pub fn has_edit(library: &Library, project: &Project) -> bool {
    edit_dir(library, project).join(CUT).exists()
}

/// The cut and the analysis a project's last run wrote.
fn run_files(project: &Project) -> Option<(PathBuf, PathBuf)> {
    let run = project.last_run.as_ref()?;
    if let (Some(dsl), Some(graph)) = (&run.dsl_path, &run.graph_path) {
        return Some((PathBuf::from(dsl), PathBuf::from(graph)));
    }
    // Before 0.2.0 a run did not say; its files are beside its draft, named
    // as the draft is.
    let draft = PathBuf::from(&run.draft_path);
    let stem = draft.file_stem()?.to_string_lossy().to_string();
    let dir = draft.parent()?;
    let dsl = dir.join(format!("{stem}.editing-dsl.json"));
    let graph = dir.join(format!("{stem}.footage-graph.json"));
    (dsl.exists() && graph.exists()).then_some((dsl, graph))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Written beside and moved into place, so a crash mid-write leaves the
/// last whole cut rather than half of the next one.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let part = path.with_extension("part");
    std::fs::write(&part, text).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether two cuts say the same thing, whatever their provenance says
/// about when they were made.
fn same_cut(a: &EditingDsl, b: &EditingDsl) -> bool {
    a.slots == b.slots && a.tracks == b.tracks && a.transitions == b.transitions && a.meta == b.meta
}

impl Session {
    /// A session from the project's last run, written to disk at once.
    fn start(library: &Library, project: &Project, lang: Lang) -> Result<Session, String> {
        let Some(run) = project.last_run.as_ref() else {
            return Err(lang
                .t(
                    "This project has no cut yet: make one first.",
                    "这个项目还没有成片，先生成一次再剪辑。",
                )
                .to_string());
        };
        let Some((dsl_path, graph_path)) = run_files(project) else {
            return Err(lang
                .t(
                    "The files of this project's last run are gone: make the cut again.",
                    "这个项目上次生成的文件找不到了，请重新生成一次。",
                )
                .to_string());
        };
        let original: EditingDsl = read_json(&dsl_path)?;
        let graph: FootageGraph = read_json(&graph_path)?;
        let dir = edit_dir(library, project);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_json(&dir.join(GRAPH), &graph)?;
        write_json(&dir.join(ORIGINAL), &original)?;
        write_json(&dir.join(CUT), &original)?;
        write_json(
            &dir.join(SESSION),
            &SessionFile {
                based_on: run.at.clone(),
                created_at: vb_contracts::now_rfc3339(),
            },
        )?;
        Ok(Session {
            project_id: project.id.clone(),
            dir,
            graph: Arc::new(graph),
            cut: original.clone(),
            original,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 1,
            based_on: run.at.clone(),
        })
    }

    /// The session a person left, from disk.
    fn load(library: &Library, project: &Project) -> Result<Session, String> {
        let dir = edit_dir(library, project);
        let file: SessionFile = read_json(&dir.join(SESSION))?;
        Ok(Session {
            project_id: project.id.clone(),
            graph: Arc::new(read_json(&dir.join(GRAPH))?),
            original: read_json(&dir.join(ORIGINAL))?,
            cut: read_json(&dir.join(CUT))?,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 1,
            based_on: file.based_on,
            dir,
        })
    }

    /// The cut on disk as it is now, stamped with who changed it and by how
    /// much; the project marked as edited.
    fn save(&self, library: &Library) -> Result<(), String> {
        let mut cut = self.cut.clone();
        let edited = !same_cut(&self.cut, &self.original);
        let provenance = cut.provenance.get_or_insert_with(Default::default);
        let now = vb_contracts::now_rfc3339();
        if edited {
            provenance.editor = Some(EDITOR.to_string());
            provenance.edited_at = Some(now.clone());
            provenance.change_ratio = Some(self.change_ratio());
        } else {
            provenance.editor = None;
            provenance.edited_at = None;
            provenance.change_ratio = None;
        }
        write_json(&self.dir.join(CUT), &cut)?;
        if let Some(mut project) = projects::get(library, &self.project_id) {
            project.edited_at = edited.then_some(now.clone());
            project.updated_at = now;
            project.save(library)?;
        }
        Ok(())
    }

    fn change_ratio(&self) -> f64 {
        (vb_edit::change_ratio(&self.original, &self.cut) * 1000.0).round() / 1000.0
    }

    /// Make `next` the cut, as one step that can be undone.
    fn commit(&mut self, library: &Library, next: EditingDsl, label: String) -> Result<(), String> {
        let previous = std::mem::replace(&mut self.cut, next);
        self.undo.push(Step {
            cut: previous,
            label,
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.version += 1;
        self.save(library)
    }
}

/// The session for a project: the one open, else the one on disk, else a
/// new one from its last run.
fn session_for(
    library: &Library,
    project_id: &str,
    lang: Lang,
) -> Result<Arc<Mutex<Session>>, String> {
    if let Some(open) = lock(sessions()).get(project_id) {
        return Ok(Arc::clone(open));
    }
    let project = projects::get(library, project_id).ok_or_else(|| no_project(project_id, lang))?;
    let session = if has_edit(library, &project) {
        match Session::load(library, &project) {
            Ok(session) => session,
            // An edit that cannot be read is not thrown away: it is moved
            // aside, and the cut starts again from the run.
            Err(e) => {
                tracing::warn!(error = %e, "an edit could not be read; starting again from the run");
                let dir = edit_dir(library, &project);
                let aside = dir.with_file_name(format!("edit-unreadable-{}", std::process::id()));
                let _ = std::fs::rename(&dir, aside);
                Session::start(library, &project, lang)?
            }
        }
    } else {
        Session::start(library, &project, lang)?
    };
    let mut open = lock(sessions());
    let entry = open
        .entry(project_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(session)));
    Ok(Arc::clone(entry))
}

fn no_project(id: &str, lang: Lang) -> String {
    match lang {
        Lang::En => format!("there is no project {id}"),
        Lang::Zh => "这个项目不存在了。".to_string(),
    }
}

/// Open a project's cut in the editor. `restart` throws the person's edit
/// away and starts again from the last run, keeping the old cut beside it.
pub fn open(
    library: &Library,
    project_id: &str,
    restart: bool,
    lang: Lang,
) -> Result<Value, String> {
    if restart {
        let project =
            projects::get(library, project_id).ok_or_else(|| no_project(project_id, lang))?;
        lock(sessions()).remove(project_id);
        let dir = edit_dir(library, &project);
        if dir.join(CUT).exists() {
            let _ = std::fs::copy(dir.join(CUT), dir.join("cut.previous.editing-dsl.json"));
        }
        let session = Session::start(library, &project, lang)?;
        session.save(library)?;
        lock(sessions()).insert(project_id.to_string(), Arc::new(Mutex::new(session)));
    }
    state(library, project_id, None, lang)
}

/// Let go of a project's session: it is on disk already.
pub fn close(project_id: &str) {
    lock(sessions()).remove(project_id);
}

/// What the editor shows, as a front end needs it.
pub fn state(
    library: &Library,
    project_id: &str,
    selection: Option<&Selection>,
    lang: Lang,
) -> Result<Value, String> {
    let session = session_for(library, project_id, lang)?;
    let s = lock(&session);
    let newer_run = projects::get(library, project_id)
        .and_then(|p| p.last_run)
        .is_some_and(|run| run.at != s.based_on);
    Ok(state_json(&s, selection, newer_run))
}

fn state_json(s: &Session, selection: Option<&Selection>, newer_run: bool) -> Value {
    let timeline = s.cut.resolve_timeline();
    let slots: Vec<Value> = timeline
        .slots
        .iter()
        .map(|r| {
            json!({
                "slotId": r.slot_id,
                "startUs": r.start_us.0,
                "endUs": r.end_us.0,
                "overlapUs": r.incoming_overlap_us.0,
            })
        })
        .collect();
    let texts: Vec<Value> = s
        .cut
        .tracks
        .text
        .iter()
        .filter_map(|t| {
            let start = timeline.resolve_anchor(&t.anchor)?;
            Some(json!({
                "id": t.id,
                "startUs": start.0,
                "endUs": (start + t.duration_us).0,
            }))
        })
        .collect();
    let audio: Vec<Value> = s
        .cut
        .tracks
        .audio
        .iter()
        .filter_map(|a| {
            let start = timeline.resolve_anchor(&a.anchor)?;
            let length = match a.duration_us {
                Some(d) => d,
                None => timeline.slot(&a.anchor.slot_id)?.duration(),
            };
            Some(json!({
                "id": a.id,
                "kind": a.kind,
                "slotId": (a.kind == AudioKind::Source).then_some(&a.anchor.slot_id),
                "startUs": start.0,
                "endUs": (start + length).0,
            }))
        })
        .collect();
    let used: HashSet<&str> = s
        .cut
        .slots
        .iter()
        .filter_map(|slot| slot.source_ref.as_ref().map(|r| r.source_id.as_str()))
        .collect();
    let sources: Vec<Value> = s
        .graph
        .sources
        .iter()
        .filter(|src| used.contains(src.id.as_str()))
        .map(source_json)
        .collect();
    let (width, height) = vb_render::frame_size(&s.cut, &s.graph, 1080);
    let edited = !same_cut(&s.cut, &s.original);
    json!({
        "projectId": s.project_id,
        "title": s.cut.meta.title,
        "version": s.version,
        "dsl": s.cut,
        "timeline": {
            "totalUs": timeline.total_duration_us.0,
            "slots": slots,
            "texts": texts,
            "audio": audio,
        },
        "sources": sources,
        "frame": { "width": width, "height": height },
        "fps": vb_render::frame_rate(&s.cut).0 as f64 / vb_render::frame_rate(&s.cut).1 as f64,
        "canUndo": !s.undo.is_empty(),
        "canRedo": !s.redo.is_empty(),
        "undoLabel": s.undo.last().map(|step| step.label.clone()),
        "redoLabel": s.redo.last().map(|step| step.label.clone()),
        "edited": edited,
        "changeRatio": if edited { s.change_ratio() } else { 0.0 },
        "newerRun": newer_run,
        // Whether this device can write a video at all: a build with no
        // video encoder (Android, Windows and Linux today) offers project
        // files only, rather than an export that fails when tapped.
        "canRenderVideo": vb_media::default_backend().can_encode(VideoCodec::H264),
        "selection": selection,
        "beat": s.graph.priors.as_ref().and_then(|p| p.beat_grid.as_ref()).map(|b| json!({
            "bpm": b.bpm,
            "firstBeatUs": b.first_beat_us.map(|m| m.0),
        })),
        "limits": {
            "minSlotUs": vb_edit::MIN_SLOT.0,
            "minTextUs": vb_edit::MIN_TEXT.0,
            "minSpeed": vb_edit::MIN_SPEED,
            "maxSpeed": vb_edit::MAX_SPEED,
            "defaultTextUs": vb_edit::DEFAULT_TEXT.0,
        },
    })
}

fn source_json(src: &vb_contracts::footage_graph::Source) -> Value {
    let (width, height) = src
        .display_size()
        .unwrap_or((src.width.unwrap_or(0), src.height.unwrap_or(0)));
    json!({
        "id": src.id,
        "path": src.path,
        "kind": if src.is_still() { "image" } else { "video" },
        "durationUs": src.duration_us.0,
        "width": width,
        "height": height,
        "hasAudio": src.has_audio,
        "exists": Path::new(&src.path).exists(),
    })
}

/// The music's length, which setting it needs and a front end does not
/// know.
fn music_length(path: &str) -> Result<u64, String> {
    let media = vb_media::default_backend();
    let audio = media
        .decode_audio(Path::new(path), 8_000, Some(Micros::from_secs(1)))
        .map_err(|e| e.to_string())?;
    audio
        .total_duration
        .map(|d| d.0)
        .filter(|d| *d > 0)
        .ok_or_else(|| "the music's length could not be read".to_string())
}

/// Apply one operation, as a step that can be undone.
pub fn apply(library: &Library, project_id: &str, op: &Value, lang: Lang) -> Result<Value, String> {
    let mut op = op.clone();
    if op["op"] == "setMusic" && op["assetDurationUs"].as_u64().unwrap_or(0) == 0 {
        let path = op["path"].as_str().unwrap_or_default().to_string();
        let length = music_length(&path).map_err(|e| match lang {
            Lang::En => format!("{path} could not be read as audio: {e}"),
            Lang::Zh => format!("无法把 {path} 当作音频读取：{e}"),
        })?;
        op["assetDurationUs"] = json!(length);
    }
    let op: Op = serde_json::from_value(op).map_err(|e| match lang {
        Lang::En => format!("not an edit this engine knows: {e}"),
        Lang::Zh => format!("无法识别的编辑操作：{e}"),
    })?;
    let session = session_for(library, project_id, lang)?;
    let mut s = lock(&session);
    let mut next = s.cut.clone();
    let selection = vb_edit::apply(&mut next, &s.graph, &op).map_err(|e| describe(&e, lang))?;
    if !same_cut(&next, &s.cut) {
        s.commit(library, next, label(&op, lang).to_string())?;
    }
    let newer_run = newer(library, &s);
    Ok(state_json(&s, selection.as_ref(), newer_run))
}

fn newer(library: &Library, s: &Session) -> bool {
    projects::get(library, &s.project_id)
        .and_then(|p| p.last_run)
        .is_some_and(|run| run.at != s.based_on)
}

/// Step back, or forward again.
pub fn undo(
    library: &Library,
    project_id: &str,
    forward: bool,
    lang: Lang,
) -> Result<Value, String> {
    let session = session_for(library, project_id, lang)?;
    let mut s = lock(&session);
    let step = if forward { s.redo.pop() } else { s.undo.pop() };
    if let Some(step) = step {
        let current = std::mem::replace(&mut s.cut, step.cut);
        let back = Step {
            cut: current,
            label: step.label,
        };
        if forward {
            s.undo.push(back);
        } else {
            s.redo.push(back);
        }
        s.version += 1;
        s.save(library)?;
    }
    let newer_run = newer(library, &s);
    Ok(state_json(&s, None, newer_run))
}

/// The cut the orchestrator made, back again -- as a step, so that it too
/// can be undone.
pub fn revert(library: &Library, project_id: &str, lang: Lang) -> Result<Value, String> {
    let session = session_for(library, project_id, lang)?;
    let mut s = lock(&session);
    if !same_cut(&s.cut, &s.original) {
        let original = s.original.clone();
        s.commit(
            library,
            original,
            lang.t("Back to the AI cut", "恢复 AI 成片").to_string(),
        )?;
    }
    let newer_run = newer(library, &s);
    Ok(state_json(&s, None, newer_run))
}

/// The cut and its analysis as they are now, for work done outside the
/// session's lock.
fn snapshot(
    library: &Library,
    project_id: &str,
    lang: Lang,
) -> Result<(EditingDsl, Arc<FootageGraph>, u64), String> {
    let session = session_for(library, project_id, lang)?;
    let s = lock(&session);
    Ok((s.cut.clone(), Arc::clone(&s.graph), s.version))
}

/// The preview's size: the piece's own frame with its longer side
/// `max_side`, both sides even.
fn preview_size(cut: &EditingDsl, graph: &FootageGraph, max_side: u32) -> (u32, u32) {
    let (w, h) = vb_render::frame_size(cut, graph, 1080);
    let long = w.max(h) as f64;
    let even = |v: f64| ((v / 2.0).round() as u32).max(1) * 2;
    let f = max_side as f64 / long;
    (even(w as f64 * f), even(h as f64 * f))
}

/// The piece at one moment, exactly as an exported video shows it, as a
/// PNG: what the editor shows when the playhead stops.
pub fn frame(
    library: &Library,
    project_id: &str,
    at: Micros,
    max_side: u32,
    lang: Lang,
) -> Result<Value, String> {
    use base64::Engine as _;
    let (cut, graph, version) = snapshot(library, project_id, lang)?;
    let (width, height) = preview_size(&cut, &graph, max_side.clamp(64, 2160));
    let media = vb_media::default_backend();
    let rgba = vb_render::frame(&cut, &graph, media.as_ref(), at, width, height)
        .map_err(|e| render_error(&e, lang))?;
    let png = png_rgb(&rgba, width, height, 4)?;
    Ok(json!({
        "atUs": at.0,
        "width": width,
        "height": height,
        "version": version,
        "png": base64::engine::general_purpose::STANDARD.encode(&png),
    }))
}

/// `pixels` (RGB or RGBA, `channels` a pixel) as an opaque RGB PNG.
fn png_rgb(pixels: &[u8], width: u32, height: u32, channels: usize) -> Result<Vec<u8>, String> {
    let rgb: std::borrow::Cow<[u8]> = if channels == 3 {
        std::borrow::Cow::Borrowed(pixels)
    } else {
        std::borrow::Cow::Owned(
            pixels
                .chunks_exact(channels)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
        )
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// Pictures of the footage for the timeline's film strip: each asked-for
/// moment of a source, `height` pixels high, kept in the library's cache by
/// the file's size and time.
pub fn strip(
    library: &Library,
    project_id: &str,
    items: &[(String, Micros)],
    height: u32,
    lang: Lang,
) -> Result<Value, String> {
    use base64::Engine as _;
    let (_, graph, _) = snapshot(library, project_id, lang)?;
    let height = height.clamp(16, 360) & !1;
    let media = vb_media::default_backend();
    let dir = library.cache_dir().join("strip");
    let _ = std::fs::create_dir_all(&dir);
    let one = |source_id: &str, at: Micros| -> Value {
        let failed = |e: String| json!({ "sourceId": source_id, "atUs": at.0, "error": e });
        let Some(src) = graph.source(source_id) else {
            return failed("no such source".into());
        };
        let path = Path::new(&src.path);
        let Ok(meta) = std::fs::metadata(path) else {
            return failed("the file is not there".into());
        };
        let (w, h) = src.display_size().unwrap_or((16, 9));
        let width = ((height as u64 * w.max(1) as u64 / h.max(1) as u64) as u32).clamp(2, 720) & !1;
        // A tenth of a second apart is the same picture in a strip.
        let at = if src.is_still() {
            Micros::ZERO
        } else {
            Micros(at.0 / 100_000 * 100_000)
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_micros());
        let key = vb_contracts::Blake3Hash::of_str(&format!(
            "strip/1|{}|{}|{modified}|{}|{height}",
            src.path,
            meta.len(),
            at.0
        ));
        let cached = dir.join(format!("{}.png", key.to_hex()));
        let png = match std::fs::read(&cached) {
            Ok(bytes) => bytes,
            Err(_) => {
                let made = media
                    .rgb_frame(path, at, width, height)
                    .map_err(|e| e.to_string())
                    .and_then(|f| png_rgb(&f.rgb, f.width, f.height, 3));
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
            "sourceId": source_id,
            "atUs": at.0,
            "png": base64::engine::general_purpose::STANDARD.encode(&png),
        })
    };
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 6);
    let chunk = items.len().div_ceil(workers).max(1);
    let mut out: Vec<Value> = Vec::with_capacity(items.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                let one = &one;
                scope.spawn(move || part.iter().map(|(id, at)| one(id, *at)).collect::<Vec<_>>())
            })
            .collect();
        for handle in handles {
            out.extend(handle.join().unwrap_or_default());
        }
    });
    Ok(json!({ "items": out }))
}

/// The project's footage, to add from: every source the analysis saw, its
/// usable stretches, and what of it the cut already uses.
pub fn footage(library: &Library, project_id: &str, lang: Lang) -> Result<Value, String> {
    let (cut, graph, _) = snapshot(library, project_id, lang)?;
    let mut used: HashMap<&str, Vec<[u64; 2]>> = HashMap::new();
    for slot in &cut.slots {
        if let Some(r) = &slot.source_ref {
            used.entry(r.source_id.as_str())
                .or_default()
                .push([r.in_us.0, r.out_us.0]);
        }
    }
    let sources: Vec<Value> = graph
        .sources
        .iter()
        .map(|src| {
            let mut stretches: Vec<Value> = Vec::new();
            for shot in graph.shots.iter().filter(|s| s.source_id == src.id) {
                if shot.segments.is_empty() {
                    stretches.push(json!({
                        "startUs": shot.start_us.0,
                        "endUs": shot.end_us.0,
                        "usableStartUs": shot.start_us.0,
                        "usableEndUs": shot.end_us.0,
                        "label": shot.description.as_ref().map(|d| d.text.clone()),
                    }));
                }
                for seg in &shot.segments {
                    stretches.push(json!({
                        "startUs": seg.start_us.0,
                        "endUs": seg.end_us.0,
                        "usableStartUs": seg.usable_start_us.map(|m| m.0),
                        "usableEndUs": seg.usable_end_us.map(|m| m.0),
                        "quality": seg.quality,
                        "label": seg
                            .caption
                            .as_ref()
                            .or(seg.description.as_ref())
                            .or(shot.description.as_ref())
                            .map(|d| d.text.clone()),
                    }));
                }
            }
            let mut value = source_json(src);
            value["capturedAt"] = json!(src.created_at);
            value["stretches"] = json!(stretches);
            value["used"] = json!(used.get(src.id.as_str()).cloned().unwrap_or_default());
            value
        })
        .collect();
    Ok(json!({ "sources": sources }))
}

/// Where videos go when the person names no folder: `VB_EXPORT_DIR`, else
/// the platform's videos folder, else the project's own.
fn default_export_dir(project_dir: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("VB_EXPORT_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        for videos in ["Movies", "Videos"] {
            if home.join(videos).is_dir() {
                return home.join(videos).join("Video Butler");
            }
        }
    }
    project_dir.join("exports")
}

/// A file name from a title: what a file system takes, and not one there
/// already.
fn free_name(dir: &Path, title: &str, ext: &str) -> PathBuf {
    let stem: String = title
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim_matches(['-', ' ', '.']).to_string();
    let stem = if stem.is_empty() {
        "Video Butler".to_string()
    } else {
        stem
    };
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}

fn record_export(library: &Library, project_id: &str, kind: &str, path: &Path) {
    if let Some(mut project) = projects::get(library, project_id) {
        project.exports.push(ExportRecord {
            at: vb_contracts::now_rfc3339(),
            kind: kind.to_string(),
            path: path.display().to_string(),
        });
        // A project exported a hundred times needs only its recent ones.
        if project.exports.len() > 20 {
            let extra = project.exports.len() - 20;
            project.exports.drain(..extra);
        }
        if let Err(e) = project.save(library) {
            tracing::warn!(error = %e, "could not record an export");
        }
    }
}

/// The cut as it leaves the editor: stamped with who changed it.
fn stamped(cut: &EditingDsl, original: &EditingDsl) -> EditingDsl {
    let mut cut = cut.clone();
    if !same_cut(&cut, original) {
        let ratio = (vb_edit::change_ratio(original, &cut) * 1000.0).round() / 1000.0;
        let provenance = cut.provenance.get_or_insert_with(Default::default);
        provenance.editor = Some(EDITOR.to_string());
        provenance.edited_at = Some(vb_contracts::now_rfc3339());
        provenance.change_ratio = Some(ratio);
    }
    cut
}

/// What a video export was asked for.
#[derive(Debug, Clone)]
pub struct VideoExport {
    /// 720, 1080, 2160: the frame's shorter side.
    pub short_side: u32,
    /// Frames a second; the piece's own when absent.
    pub fps: Option<(u32, u32)>,
    pub codec: VideoCodec,
    pub dir: Option<PathBuf>,
}

impl VideoExport {
    pub fn from_args(args: &Value) -> VideoExport {
        VideoExport {
            short_side: args["shortSide"].as_u64().unwrap_or(1080).clamp(240, 2160) as u32,
            fps: args["fps"]
                .as_f64()
                .filter(|f| *f >= 1.0 && *f <= 120.0)
                .map(|f| {
                    // 29.97 and the like, exactly.
                    if (f - f.round()).abs() < 1e-6 {
                        (f.round() as u32, 1)
                    } else {
                        ((f * 1001.0).round() as u32, 1001)
                    }
                }),
            codec: args["codec"]
                .as_str()
                .and_then(VideoCodec::from_id)
                .unwrap_or(VideoCodec::H264),
            dir: args["dir"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        }
    }
}

/// Send the cut out: a video (a job, which this returns the id of), or a
/// draft for another editor, written at once.
pub fn export(
    library: &Library,
    project_id: &str,
    args: &Value,
    lang: Lang,
) -> Result<Value, String> {
    let project = projects::get(library, project_id).ok_or_else(|| no_project(project_id, lang))?;
    let session = session_for(library, project_id, lang)?;
    let (cut, graph) = {
        let s = lock(&session);
        (stamped(&s.cut, &s.original), Arc::clone(&s.graph))
    };
    let target = args["target"].as_str().unwrap_or("video");
    match target {
        "video" => {
            let options = VideoExport::from_args(args);
            let dir = options
                .dir
                .clone()
                .unwrap_or_else(|| default_export_dir(&project.dir(library)));
            let project_id = project_id.to_string();
            let root = library.root().to_path_buf();
            let id = jobs::start("render", move |job| {
                let library = Library::at(root);
                let media = vb_media::default_backend();
                let report = render_video(&cut, &graph, media.as_ref(), &dir, &options, job, lang)?;
                record_export(&library, &project_id, "video", &report.path);
                Ok(json!({
                    "path": report.path,
                    "durationUs": report.duration.0,
                    "frames": report.frames,
                    "width": report.width,
                    "height": report.height,
                    "bytes": std::fs::metadata(&report.path).map(|m| m.len()).ok(),
                    "warnings": report.warnings,
                }))
            });
            Ok(json!({ "jobId": id }))
        }
        "jianying" | "capcut" => {
            let editor = Editor::ALL
                .into_iter()
                .find(|e| e.id() == target)
                .expect("matched above");
            let summary = crate::write_editor_draft(
                editor,
                &cut,
                &graph,
                &cut.meta.title,
                library.settings().draft_media(),
            )
            .map_err(|e| match (&e, lang) {
                (crate::PipelineError::NoDraftRoot { editor }, Lang::Zh) => {
                    format!("{editor}还没有草稿文件夹。先打开一次{editor}，它会自己创建。")
                }
                _ => e.to_string(),
            })?;
            record_export(library, project_id, target, &summary.path);
            let mut warnings = Vec::new();
            if summary.transitions_dropped > 0 {
                let (count, name) = (summary.transitions_dropped, editor.display_name());
                warnings.push(match lang {
                    Lang::En => format!(
                        "{count} transitions became hard cuts in the {name} draft, because \
                         this build only carries the ids for its dissolve"
                    ),
                    Lang::Zh => format!(
                        "{name}草稿里有 {count} 个转场改成了硬切：目前只掌握它\u{201c}叠化\u{201d}转场的资源编号"
                    ),
                });
            }
            if summary.media.unreadable > 0 {
                let (count, name) = (summary.media.unreadable, editor.display_name());
                warnings.push(match lang {
                    Lang::En => format!(
                        "{count} files are outside ~/Movies; {name} may mark them as not readable until you relink them"
                    ),
                    Lang::Zh => format!(
                        "{count} 个文件不在“影片”文件夹里，{name}可能显示“暂无访问权限”，需要在{name}里重新链接"
                    ),
                });
            }
            Ok(json!({
                "path": summary.path,
                "editor": editor.id(),
                "editorName": editor.display_name(),
                "warnings": warnings,
            }))
        }
        "lossless" => {
            let media = vb_media::default_backend();
            let plan = vb_render::lossless::plan(&cut, &graph, media.as_ref())
                .map_err(|blockers| lossless_reasons(&blockers, lang).join(lang.t("; ", "；")))?;
            let dir = args["dir"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| default_export_dir(&project.dir(library)));
            let project_id = project_id.to_string();
            let root = library.root().to_path_buf();
            let title = cut.meta.title.clone();
            let id = jobs::start("render", move |job| {
                let library = Library::at(root);
                let media = vb_media::default_backend();
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let dest = free_name(&dir, &title, "mp4");
                let report =
                    vb_render::lossless::export(&plan, media.as_ref(), &dest, &mut |share| {
                        copy_progress(job, share, "render.copy");
                        !job.cancelled()
                    })
                    .map_err(|e| render_error(&e, lang))?;
                record_export(&library, &project_id, "video", &report.path);
                Ok(json!({
                    "path": report.path,
                    "durationUs": report.duration.0,
                    "width": report.width,
                    "height": report.height,
                    "bytes": std::fs::metadata(&report.path).map(|m| m.len()).ok(),
                    "lossless": true,
                    "extraUs": plan.extra.0,
                    "warnings": [],
                }))
            });
            Ok(json!({ "jobId": id }))
        }
        "clips" => {
            let dir = args["dir"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| default_export_dir(&project.dir(library)));
            let project_id = project_id.to_string();
            let root = library.root().to_path_buf();
            let id = jobs::start("render", move |job| {
                let library = Library::at(root);
                let media = vb_media::default_backend();
                let done = export_clips(&cut, &graph, media.as_ref(), &dir, job, lang)?;
                if let Some(path) = done["path"].as_str() {
                    record_export(&library, &project_id, "clips", Path::new(path));
                }
                Ok(done)
            });
            Ok(json!({ "jobId": id }))
        }
        "otio" => {
            let dir = args["dir"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| project.dir(library).join("exports"));
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = free_name(&dir, &cut.meta.title, OtioWriter.extension());
            OtioWriter
                .write(&cut, &graph, &path)
                .map_err(|e| e.to_string())?;
            record_export(library, project_id, "otio", &path);
            Ok(json!({ "path": path, "warnings": [] }))
        }
        other => Err(format!("cannot export to {other:?}")),
    }
}

/// A copy's share done, as a render job reports it.
fn copy_progress(observer: &dyn Observer, share: f64, step: &str) {
    observer.progress(&Progress {
        stage: RunStage::Render,
        percent: (share * 100.0).clamp(0.0, 100.0),
        detail: format!("{:.0}%", share * 100.0),
        step: step.into(),
        file: None,
        source_index: 0,
        source_count: 0,
        active: 0,
    });
}

/// Whether the cut can leave without being re-encoded -- seconds, and the
/// footage exactly as filmed -- and if not, why not.
pub fn lossless(library: &Library, project_id: &str, lang: Lang) -> Result<Value, String> {
    let (cut, graph, _) = snapshot(library, project_id, lang)?;
    let media = vb_media::default_backend();
    Ok(
        match vb_render::lossless::plan(&cut, &graph, media.as_ref()) {
            Ok(plan) => json!({
                "possible": true,
                "extraUs": plan.extra.0,
                "totalUs": plan.total.0,
                "clips": plan.parts.len(),
                "audio": plan.audio,
                "reasons": [],
            }),
            Err(blockers) => json!({
                "possible": false,
                "reasons": lossless_reasons(&blockers, lang),
            }),
        },
    )
}

/// Why a cut needs re-encoding, once each, as a person can act on it.
fn lossless_reasons(blockers: &[vb_render::lossless::Blocker], lang: Lang) -> Vec<String> {
    use vb_render::lossless::Blocker as B;
    let mut out: Vec<String> = Vec::new();
    for b in blockers {
        let (en, zh) = match b {
            B::Unsupported => (
                "this build cannot join video without re-encoding",
                "这个版本不能无损拼接视频",
            ),
            B::Empty => ("the cut is empty", "成片是空的"),
            B::NotFootage(_) => ("there is a title card: delete it", "有标题卡：删掉它"),
            B::Photo(_) => (
                "there are photos, which become video only by encoding",
                "有照片：照片只能编码成视频",
            ),
            B::Speed(_) => ("some clips play at another speed", "有片段变了速"),
            B::Picture(_) => (
                "some clips are moved, cropped or colour-adjusted",
                "有片段改了位置、裁切或调色",
            ),
            B::Transition => (
                "there are transitions; a copy can only cut",
                "有转场：无损导出只能硬切",
            ),
            B::Text => ("there is text", "有文字（标题、字幕）"),
            B::Music => ("there is music", "有配乐"),
            B::Sound => (
                "some clips' sound is turned up, down or off while others' is not",
                "有片段改了音量，或只关了部分片段的原声",
            ),
            B::Canvas => (
                "the canvas is not the footage's shape",
                "画布比例和素材不一样",
            ),
            B::Mixed => (
                "the clips come from footage that cannot share one track (another camera, size or codec)",
                "片段来自不同相机或不同规格的素材，不能拼成一条轨道",
            ),
            B::NoKeyframe(_) => (
                "a clip has no keyframe to start from",
                "有片段找不到可以开始的关键帧",
            ),
            B::Missing(_) => ("a footage file is missing", "有素材文件找不到"),
        };
        let line = lang.t(en, zh).to_string();
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// Each clip of the cut as a file of its own, in a new folder: copied from
/// its keyframe when it is footage as filmed, rendered when its picture or
/// speed changed, and a title card left out. Text, transitions and music
/// belong to the cut and are not in any one clip.
fn export_clips(
    cut: &EditingDsl,
    graph: &FootageGraph,
    media: &dyn MediaBackend,
    dir: &Path,
    observer: &dyn Observer,
    lang: Lang,
) -> Result<Value, String> {
    let title = cut.meta.title.trim();
    let mut folder = dir.join(if title.is_empty() {
        "Video Butler"
    } else {
        title
    });
    let mut n = 2;
    while folder.exists() {
        folder = dir.join(format!(
            "{} {n}",
            if title.is_empty() {
                "Video Butler"
            } else {
                title
            }
        ));
        n += 1;
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let slots = cut.ordered_slots();
    let (mut files, mut copied, mut rendered, mut skipped) = (Vec::new(), 0usize, 0usize, 0usize);
    for (i, slot) in slots.iter().enumerate() {
        if observer.cancelled() {
            return Err(lang.t("stopped", "已停止。").to_string());
        }
        copy_progress(
            observer,
            i as f64 / slots.len().max(1) as f64,
            "render.clips",
        );
        let source = slot
            .source_ref
            .as_ref()
            .and_then(|r| graph.source(&r.source_id).map(|s| (r, s)));
        let Some((r, source)) = source.filter(|_| slot.kind == SlotKind::Clip) else {
            skipped += 1;
            continue;
        };
        let stem = Path::new(&source.path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let dest = folder.join(format!("{:02} {stem}.mp4", i + 1));
        let item = cut.tracks.video.iter().find(|v| v.slot_id == slot.id);
        let sound = cut
            .tracks
            .audio
            .iter()
            .find(|a| a.kind == AudioKind::Source && a.anchor.slot_id == slot.id);
        let as_filmed = !source.is_still()
            && slot.speed.is_none_or(|s| (s - 1.0).abs() < 1e-9)
            && item
                .is_none_or(|v| v.crop.is_none() && v.transform.is_none() && v.filters.is_empty())
            && sound
                .is_none_or(|a| a.muted == Some(true) || a.gain_db.is_none_or(|g| g.abs() < 1e-6));
        if as_filmed
            && media.can_copy()
            && let Some(from) =
                vb_render::lossless::keyframe_before(media, Path::new(&source.path), r.in_us)
        {
            let part = vb_media::CopyPart {
                path: PathBuf::from(&source.path),
                from,
                to: r.out_us,
            };
            let audio = sound.is_none_or(|a| a.muted != Some(true));
            if media
                .copy_parts(std::slice::from_ref(&part), &dest, audio, &mut |_| {
                    !observer.cancelled()
                })
                .is_ok()
            {
                copied += 1;
                files.push(dest);
                continue;
            }
        }
        if !media.can_encode(VideoCodec::H264) {
            skipped += 1;
            continue;
        }
        // The clip alone: its picture and its own sound, nothing over it.
        let mut one = EditingDsl::new(&cut.meta.title);
        one.meta = cut.meta.clone();
        let mut alone = (*slot).clone();
        alone.order = 0;
        one.slots = vec![alone];
        one.tracks.video = item.cloned().into_iter().collect();
        one.tracks.audio = sound.cloned().into_iter().collect();
        let options =
            vb_render::RenderOptions::for_dsl(&one, graph, 1080, None, VideoCodec::H264, 0.1);
        vb_render::render(&one, graph, media, &dest, &options, &mut |_, _| {
            !observer.cancelled()
        })
        .map_err(|e| render_error(&e, lang))?;
        rendered += 1;
        files.push(dest);
    }
    copy_progress(observer, 1.0, "render.clips");
    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(match lang {
            Lang::En => format!(
                "{skipped} clips were left out: title cards, or clips this device cannot encode"
            ),
            Lang::Zh => format!("{skipped} 段没有导出：标题卡，或这台设备不能编码的片段"),
        });
    }
    Ok(json!({
        "path": folder,
        "files": files,
        "copied": copied,
        "rendered": rendered,
        "skipped": skipped,
        "warnings": warnings,
    }))
}

/// Render the cut to a video in `dir`, reporting to `observer`.
pub fn render_video(
    cut: &EditingDsl,
    graph: &FootageGraph,
    media: &dyn MediaBackend,
    dir: &Path,
    options: &VideoExport,
    observer: &dyn Observer,
    lang: Lang,
) -> Result<vb_render::RenderReport, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let codec = if media.can_encode(options.codec) {
        options.codec
    } else {
        // HEVC where only H.264 can be written is still a video.
        VideoCodec::H264
    };
    let settings =
        vb_render::RenderOptions::for_dsl(cut, graph, options.short_side, options.fps, codec, 0.1);
    let dest = free_name(dir, &cut.meta.title, "mp4");
    let mut last = std::time::Instant::now();
    let mut progress = |done: u64, total: u64| -> bool {
        // Every frame asks whether to stop; the heat guard paces between
        // a few frames' worth of work, not every one.
        if done % 8 == 0 || done == total {
            let percent = if total == 0 {
                0.0
            } else {
                100.0 * done as f64 / total as f64
            };
            if done == total || last.elapsed() >= std::time::Duration::from_millis(100) {
                last = std::time::Instant::now();
                observer.progress(&Progress {
                    stage: RunStage::Render,
                    percent,
                    detail: format!("frame {done} of {total}"),
                    step: "render.frames".into(),
                    file: None,
                    source_index: done as usize,
                    source_count: total as usize,
                    active: 0,
                });
            }
            return !observer.cancelled();
        }
        true
    };
    vb_render::render(cut, graph, media, &dest, &settings, &mut progress)
        .map_err(|e| render_error(&e, lang))
}

fn render_error(e: &vb_render::RenderError, lang: Lang) -> String {
    use vb_render::RenderError as E;
    if lang == Lang::En {
        return e.to_string();
    }
    match e {
        E::MissingFile(path) => format!("找不到素材文件：{path}"),
        E::Empty => "成片是空的。".to_string(),
        E::NoEncoder(codec) => format!("这个版本还不能写出 {codec} 视频。"),
        E::Cancelled => "已停止。".to_string(),
        other => format!("导出失败：{other}"),
    }
}

/// What an operation is called in the undo menu.
fn label(op: &Op, lang: Lang) -> &'static str {
    let (en, zh) = match op {
        Op::Split { .. } => ("Split", "分割"),
        Op::Trim { .. } => ("Trim", "裁剪"),
        Op::SetDuration { .. } => ("Duration", "时长"),
        Op::Delete { .. } => ("Delete", "删除"),
        Op::Move { .. } => ("Move", "移动"),
        Op::Duplicate { .. } => ("Duplicate", "复制"),
        Op::Speed { .. } => ("Speed", "变速"),
        Op::Volume { .. } => ("Volume", "音量"),
        Op::MuteAll { .. } => ("Original sound", "原声开关"),
        Op::Transition { .. } | Op::TransitionAll { .. } => ("Transition", "转场"),
        Op::Transform { .. } | Op::ResetTransform { .. } => ("Position", "画面位置"),
        Op::Crop { .. } => ("Crop", "裁切"),
        Op::Adjust { .. } => ("Adjust", "调节"),
        Op::Background { .. } => ("Background", "背景"),
        Op::Insert { .. } => ("Add clip", "添加片段"),
        Op::AddText { .. } => ("Add text", "添加文字"),
        Op::UpdateText { .. } => ("Edit text", "修改文字"),
        Op::MoveText { .. } => ("Move text", "移动文字"),
        Op::DeleteText { .. } => ("Delete text", "删除文字"),
        Op::SetMusic { .. } => ("Add music", "添加音乐"),
        Op::UpdateMusic { .. } => ("Music", "调整音乐"),
        Op::DeleteMusic { .. } => ("Remove music", "删除音乐"),
        Op::Canvas { .. } => ("Canvas", "画布比例"),
        Op::Rename { .. } => ("Title", "标题"),
    };
    lang.t(en, zh)
}

/// Why an edit was not made, as the person reads it.
fn describe(e: &EditError, lang: Lang) -> String {
    if lang == Lang::En {
        return e.to_string();
    }
    match e {
        EditError::NoSuchSlot(_) => "这个片段已经不在了。".into(),
        EditError::NoSuchText(_) => "这段文字已经不在了。".into(),
        EditError::NoSuchMusic(_) => "这段音乐已经不在了。".into(),
        EditError::NoSuchSource(_) => "这个素材不在项目里。".into(),
        EditError::NotAClip(_) => "这一段不是视频片段（标题卡或空白），不能这样改。".into(),
        EditError::PhotoHasNoSpeed(_) => "照片没有速度可调，改它的时长即可。".into(),
        EditError::OutsideFootage(_) => "超出了素材本身的长度。".into(),
        EditError::TooShort(m) => format!("片段不能短于 {:.1} 秒。", m.as_secs_f64()),
        EditError::NothingToSplit(_) => "离片段两端太近，无法分割。".into(),
        EditError::NothingThere(_) => "这个位置没有片段。".into(),
        EditError::BadSpeed(_) => "速度只能在 0.25 倍到 4 倍之间。".into(),
        EditError::NoNextSlot(_) => "这是最后一个片段，后面没有可以转场的片段。".into(),
        EditError::LastSlot => "只剩一个片段，不能再删了。".into(),
        EditError::EmptyText => "文字不能为空。".into(),
        EditError::BadValue(v) => format!("取值不对：{v}"),
        EditError::Broken(v) => format!("这一步会让成片出错，没有执行：{v}"),
    }
}

#[cfg(test)]
mod tests;
