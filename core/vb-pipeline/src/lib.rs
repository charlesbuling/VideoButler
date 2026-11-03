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

//! End-to-end orchestration: media in, draft out.
//!
//! This is the only place the layers are wired together, and it is the layer
//! the CLI and the Flutter app both drive. Keeping the wiring here means
//! neither front end can reach past a contract boundary.

pub mod api;
pub mod cache;
pub mod captions;
pub mod dance;
pub mod editor;
pub mod entities;
pub mod hosting;
pub mod i18n;
pub mod jobs;
pub mod judge;
pub mod library;
pub mod memories;
pub mod models;
pub mod progress;
pub mod projects;
pub mod thermal;

pub use cache::{Cache, CacheError};
pub use i18n::Lang;
pub use library::{Library, Settings};
pub use progress::{Observer, Progress, RunStage, Silent};
pub use projects::{ModelChoice, Project, StyleChoice};

use std::path::{Path, PathBuf};
use vb_contracts::footage_graph::{BeatGrid, BeatOrigin};
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros, Stage};
use vb_export::{DraftMedia, DraftSummary, DraftWriter, Editor, JianyingWriter, OtioWriter};
use vb_infer::{AsrAdapter, TrackerAdapter, VlmAdapter};
use vb_media::MediaBackend;
use vb_narrative::{GroundingReport, MontageOrchestrator, PriorStore, Style, compose_grounded};
pub use vb_perception::DirectorModels;
use vb_perception::{Perception, PerceptionConfig};

/// Where a run reads and writes.
///
/// The three directories are separate because they have different lifetimes.
/// The project holds what the user asked for and keeps; the cache holds
/// analysis that any project may reuse; the work directory holds stills that
/// the cache's entries point at. Pointing the last two at a shared library is
/// what makes "this clip is already analysed" true across projects, and
/// moving one without the other would leave cached graphs referring to stills
/// that are not there.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub project_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub work_dir: PathBuf,
}

impl Workspace {
    /// Everything under the project directory, which is what the CLI does:
    /// one folder beside the footage holds the outputs, the cache and the
    /// stills.
    pub fn beside(project_dir: impl Into<PathBuf>) -> Self {
        let project_dir = project_dir.into();
        Workspace {
            cache_dir: project_dir.join("_vbcache"),
            work_dir: project_dir.join("_vbwork"),
            project_dir,
        }
    }

    /// Outputs in the project, analysis shared with every other project.
    pub fn shared(project_dir: impl Into<PathBuf>, library: &Path) -> Self {
        Workspace {
            project_dir: project_dir.into(),
            cache_dir: library.join("cache"),
            work_dir: library.join("work"),
        }
    }
}

/// What a run was asked for, beyond the footage itself.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// A template by id, which overrides the style.
    pub template_id: Option<String>,
    /// Mood, topic and whether to cut on the beat.
    pub style: Style,
    /// Music to cut to and lay under the result.
    pub music: Option<PathBuf>,
    /// What to call the piece. Becomes the title card.
    pub title: Option<String>,
    /// Where the portable draft goes. Defaults to the project directory.
    pub draft_path: Option<PathBuf>,
    /// Editors to write a native draft for, on top of the portable one.
    pub editors: Vec<Editor>,
    /// How long the finished piece should be, overriding the style's own.
    pub target_duration: Option<Micros>,
    /// The language warnings and reasons are written in.
    pub lang: Lang,
    /// Look at the footage again: no analysis and no rating is read back
    /// from the cache, and what this run finds replaces it.
    pub fresh: bool,
    /// Play the music again from its first beat when the cut runs longer
    /// than it, rather than letting the end of the cut fall silent.
    pub music_loop: bool,
    /// How an editor draft points at footage the editor may not read.
    pub draft_media: DraftMedia,
    /// `time` or `category`, for a cut that can be ordered either way: a
    /// year's memories. `None` is the template's own.
    pub order: Option<String>,
    /// What each source is of, by path, as the video butler filed it:
    /// written to the graph's `source.categories`, which a cut ordered by
    /// category reads.
    pub categories: std::collections::HashMap<String, Vec<String>>,
}

/// How long each stage of a run takes, as a share of the whole.
///
/// Rough by nature, but a progress bar that is roughly right is worth far
/// more than none: the alternative is a spinner that says nothing for the
/// several minutes a folder of clips takes. Analysis dominates, and within it
/// the per-source shares follow file size.
const PREPARE_SHARE: f64 = 2.0;
const ANALYSE_SHARE: f64 = 78.0;
const MUSIC_SHARE: f64 = 6.0;
const COMPOSE_SHARE: f64 = 6.0;
const EXPORT_SHARE: f64 = 8.0;

/// Everything one run produced, plus anything the user should know about it.
#[derive(Debug)]
pub struct RunResult {
    pub graph: FootageGraph,
    pub dsl: EditingDsl,
    pub grounding: GroundingReport,
    pub draft_path: PathBuf,
    pub graph_path: PathBuf,
    pub dsl_path: PathBuf,
    pub template_id: String,
    /// How many of the project's sources came back from the cache, out of how
    /// many there are. Incremental reuse is the load-bearing performance
    /// lever, so a run says plainly how much of it was reused.
    pub reused_sources: usize,
    pub total_sources: usize,
    /// Native editor drafts written, if any were asked for.
    pub drafts: Vec<DraftSummary>,
    /// The template the cut was composed from, after a style was blended.
    pub template_name: String,
    /// Things that degraded the result without stopping it. Surfaced rather
    /// than logged, because a cut assembled from mock perception must not look
    /// like a real one.
    pub warnings: Vec<String>,
}

pub struct Pipeline<'a> {
    pub media: &'a dyn MediaBackend,
    pub asr: &'a dyn AsrAdapter,
    pub vlm: &'a dyn VlmAdapter,
    pub tracker: &'a dyn TrackerAdapter,
    /// The models the director looks through; none until a caller sets them.
    pub director: DirectorModels<'a>,
    /// The composition judge, when one is installed: a second look at the
    /// moments a cut might use.
    pub judge: Option<&'a dyn VlmAdapter>,
    /// The general vision-language model, when one is installed: writes the
    /// captions, from the picture.
    pub captioner: Option<&'a dyn VlmAdapter>,
    pub config: PerceptionConfig,
    pub store: PriorStore,
}

