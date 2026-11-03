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

//! `vb` -- the headless entry point.
//!
//! The CLI exists before the Flutter app on purpose. It forces the core to be
//! drivable with no window and no platform SDK, which keeps the app one
//! consumer of the engine rather than its owner, and gives CI a way to
//! exercise every contract boundary on a machine with no display.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vb_cli::serve;
use vb_export::Editor;
use vb_media::MediaBackend;
use vb_narrative::{Mood, PriorStore, Style, Topic};
use vb_pipeline::{Library, ModelChoice, Observer, Pipeline, Progress, RunOptions, Workspace};

#[derive(Parser)]
#[command(
    name = "vb",
    version,
    about = "Video Butler: on-device AI video auto-editing"
)]
struct Cli {
    /// Show what each stage is doing.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse a video and write a draft project file.
    Run {
        /// The videos to edit. A directory is expanded into the videos in it,
        /// recursively, so a project can be a folder.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Where to write the draft. Defaults to a file beside the input.
        #[arg(short, long)]
        out: Option<PathBuf>,

        /// Template id to use. Defaults to whichever one matches the footage.
        #[arg(short, long)]
        template: Option<String>,

        /// Directory for intermediate files and the analysis cache.
        #[arg(long)]
        project_dir: Option<PathBuf>,

        /// Refuse to run if any configured adapter would need the network.
        #[arg(long)]
        offline: bool,

        /// Ignore any cached analysis and re-read the media.
        #[arg(long)]
        no_cache: bool,

        /// The emotional register of the cut.
        #[arg(long, value_enum)]
        mood: Option<MoodArg>,

        /// What the footage is about, which decides the cut's structure.
        #[arg(long, value_enum)]
        topic: Option<TopicArg>,

        /// Put every cut on a beat: of --music when given, of the footage's own
        /// audio when it has a clear one, of a steady tempo otherwise.
        #[arg(long)]
        beat: bool,

        /// Music to cut to and lay under the result.
        #[arg(long)]
        music: Option<PathBuf>,

        /// What to call the piece. Becomes the title card.
        #[arg(long)]
        title: Option<String>,

        /// How long the finished piece should be, in seconds. Defaults to
        /// whatever the style suits.
        #[arg(long)]
        duration: Option<u64>,

        /// Also write a draft into 剪映专业版's draft folder.
        #[arg(long)]
        jianying: bool,

        /// Also write a draft into CapCut's draft folder.
        #[arg(long)]
        capcut: bool,

        /// Play the music again from its first beat when the cut runs longer
        /// than it. Without this, the end of a long cut is silent.
        #[arg(long)]
        loop_music: bool,

        /// Point the editor's draft at the original files even where the
        /// sandboxed editor may not read them, rather than hard-linking them
        /// into the draft (which costs no disk space).
        #[arg(long)]
        reference_media: bool,
    },

    /// List the templates in the prior store.
    Templates,

    /// Check a footage graph or editing DSL against its schema.
    Validate {
        /// The JSON document to check.
        file: PathBuf,
    },

    /// Report whether the tools the pipeline needs are present.
    Doctor,

    /// Call any engine operation by name, as the app does, and print the
    /// JSON it returns. `vb call selftest`, `vb call models.list`,
    /// `vb call models.download '{"id":"whisper-base-q5"}' --wait`.
    Call {
        /// The operation, e.g. `projects.list` or `run.start`.
        method: String,

        /// Its arguments, as a JSON object.
        #[arg(default_value = "{}")]
        args: String,

        /// When the operation starts a job, follow it to the end and print
        /// its result, rather than printing the job id and exiting.
        #[arg(long)]
        wait: bool,
    },

