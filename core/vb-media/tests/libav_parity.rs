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

//! The linked-libav backend must agree with the CLI backend.
//!
//! Both sit behind one trait, and the pipeline treats their output as
//! interchangeable apart from the name it records. These tests hold them to
//! that: the same probe, the same shot boundaries, frames close enough that
//! scoring ranks shots the same way.
//!
//! They need `ffmpeg` on PATH and `test-media/sample.mp4`, and skip without
//! either, like the end-to-end suite. CI generates the fixture.
#![cfg(feature = "libav")]

use std::path::{Path, PathBuf};
use vb_contracts::Micros;
use vb_media::{FfmpegCli, LibAv, MediaBackend, laplacian_sharpness};

fn sample() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
    if path.exists() && FfmpegCli::default().is_available() {
        Some(path)
    } else {
        eprintln!("skipping: needs ffmpeg and test-media/sample.mp4");
        None
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vb-libav-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One frame at 30 fps, the fixture's rate.
const ONE_FRAME: u64 = 33_334;

#[test]
fn probe_matches_the_cli() {
    let Some(input) = sample() else { return };
    let cli = FfmpegCli::default().probe(&input).unwrap();
    let lib = LibAv.probe(&input).unwrap();
    assert_eq!(lib, cli);

    // Rotation and capture time come from phones; the fixture has neither.
    if let Some(clip) = std::env::var_os("VB_PARITY_CLIP") {
        let clip = Path::new(&clip);
        assert_eq!(
            LibAv.probe(clip).unwrap(),
            FfmpegCli::default().probe(clip).unwrap()
        );
    }
}

/// A phone's GPS fix, as Android writes it, is read the same by both.
#[test]
fn a_location_tag_reads_the_same_as_the_cli() {
    if sample().is_none() {
        return;
    }
    let path = scratch("located").join("located.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=size=320x240:rate=30:duration=1")
        .args(["-c:v", "mpeg4", "-metadata", "location=+43.3372+087.4084/"])
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        return;
    }
    let cli = FfmpegCli::default().probe(&path).unwrap();
    let lib = LibAv.probe(&path).unwrap();
    assert_eq!(lib, cli);
    let at = lib.location.expect("the fix is read");
    assert!((at.lat - 43.3372).abs() < 1e-9 && (at.lon - 87.4084).abs() < 1e-9);
}

#[test]
fn scene_cuts_match_the_cli() {
    let Some(input) = sample() else { return };
    let cli = FfmpegCli::default().detect_scenes(&input, 0.3).unwrap();
    let lib = LibAv.detect_scenes(&input, 0.3).unwrap();

    // The fixture has four scenes, so three hard cuts.
    assert_eq!(cli.len(), 3, "the fixture should cut three times");
    assert_eq!(lib.len(), cli.len());
    for (l, c) in lib.iter().zip(&cli) {
        assert!(
            l.at.0.abs_diff(c.at.0) < ONE_FRAME,
            "cut at {} vs {} is more than a frame apart",
            l.at,
            c.at
        );
        assert!(
            (l.score - c.score).abs() < 1e-3,
            "scene score {} vs {}",
            l.score,
            c.score
        );
    }
}

#[test]
fn greyscale_frames_match_the_cli_closely_enough_to_score_the_same() {
    let Some(input) = sample() else { return };
    let cli = FfmpegCli::default();

    // One sample inside each of the four scenes, plus one on a cut.
    let times = [1_500_000, 8_000_000, 13_000_000, 20_000_000, 6_000_000];
    let mut cli_sharpness = Vec::new();
    let mut lib_sharpness = Vec::new();

    for at in times {
        let at = Micros(at);
        let a = cli.gray_frame(&input, at, 160, 90).unwrap();
        let b = LibAv.gray_frame(&input, at, 160, 90).unwrap();
        assert_eq!(b.pixels.len(), 160 * 90);

        let mean_abs_diff = a
            .pixels
            .iter()
            .zip(&b.pixels)
            .map(|(x, y)| x.abs_diff(*y) as f64)
            .sum::<f64>()
            / a.pixels.len() as f64;
        // Bit-identical when both backends use the same ffmpeg build, as they
        // do on a Homebrew machine. The margin allows for the CLI and the
        // linked library being different builds; it does not allow for a
        // range mismatch, which costs about eight levels.
        assert!(
            mean_abs_diff < 1.0,
            "frames at {at} differ by {mean_abs_diff:.2} grey levels on average"
        );

        cli_sharpness.push(laplacian_sharpness(&a));
        lib_sharpness.push(laplacian_sharpness(&b));
    }

    // What matters downstream is ordering: the same frame must win.
    let rank = |v: &[f64]| {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&i, &j| v[i].total_cmp(&v[j]));
        idx
    };
    assert_eq!(rank(&lib_sharpness), rank(&cli_sharpness));
}

#[test]
fn colour_frames_match_the_cli() {
    let Some(input) = sample() else { return };
    colour_frames_agree(&input, &[1_500_000, 8_000_000, 13_000_000, 20_000_000]);
    // A phone clip checks the colour matrix and, when it was filmed on its
    // side, the turn.
    if let Some(clip) = std::env::var_os("VB_PARITY_CLIP") {
        colour_frames_agree(Path::new(&clip), &[500_000, 1_200_000]);
    }
}

