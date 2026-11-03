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

//! Why the video butler filed a face where it did.
//!
//! Not a test of correctness: for one video it looks at the frames the
//! butler looks at, finds the faces with the tracker the library has, and
//! prints each face's distance to every face a subject is kept by -- its
//! first and its exemplars -- with the subjects it comes closest to, and
//! how far the subject's own kept faces lie from each other. How a stranger
//! on a screen landed in a child's growth memory was traced with it.
//!
//!   VB_PROBE_VIDEO=<file> [VB_PROBE_SUBJECT=<sub-…>] [VB_LIBRARY_DIR=<library>] \
//!     cargo test --release -p vb-pipeline --features libav --test face_probe -- --nocapture

use std::path::PathBuf;
use vb_contracts::Micros;
use vb_contracts::footage_graph::signature_distance;
use vb_infer::tracker::TrackWindow;
use vb_pipeline::api::adapters_for;
use vb_pipeline::{Lang, Library, ModelChoice};

#[test]
fn why_a_face_was_filed_where_it_was() {
    let Some(video) = std::env::var_os("VB_PROBE_VIDEO").map(PathBuf::from) else {
        eprintln!("skipping: set VB_PROBE_VIDEO");
        return;
    };
    let library = Library::open();
    let work = std::env::temp_dir().join(format!("vb-face-probe-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let adapters = adapters_for(&library, ModelChoice::Local, &work, Lang::En);
    let media = vb_media::default_backend();
    let info = media.probe(&video).unwrap();

    // The butler's frames: a fifth, a half and four fifths of the way in.
    let mut windows = Vec::new();
    for (i, f) in [0.2, 0.5, 0.8].iter().enumerate() {
        let at = Micros((info.duration.0 as f64 * f) as u64);
        let still = media
            .extract_still(&video, at, &work.join(format!("f{i}.jpg")))
            .unwrap();
        windows.push((
            format!("f{i}"),
            TrackWindow {
                start: at,
                end: at,
                stills: vec![still],
            },
        ));
    }
    let faces = adapters.tracker.track(&video, &windows).unwrap();
    eprintln!("tracker: {}", adapters.tracker.name());

    let album = vb_pipeline::entities::load(&library);
    let wanted = std::env::var("VB_PROBE_SUBJECT").ok();
    for (n, face) in faces.iter().enumerate() {
        let signature: Vec<f64> = face.signature.iter().map(|v| *v as f64).collect();
        eprintln!(
            "\nface {n}: area {:.4}, frames {:?}, picture {:?}",
            face.face_area.unwrap_or(0.0),
            face.shot_ids,
            face.thumbnail
        );
        let mut nearest: Vec<(f32, String, usize)> = album
            .subjects
            .iter()
            .filter(|s| s.signature.len() == signature.len())
            .map(|s| {
                let kept: Vec<&Vec<f64>> =
                    std::iter::once(&s.signature).chain(&s.exemplars).collect();
                let d = kept
                    .iter()
                    .map(|k| signature_distance(k, &signature))
                    .fold(f32::MAX, f32::min);
                (d, s.id.clone(), s.clips())
            })
            .collect();
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (d, id, clips) in nearest.iter().take(6) {
            eprintln!("  {d:.3}  {id}  ({clips} videos)");
        }
        if let Some(id) = &wanted
            && let Some(s) = album.subjects.iter().find(|s| &s.id == id)
        {
            eprintln!("  to each face {id} is kept by (first, then exemplars):");
            for (k, kept) in std::iter::once(&s.signature)
                .chain(&s.exemplars)
                .enumerate()
            {
                eprintln!("    #{k:<2} {:.3}", signature_distance(kept, &signature));
            }
        }
    }

    if let Some(id) = &wanted
        && let Some(s) = album.subjects.iter().find(|s| &s.id == id)
    {
        let kept: Vec<&Vec<f64>> = std::iter::once(&s.signature).chain(&s.exemplars).collect();
        eprintln!(
            "\n{id}: {} kept faces, {} videos; each kept face's distance to the first:",
            kept.len(),
            s.clips()
        );
        for (k, face) in kept.iter().enumerate() {
            eprintln!("  #{k:<2} {:.3}", signature_distance(kept[0], face));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
}
