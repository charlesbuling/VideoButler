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

//! Long work, started and then watched.
//!
//! Analysing a folder of clips takes minutes and downloading a model takes
//! longer, and neither can be a call that blocks until it finishes: the app
//! would have nothing to show and no way to stop it. So the front end starts
//! a job, polls it for a stage and a percentage, and can cancel it.
//!
//! The registry is process-wide because all three front ends are one process
//! with the engine linked in. Jobs run on their own threads; nothing here
//! blocks the caller.
//!
//! Cancellation is cooperative and honest: it asks, and the job stops at its
//! next checkpoint. A job that has already written a draft stays written --
//! cancelling is "stop working", not "undo".

use crate::progress::{Observer, Progress, RunStage};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// What a job is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn id(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    pub fn is_finished(self) -> bool {
        self != JobState::Running
    }
}

#[derive(Debug, Clone)]
struct Report {
    state: JobState,
    stage: String,
    stage_label: String,
    percent: f64,
    detail: String,
    step: String,
    file: Option<String>,
    source_index: usize,
    source_count: usize,
    active: usize,
    result: Option<Value>,
    error: Option<String>,
}

impl Report {
    fn starting() -> Report {
        Report {
            state: JobState::Running,
            stage: RunStage::Prepare.id().into(),
            stage_label: RunStage::Prepare.label().into(),
            percent: 0.0,
            detail: String::new(),
            step: String::new(),
            file: None,
            source_index: 0,
            source_count: 0,
            active: 0,
            result: None,
            error: None,
        }
    }
}

/// A running job, and the handle the work reports through.
pub struct Job {
    pub id: String,
    pub kind: String,
    report: Mutex<Report>,
    cancel: AtomicBool,
    finished_at: Mutex<Option<std::time::Instant>>,
}

impl Job {
    /// Report where the work has got to.
    pub fn update(&self, progress: &Progress) {
        let mut report = self.report.lock().expect("a job report is never poisoned");
        report.stage = progress.stage.id().to_string();
        report.stage_label = progress.stage.label().to_string();
        // Never backwards: a bar that retreats reads as a bug even when the
        // work is going fine, and stages do overlap slightly in their
        // estimates.
        report.percent = report.percent.max(progress.percent).clamp(0.0, 100.0);
        report.detail = progress.detail.clone();
        report.step = progress.step.clone();
        report.file = progress.file.clone();
        report.source_index = progress.source_index;
        report.source_count = progress.source_count;
        report.active = progress.active;
    }

