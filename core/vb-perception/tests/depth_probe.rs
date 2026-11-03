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

//! What the depth model makes of real footage.
//!
//! Not a test of correctness: it writes a depth map for every segment's
//! keyframe of an analysed project -- and for every clip of a cut, when given
//! the cut -- so the director's occlusion reading can be checked against
//! frames a person has looked at. Runs only when pointed at a project:
//!
//!   VB_PROBE_GRAPH=<project.footage-graph.json> VB_PROBE_OUT=<dir> \
//!   VB_DEPTH_MODEL=<depth-anything-v2-small.onnx> [VB_FACE_MODEL=<yunet.onnx>] \
//!   [VB_PERSON_MODEL=<pphumanseg.onnx>] \
//!   [VB_PROBE_DSL=<project.editing-dsl.json>] \
//!     cargo test --release -p vb-perception --test depth_probe -- --nocapture
//!
//! Each map lands as `<id>.pgm` (a preview, near is bright) and `<id>.f32`
//! (the values, little-endian, row-major), listed in `index.tsv`; faces, when
//! a detector is given, in `faces.tsv`; the person mask, when a segmenter is
//! given, as `<id>.people` (one byte a pixel at the segmenter's size).

use std::io::Write;
use std::path::{Path, PathBuf};
use vb_contracts::{EditingDsl, FootageGraph, Micros};
use vb_infer::{DepthAdapter, DepthMap, FaceDetector, PersonSegmenter, RgbImage};
use vb_onnx::{DepthAnything, PpHumanSeg, YuNet};
use vb_perception::director;

#[test]
fn depth_for_every_segment_of_a_project() {
    let (Some(graph), Some(out), Some(model)) = (
        std::env::var_os("VB_PROBE_GRAPH"),
        std::env::var_os("VB_PROBE_OUT"),
        std::env::var_os("VB_DEPTH_MODEL"),
    ) else {
        eprintln!("skipping: set VB_PROBE_GRAPH, VB_PROBE_OUT and VB_DEPTH_MODEL");
        return;
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let graph: FootageGraph =
        serde_json::from_str(&std::fs::read_to_string(&graph).unwrap()).unwrap();

    // (id, source, when)
    let mut moments: Vec<(String, String, Micros)> = Vec::new();
    if let Some(dsl) = std::env::var_os("VB_PROBE_DSL") {
        let dsl: EditingDsl = serde_json::from_str(&std::fs::read_to_string(dsl).unwrap()).unwrap();
        for slot in &dsl.slots {
            if let Some(r) = &slot.source_ref {
                let mid = Micros((r.in_us.0 + r.out_us.0) / 2);
                moments.push((format!("slot-{:02}", slot.order), r.source_id.clone(), mid));
            }
        }
    }
    for shot in &graph.shots {
        for seg in &shot.segments {
            let at = seg
                .keyframe_id
                .as_ref()
                .and_then(|k| shot.keyframes.iter().find(|kf| &kf.id == k))
                .map(|kf| kf.at_us)
                .unwrap_or_else(|| {
                    let start = seg.usable_start_us.unwrap_or(seg.start_us);
                    let end = seg.usable_end_us.unwrap_or(seg.end_us);
                    Micros((start.0 + end.0) / 2)
                });
            moments.push((seg.id.clone(), shot.source_id.clone(), at));
        }
    }
    let limit: usize = std::env::var("VB_PROBE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    let media = vb_media::default_backend();
    let depth = DepthAnything::new(PathBuf::from(model));
    let faces = std::env::var_os("VB_FACE_MODEL").map(|m| {
        let mut detector = YuNet::new(PathBuf::from(m));
        if let Some(min) = std::env::var("VB_FACE_MIN")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            detector.min_confidence = min;
        }
        detector
    });
    let mut face_log = std::fs::File::create(out.join("faces.tsv")).unwrap();
    let people = std::env::var_os("VB_PERSON_MODEL").map(|m| PpHumanSeg::new(PathBuf::from(m)));
    let mut index = std::fs::File::create(out.join("index.tsv")).unwrap();
    writeln!(index, "id\tfile\tatUs\tmapW\tmapH\tfaces\toccluded").unwrap();
    let started = std::time::Instant::now();
    for (n, (id, source_id, at)) in moments.iter().take(limit).enumerate() {
        let source = graph.sources.iter().find(|s| &s.id == source_id).unwrap();
        let (w, h) = source.display_size().unwrap_or((1920, 1080));
        let (fw, fh) = DepthAnything::input_size(w, h);
        let frame = match media.rgb_frame(Path::new(&source.path), *at, fw, fh) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{id}: {e}");
                continue;
            }
        };
        let image = RgbImage {
            width: frame.width,
            height: frame.height,
            rgb: frame.rgb,
        };
        let map = depth.depth(&image).unwrap();
        let found = match &faces {
            Some(detector) => detector.faces(&image).unwrap(),
            None => Vec::new(),
        };
        let mask = people.as_ref().map(|s| s.people(&image).unwrap());
        {
            for f in &found {
                writeln!(
                    face_log,
                    "{id}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.3}",
                    f.x, f.y, f.w, f.h, f.confidence
                )
                .unwrap();
            }
        }
        if let Some(mask) = &mask {
            let bytes: Vec<u8> = mask
                .values
                .iter()
                .map(|v| (v * 255.0).round() as u8)
                .collect();
            std::fs::write(out.join(format!("{id}.people")), bytes).unwrap();
        }
        write_pgm(&out.join(format!("{id}.pgm")), &map);
        let bytes: Vec<u8> = map.values.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(out.join(format!("{id}.f32")), bytes).unwrap();
        writeln!(
            index,
            "{id}\t{}\t{}\t{}\t{}\t{}\t{:.3}",
            Path::new(&source.path)
                .file_name()
                .unwrap()
                .to_string_lossy(),
            at.0,
            map.width,
            map.height,
            found.len(),
            director::occlusion(&map, &found, mask.as_ref()),
        )
        .unwrap();
        if n % 25 == 0 {
            eprintln!("{n}/{} in {:?}", moments.len(), started.elapsed());
        }
    }
    eprintln!("done: {} moments in {:?}", moments.len(), started.elapsed());
}

