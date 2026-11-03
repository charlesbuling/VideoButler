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

//! Beat detection against real decoded audio.
//!
//! The unit tests synthesise samples in memory, which leaves the decode path
//! untested: a resample that shifts the signal, or a backend that hands back
//! the wrong rate, would not show there. This runs the whole path on a file,
//! and the file is a metronome, so the right answer is not a matter of taste.
//!
//! Needs ffmpeg and `test-media/beat-120.wav`, and skips without them.

use std::path::{Path, PathBuf};
use vb_media::backend::AudioSamples;
use vb_perception::beats::{ANALYSIS_RATE, detect};

fn fixture(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-media")
        .join(name);
    let backend = vb_media::default_backend();
    if path.exists() && backend.is_available() {
        Some(path)
    } else {
        eprintln!("skipping: needs ffmpeg and test-media/{name}");
        None
    }
}

fn decode(path: &Path) -> AudioSamples {
    vb_media::default_backend()
        .decode_audio(path, ANALYSIS_RATE, None)
        .expect("the fixture decodes")
}

#[test]
fn a_metronome_file_reports_its_own_tempo_and_downbeat() {
    let Some(path) = fixture("beat-120.wav") else {
        return;
    };
    let audio = decode(&path);
    assert_eq!(audio.sample_rate, ANALYSIS_RATE);
    assert!(
        audio.total_duration.is_some(),
        "the file's length is needed to lay music under a cut"
    );

    let found = detect(&audio.samples, audio.sample_rate).expect("a click track has a tempo");
    assert!(
        (found.bpm - 120.0).abs() < 1.5,
        "a 120 BPM click track was read as {:.2} BPM",
        found.bpm
    );
    assert!(found.confidence > 0.3, "confidence {:.2}", found.confidence);

    // The clicks start at zero, so the first beat is at zero or a whole
    // number of beats later; anything between beats would misplace every cut.
    let beat = found.beat().0 as f64;
    let offset = found.first_beat.0 as f64 % beat;
    let off_grid = offset.min(beat - offset);
    assert!(
        off_grid < 30_000.0,
        "the first beat is {off_grid} us off the grid"
    );
}

#[test]
fn a_clips_own_audio_is_readable_too() {
    // Dance footage carries the music it was danced to, so the grid can come
    // from the clip itself rather than from a separate file.
    let Some(path) = fixture("sample-with-audio.mp4") else {
        return;
    };
    let audio = decode(&path);
    let found = detect(&audio.samples, audio.sample_rate).expect("the clip has a beat");
    assert!(
        (found.bpm - 120.0).abs() < 1.5,
        "the clip's own click track was read as {:.2} BPM",
        found.bpm
    );
}