impl<'a> Pipeline<'a> {
    pub fn new(
        media: &'a dyn MediaBackend,
        asr: &'a dyn AsrAdapter,
        vlm: &'a dyn VlmAdapter,
        tracker: &'a dyn TrackerAdapter,
    ) -> Self {
        Pipeline {
            media,
            asr,
            vlm,
            tracker,
            director: DirectorModels::none(),
            judge: None,
            captioner: None,
            config: PerceptionConfig::default(),
            store: PriorStore::with_builtins(),
        }
    }

    /// Would this run need the network?
    ///
    /// The offline guarantee is a hard product requirement, so it is checked
    /// rather than assumed.
    pub fn requires_network(&self) -> bool {
        self.asr.requires_network()
            || self.vlm.requires_network()
            || self.tracker.requires_network()
    }

    /// Analyse one file and write a draft beside it.
    ///
    /// The simple case, kept because the CLI and the tests use it. A project
    /// of several clips with a style goes through [`Self::run_project`].
    pub fn run(
        &self,
        input: &Path,
        project_dir: &Path,
        template_id: Option<&str>,
        draft_path: Option<&Path>,
    ) -> Result<RunResult, PipelineError> {
        self.run_many(
            std::slice::from_ref(&input.to_path_buf()),
            project_dir,
            template_id,
            draft_path,
        )
    }

    /// Analyse every input into one project and compose a cut across them.
    pub fn run_many(
        &self,
        inputs: &[PathBuf],
        project_dir: &Path,
        template_id: Option<&str>,
        draft_path: Option<&Path>,
    ) -> Result<RunResult, PipelineError> {
        self.run_project(
            inputs,
            &Workspace::beside(project_dir),
            &RunOptions {
                template_id: template_id.map(str::to_string),
                draft_path: draft_path.map(Path::to_path_buf),
                ..RunOptions::default()
            },
            &Silent,
        )
    }

    /// Analyse a project's footage and write its drafts.
    ///
    /// Each file is analysed and cached on its own, keyed by its content, so
    /// adding a clip to a project re-analyses that clip and reuses the rest,
    /// and a clip already analysed for another project is not analysed again.
    pub fn run_project(
        &self,
        inputs: &[PathBuf],
        workspace: &Workspace,
        options: &RunOptions,
        observer: &dyn Observer,
    ) -> Result<RunResult, PipelineError> {
        if inputs.is_empty() {
            return Err(PipelineError::NoInputs);
        }
        for input in inputs {
            if !input.exists() {
                return Err(PipelineError::InputNotFound {
                    path: input.display().to_string(),
                });
            }
        }
        let input = &inputs[0];

        let lang = options.lang;
        let report = |stage: RunStage, percent: f64, step: &str, detail: &str| Progress {
            stage,
            percent,
            detail: detail.to_string(),
            step: step.to_string(),
            file: None,
            source_index: 0,
            source_count: inputs.len(),
            active: 0,
        };
        let stop_if_cancelled = |observer: &dyn Observer| -> Result<(), PipelineError> {
            if observer.cancelled() {
                return Err(PipelineError::Cancelled);
            }
            Ok(())
        };

        observer.progress(&report(
            RunStage::Prepare,
            0.0,
            "prepare.list",
            "reading the file list",
        ));
        stop_if_cancelled(observer)?;

        let work_dir = &workspace.work_dir;
        let cache = if options.fresh {
            Cache::new(workspace.cache_dir.clone()).write_only()
        } else {
            Cache::new(workspace.cache_dir.clone())
        };
        std::fs::create_dir_all(work_dir).map_err(|e| PipelineError::Io {
            path: work_dir.display().to_string(),
            source: e,
        })?;

        let mut warnings = Vec::new();

        // --- perception, one cache entry per source file -------------------
        //
        // Keyed on the file's own content, so a project's other clips are
        // untouched when one is added, and so the same clip in two projects is
        // analysed once. The whole per-file analysis is one entry: the
        // per-stage invalidation machinery in `cache` is built and tested, and
        // becomes useful when perception is split into independently cached
        // stages.
        let params_hash = self.config_hash();

        // Weighted by file size, because that is the best guide to how long a
        // clip takes and it is known before anything is decoded.
        let sizes: Vec<f64> = inputs
            .iter()
            .map(|p| {
                std::fs::metadata(p)
                    .map(|m| m.len() as f64)
                    .unwrap_or(1.0)
                    .max(1.0)
            })
            .collect();
        let names: Vec<String> = inputs
            .iter()
            .map(|source| {
                source
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.display().to_string())
            })
            .collect();

        // Every file's content, and whether it was analysed before.
        let mut parts: Vec<Option<FootageGraph>> = vec![None; inputs.len()];
        let mut pending: Vec<(usize, Blake3Hash, Blake3Hash)> = Vec::new();
        let mut reused_sources = 0usize;
        for (index, source) in inputs.iter().enumerate() {
            stop_if_cancelled(observer)?;
            observer.progress(&Progress {
                file: Some(names[index].clone()),
                source_index: index,
                ..report(
                    RunStage::Prepare,
                    PREPARE_SHARE * index as f64 / inputs.len() as f64,
                    // Every file is read for its fingerprint either way; a
                    // fresh run says it is not looking for earlier work.
                    if options.fresh {
                        "prepare.read"
                    } else {
                        "prepare.hash"
                    },
                    &format!("{} · reading", names[index]),
                )
            });
            let source_hash = Blake3Hash::of_file(source).map_err(|e| PipelineError::Io {
                path: source.display().to_string(),
                source: e,
            })?;
            let cache_key = Cache::key(&source_hash, Stage::MinePriors, &params_hash);
            let cached = cache.get(&cache_key).and_then(|text| {
                match serde_json::from_str::<FootageGraph>(&text) {
                    Ok(graph) => Some(graph),
                    Err(e) => {
                        // A stale entry from an older schema is a miss, not a
                        // failure: re-analysing is always possible.
                        tracing::warn!(error = %e, "discarding an unreadable cache entry");
                        None
                    }
                }
            });
            match cached {
                Some(graph) => {
                    tracing::info!(source = %source.display(), "reusing cached analysis");
                    reused_sources += 1;
                    parts[index] = Some(graph);
                }
                None => pending.push((index, source_hash, cache_key)),
            }
        }