fn colour_frames_agree(input: &Path, times: &[u64]) {
    let cli = FfmpegCli::default();
    let info = cli.probe(input).unwrap();
    // Upright, as a caller asks: a portrait clip gets a tall frame.
    let (w, h) = if info.rotation % 180 == 90 {
        (72, 128)
    } else {
        (128, 72)
    };
    for &at in times {
        let at = Micros(at);
        let a = cli.rgb_frame(input, at, w, h).unwrap();
        let b = LibAv.rgb_frame(input, at, w, h).unwrap();
        assert_eq!(b.rgb.len(), (w * h * 3) as usize);
        let mean_abs_diff = a
            .rgb
            .iter()
            .zip(&b.rgb)
            .map(|(x, y)| x.abs_diff(*y) as f64)
            .sum::<f64>()
            / a.rgb.len() as f64;
        assert!(
            mean_abs_diff < 1.5,
            "colour frames at {at} differ by {mean_abs_diff:.2} levels on average"
        );
    }
}

/// Sweep a file with one backend, keeping every frame.
fn sweep(backend: &dyn MediaBackend, input: &Path) -> Vec<vb_media::SampledFrame> {
    let mut frames = Vec::new();
    let count = backend
        .sample_frames(input, Micros::from_millis(500), 64, 36, &mut |f| {
            frames.push(f);
            true
        })
        .unwrap();
    assert_eq!(count, frames.len());
    frames
}

#[test]
fn a_sweep_takes_the_same_frames_as_the_cli() {
    let Some(input) = sample() else { return };
    let frames = sweeps_agree(&input);
    // The fixture runs 24 s, so about two frames a second.
    assert!((45..=50).contains(&frames), "{frames} frames");

    // The fixture is untagged. Phone footage is BT.709, which is the case
    // the colour matrix matters for; name a clip to check it too.
    if let Some(clip) = std::env::var_os("VB_PARITY_CLIP") {
        sweeps_agree(Path::new(&clip));
    }
}

/// Sweep with both backends and hold them to the same frames. Returns how
/// many there were.
fn sweeps_agree(input: &Path) -> usize {
    let cli = sweep(&FfmpegCli::default(), input);
    let lib = sweep(&LibAv, input);
    assert_eq!(lib.len(), cli.len(), "the two sweeps took different frames");
    for pair in cli.windows(2) {
        let gap = pair[1].at.0 - pair[0].at.0;
        assert!(gap >= 499_000, "frames {gap} us apart");
    }
    for (l, c) in lib.iter().zip(&cli) {
        assert!(
            l.at.0.abs_diff(c.at.0) < ONE_FRAME,
            "a frame at {} vs {}",
            l.at,
            c.at
        );
        assert_eq!(l.rgb.len(), 64 * 36 * 3);
        let mean_abs_diff = l
            .rgb
            .iter()
            .zip(&c.rgb)
            .map(|(x, y)| x.abs_diff(*y) as f64)
            .sum::<f64>()
            / l.rgb.len() as f64;
        // Colour goes through the source's matrix in both; reading BT.709 as
        // BT.601 costs several levels on saturated colour, which this catches.
        assert!(
            mean_abs_diff < 1.5,
            "frames at {} differ by {mean_abs_diff:.2} levels on average",
            c.at
        );
    }
    cli.len()
}

#[test]
fn one_scan_finds_what_the_cli_finds_in_two_decodes() {
    let Some(input) = sample() else { return };
    let cli = FfmpegCli::default();
    let (cli_cuts, cli_frames) = (cli.detect_scenes(&input, 0.3).unwrap(), sweep(&cli, &input));
    let mut frames = Vec::new();
    let scan = LibAv
        .scan(&input, 0.3, Micros::from_millis(500), 64, 36, &mut |f| {
            frames.push(f);
            true
        })
        .unwrap();
    assert_eq!(scan.frames.unwrap(), frames.len());
    assert_eq!(scan.cuts.len(), cli_cuts.len());
    for (l, c) in scan.cuts.iter().zip(&cli_cuts) {
        assert!(
            l.at.0.abs_diff(c.at.0) < ONE_FRAME,
            "cut at {} vs {}",
            l.at,
            c.at
        );
        assert!((l.score - c.score).abs() < 1e-3);
    }
    assert_eq!(frames.len(), cli_frames.len());
    for (l, c) in frames.iter().zip(&cli_frames) {
        assert!(l.at.0.abs_diff(c.at.0) < ONE_FRAME);
        let diff = l
            .rgb
            .iter()
            .zip(&c.rgb)
            .map(|(x, y)| x.abs_diff(*y) as f64)
            .sum::<f64>()
            / l.rgb.len() as f64;
        assert!(diff < 1.5, "frames at {} differ by {diff:.2} levels", c.at);
    }
}

