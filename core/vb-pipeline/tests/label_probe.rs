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

//! What the scene labellers say about a folder of stills, side by side.
//!
//! Not a test of correctness: it prints, for each still, what SigLIP 2
//! chose, its best place, subject and activity with their scores, and what
//! Apple Vision says where there is one, so the labeller's bars in
//! `vb-onnx/src/labels.rs` can be checked against frames a person has
//! looked at. Runs only when pointed at stills and the model:
//!
//!   VB_PROBE_STILLS=<folder of .jpg> VB_LABEL_MODEL=<vision_model_fp16.onnx> \
//!     [VB_PROBE_OUT=labels.tsv] \
//!     cargo test --release -p vb-pipeline --test label_probe -- --nocapture

use std::io::Write;
use std::path::PathBuf;
use vb_infer::{VlmAdapter, VlmRequest};
use vb_onnx::labels::{self, Kind};

#[test]
fn label_a_folder_of_stills() {
    let (Some(dir), Some(model)) = (
        std::env::var_os("VB_PROBE_STILLS"),
        std::env::var_os("VB_LABEL_MODEL"),
    ) else {
        eprintln!("skipping: set VB_PROBE_STILLS and VB_LABEL_MODEL");
        return;
    };
    let mut stills: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg")))
        .collect();
    stills.sort();

    let siglip = vb_onnx::SiglipLabels::new(PathBuf::from(model));
    let apple = vb_apple::is_available().then(vb_apple::VisionVlm::default);
    let mut out: Box<dyn Write> = match std::env::var_os("VB_PROBE_OUT") {
        Some(path) => Box::new(std::fs::File::create(path).unwrap()),
        None => Box::new(std::io::stderr()),
    };
    writeln!(out, "still\tsiglip\tplace\tsubject\tactivity\tapple vision").unwrap();
    let started = std::time::Instant::now();
    for still in &stills {
        let image = labels::read_jpeg(still).unwrap();
        let scores = siglip.score(&image).unwrap();
        let chosen: Vec<_> = labels::choose(&scores).iter().map(|s| s.label).collect();
        let best = |kind: Kind| {
            scores
                .iter()
                .filter(|s| s.kind == kind)
                .max_by(|a, b| a.share.total_cmp(&b.share))
                .map(|s| format!("{} {:.3}/{:.2}", s.label, s.probability, s.share))
                .unwrap_or_default()
        };
        let apple_says = apple
            .as_ref()
            .map(|a| {
                a.describe(&VlmRequest::describe(still))
                    .unwrap_or_else(|e| e.to_string())
            })
            .unwrap_or_default();
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}",
            still.file_name().unwrap().to_string_lossy(),
            chosen.join(", "),
            best(Kind::Place),
            best(Kind::Subject),
            best(Kind::Activity),
            apple_says,
        )
        .unwrap();
    }
    eprintln!("{} stills in {:?}", stills.len(), started.elapsed());
}