        // --- perception, several clips at once ------------------------------
        //
        // One clip leaves most of the machine idle: the media engine decodes
        // a 4K stream well below what it can, and most stages are one thread.
        // Largest first, so the long clips are not left running alone at the
        // end.
        pending.sort_by(|a, b| sizes[b.0].total_cmp(&sizes[a.0]));
        let already: Vec<usize> = (0..inputs.len()).filter(|i| parts[*i].is_some()).collect();
        let batch = progress::Analysing::new(
            observer,
            PREPARE_SHARE,
            ANALYSE_SHARE,
            sizes.clone(),
            names.clone(),
            &already,
        );
        let queue = std::sync::Mutex::new(pending.into_iter());
        let results: std::sync::Mutex<Vec<(usize, Result<FootageGraph, PipelineError>)>> =
            std::sync::Mutex::new(Vec::new());
        let workers = analysis_workers().min(inputs.len().saturating_sub(reused_sources).max(1));
        let numbered = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    let worker = numbered.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    thermal::set_worker(worker);
                    // Waking at the priority the user's own work gets, not
                    // as background work the system may starve.
                    vb_apple::power::user_initiated_thread();
                    loop {
                        // A warm device analyses one clip at a time: the
                        // other workers wait rather than take the next.
                        while worker > 0
                            && !thermal::may_widen()
                            && !observer.cancelled()
                            && !batch.failed.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                        if batch.failed.load(std::sync::atomic::Ordering::Relaxed)
                            || observer.cancelled()
                        {
                            break;
                        }
                        let next = queue.lock().ok().and_then(|mut q| q.next());
                        let Some((index, hash, key)) = next else {
                            break;
                        };
                        batch.started(index);
                        let outcome = self.analyze(&inputs[index], hash, work_dir, &batch.one(index));
                        match &outcome {
                            Ok(graph) => {
                                match serde_json::to_string(graph) {
                                    Ok(text) => {
                                        if let Err(e) = cache.put(&key, &text) {
                                            tracing::warn!(error = %e, "could not cache the analysis");
                                        }
                                    }
                                    Err(e) => tracing::warn!(
                                        error = %e,
                                        "could not serialise the analysis"
                                    ),
                                }
                                batch.finished(index);
                            }
                            Err(_) => {
                                batch.stopped(index);
                                batch
                                    .failed
                                    .store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        if let Ok(mut results) = results.lock() {
                            results.push((index, outcome));
                        }
                    }
                });
            }
        });
        stop_if_cancelled(observer)?;
        let mut results = results.into_inner().unwrap_or_default();
        // A real failure is reported over the cancellations it caused in the
        // clips that were stopped because of it.
        results.sort_by_key(|(_, r)| {
            matches!(
                r,
                Err(PipelineError::Perception(
                    vb_perception::PerceptionError::Cancelled
                ))
            )
        });
        for (index, outcome) in results {
            parts[index] = Some(outcome?);
        }
        let parts: Vec<FootageGraph> = parts.into_iter().flatten().collect();

        // One project, one graph. Ids are local to each part until here.
        let mut graph = vb_perception::merge_graphs("vb-project", parts);
        // Mined over the whole project rather than per clip: shot-length
        // distribution across five clips of a holiday is the statistic the
        // narrative layer wants, not five separate ones.
        vb_perception::attach_priors(&mut graph);
        // Scenes span sources, so they are found here, over the whole
        // project, rather than in any one source's cached analysis.
        vb_perception::attach_scenes(&mut graph);
        // What each video is of, from the video butler: not cached with the
        // analysis, because removing a category changes it and nothing in
        // the footage does.
        for source in graph.sources.iter_mut() {
            if let Some(categories) = options.categories.get(&source.path) {
                source.categories = categories.clone();
            }
        }

        for adapter in self.mocked_adapters() {
            warnings.push(match lang {
                Lang::En => {
                    format!("{adapter} produced no real output; this cut was assembled without it")
                }
                Lang::Zh => {
                    format!("{adapter} 没有产生真实结果，这一版剪辑是在缺少它的情况下完成的")
                }
            });
        }

        // --- the beat ------------------------------------------------------
        stop_if_cancelled(observer)?;
        let analysing_beat = PREPARE_SHARE + ANALYSE_SHARE;
        if let Some(music) = &options.music {
            observer.progress(&report(
                RunStage::Music,
                analysing_beat,
                "music.listen",
                "listening to the music",
            ));
            match self.beat_grid(music, BeatOrigin::Music) {
                Ok(grid) => {
                    tracing::info!(bpm = grid.bpm, "found a beat in the music");
                    set_beat_grid(&mut graph, grid);
                }
                Err(e) => warnings.push(match lang {
                    Lang::En => format!(
                        "the music could not be read for its tempo ({e}); the cut follows a \
                         steady grid instead"
                    ),
                    Lang::Zh => format!("无法从配乐中识别出节拍（{e}），改按固定节奏剪辑"),
                }),
            }
        } else if options.style.beat_sync == Some(true) {
            // No music, but the footage may carry its own -- dance footage
            // usually does, and cutting to the music that is already there is
            // better than cutting to a tempo nobody chose.
            observer.progress(&report(
                RunStage::Music,
                analysing_beat,
                "music.footage",
                "listening to the footage",
            ));
            if let Some(grid) = self.beat_in_footage(&graph) {
                tracing::info!(bpm = grid.bpm, "found a beat in the footage");
                set_beat_grid(&mut graph, grid);
            }
        }

        // --- narrative -----------------------------------------------------
        stop_if_cancelled(observer)?;
        let composing = analysing_beat + MUSIC_SHARE;
        observer.progress(&report(
            RunStage::Compose,
            composing,
            "compose.choose",
            "choosing the clips",
        ));

        let mut template = match &options.template_id {
            Some(id) => self
                .store
                .get(id)
                .cloned()
                .ok_or_else(|| PipelineError::UnknownTemplate(id.clone()))?,
            None => self
                .store
                .template_for_style(&options.style, &graph)
                .ok_or(PipelineError::NoTemplates)?,
        };
        if let Some(title) = options
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            template
                .params
                .extra
                .insert("title".into(), serde_json::Value::String(title.to_string()));
        }
        // What the user asked for beats what the style would have chosen:
        // a style decides pacing, and the length is the user's.
        if let Some(duration) = options.target_duration.filter(|d| *d > Micros::ZERO) {
            template.params.target_duration_us = duration;
        }
        // The reasons written against each clip are read by the user, so
        // they are written in the user's language.
        template.params.extra.insert(
            "locale".into(),
            serde_json::Value::String(lang.id().to_string()),
        );
        if options.music_loop {
            template
                .params
                .extra
                .insert("musicLoop".into(), serde_json::Value::Bool(true));
        }
        // A year in review is ordered by category unless the user chose
        // time; any cut may be ordered by category on request.
        match options.order.as_deref() {
            Some("time") => {
                template.params.extra.remove("order");
            }
            Some("category") if !template.params.extra.contains_key("sections") => {
                template
                    .params
                    .extra
                    .insert("order".into(), "sections".into());
                template.params.extra.insert(
                    "sections".into(),
                    serde_json::json!(["people", "scenery", "food", "other"]),
                );
            }
            _ => {}
        }
        let continuity = vb_narrative::continuity::is_continuity(&template.params.extra);
        // No length chosen: a 主体衔接 cut is as long as the action it holds.
        if continuity
            && options
                .target_duration
                .filter(|d| *d > Micros::ZERO)
                .is_none()
        {
            template
                .params
                .extra
                .insert("autoLength".into(), serde_json::Value::Bool(true));
        }
        tracing::info!(template = %template.id, "selected template");

        // --- 主体衔接: the subject followed through every take first -------
        //
        // The whole cut is the action, so the poses come before any of it
        // is chosen: every stretch someone is in, best first.
        let mut action: Option<vb_narrative::continuity::Report> = None;
        let mut composition = if continuity {
            if self.director.pose.is_mock() {
                return Err(PipelineError::NeedsPoseModel);
            }
            let list = dance::with_people(&graph, dance::CONTINUITY_BUDGET);
            let started = std::time::Instant::now();
            let pose = self.director.pose;
            let read = dance::read(
                &mut graph,
                &list,
                dance::CONTINUITY_LONGEST,
                dance::CONTINUITY_EVERY,
                pose,
                self.media,
                &cache,
                &|n, total| {
                    observer.progress(&Progress {
                        file: Some(format!("{n}/{total}")),
                        ..report(
                            RunStage::Compose,
                            composing + COMPOSE_SHARE * 0.4 * n as f64 / total.max(1) as f64,
                            "compose.continuity",
                            "following the subject through every take",
                        )
                    })
                },
                &|| observer.cancelled(),
            );
            stop_if_cancelled(observer)?;
            let rhythms = dance::rhythms(&mut graph, &list);
            tracing::info!(
                ?read,
                rhythms,
                "poses and their beats read for subject continuity"
            );
            graph
                .provenance
                .push(vb_contracts::footage_graph::ProvenanceEntry {
                    stage: Stage::Pose,
                    tool: None,
                    tool_version: None,
                    adapter: Some(pose.name().to_string()),
                    params_hash: Blake3Hash::of_parts(&[dance::POSE_VERSION.as_bytes()]),
                    completed_at: Some(vb_contracts::now_rfc3339()),
                    duration_ms: Some(started.elapsed().as_millis() as u64),
                });
            let (c, r) = compose_action(&graph, &template)?;
            action = Some(r);
            c
        } else {
            compose_grounded(&MontageOrchestrator, &graph, &template)?
        };

        // --- dance: how the people move, where the cut may join them -------
        if dance::is_dance(&template) {
            if self.director.pose.is_mock() {
                warnings.push(
                    lang.t(
                        "Dance: no body-pose model, so clips were chosen by how much the \
                         picture moves -- not all of them may be dancing, and the movement is \
                         not carried across the cuts. Download RTMO-s (40 MB) for both.",
                        "舞蹈：还没有下载人体姿态模型，只能按画面动感挑选，不能保证都是跳舞的画面，也没有衔接动作。下载 RTMO-s（40 MB）后两件事都会做。",
                    )
                    .to_string(),
                );
            } else {
                let size = (composition.dsl.slots.len() * dance::CANDIDATE_FACTOR)
                    .clamp(dance::CANDIDATE_MIN, dance::CANDIDATE_MAX);
                let list = dance::candidates(&graph, &composition.dsl, size);
                let started = std::time::Instant::now();
                let pose = self.director.pose;
                let read = dance::read(
                    &mut graph,
                    &list,
                    dance::LONGEST,
                    dance::EVERY,
                    pose,
                    self.media,
                    &cache,
                    &|n, total| {
                        observer.progress(&Progress {
                            file: Some(format!("{n}/{total}")),
                            ..report(
                                RunStage::Compose,
                                composing + COMPOSE_SHARE * 0.3 * n as f64 / total.max(1) as f64,
                                "compose.dance",
                                "following the dancers",
                            )
                        })
                    },
                    &|| observer.cancelled(),
                );
                stop_if_cancelled(observer)?;
                tracing::info!(?read, "poses read");
                graph
                    .provenance
                    .push(vb_contracts::footage_graph::ProvenanceEntry {
                        stage: Stage::Pose,
                        tool: None,
                        tool_version: None,
                        adapter: Some(pose.name().to_string()),
                        params_hash: Blake3Hash::of_parts(&[dance::POSE_VERSION.as_bytes()]),
                        completed_at: Some(vb_contracts::now_rfc3339()),
                        duration_ms: Some(started.elapsed().as_millis() as u64),
                    });
                composition = match compose_grounded(&MontageOrchestrator, &graph, &template) {
                    Ok(c) => c,
                    Err(vb_narrative::NarrativeError::NoUsableShots { .. }) => {
                        return Err(PipelineError::NoDancing);
                    }
                    Err(e) => return Err(e.into()),
                };
            }
        }

        // --- the composition judge: a second look at the shortlist ---------
        if let Some(vlm) = self.judge {
            let started = std::time::Instant::now();
            let size = (composition.dsl.slots.len() * judge::SHORTLIST_FACTOR)
                .clamp(judge::SHORTLIST_MIN, judge::SHORTLIST_MAX);
            let list = judge::shortlist(&graph, &composition.dsl, size);
            let judged = judge::judge(
                &mut graph,
                &list,
                vlm,
                self.media,
                work_dir,
                &cache,
                &|n, total| {
                    observer.progress(&Progress {
                        file: Some(format!("{n}/{total}")),
                        ..report(
                            RunStage::Compose,
                            composing + COMPOSE_SHARE * 0.6 * n as f64 / total.max(1) as f64,
                            "compose.judge",
                            "judging the composition of the shortlist",
                        )
                    })
                },
                &|| observer.cancelled(),
            );
            stop_if_cancelled(observer)?;
            tracing::info!(?judged, "composition judged");
            graph
                .provenance
                .push(vb_contracts::footage_graph::ProvenanceEntry {
                    stage: Stage::Judge,
                    tool: None,
                    tool_version: None,
                    adapter: Some(vlm.name().to_string()),
                    params_hash: Blake3Hash::of_parts(&[judge::PROMPT.as_bytes()]),
                    completed_at: Some(vb_contracts::now_rfc3339()),
                    duration_ms: Some(started.elapsed().as_millis() as u64),
                });
            if judged.failed > 0 && judged.asked + judged.remembered == 0 {
                warnings.push(match lang {
                    Lang::En => format!(
                        "{} could not rate any shot, so composition played no part in this cut",
                        vlm.name()
                    ),
                    Lang::Zh => {
                        format!("{} 一个画面也没能评分，这一版选片没有参考构图", vlm.name())
                    }
                });
            }
            // Out of memory before the next model comes in.
            vlm.release();
            if continuity {
                let (c, r) = compose_action(&graph, &template)?;
                composition = c;
                action = Some(r);
            } else {
                composition = compose_grounded(&MontageOrchestrator, &graph, &template)?;
            }
        }

        // --- captions: what the picture shows, in a sentence ---------------
        //
        // Only the clips the plan gives a caption to, and only when the
        // style puts words on screen at all. Without a model the cut has no
        // captions, which the run's notes say; it never falls back to
        // transcribing what was said.
        if let (Some(vlm), true) = (self.captioner, template.params.subtitles) {
            let list = captions::targets(&graph, &composition.dsl);
            if !list.is_empty() {
                let started = std::time::Instant::now();
                let done = captions::caption(
                    &mut graph,
                    &list,
                    vlm,
                    self.media,
                    work_dir,
                    &cache,
                    lang,
                    &|n, total| {
                        observer.progress(&Progress {
                            file: Some(format!("{n}/{total}")),
                            ..report(
                                RunStage::Compose,
                                composing
                                    + COMPOSE_SHARE * (0.6 + 0.35 * n as f64 / total.max(1) as f64),
                                "compose.caption",
                                "writing captions from the picture",
                            )
                        })
                    },
                    &|| observer.cancelled(),
                );
                stop_if_cancelled(observer)?;
                vlm.release();
                tracing::info!(?done, "captions written");
                graph
                    .provenance
                    .push(vb_contracts::footage_graph::ProvenanceEntry {
                        stage: Stage::Caption,
                        tool: None,
                        tool_version: None,
                        adapter: Some(vlm.name().to_string()),
                        params_hash: Blake3Hash::of_parts(&[
                            captions::CAPTION_VERSION.as_bytes(),
                            lang.id().as_bytes(),
                        ]),
                        completed_at: Some(vb_contracts::now_rfc3339()),
                        duration_ms: Some(started.elapsed().as_millis() as u64),
                    });
                if done.failed > 0 && done.asked + done.remembered == 0 {
                    warnings.push(match lang {
                        Lang::En => format!(
                            "{} could not caption any clip, so this cut has no captions",
                            vlm.name()
                        ),
                        Lang::Zh => {
                            format!("{} 一个画面也没能写出字幕，这一版没有字幕", vlm.name())
                        }
                    });
                }
                if continuity {
                    let (c, r) = compose_action(&graph, &template)?;
                    composition = c;
                    action = Some(r);
                } else {
                    composition = compose_grounded(&MontageOrchestrator, &graph, &template)?;
                }
            }
        }

        if !composition.grounding.is_clean() {
            let rejected = composition.grounding.rejected.len();
            let total = rejected + composition.grounding.verified.len();
            warnings.push(match lang {
                Lang::En => {
                    format!("{rejected} of {total} decisions failed grounding and were dropped")
                }
                Lang::Zh => format!("{total} 个剪辑决定中有 {rejected} 个没有通过素材核验，已丢弃"),
            });
        }
        if composition.dsl.slots.is_empty() {
            return Err(PipelineError::EmptyCut);
        }
        // A dance cut is made of dancing only, so the footage may not hold
        // as much as was asked for; that is said, not padded out.
        if dance::is_dance(&template) && !self.director.pose.is_mock() {
            let made = composition.dsl.resolve_timeline().total_duration_us;
            let asked = template.params.target_duration_us;
            if made.0 * 10 < asked.0 * 9 {
                warnings.push(match lang {
                    Lang::En => format!(
                        "the footage holds only {made} of dancing that a cut can use, so this \
                         cut is shorter than the {asked} asked for"
                    ),
                    Lang::Zh => format!(
                        "素材里能用的跳舞画面只够剪 {made}，比设定的 {asked} 短，没有拿别的画面凑时长"
                    ),
                });
            }
        }

        if let Some(report) = &action {
            warnings.extend(continuity_notes(report, lang));
        }

        // --- validate both contracts before anything is written ------------
        vb_contracts::validate::validate_footage_graph(&graph)?;
        vb_contracts::validate::validate_editing_dsl(&composition.dsl)?;

        // --- export --------------------------------------------------------
        stop_if_cancelled(observer)?;
        let exporting = composing + COMPOSE_SHARE;
        observer.progress(&report(
            RunStage::Export,
            exporting,
            "export.otio",
            "writing the draft",
        ));

        // Named after the first clip when there is one, after the project when
        // there are several: "holiday-1.otio" for a cut of twelve clips would
        // name the draft after a twelfth of itself.
        let stem = if inputs.len() == 1 {
            input
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        } else {
            "project".to_string()
        };

        let project_dir = &workspace.project_dir;
        let graph_path = project_dir.join(format!("{stem}.footage-graph.json"));
        let dsl_path = project_dir.join(format!("{stem}.editing-dsl.json"));
        write_json(&graph_path, &graph)?;
        write_json(&dsl_path, &composition.dsl)?;

        let writer = OtioWriter;
        let draft_path = options
            .draft_path
            .clone()
            .unwrap_or_else(|| project_dir.join(format!("{stem}.{ext}", ext = writer.extension())));
        writer.write(&composition.dsl, &graph, &draft_path)?;

        // --- the editor's own draft ----------------------------------------
        let mut drafts = Vec::new();
        for (index, editor) in options.editors.iter().enumerate() {
            let percent = exporting
                + EXPORT_SHARE * (index as f64 + 1.0) / (options.editors.len() as f64 + 1.0);
            observer.progress(&report(
                RunStage::Export,
                percent,
                &format!("export.editor.{}", editor.id()),
                &format!("writing the {} draft", editor.display_name()),
            ));
            match self.write_editor_draft(*editor, &composition.dsl, &graph, options) {
                Ok(summary) => {
                    if summary.media.unreadable > 0 {
                        let (count, name) = (summary.media.unreadable, editor.display_name());
                        warnings.push(match (options.draft_media, lang) {
                            (DraftMedia::Reference, Lang::En) => format!(
                                "{count} files are referenced where they are, outside ~/Movies; \
                                 {name} will mark them as not readable until you relink them there"
                            ),
                            (DraftMedia::Reference, Lang::Zh) => format!(
                                "{count} 个文件按原路径引用，它们不在“影片”文件夹里，{name}会显示“暂无访问权限”，需要在{name}里重新链接"
                            ),
                            (DraftMedia::Link, Lang::En) => format!(
                                "{count} files are on another disk, where a link cannot reach, so \
                                 they are referenced where they are; {name} may ask you to relink them"
                            ),
                            (DraftMedia::Link, Lang::Zh) => format!(
                                "{count} 个文件在另一个磁盘上，无法建立硬链接，已按原路径引用，{name}可能会要求重新链接"
                            ),
                        });
                    }
                    if summary.transitions_dropped > 0 {
                        let (count, name) = (summary.transitions_dropped, editor.display_name());
                        warnings.push(match lang {
                            Lang::En => format!(
                                "{count} transitions became hard cuts in the {name} draft, \
                                 because this build only carries the ids for its dissolve"
                            ),
                            Lang::Zh => format!(
                                "{name}草稿里有 {count} 个转场改成了硬切：目前只掌握它\u{201c}叠化\u{201d}转场的资源编号"
                            ),
                        });
                    }
                    drafts.push(summary);
                }
                Err(e) => warnings.push(match lang {
                    Lang::En => format!(
                        "the {} draft could not be written: {e}",
                        editor.display_name()
                    ),
                    Lang::Zh => format!("{}草稿没有写成：{e}", editor.display_name()),
                }),
            }
        }

        observer.progress(&report(RunStage::Done, 100.0, "done", "done"));

        Ok(RunResult {
            graph,
            dsl: composition.dsl,
            grounding: composition.grounding,
            draft_path,
            graph_path,
            dsl_path,
            template_id: template.id.clone(),
            template_name: template.name.clone(),
            reused_sources,
            total_sources: inputs.len(),
            drafts,
            warnings,
        })
    }

    /// Write the draft into the editor's own draft folder.
    ///
    /// Anywhere else and the editor never sees it, which looks to a user
    /// exactly like the export having failed.
    fn write_editor_draft(
        &self,
        editor: Editor,
        dsl: &EditingDsl,
        graph: &FootageGraph,
        options: &RunOptions,
    ) -> Result<DraftSummary, PipelineError> {
        write_editor_draft(
            editor,
            dsl,
            graph,
            options.title.as_deref().unwrap_or(&dsl.meta.title),
            options.draft_media,
        )
    }

    /// The tempo of a piece of music.
    fn beat_grid(&self, music: &Path, origin: BeatOrigin) -> Result<BeatGrid, PipelineError> {
        // Two minutes is plenty to find a tempo and keeps a long album track
        // from being decoded in full.
        let audio = self
            .media
            .decode_audio(
                music,
                vb_perception::beats::ANALYSIS_RATE,
                Some(Micros::from_secs(120)),
            )
            .map_err(|e| PipelineError::Music {
                path: music.display().to_string(),
                detail: e.to_string(),
            })?;
        let found = vb_perception::detect_beats(&audio.samples, audio.sample_rate).ok_or(
            PipelineError::Music {
                path: music.display().to_string(),
                detail: "no steady beat was found in it".into(),
            },
        )?;
        Ok(BeatGrid {
            bpm: found.bpm,
            // Where the beat falls in the file, which is where the decode
            // started plus where it falls in what was decoded.
            first_beat_us: Some(audio.start + found.first_beat),
            confidence: Some(found.confidence),
            origin: Some(origin),
            asset_path: (origin == BeatOrigin::Music).then(|| music.display().to_string()),
            asset_duration_us: audio.total_duration,
        })
    }

    /// A beat in the footage's own audio, when it is convincing enough to cut
    /// to. Dance footage carries the music it was danced to; a windy beach
    /// does not, and a grid mined from one would be noise.
    fn beat_in_footage(&self, graph: &FootageGraph) -> Option<BeatGrid> {
        const CONVINCING: f64 = 0.35;
        let source = graph
            .sources
            .iter()
            .filter(|s| s.has_audio.unwrap_or(false))
            .max_by_key(|s| s.duration_us.0)?;
        let mut grid = self
            .beat_grid(Path::new(&source.path), BeatOrigin::Source)
            .ok()?;
        if grid.confidence.unwrap_or(0.0) < CONVINCING {
            tracing::info!(
                confidence = grid.confidence,
                "the footage has no convincing beat; leaving the grid to the template"
            );
            return None;
        }
        // The grid belongs to a clip, not to the finished cut, so it carries
        // no asset to lay under it.
        grid.asset_path = None;
        grid.asset_duration_us = None;
        Some(grid)
    }

    fn analyze(
        &self,
        input: &Path,
        hash: Blake3Hash,
        work_dir: &Path,
        observer: &dyn vb_perception::Observer,
    ) -> Result<FootageGraph, PipelineError> {
        let perception = Perception {
            media: self.media,
            asr: self.asr,
            vlm: self.vlm,
            tracker: self.tracker,
            director: self.director,
            config: self.config.clone(),
        };
        // Priors are mined after the project's sources are merged, not here:
        // one clip's shot-length distribution is not the project's.
        Ok(perception.analyze_hashed(input, Some(hash), "vb-project", work_dir, observer)?)
    }

    fn mocked_adapters(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (is_mock, name) in [
            (self.asr.is_mock(), self.asr.name()),
            (self.vlm.is_mock(), self.vlm.name()),
            (self.tracker.is_mock(), self.tracker.name()),
        ] {
            if is_mock {
                out.push(name);
            }
        }
        out
    }

    /// Hash the settings that affect analysis output, so changing one misses
    /// the cache and leaving them alone hits it.
    fn config_hash(&self) -> Blake3Hash {
        let c = &self.config;
        let vlm = self.vlm.cache_identity();
        Blake3Hash::of_parts(&[
            // The analysis itself: a new way of reading footage must not be
            // served an old reading of it from the cache.
            vb_perception::ANALYSIS_VERSION.as_bytes(),
            format!("{:.4}", c.scene_threshold).as_bytes(),
            c.min_shot.0.to_string().as_bytes(),
            c.max_keyframes_per_shot.to_string().as_bytes(),
            c.keyframe_every_seconds.to_string().as_bytes(),
            c.score_width.to_string().as_bytes(),
            c.score_height.to_string().as_bytes(),
            // Adapter identities belong in the key: swapping a mock for a real
            // model must not silently reuse the mock's output. The media
            // backend is here for the same reason: a graph cached by one
            // backend and served under another would carry a provenance
            // record naming a backend that never ran.
            self.media.name().as_bytes(),
            self.asr.name().as_bytes(),
            vlm.as_bytes(),
            self.tracker.name().as_bytes(),
            self.director.name().as_bytes(),
        ])
    }
}