#[test]
fn a_range_is_read_as_the_frames_at_those_times() {
    let Some(input) = sample() else { return };
    let (from, to) = (Micros::from_secs(3), Micros::from_secs(5));
    let mut frames = Vec::new();
    let count = LibAv
        .sample_range(
            &input,
            from,
            to,
            Micros::from_millis(250),
            64,
            36,
            &mut |f| {
                frames.push(f);
                true
            },
        )
        .unwrap();
    assert_eq!(count, frames.len());
    // Two seconds at four a second, both ends included.
    assert!((8..=9).contains(&count), "{count} frames");
    assert!(frames.first().unwrap().at >= from);
    assert!(frames.last().unwrap().at <= to);
    // Each is the frame a seek to its time would give.
    for f in frames.iter().step_by(3) {
        let one = LibAv.rgb_frame(&input, f.at, 64, 36).unwrap();
        assert_eq!(
            one.rgb, f.rgb,
            "the frame at {} differs from a seek to it",
            f.at
        );
    }
}

#[test]
fn a_sweep_stops_when_the_sink_says_so() {
    let Some(input) = sample() else { return };
    for backend in [&FfmpegCli::default() as &dyn MediaBackend, &LibAv] {
        let mut seen = 0;
        let count = backend
            .sample_frames(&input, Micros::from_millis(500), 32, 18, &mut |_| {
                seen += 1;
                seen < 3
            })
            .unwrap();
        assert_eq!(seen, 3, "{} kept going", backend.name());
        assert_eq!(count, 3);
    }
}

#[test]
fn a_still_is_a_real_image_at_full_resolution() {
    let Some(input) = sample() else { return };
    let dir = scratch("stills");

    for name in ["kf.jpg", "kf.png"] {
        let dest = dir.join(name);
        let written = LibAv
            .extract_still(&input, Micros(2_000_000), &dest)
            .unwrap();
        assert_eq!(written, dest);

        let bytes = std::fs::read(&dest).unwrap();
        let magic_ok = match name {
            "kf.jpg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
            _ => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        };
        assert!(
            magic_ok,
            "{name} does not start with its format's signature"
        );

        // Read back by the CLI, an independent decoder.
        let info = FfmpegCli::default().probe(&dest).unwrap();
        assert_eq!((info.width, info.height), (1280, 720), "{name}");
    }
}

/// A clip filmed on its side, or upside down, gives an upright still, as the
/// CLI's autorotate writes it, in the same colours. Every model that reads
/// stills -- labels, faces, the judge, captions -- reads the picture as it
/// is, and LibAv once handed them every portrait phone clip on its side, and
/// every still with its blacks 16 levels up.
#[test]
fn a_still_is_upright_and_in_the_colours_the_cli_writes() {
    let Some(input) = sample() else { return };
    let dir = scratch("turned-stills");
    for degrees in [0, 90, 180, 270] {
        let clip = dir.join(format!("turned-{degrees}.mp4"));
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-display_rotation:v:0"])
            .arg(degrees.to_string())
            .arg("-i")
            .arg(&input)
            .args(["-c", "copy"])
            .arg(&clip)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("skipping: this ffmpeg cannot tag a rotation");
            return;
        }
        let cli = dir.join(format!("cli-{degrees}.jpg"));
        let lib = dir.join(format!("lib-{degrees}.jpg"));
        FfmpegCli::default()
            .extract_still(&clip, Micros(8_000_000), &cli)
            .unwrap();
        LibAv.extract_still(&clip, Micros(8_000_000), &lib).unwrap();

        let size = |p: &Path| {
            let info = FfmpegCli::default().probe(p).unwrap();
            (info.width, info.height)
        };
        let upright = if degrees % 180 == 0 {
            (1280, 720)
        } else {
            (720, 1280)
        };
        assert_eq!(size(&cli), upright, "the CLI's still at {degrees}");
        assert_eq!(size(&lib), upright, "LibAv's still at {degrees}");

        // The same picture the same way up: a still left upside down
        // differs from the CLI's by far more than encoding does.
        let (w, h) = (upright.0 / 10, upright.1 / 10);
        let a = LibAv.rgb_frame(&cli, Micros::ZERO, w, h).unwrap();
        let b = LibAv.rgb_frame(&lib, Micros::ZERO, w, h).unwrap();
        let mean_abs_diff = a
            .rgb
            .iter()
            .zip(&b.rgb)
            .map(|(x, y)| x.abs_diff(*y) as f64)
            .sum::<f64>()
            / a.rgb.len() as f64;
        assert!(
            mean_abs_diff < 3.0,
            "stills at {degrees} differ by {mean_abs_diff:.2} levels on average"
        );
    }
}

#[test]
fn a_still_goes_into_a_directory_that_does_not_exist_yet() {
    let Some(input) = sample() else { return };
    let dest = scratch("nested").join("a/b/kf.jpg");
    LibAv
        .extract_still(&input, Micros(1_000_000), &dest)
        .unwrap();
    assert!(dest.is_file());
}

#[test]
fn a_time_past_the_end_is_an_error_not_a_stale_frame() {
    let Some(input) = sample() else { return };
    let duration = LibAv.probe(&input).unwrap().duration;
    let err = LibAv
        .gray_frame(&input, duration + Micros::from_secs(5), 16, 9)
        .unwrap_err();
    assert!(err.to_string().contains("past the end"), "got {err}");
}

#[test]
fn the_last_frame_is_reachable() {
    // A keyframe sampled inside the final frame's display time must still
    // resolve. The CLI backend loses these; this one should not.
    let Some(input) = sample() else { return };
    let duration = LibAv.probe(&input).unwrap().duration;
    let near_end = duration.saturating_sub(Micros(5_000));
    LibAv.gray_frame(&input, near_end, 16, 9).unwrap();
}

