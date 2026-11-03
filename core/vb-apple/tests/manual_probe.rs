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

//! A probe for looking at what Vision actually says about a real image.
//!
//! Skips unless `VB_PROBE_IMAGE` names one, so it is inert in CI and
//! available by hand:
//!   VB_PROBE_IMAGE=/path/to.jpg cargo test -p vb-apple --test manual_probe -- --nocapture
#![cfg(target_vendor = "apple")]

use vb_apple::{VisionTracker, VisionVlm};
use vb_contracts::Micros;
use vb_infer::tracker::TrackWindow;
use vb_infer::{TrackerAdapter, VlmAdapter, VlmRequest};

#[test]
fn describe_and_find_faces_in_a_named_image() {
    let Ok(path) = std::env::var("VB_PROBE_IMAGE") else {
        return;
    };
    let image = std::path::PathBuf::from(&path);
    println!(
        "labels: {:?}",
        VisionVlm::default().describe(&VlmRequest::describe(&image))
    );
    let entities = VisionTracker::default()
        .track(
            std::path::Path::new("/unused.mp4"),
            &[(
                "shot-0000".to_string(),
                TrackWindow {
                    start: Micros::ZERO,
                    end: Micros::from_secs(1),
                    stills: vec![image],
                },
            )],
        )
        .unwrap();
    println!("faces: {}", entities.len());
    for entity in &entities {
        println!(
            "  {} confidence {:?} signature {} values",
            entity.id,
            entity.confidence,
            entity.signature.len()
        );
    }
}