/// A 主体衔接 cut, verified, and what became of the action.
fn compose_action(
    graph: &FootageGraph,
    template: &vb_narrative::Template,
) -> Result<(vb_narrative::Composition, vb_narrative::continuity::Report), PipelineError> {
    match vb_narrative::compose_continuity_grounded(graph, template) {
        Ok(done) => Ok(done),
        Err(vb_narrative::NarrativeError::NoAction) => Err(PipelineError::NoAction),
        Err(e) => Err(e.into()),
    }
}

/// What a 主体衔接 run says about the action it cut: how many actions, how
/// fast they play, how much of them is shown, and how many joins do not
/// carry the movement -- said, never hidden behind a cut that looks whole.
fn continuity_notes(report: &vb_narrative::continuity::Report, lang: Lang) -> Vec<String> {
    let mut notes = Vec::new();
    if report.pieces > 1 {
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: the footage holds {} actions no other take repeats; they \
                 play one after another, in the order their movements join",
                report.pieces
            ),
            Lang::Zh => format!(
                "主体衔接：素材里有 {} 段互不重复的动作，按动作衔接得上的顺序依次播放",
                report.pieces
            ),
        });
    }
    // The dance on the beat, first: at what speed each piece's own beat was
    // put on the cut's, or that it could not be.
    for (k, pace) in report.speeds.iter().enumerate() {
        let which = if report.pieces > 1 {
            match lang {
                Lang::En => format!(" (action {} of {})", k + 1, report.pieces),
                Lang::Zh => format!("（第 {}/{} 段动作）", k + 1, report.pieces),
            }
        } else {
            String::new()
        };
        match (pace.on_beat, pace.dance_bpm) {
            (true, Some(bpm)) if (pace.speed - 1.0).abs() > 1e-9 => notes.push(match lang {
                Lang::En => format!(
                    "Subject continuity{which}: the dancers keep {bpm:.0} BPM; played at {:.2}x \
                     so their steps land on the cut's beat",
                    pace.speed
                ),
                Lang::Zh => format!(
                    "主体衔接{which}：舞者自己的节奏是 {bpm:.0} BPM，按 {:.2} 倍速播放，让舞步落在剪辑的拍子上",
                    pace.speed
                ),
            }),
            (false, Some(bpm)) => notes.push(match lang {
                Lang::En => format!(
                    "Subject continuity{which}: the dancers keep {bpm:.0} BPM, further from the \
                     cut's beat than a change of up to {:.1}x either way can bring it, so the \
                     cuts are on the beat and the steps are not",
                    vb_narrative::continuity::MAX_SPEED
                ),
                Lang::Zh => format!(
                    "主体衔接{which}：舞者自己的节奏是 {bpm:.0} BPM，和剪辑的节拍相差太大，{:.1} 倍以内的变速对不上：剪辑点在拍上，舞步不在",
                    vb_narrative::continuity::MAX_SPEED
                ),
            }),
            _ if (pace.speed - 1.0).abs() > 1e-9 => notes.push(match lang {
                Lang::En => format!(
                    "Subject continuity{which}: every clip plays at {:.2}x so the action fits \
                     the length chosen ({})",
                    pace.speed, report.room
                ),
                Lang::Zh => format!(
                    "主体衔接{which}：按 {:.2} 倍速播放，让动作放进设定的时长（{}）",
                    pace.speed, report.room
                ),
            }),
            _ => {}
        }
    }
    if report.practice > 0 {
        let (count, len) = (report.practice, report.practice_len);
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: {count} take{} ({len}) left out whole -- their people danced \
                 less than a quarter of it: learning the steps from a phone, or waiting about",
                if count == 1 { "" } else { "s" }
            ),
            Lang::Zh => format!(
                "主体衔接：{count} 段素材（共 {len}）整段不用——里面的人跳舞的时间不到四分之一，像是在看手机学动作或在等"
            ),
        });
    }
    if report.idle >= Micros::from_secs(1) {
        let gone = report.idle;
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: {gone} of the takes is left out as not dancing -- someone \
                 standing still for more than a beat and a half, watching the phone in their \
                 hands, or a step with no dancing either side"
            ),
            Lang::Zh => format!(
                "主体衔接：素材里有 {gone} 不算跳舞——有人停着超过一拍半、拿着手机在看、或前后都不跳的零碎动作——已从动作里剔除"
            ),
        });
    }
    if report.shortened {
        let missing = ((1.0 - report.shown) * 100.0).round() as u32;
        let tail = report.dropped_tail;
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: the action is longer than the length chosen, so every clip \
                 was shortened by one share and {missing}% of the action is not shown{}",
                if tail > Micros::ZERO {
                    format!(", {tail} of it from the end")
                } else {
                    String::new()
                }
            ),
            Lang::Zh => format!(
                "主体衔接：动作比设定的时长长，每段按同样比例缩短，动作有 {missing}% 没有播出{}",
                if tail > Micros::ZERO {
                    format!("，其中结尾的 {tail} 被截掉")
                } else {
                    String::new()
                }
            ),
        });
    }
    if report.shorter_than_chosen {
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: the whole action runs {}, shorter than the length chosen, \
                 so the cut ends with it rather than being padded",
                report.natural
            ),
            Lang::Zh => format!(
                "主体衔接：完整的动作只有 {}，比设定的时长短，成片随动作结束，没有拿别的画面凑时长",
                report.natural
            ),
        });
    }
    let missed = report.joins.saturating_sub(report.carried);
    if missed > 0 {
        notes.push(match lang {
            Lang::En => format!(
                "Subject continuity: {missed} of {} joins do not quite carry the movement \
                 across; each clip's reason says which",
                report.joins
            ),
            Lang::Zh => format!(
                "主体衔接：{} 个接点里有 {missed} 个动作没有完全接上，具体见每段的选择理由",
                report.joins
            ),
        });
    }
    notes
}