    /// Run the desktop app as a local web app.
    ///
    /// Flutter cannot build a macOS app without full Xcode, but its web target
    /// needs no Xcode at all. This serves the same Dart UI from the same
    /// engine, so a working desktop app does not wait on a 10 GB install.
    Serve {
        /// Port to listen on. Loopback only, never exposed to the network.
        #[arg(short, long, default_value_t = 8787)]
        port: u16,

        /// The built Flutter web app. Defaults to app/build/web.
        #[arg(long)]
        web_root: Option<PathBuf>,

        /// Do not open a browser window.
        #[arg(long)]
        no_open: bool,

        /// Allow cross-origin requests from this origin, for hot-reload
        /// development. Insecure; leave it off otherwise.
        #[arg(long)]
        allow_origin: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(if cli.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        })
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Command::Run {
            inputs,
            out,
            template,
            project_dir,
            offline,
            no_cache,
            mood,
            topic,
            beat,
            music,
            title,
            duration,
            jianying,
            capcut,
            loop_music,
            reference_media,
        } => {
            let mut editors = Vec::new();
            if jianying {
                editors.push(Editor::Jianying);
            }
            if capcut {
                editors.push(Editor::CapCut);
            }
            run(RunArgs {
                inputs,
                out,
                template,
                project_dir,
                offline,
                no_cache,
                style: Style {
                    mood: mood.map(Into::into),
                    topic: topic.map(Into::into),
                    beat_sync: beat.then_some(true),
                },
                music,
                title,
                duration: duration.map(vb_contracts::Micros::from_secs),
                editors,
                loop_music,
                reference_media,
            })
        }
        Command::Templates => templates(),
        Command::Validate { file } => validate(&file),
        Command::Doctor => doctor(),
        Command::Call { method, args, wait } => call(&method, &args, wait),
        Command::Serve {
            port,
            web_root,
            no_open,
            allow_origin,
        } => {
            let web_root = web_root.or_else(default_web_root);
            serve::serve(serve::ServeOptions {
                port,
                open_browser: !no_open && web_root.is_some(),
                web_root,
                allow_origin,
            })
        }
    }
}

/// Find the Flutter web build relative to the repository.
///
/// Checked rather than assumed, so `serve` can say the app has not been built
/// instead of serving a directory that is not there.
fn default_web_root() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("app/build/web"),
        PathBuf::from("../app/build/web"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/build/web"),
    ];
    candidates
        .into_iter()
        .find(|p| p.join("index.html").is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}

#[derive(Clone, Copy, ValueEnum)]
enum MoodArg {
    Happy,
    Sad,
    Humor,
}