#[test]
fn a_filename_a_filtergraph_cannot_address_still_analyses() {
    // Single quotes cannot be escaped inside an ffmpeg filtergraph, which is
    // why the CLI path has a segmentation fallback. libav opens the file
    // directly and never puts the path in a filtergraph, so the pitfall does
    // not apply.
    let Some(input) = sample() else { return };
    let tricky = scratch("quote").join("it's a clip, [final].mp4");
    std::fs::copy(&input, &tricky).unwrap();

    let cuts = LibAv.detect_scenes(&tricky, 0.3).unwrap();
    assert_eq!(cuts.len(), 3);
    LibAv.gray_frame(&tricky, Micros(1_000_000), 16, 9).unwrap();
}

#[test]
fn analysis_is_safe_to_run_from_many_threads_at_once() {
    // The pipeline shares one backend across threads, and the shim keeps
    // per-call state only. A data race here would show up as a crash or as
    // threads disagreeing.
    let Some(input) = sample() else { return };
    let expected = LibAv.detect_scenes(&input, 0.3).unwrap();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let input = input.clone();
            std::thread::spawn(move || LibAv.detect_scenes(&input, 0.3).unwrap())
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}

// Audio: both backends must hear the same thing, because beat detection and
// speech recognition read whichever one this build has.
mod audio_parity {
    use super::*;

    fn fixture(name: &str) -> Option<PathBuf> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-media")
            .join(name);
        path.exists().then_some(path)
    }

    #[test]
    fn both_backends_decode_the_same_mono_audio() {
        let Some(path) = fixture("sample-with-audio.mp4") else {
            return;
        };
        if !FfmpegCli::default().is_available() {
            return;
        }
        let cli = FfmpegCli::default()
            .decode_audio(&path, 11025, Some(Micros::from_secs(4)))
            .expect("the CLI backend decodes audio");
        let linked = LibAv
            .decode_audio(&path, 11025, Some(Micros::from_secs(4)))
            .expect("the linked backend decodes audio");

        assert_eq!(cli.sample_rate, linked.sample_rate);
        // Decoder priming and where each stops the decode differ by a few
        // milliseconds; the content must not.
        let difference = cli.samples.len().abs_diff(linked.samples.len());
        assert!(
            difference < 11025 / 10,
            "sample counts differ by {difference}: {} vs {}",
            cli.samples.len(),
            linked.samples.len()
        );

        let n = cli.samples.len().min(linked.samples.len());
        let energy =
            |s: &[f32]| s.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / n as f64;
        let a = energy(&cli.samples[..n]);
        let b = energy(&linked.samples[..n]);
        assert!(
            (a - b).abs() <= a.max(b) * 0.05,
            "loudness differs: {a:.6} vs {b:.6}"
        );
    }

    #[test]
    fn a_file_with_no_audio_says_so_rather_than_returning_silence() {
        // Silence is a valid answer for a quiet file. "There is no audio
        // here" is a different answer, and beat detection needs to tell them
        // apart.
        let Some(silent) = fixture("sample.mp4") else {
            return;
        };
        let err = LibAv.decode_audio(&silent, 11025, None).unwrap_err();
        assert!(
            matches!(err, vb_media::MediaError::NoAudioStream),
            "got {err}"
        );
    }
}

// Photographs: both backends read one picture, turned by the EXIF both of
// them share, and a HEIF grid assembled from its tiles.
mod photo_parity {
    use super::*;