/// How many clips to analyse at once.
///
/// Three on a machine with ten cores or more, fewer below: the Mac's media
/// engine decodes three 4K streams in the time it takes one, and each clip's
/// own stages mostly use one core. `VB_ANALYSIS_WORKERS` overrides it.
pub fn analysis_workers() -> usize {
    if let Some(n) = std::env::var("VB_ANALYSIS_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
    {
        return n.min(16);
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .div_ceil(3)
        .clamp(1, 3)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), PipelineError> {
    let text = serde_json::to_string_pretty(value)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| PipelineError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }
    std::fs::write(path, text).map_err(|e| PipelineError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// The templates this build ships with.
///
/// Exposed here so the FFI layer does not have to reach into the narrative
/// crate directly; front ends talk to the pipeline, not past it.
pub fn templates() -> Vec<vb_narrative::Template> {
    PriorStore::with_builtins().iter().cloned().collect()
}

/// A folder name for a draft that no other draft in that root has.
///
/// The editor lists drafts by folder name, so two runs of the same project
/// must not collide -- the second would overwrite the first, silently losing
/// whatever the user had already changed in it.
/// Write a cut into an editor's own draft folder, named after `title`.
///
/// Anywhere else and the editor never sees it, which looks to a user
/// exactly like the export having failed. A run writes one for each editor
/// the project names; the built-in editor writes one on request.
pub fn write_editor_draft(
    editor: Editor,
    dsl: &EditingDsl,
    graph: &FootageGraph,
    title: &str,
    media: DraftMedia,
) -> Result<DraftSummary, PipelineError> {
    let install = vb_export::detect_editors()
        .into_iter()
        .find(|i| i.editor == editor)
        .ok_or(PipelineError::NoDraftRoot { editor })?;
    let root = install
        .draft_root
        .ok_or(PipelineError::NoDraftRoot { editor })?;

    let name = draft_folder_name(&root, title);
    let writer = JianyingWriter {
        media,
        ..JianyingWriter::for_editor(editor)
    };
    Ok(writer.write_draft(dsl, graph, &root.join(name))?)
}

fn draft_folder_name(root: &Path, title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    // One dash where punctuation and spaces ran together: "VB 验证 · 旅行"
    // becomes "VB-验证-旅行", not "VB-验证---旅行".
    let mut collapsed = String::with_capacity(slug.len());
    for c in slug.chars() {
        if !(c == '-' && collapsed.ends_with('-')) {
            collapsed.push(c);
        }
    }
    let slug = collapsed.trim_matches('-').to_string();
    let slug = if slug.is_empty() {
        "vb-draft".to_string()
    } else {
        slug
    };

    let mut name = slug.clone();
    let mut attempt = 2;
    while root.join(&name).exists() {
        name = format!("{slug}-{attempt}");
        attempt += 1;
    }
    name
}

/// Attach a beat grid, keeping whatever else was mined.
fn set_beat_grid(graph: &mut FootageGraph, grid: BeatGrid) {
    let priors = graph.priors.get_or_insert_with(Default::default);
    priors.beat_grid = Some(grid);
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("input file not found: {path}")]
    InputNotFound { path: String },

    #[error("no templates are available; the prior store is empty")]
    NoTemplates,

    #[error("template {0:?} is not in the prior store")]
    UnknownTemplate(String),

    #[error("every decision failed grounding; there is no cut to export")]
    EmptyCut,

    #[error("a project needs at least one video or photo")]
    NoInputs,

    #[error("the run was cancelled")]
    Cancelled,

    #[error("a dance cut was asked for, and nobody is dancing in this footage")]
    NoDancing,

    #[error("a 主体衔接 cut was asked for, and no subject acts in this footage")]
    NoAction,

    #[error("a 主体衔接 cut follows its subject by body pose, and no pose model is installed")]
    NeedsPoseModel,

    #[error("{editor} has no draft folder on this machine; open it once and it will create one")]
    NoDraftRoot { editor: Editor },

    #[error("could not read {path} as music: {detail}")]
    Music { path: String, detail: String },

    #[error(transparent)]
    Perception(#[from] vb_perception::PerceptionError),

    #[error(transparent)]
    Narrative(#[from] vb_narrative::NarrativeError),

    #[error(transparent)]
    Export(#[from] vb_export::ExportError),

    #[error(transparent)]
    Validation(#[from] vb_contracts::validate::ValidationError),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_name_keeps_the_title_readable() {
        let root = std::env::temp_dir().join("vb-draft-names-readable");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(draft_folder_name(&root, "VB 验证 · 旅行"), "VB-验证-旅行");
        assert_eq!(draft_folder_name(&root, "  Kyoto!! "), "Kyoto");
        assert_eq!(draft_folder_name(&root, "···"), "vb-draft");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_run_never_writes_over_the_first_draft() {
        // The user may already have edited the first one in the editor.
        let root = std::env::temp_dir().join("vb-draft-names-unique");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Kyoto")).unwrap();
        std::fs::create_dir_all(root.join("Kyoto-2")).unwrap();
        assert_eq!(draft_folder_name(&root, "Kyoto"), "Kyoto-3");
        let _ = std::fs::remove_dir_all(&root);
    }
}
