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

use super::*;
use crate::projects::LastRun;
use vb_contracts::Blake3Hash;
use vb_contracts::editing_dsl::{Slot, SlotKind, SourceRef};
use vb_contracts::footage_graph::{MediaKind, Shot, Source};

fn scratch(name: &str) -> Library {
    let root = std::env::temp_dir().join(format!("vb-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let library = Library::at(root);
    library.ensure().unwrap();
    library
}

fn media_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media")
}

fn source(id: &str, path: &Path, seconds: u64) -> Source {
    Source {
        id: id.into(),
        path: path.display().to_string(),
        content_hash: Blake3Hash::of_str(id),
        media_kind: Some(MediaKind::Video),
        duration_us: Micros::from_secs(seconds),
        fps_num: Some(30),
        fps_den: Some(1),
        // As the fixture is.
        width: Some(1280),
        height: Some(720),
        video_codec: None,
        audio_codec: None,
        has_audio: Some(false),
        created_at: None,
        rotation: None,
        categories: Vec::new(),
    }
}

fn clip(id: &str, order: u32, source: &str, in_s: u64, out_s: u64) -> Slot {
    Slot {
        id: id.into(),
        order,
        kind: SlotKind::Clip,
        duration_us: Micros::from_secs(out_s - in_s),
        source_ref: Some(SourceRef {
            source_id: source.into(),
            shot_id: Some(format!("{source}-shot")),
            in_us: Micros::from_secs(in_s),
            out_us: Micros::from_secs(out_s),
        }),
        speed: None,
        reason: None,
        score: None,
    }
}

/// A project whose last run made a cut of two clips of `video`, written as
/// a run writes them.
fn project_with_run(library: &Library, video: &Path) -> Project {
    let mut graph = FootageGraph::new("editor-test");
    graph.sources.push(source("src", video, 20));
    graph.shots.push(Shot {
        id: "src-shot".into(),
        source_id: "src".into(),
        start_us: Micros::ZERO,
        end_us: Micros::from_secs(20),
        shot_change_confidence: None,
        keyframes: Vec::new(),
        description: None,
        entity_refs: Vec::new(),
        actions: Vec::new(),
        audio: None,
        tags: Vec::new(),
        segments: Vec::new(),
        timeline: Vec::new(),
    });
    let mut dsl = EditingDsl::new("编辑测试");
    dsl.meta.aspect_ratio = Some("16:9".into());
    dsl.slots = vec![clip("a", 0, "src", 2, 6), clip("b", 1, "src", 10, 13)];

    let mut project = Project::new("编辑测试");
    let dir = project.dir(library);
    std::fs::create_dir_all(&dir).unwrap();
    let (dsl_path, graph_path) = (
        dir.join("project.editing-dsl.json"),
        dir.join("project.footage-graph.json"),
    );
    std::fs::write(&dsl_path, serde_json::to_string(&dsl).unwrap()).unwrap();
    std::fs::write(&graph_path, serde_json::to_string(&graph).unwrap()).unwrap();
    project.last_run = Some(LastRun {
        at: "2026-10-09T10:00:00Z".into(),
        draft_path: dir.join("project.otio").display().to_string(),
        duration_us: 7_000_000,
        clips_used: 2,
        template_id: "t".into(),
        editor_drafts: Vec::new(),
        dsl_path: Some(dsl_path.display().to_string()),
        graph_path: Some(graph_path.display().to_string()),
    });
    project.save(library).unwrap();
    project
}

fn slot_ids(state: &Value) -> Vec<String> {
    state["timeline"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["slotId"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_cut_is_edited_undone_redone_and_kept_on_disk() {
    let library = scratch("history");
    let project = project_with_run(&library, Path::new("/nowhere/a.mp4"));
    let id = project.id.as_str();

    let state = open(&library, id, false, Lang::En).unwrap();
    assert_eq!(slot_ids(&state), ["a", "b"]);
    assert_eq!(state["timeline"]["totalUs"], 7_000_000);
    assert_eq!(state["canUndo"], false);
    assert_eq!(state["edited"], false);
    assert_eq!(state["frame"]["width"], 1920);

    // Split the first clip at 2 s of the piece: three clips, same length.
    let state = apply(
        &library,
        id,
        &json!({ "op": "split", "atUs": 2_000_000 }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(slot_ids(&state).len(), 3);
    assert_eq!(state["timeline"]["totalUs"], 7_000_000);
    assert_eq!(state["canUndo"], true);
    assert_eq!(state["undoLabel"], "Split");
    assert_eq!(state["selection"]["kind"], "slot");

    // Delete the clip after the split: a second of footage gone.
    let second = slot_ids(&state)[1].clone();
    let state = apply(
        &library,
        id,
        &json!({ "op": "delete", "slotId": second }),
        Lang::En,
    )
    .unwrap();
    assert_eq!(state["timeline"]["totalUs"], 5_000_000);
    assert_eq!(state["edited"], true);
    assert!(state["changeRatio"].as_f64().unwrap() > 0.2);

    // On disk, stamped as a person's edit, and the project says so.
    let saved: EditingDsl = read_json(&edit_dir(&library, &project).join(CUT)).unwrap();
    assert_eq!(saved.slots.len(), 2);
    let provenance = saved.provenance.unwrap();
    assert_eq!(provenance.editor.as_deref(), Some(EDITOR));
    assert!(provenance.change_ratio.unwrap() > 0.2);
    assert!(projects::get(&library, id).unwrap().edited_at.is_some());

    let state = undo(&library, id, false, Lang::En).unwrap();
    assert_eq!(state["timeline"]["totalUs"], 7_000_000);
    assert_eq!(state["canRedo"], true);
    assert_eq!(state["redoLabel"], "Delete");
    let state = undo(&library, id, true, Lang::En).unwrap();
    assert_eq!(state["timeline"]["totalUs"], 5_000_000);

    // Closed and opened again: the edit is there, the history is not.
    close(id);
    let state = open(&library, id, false, Lang::En).unwrap();
    assert_eq!(state["timeline"]["totalUs"], 5_000_000);
    assert_eq!(state["canUndo"], false);
    assert_eq!(state["edited"], true);

    // Back to the AI's cut, as a step of its own.
    let state = revert(&library, id, Lang::Zh).unwrap();
    assert_eq!(slot_ids(&state), ["a", "b"]);
    assert_eq!(state["edited"], false);
    assert_eq!(state["undoLabel"], "恢复 AI 成片");
    assert!(projects::get(&library, id).unwrap().edited_at.is_none());

    close(id);
    let _ = std::fs::remove_dir_all(library.root());
}

#[test]
fn an_edit_that_cannot_be_made_leaves_the_cut_and_says_why() {
    let library = scratch("refused");
    let project = project_with_run(&library, Path::new("/nowhere/a.mp4"));
    let id = project.id.as_str();
    open(&library, id, false, Lang::Zh).unwrap();
    let e = apply(
        &library,
        id,
        &json!({ "op": "speed", "slotId": "a", "speed": 9.0 }),
        Lang::Zh,
    )
    .unwrap_err();
    assert_eq!(e, "速度只能在 0.25 倍到 4 倍之间。");
    let e = apply(&library, id, &json!({ "op": "explode" }), Lang::En).unwrap_err();
    assert!(e.starts_with("not an edit"), "{e}");
    let state = state(&library, id, None, Lang::En).unwrap();
    assert_eq!(state["canUndo"], false);
    assert_eq!(state["version"], 1);
    close(id);
    let _ = std::fs::remove_dir_all(library.root());
}

#[test]
fn a_newer_run_is_offered_and_never_taken_over_the_persons_work() {
    let library = scratch("newer");
    let mut project = project_with_run(&library, Path::new("/nowhere/a.mp4"));
    let id = project.id.clone();
    open(&library, &id, false, Lang::En).unwrap();
    apply(
        &library,
        &id,
        &json!({ "op": "delete", "slotId": "b" }),
        Lang::En,
    )
    .unwrap();

    // The project is cut again.
    project = projects::get(&library, &id).unwrap();
    project.last_run.as_mut().unwrap().at = "2026-10-09T11:00:00Z".into();
    project.save(&library).unwrap();
    let state = state(&library, &id, None, Lang::En).unwrap();
    assert_eq!(state["newerRun"], true);
    assert_eq!(
        slot_ids(&state),
        ["a"],
        "the person's cut stays until they choose"
    );

    // Starting again keeps what they had beside it.
    let state = open(&library, &id, true, Lang::En).unwrap();
    assert_eq!(state["newerRun"], false);
    assert_eq!(slot_ids(&state), ["a", "b"]);
    let dir = edit_dir(&library, &project);
    let previous: EditingDsl = read_json(&dir.join("cut.previous.editing-dsl.json")).unwrap();
    assert_eq!(previous.slots.len(), 1);
    close(&id);
    let _ = std::fs::remove_dir_all(library.root());
}

#[test]
fn a_project_without_a_cut_cannot_be_opened() {
    let library = scratch("nocut");
    let project = Project::new("empty");
    project.save(&library).unwrap();
    let e = open(&library, &project.id, false, Lang::Zh).unwrap_err();
    assert!(e.contains("还没有成片"), "{e}");
    assert!(open(&library, "p-missing", false, Lang::En).is_err());
    let _ = std::fs::remove_dir_all(library.root());
}

#[test]
fn a_run_from_before_0_2_is_found_beside_its_draft() {
    let library = scratch("legacy");
    let mut project = project_with_run(&library, Path::new("/nowhere/a.mp4"));
    let run = project.last_run.as_mut().unwrap();
    run.dsl_path = None;
    run.graph_path = None;
    project.save(&library).unwrap();
    let state = open(&library, &project.id, false, Lang::En).unwrap();
    assert_eq!(slot_ids(&state), ["a", "b"]);
    close(&project.id);
    let _ = std::fs::remove_dir_all(library.root());
}

#[test]
fn file_names_are_safe_and_never_overwrite() {
    let dir = std::env::temp_dir().join(format!("vb-editor-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = free_name(&dir, "旅行: 9/29", "mp4");
    assert_eq!(first.file_name().unwrap(), "旅行- 9-29.mp4");
    std::fs::write(&first, b"x").unwrap();
    assert_eq!(
        free_name(&dir, "旅行: 9/29", "mp4").file_name().unwrap(),
        "旅行- 9-29-2.mp4"
    );
    assert_eq!(
        free_name(&dir, "  ", "mp4").file_name().unwrap(),
        "Video Butler.mp4"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The real footage: a preview frame, film-strip pictures, an OTIO draft
/// and a video, through the default media backend.
#[test]
fn a_cut_is_previewed_and_exported_from_real_footage() {
    let video = media_dir().join("sample.mp4");
    let media = vb_media::default_backend();
    if !media.is_available() || !video.exists() {
        eprintln!("skipping: needs ffmpeg and test-media (scripts/make-test-media.sh)");
        return;
    }
    let library = scratch("real");
    let project = project_with_run(&library, &video);
    let id = project.id.as_str();
    open(&library, id, false, Lang::En).unwrap();
    apply(
        &library,
        id,
        &json!({ "op": "addText", "atUs": 500_000, "content": "山顶到了" }),
        Lang::En,
    )
    .unwrap();

    let frame = frame(&library, id, Micros::from_secs(1), 480, Lang::En).unwrap();
    assert_eq!(
        (frame["width"].as_u64(), frame["height"].as_u64()),
        (Some(480), Some(270))
    );
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(frame["png"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png[1..4], b"PNG");

    let strip = strip(
        &library,
        id,
        &[
            ("src".into(), Micros::from_secs(2)),
            ("src".into(), Micros::from_secs(3)),
        ],
        48,
        Lang::En,
    )
    .unwrap();
    let items = strip["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["png"].is_string()), "{strip}");

    let footage = footage(&library, id, Lang::En).unwrap();
    assert_eq!(footage["sources"][0]["used"].as_array().unwrap().len(), 2);

    let out = std::env::temp_dir().join(format!("vb-editor-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let otio = export(
        &library,
        id,
        &json!({ "target": "otio", "dir": out }),
        Lang::En,
    )
    .unwrap();
    assert!(Path::new(otio["path"].as_str().unwrap()).exists());

    if media.can_encode(VideoCodec::H264) {
        let job = export(
            &library,
            id,
            &json!({ "target": "video", "dir": out, "shortSide": 240 }),
            Lang::En,
        )
        .unwrap();
        let job_id = job["jobId"].as_str().unwrap().to_string();
        let status = loop {
            let status = jobs::status(&job_id).unwrap();
            if status["finished"] == true {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(status["state"], "done", "{status}");
        let path = PathBuf::from(status["result"]["path"].as_str().unwrap());
        let info = media.probe(&path).unwrap();
        assert_eq!((info.width, info.height), (426, 240));
        let length = info.duration.0 as i64;
        assert!((length - 7_000_000).abs() < 100_000, "{length}");
        let kinds: Vec<String> = projects::get(&library, id)
            .unwrap()
            .exports
            .iter()
            .map(|e| e.kind.clone())
            .collect();
        assert_eq!(kinds, ["otio", "video"]);
    } else {
        eprintln!("this ffmpeg has no H.264 encoder: the video export is not checked");
    }
    close(id);
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(library.root());
}

fn wait_for(job: &str) -> Value {
    loop {
        let status = jobs::status(job).unwrap();
        if status["finished"] == true {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_cut_of_only_cuts_leaves_losslessly_and_clip_by_clip() {
    let video = media_dir().join("sample.mp4");
    let media = vb_media::default_backend();
    if !media.is_available() || !video.exists() || !media.can_copy() {
        eprintln!("skipping: needs ffmpeg that joins files and test-media");
        return;
    }
    let library = scratch("lossless");
    let project = project_with_run(&library, &video);
    let id = project.id.as_str();
    open(&library, id, false, Lang::Zh).unwrap();

    // Two clips of one file, each starting on a keyframe: nothing extra.
    let plan = lossless(&library, id, Lang::Zh).unwrap();
    assert_eq!(plan["possible"], true, "{plan}");
    assert_eq!(plan["extraUs"], 0);
    assert_eq!(plan["totalUs"], 7_000_000);

    let out = std::env::temp_dir().join(format!("vb-editor-lossless-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let job = export(
        &library,
        id,
        &json!({ "target": "lossless", "dir": out }),
        Lang::Zh,
    )
    .unwrap();
    let status = wait_for(job["jobId"].as_str().unwrap());
    assert_eq!(status["state"], "done", "{status}");
    assert_eq!(status["result"]["lossless"], true);
    let path = PathBuf::from(status["result"]["path"].as_str().unwrap());
    let info = media.probe(&path).unwrap();
    assert!(
        info.duration.0.abs_diff(7_000_000) < 100_000,
        "{}",
        info.duration
    );
    assert_eq!(
        (info.width, info.height),
        (1280, 720),
        "the footage's own frame"
    );

    // Each clip a file of its own, both copied.
    let job = export(
        &library,
        id,
        &json!({ "target": "clips", "dir": out }),
        Lang::Zh,
    )
    .unwrap();
    let status = wait_for(job["jobId"].as_str().unwrap());
    assert_eq!(status["state"], "done", "{status}");
    assert_eq!(status["result"]["copied"], 2, "{status}");
    let files = status["result"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[0].as_str().unwrap().ends_with("01 sample.mp4"));

    // Text over it: the plan says why it needs encoding, and refuses.
    apply(
        &library,
        id,
        &json!({ "op": "addText", "atUs": 500_000, "content": "山顶" }),
        Lang::Zh,
    )
    .unwrap();
    let plan = lossless(&library, id, Lang::Zh).unwrap();
    assert_eq!(plan["possible"], false);
    assert_eq!(plan["reasons"], json!(["有文字（标题、字幕）"]));
    let refused = export(
        &library,
        id,
        &json!({ "target": "lossless", "dir": out }),
        Lang::Zh,
    );
    assert!(refused.unwrap_err().contains("有文字"));

    let kinds: Vec<String> = projects::get(&library, id)
        .unwrap()
        .exports
        .iter()
        .map(|e| e.kind.clone())
        .collect();
    assert_eq!(kinds, ["video", "clips"]);
    close(id);
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(library.root());
}