    fn photo(name: &str) -> Option<PathBuf> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-media")
            .join(name);
        if path.exists() && FfmpegCli::default().is_available() {
            Some(path)
        } else {
            eprintln!("skipping: needs ffmpeg and test-media/{name}");
            None
        }
    }

    /// Every photo fixture there is, and a real photo named by
    /// `VB_PARITY_PHOTO`.
    fn photos() -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = ["photo-rotated.jpg", "photo.png", "photo-grid.heic"]
            .into_iter()
            .filter_map(photo)
            .collect();
        if let Some(real) = std::env::var_os("VB_PARITY_PHOTO") {
            all.push(PathBuf::from(real));
        }
        all
    }

    #[test]
    fn a_photo_probes_the_same_and_reads_as_a_still() {
        for path in photos() {
            let cli = FfmpegCli::default().probe(&path).unwrap();
            let lib = LibAv.probe(&path).unwrap();
            assert_eq!(lib, cli, "{}", path.display());
            assert!(lib.still, "{} should be a still", path.display());
            assert_eq!(lib.duration, Micros::ZERO);
            assert!(!lib.has_audio);
        }
    }

    #[test]
    fn a_photo_stored_on_its_side_is_turned_by_its_exif() {
        let Some(path) = photo("photo-rotated.jpg") else {
            return;
        };
        let info = LibAv.probe(&path).unwrap();
        assert_eq!((info.width, info.height), (800, 600));
        assert_eq!(info.rotation, 90);
        assert_eq!(info.display_size(), (600, 800));
        assert_eq!(
            info.created_at.as_deref(),
            Some("2023-09-29T18:31:43+08:00")
        );

        // The red block drawn at the stored top-left ends up top-right once
        // the picture is turned a quarter clockwise.
        for backend in [&FfmpegCli::default() as &dyn MediaBackend, &LibAv] {
            let frame = backend.rgb_frame(&path, Micros::ZERO, 60, 80).unwrap();
            let at = |x: u32, y: u32| {
                let i = ((y * 60 + x) * 3) as usize;
                (frame.rgb[i], frame.rgb[i + 1], frame.rgb[i + 2])
            };
            let (r, g, b) = at(56, 4);
            assert!(
                r > 180 && g < 80 && b < 80,
                "{}: top-right is {:?}, not red",
                backend.name(),
                (r, g, b)
            );
            let (r, g, b) = at(4, 76);
            assert!(
                !(r > 180 && g < 80 && b < 80),
                "{}: bottom-left is red, so the picture was turned the wrong way",
                backend.name()
            );
            let (r, g, b) = at(4, 4);
            assert!(
                !(r > 180 && g < 80 && b < 80),
                "{}: top-left is red, so the picture was not turned",
                backend.name()
            );
        }
    }

    #[test]
    fn photo_pixels_match_the_cli() {
        for path in photos() {
            let info = LibAv.probe(&path).unwrap();
            let (w, h) = info.still_size(128);
            let a = FfmpegCli::default()
                .rgb_frame(&path, Micros::ZERO, w, h)
                .unwrap();
            let b = LibAv.rgb_frame(&path, Micros::ZERO, w, h).unwrap();
            let mean_abs_diff = a
                .rgb
                .iter()
                .zip(&b.rgb)
                .map(|(x, y)| x.abs_diff(*y) as f64)
                .sum::<f64>()
                / a.rgb.len() as f64;
            assert!(
                mean_abs_diff < 1.5,
                "{} differs by {mean_abs_diff:.2} levels on average",
                path.display()
            );
            let g = LibAv.gray_frame(&path, Micros::ZERO, w, h).unwrap();
            assert_eq!(g.pixels.len(), (w * h) as usize);
        }
    }

    #[test]
    fn a_heif_grid_is_the_whole_picture_not_one_tile() {
        let Some(path) = photo("photo-grid.heic") else {
            return;
        };
        let info = LibAv.probe(&path).unwrap();
        assert_eq!((info.width, info.height), (1400, 1000));
        assert_eq!(info.video_codec.as_deref(), Some("hevc"));
    }

    #[test]
    fn a_photos_still_is_upright_and_no_larger_than_the_models_need() {
        let dir = scratch("photo-stills");
        for path in photos() {
            let info = LibAv.probe(&path).unwrap();
            let want = info.still_size(vb_media::STILL_MAX_SIDE);
            for backend in [&FfmpegCli::default() as &dyn MediaBackend, &LibAv] {
                let dest = dir.join(format!(
                    "{}-{}.jpg",
                    path.file_stem().unwrap().to_string_lossy(),
                    backend.name()
                ));
                backend.extract_still(&path, Micros::ZERO, &dest).unwrap();
                // Read back as a picture of its own: no EXIF, so as stored.
                let back = FfmpegCli::default().probe(&dest).unwrap();
                assert_eq!(
                    (back.width, back.height),
                    want,
                    "{} from {}",
                    backend.name(),
                    path.display()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The renderer's half: frames and sound in order, and a piece written.
// ---------------------------------------------------------------------------

fn with_audio() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample-with-audio.mp4");
    path.exists().then_some(path)
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y) as f64)
        .sum::<f64>()
        / a.len().max(1) as f64
}

/// Read `count` frames from `from` with one backend.
fn read_in_order(
    backend: &dyn MediaBackend,
    input: &Path,
    from: Micros,
    (w, h): (u32, u32),
    count: usize,
) -> Vec<vb_media::VideoFrame> {
    let mut reader = backend.read_frames(input, from, w, h).unwrap();
    let mut frames = Vec::new();
    while frames.len() < count {
        match reader.next_frame().unwrap() {
            Some(f) => frames.push(f),
            None => break,
        }
    }
    frames
}

fn frames_in_order_agree(input: &Path, from: Micros) {
    let info = FfmpegCli::default().probe(input).unwrap();
    let size = if info.rotation % 180 == 90 {
        (72, 128)
    } else {
        (128, 72)
    };
    let cli = read_in_order(&FfmpegCli::default(), input, from, size, 12);
    let lib = read_in_order(&LibAv, input, from, size, 12);
    assert_eq!(lib.len(), cli.len(), "both read as many frames");
    assert!(!cli.is_empty());
    // The first is the first at or after the moment, as an exact seek
    // finds it, and each after it is the next.
    assert!(cli[0].at >= from && cli[0].at.0 - from.0 < ONE_FRAME);
    for (l, c) in lib.iter().zip(&cli) {
        // The CLI's times are its seek offset plus the frame's time from
        // there, each rounded to the stream's time base; this backend's
        // are the frame's own. A tick apart at most.
        assert!(
            l.at.0.abs_diff(c.at.0) < 100,
            "frame at {} vs {} from {from}",
            l.at,
            c.at
        );
        assert_eq!(l.rgba.len(), (size.0 * size.1 * 4) as usize);
        assert!(l.rgba.chunks_exact(4).all(|p| p[3] == 255), "opaque");
        let d = mean_abs_diff(&l.rgba, &c.rgba);
        assert!(d < 1.5, "frames at {} differ by {d:.2} levels", l.at);
    }
}

#[test]
fn frames_read_in_order_match_the_cli() {
    let Some(input) = sample() else { return };
    // From the start, from a moment between frames, and across a cut.
    frames_in_order_agree(&input, Micros::ZERO);
    frames_in_order_agree(&input, Micros(12_010_000));
    frames_in_order_agree(&input, Micros(5_900_000));
    if let Some(clip) = std::env::var_os("VB_PARITY_CLIP") {
        frames_in_order_agree(Path::new(&clip), Micros(700_000));
    }
}

#[test]
fn frames_read_in_order_are_the_frames_at_those_times() {
    // The reader and the one-frame decode are two ways to the same picture.
    let Some(input) = sample() else { return };
    let frames = read_in_order(&LibAv, &input, Micros(8_000_000), (128, 72), 5);
    for f in &frames {
        let one = LibAv.rgb_frame(&input, f.at, 128, 72).unwrap();
        let rgb: Vec<u8> = f
            .rgba
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        assert!(mean_abs_diff(&rgb, &one.rgb) < 0.5, "frame at {}", f.at);
    }
}

/// Every sample from `from` to `to` at one backend.
fn sound(backend: &dyn MediaBackend, input: &Path, from: u64, to: u64, tempo: f64) -> Vec<f32> {
    let mut reader = backend
        .read_samples(input, Micros(from), Some(Micros(to)), 48_000, 2, tempo)
        .unwrap();
    let mut all = Vec::new();
    loop {
        let part = reader.read(4096).unwrap();
        if part.is_empty() {
            break;
        }
        all.extend(part);
    }
    all
}

#[test]
fn sound_read_in_order_matches_the_cli() {
    if sample().is_none() {
        return;
    }
    let Some(input) = with_audio() else { return };
    for tempo in [1.0, 2.0, 0.5] {
        let cli = sound(&FfmpegCli::default(), &input, 1_000_000, 3_000_000, tempo);
        let lib = sound(&LibAv, &input, 1_000_000, 3_000_000, tempo);
        // Two seconds of footage at that speed, stereo at 48 kHz.
        let expected = (2.0 / tempo * 48_000.0 * 2.0) as i64;
        for (name, s) in [("cli", &cli), ("libav", &lib)] {
            assert!(
                (s.len() as i64 - expected).abs() < 48 * 2 * 30,
                "{name} at {tempo}x read {} samples, expected about {expected}",
                s.len()
            );
        }
        let n = cli.len().min(lib.len());
        let energy = cli[..n].iter().map(|x| (x * x) as f64).sum::<f64>() / n as f64;
        let error = cli[..n]
            .iter()
            .zip(&lib[..n])
            .map(|(a, b)| ((a - b) * (a - b)) as f64)
            .sum::<f64>()
            / n as f64;
        assert!(energy > 1e-5, "the fixture's clicks are heard");
        assert!(
            error < energy * 0.01,
            "at {tempo}x the sound differs: error {error:.2e} against energy {energy:.2e}"
        );
    }
}

#[test]
fn a_file_without_sound_says_so() {
    let Some(input) = sample() else { return };
    let e = LibAv
        .read_samples(&input, Micros::ZERO, None, 48_000, 2, 1.0)
        .err()
        .expect("no sound to read");
    assert!(matches!(e, vb_media::MediaError::NoAudioStream), "{e}");
}

#[test]
fn a_piece_is_written_and_reads_back() {
    if sample().is_none() || !LibAv.can_encode(vb_media::VideoCodec::H264) {
        eprintln!("skipping: this libav cannot write H.264");
        return;
    }
    let dest = scratch("written").join("piece.mp4");
    let settings = vb_media::EncodeSettings {
        width: 320,
        height: 180,
        fps_num: 30,
        fps_den: 1,
        codec: vb_media::VideoCodec::H264,
        video_bitrate: 2_000_000,
        sample_rate: 48_000,
        channels: 2,
        audio_bitrate: 128_000,
    };
    let mut encoder = LibAv.encode(&dest, &settings).unwrap();
    // Two seconds: the left half red, the right half blue, and a tone.
    let mut frame = vec![0u8; 320 * 180 * 4];
    for (i, px) in frame.chunks_exact_mut(4).enumerate() {
        let x = i % 320;
        px.copy_from_slice(if x < 160 {
            &[220, 30, 30, 255]
        } else {
            &[30, 30, 220, 255]
        });
    }
    for n in 0..60 {
        encoder.video(&frame).unwrap();
        let sound: Vec<f32> = (0..1600)
            .flat_map(|i| {
                let t = (n * 1600 + i) as f32 / 48_000.0;
                let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
                [v, v]
            })
            .collect();
        encoder.audio(&sound).unwrap();
    }
    encoder.finish().unwrap();

    for backend in [&LibAv as &dyn MediaBackend, &FfmpegCli::default()] {
        let info = backend.probe(&dest).unwrap();
        assert_eq!((info.width, info.height), (320, 180));
        assert!(info.has_audio);
        assert!(
            info.duration.0.abs_diff(2_000_000) < 50_000,
            "{} reads {}",
            backend.name(),
            info.duration
        );
        let one = backend
            .rgb_frame(&dest, Micros(1_000_000), 320, 180)
            .unwrap();
        let left = &one.rgb[(90 * 320 + 40) * 3..][..3];
        let right = &one.rgb[(90 * 320 + 280) * 3..][..3];
        assert!(left[0] > 180 && left[2] < 80, "left is red: {left:?}");
        assert!(right[2] > 180 && right[0] < 80, "right is blue: {right:?}");
        let tone = backend.decode_audio(&dest, 16_000, None).unwrap();
        let rms =
            (tone.samples.iter().map(|x| x * x).sum::<f32>() / tone.samples.len() as f32).sqrt();
        // Both channels carry the tone; how the downmix weighs them is
        // swresample's business, so only its presence is checked.
        assert!(rms > 0.15 && rms < 0.35, "the tone is there: rms {rms}");
    }
}

#[test]
fn an_encoder_dropped_unfinished_leaves_nothing() {
    if sample().is_none() || !LibAv.can_encode(vb_media::VideoCodec::H264) {
        return;
    }
    let dest = scratch("dropped").join("piece.mp4");
    let settings = vb_media::EncodeSettings {
        width: 160,
        height: 90,
        fps_num: 30,
        fps_den: 1,
        codec: vb_media::VideoCodec::H264,
        video_bitrate: 500_000,
        sample_rate: 48_000,
        channels: 2,
        audio_bitrate: 128_000,
    };
    let mut encoder = LibAv.encode(&dest, &settings).unwrap();
    encoder.video(&vec![128u8; 160 * 90 * 4]).unwrap();
    drop(encoder);
    assert!(!dest.exists());
}

// ---------------------------------------------------------------------------
// Lossless export: keyframes, fingerprints, and stretches joined by copying.
// ---------------------------------------------------------------------------

#[test]
fn keyframes_match_the_cli() {
    let Some(input) = sample() else { return };
    let cli = FfmpegCli::default()
        .keyframes(&input, Micros(1_500_000), Micros(12_000_000))
        .unwrap();
    let lib = LibAv
        .keyframes(&input, Micros(1_500_000), Micros(12_000_000))
        .unwrap();
    // The fixture has a keyframe every second: 2 to 12, 12 included.
    assert_eq!(cli.len(), 11, "{cli:?}");
    assert_eq!(lib.len(), cli.len(), "{lib:?} vs {cli:?}");
    for (l, c) in lib.iter().zip(&cli) {
        assert!(l.0.abs_diff(c.0) < 100, "keyframe {l} vs {c}");
    }
    assert_eq!(lib[0], Micros(2_000_000));
}

#[test]
fn both_backends_agree_on_what_can_be_joined() {
    let Some(input) = sample() else { return };
    let Some(other) = with_audio() else { return };
    let copy = scratch("fingerprint").join("same.mp4");
    std::fs::copy(&input, &copy).unwrap();
    for backend in [&LibAv as &dyn MediaBackend, &FfmpegCli::default()] {
        let a = backend.copy_fingerprint(&input).unwrap();
        let b = backend.copy_fingerprint(&copy).unwrap();
        let c = backend.copy_fingerprint(&other).unwrap();
        assert_eq!(a, b, "{}: a copy of a file joins it", backend.name());
        assert_ne!(
            a,
            c,
            "{}: a file with sound and another size does not",
            backend.name()
        );
    }
}

/// Two stretches of the fixture, each from a keyframe, joined by `backend`:
/// the first ends on a keyframe, the second inside a group.
fn joined(backend: &dyn MediaBackend, name: &str) -> PathBuf {
    let input = sample().unwrap();
    join_two(
        backend,
        name,
        &input,
        (2_000_000, 4_000_000),
        (13_000_000, 15_500_000),
    )
}

fn join_two(
    backend: &dyn MediaBackend,
    name: &str,
    input: &Path,
    a: (u64, u64),
    b: (u64, u64),
) -> PathBuf {
    let dest = scratch(name).join("joined.mp4");
    let parts = [a, b].map(|(from, to)| vb_media::CopyPart {
        path: input.to_path_buf(),
        from: Micros(from),
        to: Micros(to),
    });
    let mut shares = Vec::new();
    backend
        .copy_parts(&parts, &dest, true, &mut |s| {
            shares.push(s);
            true
        })
        .unwrap();
    assert_eq!(
        shares.last().copied(),
        Some(1.0),
        "{}: progress ends whole",
        backend.name()
    );
    dest
}

/// When each packet of a stream of `file` is shown, in seconds, in order:
/// what a player puts on the screen, read by ffprobe.
fn shown(file: &Path, stream: &str) -> Vec<f64> {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", stream])
        .args(["-show_entries", "packet=pts_time", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    let mut times: Vec<f64> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().trim_end_matches(',').parse().ok())
        .collect();
    times.sort_by(f64::total_cmp);
    times
}

/// The longest wait between one shown time and the next, and where.
fn longest_gap(times: &[f64]) -> (f64, f64) {
    times
        .windows(2)
        .map(|w| (w[1] - w[0], w[0]))
        .fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
}

#[test]
fn stretches_join_into_the_frames_they_were() {
    if sample().is_none() || !LibAv.can_copy() {
        eprintln!("skipping: this libav cannot join files");
        return;
    }
    let input = sample().unwrap();
    let cli = joined(&FfmpegCli::default(), "joined-cli");
    let lib = joined(&LibAv, "joined-lib");
    // Both backends keep the same frames at the same times.
    let (cli_frames, lib_frames) = (shown(&cli, "v:0"), shown(&lib, "v:0"));
    assert_eq!(cli_frames.len(), lib_frames.len());
    for (c, l) in cli_frames.iter().zip(&lib_frames) {
        assert!((c - l).abs() < 1e-4, "a frame at {c} s against {l} s");
    }
    for (name, out) in [("cli", &cli), ("libav", &lib)] {
        let info = FfmpegCli::default().probe(out).unwrap();
        // 2 s ending on a keyframe, exactly; 2.5 s ending inside a group,
        // run on to the reference frame its last frames wait for: two
        // frames more, both shown.
        assert!(
            info.duration.0.abs_diff(4_566_667) < 1_000,
            "{name}: {} long, 4.567 s expected",
            info.duration
        );
        let frames = shown(out, "v:0");
        assert!(frames[0].abs() < 1e-6, "{name}: starts at {}", frames[0]);
        assert_eq!(frames.len(), 60 + 77, "{name}: 60 frames, then 75 and two");
        // The seam on a keyframe is frame-exact: the frame shown at 2 s is
        // the next stretch's first, a frame after the last of the first.
        assert!((frames[60] - 2.0).abs() < 1e-4 && (frames[59] - 59.0 / 30.0).abs() < 1e-4);
        // No frame is held over another's place, at a seam or at the end.
        let (gap, at) = longest_gap(&frames);
        assert!(
            gap < 1.5 / 30.0,
            "{name}: {gap} s without a frame at {at} s"
        );
        assert_eq!((info.width, info.height), (1280, 720));
        // Nothing was re-encoded: the frames are the source's own, a second
        // into each stretch.
        for (at_out, at_in) in [(1_000_000, 3_000_000), (3_000_000, 14_000_000)] {
            let a = FfmpegCli::default()
                .gray_frame(out, Micros(at_out), 160, 90)
                .unwrap();
            let b = FfmpegCli::default()
                .gray_frame(&input, Micros(at_in), 160, 90)
                .unwrap();
            let d = mean_abs_diff(&a.pixels, &b.pixels);
            assert!(
                d < 0.01,
                "{name}: frame {at_out} differs from the source by {d}"
            );
        }
    }
}

#[test]
fn sound_joins_with_its_picture_and_stays_on_it() {
    let (Some(_), Some(input)) = (sample(), with_audio()) else {
        return;
    };
    if !LibAv.can_copy() {
        return;
    }
    let (a, b) = ((2_000_000, 4_000_000), (7_000_000, 9_500_000));
    let cli = join_two(&FfmpegCli::default(), "joined-sound-cli", &input, a, b);
    let lib = join_two(&LibAv, "joined-sound-lib", &input, a, b);
    let (cli_sound, lib_sound) = (shown(&cli, "a:0"), shown(&lib, "a:0"));
    assert_eq!(cli_sound.len(), lib_sound.len(), "the same sound packets");
    for (c, l) in cli_sound.iter().zip(&lib_sound) {
        assert!((c - l).abs() < 1e-3, "sound at {c} s against {l} s");
    }
    // One AAC packet: 1024 samples at 44.1 kHz.
    let packet = 1024.0 / 44_100.0;
    for (name, out) in [("cli", &cli), ("libav", &lib)] {
        let frames = shown(out, "v:0");
        let sound = shown(out, "a:0");
        // The second stretch starts after the first's last packet of
        // sound, which runs on past its picture by less than a packet.
        let wait = frames[60] - 2.0;
        assert!(
            (0.0..packet).contains(&wait),
            "{name}: the seam waits {wait} s"
        );
        // The sound runs the whole piece, no packet missing at the seam,
        // and ends with the picture.
        assert!(sound[0] < packet, "{name}: sound starts at {}", sound[0]);
        let (gap, at) = longest_gap(&sound);
        assert!(gap < 2.0 * packet, "{name}: {gap} s of no sound at {at} s");
        let picture_end = frames.last().unwrap() + 1.0 / 30.0;
        let sound_end = sound.last().unwrap() + packet;
        assert!(
            (sound_end - picture_end).abs() < packet,
            "{name}: sound ends at {sound_end}, picture at {picture_end}"
        );
        // The second stretch's sound starts with its picture: nothing of
        // the first's runs on into it.
        let second = frames[60];
        let first_sound = sound.iter().find(|t| **t >= second - 1e-6).unwrap();
        assert!(
            first_sound - second < packet,
            "{name}: the second stretch's sound starts {} s after its picture",
            first_sound - second
        );
    }
}

#[test]
fn a_stopped_join_leaves_nothing() {
    if sample().is_none() || !LibAv.can_copy() {
        return;
    }
    let input = sample().unwrap();
    for backend in [&LibAv as &dyn MediaBackend, &FfmpegCli::default()] {
        let dest = scratch(&format!("join-stopped-{}", backend.name())).join("joined.mp4");
        let parts = [vb_media::CopyPart {
            path: input.clone(),
            from: Micros::ZERO,
            to: Micros(20_000_000),
        }];
        let result = backend.copy_parts(&parts, &dest, true, &mut |_| false);
        assert!(result.is_err(), "{}: stopped", backend.name());
        assert!(!dest.exists(), "{}: nothing left behind", backend.name());
    }
}
