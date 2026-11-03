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

//! A look at what the sweep makes of real footage.
//!
//! Not a test of correctness: it prints each clip's readings and segments so
//! the thresholds in `timeline.rs` can be checked against footage a person
//! has looked at. Runs only when pointed at a folder:
//!
//!   VB_PROBE_DIR=<folder> [VB_PROBE_LIMIT=20] \
//!     cargo test --release -p vb-perception --test sweep_probe -- --nocapture

use std::path::PathBuf;
use vb_contracts::Micros;
use vb_perception::timeline::{self, Reader, Reading};

#[test]
fn sweep_a_folder_and_print_what_it_found() {
    let Some(dir) = std::env::var_os("VB_PROBE_DIR") else {
        eprintln!("skipping: set VB_PROBE_DIR to a folder of footage");
        return;
    };
    let limit: usize = std::env::var("VB_PROBE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let skip: usize = std::env::var("VB_PROBE_SKIP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")))
        .collect();
    files.sort();

    let media = vb_media::default_backend();
    let mut all_segments = Vec::new();
    let mut sources = Vec::new();
    for path in files.iter().skip(skip).take(limit) {
        let info = media.probe(path).unwrap();
        sources.push(vb_contracts::Source {
            id: path.file_name().unwrap().to_string_lossy().to_string(),
            path: path.display().to_string(),
            content_hash: vb_contracts::Blake3Hash::of_str("probe"),
            media_kind: None,
            duration_us: info.duration,
            fps_num: None,
            fps_den: None,
            width: Some(info.width),
            height: Some(info.height),
            video_codec: None,
            audio_codec: None,
            has_audio: None,
            created_at: info.created_at.clone(),
            rotation: Some(info.rotation),
            categories: Vec::new(),
        });
        let mut reader = Reader::default();
        let mut readings: Vec<Reading> = Vec::new();
        let started = std::time::Instant::now();
        media
            .sample_frames(
                path,
                timeline::SWEEP_EVERY,
                timeline::SWEEP_WIDTH,
                timeline::SWEEP_HEIGHT,
                &mut |f| {
                    readings.push(reader.read(&f));
                    true
                },
            )
            .unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        println!(
            "\n== {name}  {:.1}s  {} readings  swept in {:.2}s",
            info.duration.as_secs_f64(),
            readings.len(),
            started.elapsed().as_secs_f64()
        );
        for r in &readings {
            println!(
                "  {:6.1}s sharp {:.2} exp {:.2} cam {:.2} ({:+.2},{:+.2}) change {:.3} q {:.2}{}",
                r.at.as_secs_f64(),
                r.sharpness,
                r.exposure,
                r.camera_speed(),
                r.shift.0,
                r.shift.1,
                r.change,
                r.quality(),
                if r.unsettled() { "  UNSETTLED" } else { "" }
            );
        }
        let d = timeline::divide(
            "shot",
            Micros::ZERO,
            info.duration,
            info.duration,
            &readings,
        );
        for (seg, rep) in d.segments.iter().zip(&d.representative) {
            println!(
                "  SEG {} {:.1}-{:.1}s usable {:?} q {:?} cam {:?} key {:.1}s",
                seg.id,
                seg.start_us.as_secs_f64(),
                seg.end_us.as_secs_f64(),
                seg.usable()
                    .map(|(a, b)| format!("{:.1}-{:.1}", a.as_secs_f64(), b.as_secs_f64())),
                seg.quality,
                seg.camera,
                readings[*rep].at.as_secs_f64()
            );
            all_segments.push((name.clone(), seg.clone(), readings[*rep].at));
        }
    }

    // The scenes the segments group into, and a list a script can turn into
    // contact sheets: one line per segment, file, keyframe time and scene.
    let mut graph = vb_contracts::FootageGraph::new("probe");
    graph.sources = sources;
    for (i, (name, seg, _)) in all_segments.iter().enumerate() {
        graph.shots.push(vb_contracts::Shot {
            id: format!("shot-{i:04}"),
            source_id: name.clone(),
            start_us: seg.start_us,
            end_us: seg.end_us,
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: vec![],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: vec![seg.clone()],
            timeline: vec![],
        });
    }
    vb_perception::scenes::attach_scenes(&mut graph);
    println!(
        "\n== {} segments in {} scenes",
        all_segments.len(),
        graph.scenes.len()
    );
    let mut listing = String::new();
    for (shot, (name, seg, at)) in graph.shots.iter().zip(&all_segments) {
        let scene = shot.segments[0].scene_id.clone().unwrap_or_default();
        listing.push_str(&format!(
            "{scene}\t{name}\t{:.2}\t{}\n",
            at.as_secs_f64(),
            match seg.usable() {
                Some(_) => format!("q{:.2}", seg.quality.unwrap_or(0.0)),
                None => "unusable".into(),
            }
        ));
    }
    if let Some(out) = std::env::var_os("VB_PROBE_OUT") {
        std::fs::write(out, &listing).unwrap();
    } else {
        print!("{listing}");
    }

    // How far apart the views are, segment to segment, to set the scene
    // grouping threshold against.
    println!("\n== nearest other view for each segment");
    for (i, (name, seg, at)) in all_segments.iter().enumerate() {
        let nearest = all_segments
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, (n, s, t))| {
                (
                    timeline::view_distance(&seg.signature, &s.signature),
                    n.clone(),
                    t.as_secs_f64(),
                )
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((d, n, t)) = nearest {
            println!("  {name} @{:.1}s -> {d:.3} {n} @{t:.1}s", at.as_secs_f64());
        }
    }
}