    /// A message for the user with no percentage attached, for work that has
    /// no useful fraction to report.
    pub fn say(&self, stage: RunStage, percent: f64, detail: impl Into<String>) {
        let detail = detail.into();
        self.update(&Progress {
            stage,
            percent,
            step: stage.id().to_string(),
            detail,
            file: None,
            source_index: 0,
            source_count: 0,
            active: 0,
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Work that heats a device: a run, the video butler's pass, and a
    /// video being exported.
    fn paced(&self) -> bool {
        matches!(self.kind.as_str(), "run" | "hosting" | "render")
    }

    fn finish(&self, state: JobState, result: Option<Value>, error: Option<String>) {
        {
            let mut report = self.report.lock().expect("a job report is never poisoned");
            report.state = state;
            if state == JobState::Done {
                report.percent = 100.0;
                report.stage = RunStage::Done.id().into();
                report.stage_label = RunStage::Done.label().into();
            }
            report.result = result;
            report.error = error;
        }
        *self
            .finished_at
            .lock()
            .expect("a job clock is never poisoned") = Some(std::time::Instant::now());
    }

    pub fn status(&self) -> Value {
        let report = self.report.lock().expect("a job report is never poisoned");
        json!({
            "jobId": self.id,
            "kind": self.kind,
            "state": report.state.id(),
            "finished": report.state.is_finished(),
            "stage": report.stage,
            "stageLabel": report.stage_label,
            "percent": (report.percent * 10.0).round() / 10.0,
            "detail": report.detail,
            "step": report.step,
            "file": report.file,
            "sourceIndex": report.source_index,
            "sourceCount": report.source_count,
            "activeSources": report.active,
            // What the heat guard is doing to this run, if anything.
            "thermal": if self.paced() && !report.state.is_finished() {
                crate::thermal::acting()
            } else {
                None
            },
            "result": report.result,
            "error": report.error,
            "stages": RunStage::ALL.map(|s| json!({ "id": s.id(), "label": s.label() })),
        })
    }
}

/// A job's handle is what the pipeline reports progress through.
impl Observer for Job {
    fn progress(&self, progress: &Progress) {
        self.update(progress);
    }

    fn cancelled(&self) -> bool {
        // Between units of work is where a run paces itself on a hot device.
        if self.paced() {
            crate::thermal::pace(&|| self.is_cancelled());
        }
        self.is_cancelled()
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Job>>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Arc<Job>>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// How long a finished job's result stays readable.
///
/// Long enough that a front end which stopped polling -- a laptop that slept,
/// a window that was closed -- can still collect it, and short enough that a
/// day of runs does not sit in memory.
const KEEP_FINISHED: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Start work on its own thread and return its job id.
pub fn start<F>(kind: &str, work: F) -> String
where
    F: FnOnce(&Job) -> Result<Value, String> + Send + 'static,
{
    let id = new_id();
    let job = Arc::new(Job {
        id: id.clone(),
        kind: kind.to_string(),
        report: Mutex::new(Report::starting()),
        cancel: AtomicBool::new(false),
        finished_at: Mutex::new(None),
    });

    {
        let mut jobs = registry()
            .lock()
            .expect("the job registry is never poisoned");
        prune(&mut jobs);
        jobs.insert(id.clone(), Arc::clone(&job));
    }

    let worker = Arc::clone(&job);
    std::thread::Builder::new()
        .name(format!("vb-{kind}"))
        .spawn(move || {
            // A run takes minutes to hours and is usually left alone: the
            // app behind other windows, the screen locked overnight. Without
            // this macOS naps the app onto its slow cores, or sleeps.
            vb_apple::power::user_initiated_thread();
            let _awake = vb_apple::power::keep_awake(match worker.kind.as_str() {
                "download" => "Video Butler is downloading a model",
                "render" => "Video Butler is exporting a video",
                _ => "Video Butler is analysing footage",
            });
            #[cfg(windows)]
            let _awake_here = windows_awake::hold();
            // A panic in the work must become a failed job, not a dead
            // process: this thread is inside the user's app.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&worker)));
            match outcome {
                Ok(Ok(value)) => worker.finish(JobState::Done, Some(value), None),
                Ok(Err(message)) => {
                    if worker.is_cancelled() {
                        worker.finish(JobState::Cancelled, None, None);
                    } else {
                        worker.finish(JobState::Failed, None, Some(message));
                    }
                }
                Err(payload) => {
                    let detail = payload
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    worker.finish(
                        JobState::Failed,
                        None,
                        Some(format!("the engine panicked: {detail}")),
                    );
                }
            }
        })
        .expect("the host must allow a worker thread");

    id
}

pub fn status(id: &str) -> Option<Value> {
    let jobs = registry()
        .lock()
        .expect("the job registry is never poisoned");
    jobs.get(id).map(|job| job.status())
}

/// Ask a job to stop. Returns false when there is no such job.
pub fn cancel(id: &str) -> bool {
    let jobs = registry()
        .lock()
        .expect("the job registry is never poisoned");
    match jobs.get(id) {
        Some(job) => {
            job.cancel.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Every job this process knows about, newest first.
pub fn list() -> Vec<Value> {
    let jobs = registry()
        .lock()
        .expect("the job registry is never poisoned");
    let mut all: Vec<Value> = jobs.values().map(|job| job.status()).collect();
    all.sort_by(|a, b| b["jobId"].as_str().cmp(&a["jobId"].as_str()));
    all
}

fn prune(jobs: &mut HashMap<String, Arc<Job>>) {
    jobs.retain(|_, job| {
        job.finished_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .map(|at| at.elapsed() < KEEP_FINISHED)
            .unwrap_or(true)
    });
}

fn new_id() -> String {
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!(
        "job-{stamp:013}-{:04}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}


/// What `vb_apple::power::keep_awake` is on a Mac, for Windows: while held,
/// the machine does not sleep for want of input -- a desktop sleeps after
/// half an hour of none by default, and a run left alone would stop with
/// it. The screen may still turn off. It holds for the calling thread, the
/// job's, until dropped there.
#[cfg(windows)]
mod windows_awake {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    pub struct Awake;

    pub fn hold() -> Awake {
        // SAFETY: flags only; it changes the calling thread's state.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        Awake
    }

    impl Drop for Awake {
        fn drop(&mut self) {
            // SAFETY: as above, on the same thread: back to the system's
            // own idle timers.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_for_finish(id: &str) -> Value {
        for _ in 0..200 {
            let status = status(id).expect("the job is in the registry");
            if status["finished"] == true {
                return status;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the job never finished");
    }

    #[test]
    fn a_job_reports_progress_and_then_its_result() {
        let id = start("test", |job| {
            job.say(RunStage::Analyse, 40.0, "halfway");
            Ok(json!({ "answer": 42 }))
        });
        let status = wait_for_finish(&id);
        assert_eq!(status["state"], "done");
        assert_eq!(status["result"]["answer"], 42);
        assert_eq!(status["percent"], 100.0);
        assert_eq!(status["error"], Value::Null);
    }

    #[test]
    fn a_failure_comes_back_as_a_message_rather_than_a_lost_job() {
        let id = start("test", |_| Err("ffmpeg was not found".to_string()));
        let status = wait_for_finish(&id);
        assert_eq!(status["state"], "failed");
        assert!(status["error"].as_str().unwrap().contains("ffmpeg"));
    }

    #[test]
    fn a_panic_in_the_work_fails_that_job_and_nothing_else() {
        let id = start("test", |_| panic!("boom"));
        let status = wait_for_finish(&id);
        assert_eq!(status["state"], "failed");
        assert!(status["error"].as_str().unwrap().contains("boom"));

        // The registry still works afterwards.
        let after = start("test", |_| Ok(json!("fine")));
        assert_eq!(wait_for_finish(&after)["state"], "done");
    }

    #[test]
    fn cancelling_stops_the_work_at_its_next_checkpoint() {
        let (started, wait) = mpsc::channel();
        let id = start("test", move |job| {
            started.send(()).unwrap();
            for _ in 0..500 {
                if job.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(json!("ran to completion"))
        });
        wait.recv().expect("the job started");
        assert!(cancel(&id));

        let status = wait_for_finish(&id);
        assert_eq!(
            status["state"], "cancelled",
            "a job that stopped because it was asked to is cancelled, not failed"
        );
    }

    #[test]
    fn cancelling_a_job_that_is_not_there_says_so() {
        assert!(!cancel("job-does-not-exist"));
        assert!(status("job-does-not-exist").is_none());
    }

    #[test]
    fn progress_never_goes_backwards() {
        // Stage estimates overlap, and a bar that retreats reads as a bug.
        let id = start("test", |job| {
            job.say(RunStage::Analyse, 60.0, "on");
            job.say(RunStage::Analyse, 20.0, "an earlier estimate");
            Err("stop here".to_string())
        });
        wait_for_finish(&id);
        // The failure keeps the last percentage rather than resetting it.
        let status = status(&id).unwrap();
        assert!(status["percent"].as_f64().unwrap() >= 60.0);
    }

    #[test]
    fn a_job_lists_the_stages_so_the_ui_can_show_them_all() {
        let id = start("test", |_| Ok(Value::Null));
        let status = wait_for_finish(&id);
        let stages = status["stages"].as_array().unwrap();
        assert_eq!(stages.len(), RunStage::ALL.len());
        assert_eq!(stages[0]["id"], "prepare");
    }
}