fn write_pgm(path: &Path, map: &DepthMap) {
    let mut bytes = format!("P5\n{} {}\n255\n", map.width, map.height).into_bytes();
    bytes.extend(map.values.iter().map(|v| (v * 255.0).round() as u8));
    std::fs::write(path, bytes).unwrap();
}

/// The same reading at moments named by hand, for a frame a person has
/// seen something in:
///
///   VB_PROBE_MOMENTS=<clip>@<us>,<clip>@<us> VB_PROBE_OUT=<dir> \
///   VB_DEPTH_MODEL=... [VB_FACE_MODEL=...] [VB_PERSON_MODEL=...] \
///     cargo test --release -p vb-perception --test depth_probe named -- --nocapture
///
/// Writes each frame as `<n>.ppm` beside its depth map `<n>.pgm`.
#[test]
fn depth_at_named_moments() {
    let (Some(moments), Some(out), Some(model)) = (
        std::env::var("VB_PROBE_MOMENTS").ok(),
        std::env::var_os("VB_PROBE_OUT"),
        std::env::var_os("VB_DEPTH_MODEL"),
    ) else {
        eprintln!("skipping: set VB_PROBE_MOMENTS, VB_PROBE_OUT and VB_DEPTH_MODEL");
        return;
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let media = vb_media::default_backend();
    let depth = DepthAnything::new(PathBuf::from(model));
    let faces = std::env::var_os("VB_FACE_MODEL").map(|m| YuNet::new(PathBuf::from(m)));
    let people = std::env::var_os("VB_PERSON_MODEL").map(|m| PpHumanSeg::new(PathBuf::from(m)));
    for (n, moment) in moments.split(',').enumerate() {
        let (clip, at) = moment.rsplit_once('@').expect("<clip>@<us>");
        let at = Micros(at.parse().expect("microseconds"));
        let info = media.probe(Path::new(clip)).unwrap();
        let (fw, fh) = DepthAnything::input_size(info.display_size().0, info.display_size().1);
        let frame = media.rgb_frame(Path::new(clip), at, fw, fh).unwrap();
        let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        ppm.extend(&frame.rgb);
        std::fs::write(out.join(format!("{n}.ppm")), ppm).unwrap();
        let image = RgbImage {
            width: frame.width,
            height: frame.height,
            rgb: frame.rgb,
        };
        let map = depth.depth(&image).unwrap();
        write_pgm(&out.join(format!("{n}.pgm")), &map);
        let found = faces
            .as_ref()
            .map(|d| d.faces(&image).unwrap())
            .unwrap_or_default();
        let mask = people.as_ref().map(|s| s.people(&image).unwrap());
        eprintln!(
            "{n}: {moment} faces={} occlusion={:.3}",
            found.len(),
            director::occlusion(&map, &found, mask.as_ref())
        );
    }
}