impl From<MoodArg> for Mood {
    fn from(m: MoodArg) -> Mood {
        match m {
            MoodArg::Happy => Mood::Happy,
            MoodArg::Sad => Mood::Sad,
            MoodArg::Humor => Mood::Humor,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum TopicArg {
    Dance,
    Travel,
    Growth,
    /// 主体衔接: one action carried across every cut.
    Continuity,
    /// A year in review: people, scenery, food, the rest.
    Annual,
    /// The same group of people over time.
    Together,
}

impl From<TopicArg> for Topic {
    fn from(t: TopicArg) -> Topic {
        match t {
            TopicArg::Dance => Topic::Dance,
            TopicArg::Travel => Topic::Travel,
            TopicArg::Growth => Topic::Growth,
            TopicArg::Continuity => Topic::Continuity,
            TopicArg::Annual => Topic::Annual,
            TopicArg::Together => Topic::Together,
        }
    }
}

struct RunArgs {
    inputs: Vec<PathBuf>,
    out: Option<PathBuf>,
    template: Option<String>,
    project_dir: Option<PathBuf>,
    offline: bool,
    no_cache: bool,
    style: Style,
    music: Option<PathBuf>,
    title: Option<String>,
    duration: Option<vb_contracts::Micros>,
    editors: Vec<Editor>,
    loop_music: bool,
    reference_media: bool,
}

/// Prints where a run has got to: a line when the stage changes, and one
/// every five percent within it, so a long run shows it is working without
/// flooding the scrollback.
struct TerminalProgress {
    last: Mutex<(Option<vb_pipeline::RunStage>, i64)>,
}

impl Observer for TerminalProgress {
    fn progress(&self, progress: &Progress) {
        let Ok(mut last) = self.last.lock() else {
            return;
        };
        let percent = progress.percent.floor() as i64;
        let new_stage = last.0 != Some(progress.stage);
        if !new_stage && percent < last.1 + 5 {
            return;
        }
        *last = (Some(progress.stage), percent);
        eprintln!(
            "[{percent:>3}%] {:<24} {}",
            progress.stage.label(),
            progress.detail
        );
    }
}

fn run(args: RunArgs) -> Result<()> {
    let RunArgs {
        inputs,
        out,
        template,
        project_dir,
        offline,
        no_cache,
        style,
        music,
        title,
        duration,
        editors,
        loop_music,
        reference_media,
    } = args;

    let media = vb_media::default_backend();
    if !media.is_available() {
        bail!(
            "ffmpeg was not found on PATH. Install it, or point the pipeline at a build \
             explicitly. On macOS: brew install ffmpeg"
        );
    }

    // Folders become the videos inside them here, so the rest of this
    // function and the engine see one flat list either way.
    let named: Vec<String> = inputs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let sources = vb_pipeline::api::expand_inputs(&named).map_err(|e| anyhow::anyhow!(e))?;
    if sources.is_empty() {
        bail!("no videos were found in what you named");
    }

    let project_dir = project_dir.unwrap_or_else(|| {
        sources[0]
            .parent()
            .unwrap_or(Path::new("."))
            .join("vb-project")
    });
    let workspace = Workspace::beside(&project_dir);

    // The same adapters the app would use: on-device perception where this
    // platform has it, a downloaded speech model when there is one, and the
    // mocks otherwise -- each reported below rather than passed off as real.
    let library = Library::open();
    let adapters = vb_pipeline::api::adapters_for(
        &library,
        ModelChoice::Local,
        &workspace.work_dir,
        vb_pipeline::Lang::En,
    );
    let mut pipeline = Pipeline::new(
        media.as_ref(),
        adapters.asr.as_ref(),
        adapters.vlm.as_ref(),
        adapters.tracker.as_ref(),
    );
    pipeline.director = adapters.director();
    pipeline.judge = adapters.judge.as_deref();
    pipeline.captioner = adapters.captioner.as_deref();
    if offline && pipeline.requires_network() {
        bail!("--offline was requested but a configured adapter needs the network");
    }

    if sources.len() == 1 {
        println!("Analysing {}", sources[0].display());
    } else {
        println!("Analysing {} videos", sources.len());
        for source in &sources {
            println!("  {}", source.display());
        }
    }
    let annual = style.topic == Some(Topic::Annual);
    let options = RunOptions {
        template_id: template,
        style,
        music,
        title,
        draft_path: out,
        editors,
        target_duration: duration,
        // The CLI speaks English; the app passes the user's language.
        lang: vb_pipeline::Lang::En,
        // Read nothing back, write what is found: the next run reuses it.
        fresh: no_cache,
        music_loop: loop_music,
        draft_media: if reference_media {
            vb_export::DraftMedia::Reference
        } else {
            vb_export::DraftMedia::Link
        },
        order: None,
        // A year in review is cut in sections by what each video is of:
        // filed now when the video butler has not filed it already.
        categories: {
            let eyes = vb_pipeline::hosting::Eyes {
                media: pipeline.media,
                labels: pipeline.vlm,
                faces: pipeline.tracker,
            };
            vb_pipeline::hosting::categories_for(
                &vb_pipeline::Library::open(),
                &sources,
                annual.then_some(&eyes),
            )
        },
    };
    let progress = TerminalProgress {
        last: Mutex::new((None, -100)),
    };
    let result = pipeline
        .run_project(&sources, &workspace, &options, &progress)
        .context("the pipeline failed")?;

    if result.reused_sources > 0 {
        println!(
            "Reused the cached analysis for {} of {} videos.",
            result.reused_sources, result.total_sources
        );
    }

    println!();
    println!(
        "Template   {} ({})",
        result.template_name, result.template_id
    );
    if let Some(beat) = result
        .graph
        .priors
        .as_ref()
        .and_then(|p| p.beat_grid.as_ref())
    {
        println!(
            "Beat       {:.1} BPM, {}",
            beat.bpm,
            match beat.origin {
                Some(vb_contracts::footage_graph::BeatOrigin::Music) => "from the music",
                Some(vb_contracts::footage_graph::BeatOrigin::Source) => "from the footage",
                _ => "a steady grid",
            }
        );
    }
    let clips = result
        .dsl
        .slots
        .iter()
        .filter(|s| s.kind == vb_contracts::SlotKind::Clip)
        .count();
    println!(
        "Shots      {} detected, {} used",
        result.graph.shots.len(),
        clips
    );
    let timeline = result.dsl.resolve_timeline();
    println!("Duration   {}", timeline.total_duration_us);
    println!(
        "Grounding  {} verified, {} rejected",
        result.grounding.verified.len(),
        result.grounding.rejected.len()
    );
    println!();
    println!("Draft         {}", result.draft_path.display());
    println!("Footage graph {}", result.graph_path.display());
    println!("Editing DSL   {}", result.dsl_path.display());
    for draft in &result.drafts {
        println!("Editor draft  {}", draft.path.display());
    }
    if !adapters.notes.is_empty() {
        println!();
        for note in &adapters.notes {
            println!("  {note}");
        }
    }

    if !result.grounding.rejected.is_empty() {
        println!();
        println!("Rejected decisions:");
        for rejection in &result.grounding.rejected {
            println!("  {}: {}", rejection.slot_id, rejection.reason);
        }
    }

    if !result.warnings.is_empty() {
        println!();
        println!("This is not a finished cut:");
        for warning in &result.warnings {
            println!("  {warning}");
        }
    }

    Ok(())
}

fn call(method: &str, args: &str, wait: bool) -> Result<()> {
    let args: serde_json::Value =
        serde_json::from_str(args).context("the arguments are not a JSON object")?;
    let value = vb_pipeline::api::call(method, &args).map_err(|e| anyhow::anyhow!(e))?;

    let job = value["jobId"].as_str().map(str::to_string);
    let Some(job) = job.filter(|_| wait) else {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    };

    // The job runs on a thread in this process, so exiting would kill it:
    // follow it here, the way the app polls it.
    let mut shown = String::new();
    loop {
        let status = vb_pipeline::jobs::status(&job).context("the job vanished")?;
        let line = format!(
            "[{:>5.1}%] {:<24} {}",
            status["percent"].as_f64().unwrap_or(0.0),
            status["stageLabel"].as_str().unwrap_or_default(),
            status["detail"].as_str().unwrap_or_default()
        );
        if line != shown {
            eprintln!("{line}");
            shown = line;
        }
        if status["finished"] == true {
            match status["state"].as_str() {
                Some("done") => {
                    println!("{}", serde_json::to_string_pretty(&status["result"])?);
                    return Ok(());
                }
                Some("cancelled") => bail!("the job was cancelled"),
                _ => bail!("{}", status["error"].as_str().unwrap_or("the job failed")),
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn templates() -> Result<()> {
    let store = PriorStore::with_builtins();
    println!("{} templates\n", store.len());
    for t in store.iter() {
        println!("{}", t.id);
        println!("  {} ({:?})", t.name, t.kind);
        println!(
            "  target {}, clips {} to {}, {}",
            t.params.target_duration_us,
            t.params.min_slot_us,
            t.params.max_slot_us,
            t.params.transition
        );
        if !t.tags.is_empty() {
            println!("  tags: {}", t.tags.join(", "));
        }
        println!();
    }
    Ok(())
}

fn validate(file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("could not read {}", file.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not valid JSON", file.display()))?;

    // Dispatch on the document's own declared version rather than on the file
    // name, so a renamed file still validates against the right contract.
    let version = value
        .get("schemaVersion")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    match version {
        "footage-graph/0.1" => {
            vb_contracts::validate::validate_footage_graph_value(&value)?;
            println!("{} satisfies footage-graph/0.1", file.display());
        }
        "editing-dsl/0.1" => {
            vb_contracts::validate::validate_editing_dsl_value(&value)?;
            println!("{} satisfies editing-dsl/0.1", file.display());
        }
        "" => bail!("{} has no schemaVersion field", file.display()),
        other => bail!("unknown contract version {other:?}"),
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let media = vb_media::default_backend();
    let ffmpeg = media.is_available();

    println!("ffmpeg       {}", status(ffmpeg));
    println!("  backend    {}", describe_backend(media.as_ref()));
    if let Some(version) = media.version() {
        println!("  version    {version}");
    }
    if let Some(license) = media.license() {
        let verdict = if vb_media::is_shippable_license(&license) {
            "shippable"
        } else {
            "fine for development, not shippable; a release needs an LGPL build"
        };
        println!("  license    {license} ({verdict})");
    }

    let store = PriorStore::with_builtins();
    println!("templates    {} built in", store.len());

    // What a run would actually use, from the same report the app shows:
    // printing a fixed list here once claimed mocks on a machine where every
    // adapter was real.
    println!();
    println!("Adapters a run would use here:");
    let report = vb_pipeline::api::doctor();
    for adapter in report["adapters"].as_array().into_iter().flatten() {
        println!(
            "  {:<10} {}{}",
            adapter["role"].as_str().unwrap_or("?"),
            adapter["name"].as_str().unwrap_or("?"),
            if adapter["mock"].as_bool().unwrap_or(true) {
                " (mock: produces nothing real)"
            } else {
                ""
            }
        );
    }
    println!();
    println!("A run needs no model weights and no network. Where an adapter is a");
    println!("mock, the draft is structurally complete rather than a finished cut.");

    if !ffmpeg {
        bail!("ffmpeg is required and was not found on PATH");
    }
    Ok(())
}

fn describe_backend(media: &dyn MediaBackend) -> String {
    if media.spawns_processes() {
        format!("{} (runs the ffmpeg binaries; desktop only)", media.name())
    } else {
        format!("{} (linked in-process)", media.name())
    }
}

fn status(ok: bool) -> &'static str {
    if ok { "found" } else { "MISSING" }
}
