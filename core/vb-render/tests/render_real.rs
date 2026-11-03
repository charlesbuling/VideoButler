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

//! A finished piece rendered from real media: clips, a photograph, a title,
//! transitions, a moved picture, an adjustment, a blurred background and
//! music, through the default media backend, then read back.
//!
//! Needs ffmpeg and the fixtures from `scripts/make-test-media.sh`, and
//! skips without them. `VB_RENDER_OUT=<dir>` keeps the piece and a still of
//! every second, to look at.

use std::path::{Path, PathBuf};
use vb_contracts::editing_dsl::{
    Anchor, AudioItem, AudioKind, Background, BackgroundKind, Filter, Slot, SlotKind, SourceRef,
    TextItem, TextRole, TextStyle, Transform, TransformKeyframe, Transition, TransitionType,
    VideoItem,
};
use vb_contracts::footage_graph::{MediaKind, Source};
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros};
use vb_media::{MediaBackend, VideoCodec};
use vb_render::{RenderOptions, frame, render};

fn media_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media")
}

fn backend() -> Option<Box<dyn MediaBackend>> {
    let media = vb_media::default_backend();
    let ok = media.is_available()
        && media_dir().join("sample.mp4").exists()
        && media_dir().join("sample-with-audio.mp4").exists();
    if !ok {
        eprintln!("skipping: needs ffmpeg and test-media (scripts/make-test-media.sh)");
        return None;
    }
    if !media.can_encode(VideoCodec::H264) {
        eprintln!("skipping: this ffmpeg has no H.264 encoder");
        return None;
    }
    Some(media)
}

fn source(media: &dyn MediaBackend, id: &str, name: &str) -> Source {
    let path = media_dir().join(name);
    let info = media.probe(&path).expect("fixture probes");
    Source {
        id: id.into(),
        path: path.display().to_string(),
        content_hash: Blake3Hash::of_str(name),
        media_kind: info.still.then_some(MediaKind::Image),
        duration_us: if info.still {
            Micros::from_secs(10)
        } else {
            info.duration
        },
        fps_num: Some(info.fps_num),
        fps_den: Some(info.fps_den),
        width: Some(info.width),
        height: Some(info.height),
        video_codec: info.video_codec,
        audio_codec: info.audio_codec,
        has_audio: Some(info.has_audio),
        created_at: None,
        rotation: Some(info.rotation),
        categories: Vec::new(),
    }
}

fn clip(id: &str, order: u32, source: &str, in_s: f64, out_s: f64, speed: f64) -> Slot {
    let (i, o) = (Micros::from_secs_f64(in_s), Micros::from_secs_f64(out_s));
    Slot {
        id: id.into(),
        order,
        kind: SlotKind::Clip,
        duration_us: Micros(((o.0 - i.0) as f64 / speed).round() as u64),
        source_ref: Some(SourceRef {
            source_id: source.into(),
            shot_id: None,
            in_us: i,
            out_us: o,
        }),
        speed: (speed != 1.0).then_some(speed),
        reason: None,
        score: None,
    }
}

/// A title card, a clip, a photograph turned upright on a blurred
/// background, a clip moved and brightened at double speed, and music.
fn piece(media: &dyn MediaBackend) -> (EditingDsl, FootageGraph) {
    let mut graph = FootageGraph::new("render-test");
    graph.sources.push(source(media, "bars", "sample.mp4"));
    graph
        .sources
        .push(source(media, "click", "sample-with-audio.mp4"));
    graph
        .sources
        .push(source(media, "photo", "photo-rotated.jpg"));

    let mut dsl = EditingDsl::new("渲染测试");
    dsl.meta.aspect_ratio = Some("16:9".into());
    dsl.meta.fps_num = Some(30);
    dsl.meta.fps_den = Some(1);
    dsl.slots = vec![
        Slot {
            id: "title".into(),
            order: 0,
            kind: SlotKind::TitleCard,
            duration_us: Micros::from_secs(1),
            source_ref: None,
            speed: None,
            reason: None,
            score: None,
        },
        clip("a", 1, "bars", 12.0, 14.0, 1.0),
        clip("b", 2, "photo", 0.0, 1.5, 1.0),
        clip("c", 3, "click", 1.0, 5.0, 2.0),
    ];
    dsl.transitions = vec![
        Transition {
            from_slot_id: "title".into(),
            to_slot_id: "a".into(),
            kind: TransitionType::FadeToBlack,
            duration_us: Micros::from_millis(400),
        },
        Transition {
            from_slot_id: "a".into(),
            to_slot_id: "b".into(),
            kind: TransitionType::Xfade,
            duration_us: Micros::from_millis(500),
        },
        Transition {
            from_slot_id: "b".into(),
            to_slot_id: "c".into(),
            kind: TransitionType::WipeLeft,
            duration_us: Micros::from_millis(300),
        },
    ];
    dsl.tracks.video = vec![
        VideoItem {
            background: Some(Background {
                kind: BackgroundKind::Blur,
                color: None,
            }),
            ..VideoItem::plain("b")
        },
        VideoItem {
            filters: vec![Filter {
                name: "eq".into(),
                params: [("brightness".to_string(), serde_json::json!(0.15))]
                    .into_iter()
                    .collect(),
            }],
            transform: Some(Transform {
                keyframes: vec![
                    TransformKeyframe {
                        offset_us: Micros::ZERO,
                        scale: 1.3,
                        x: 0.1,
                        y: 0.0,
                        rotation_deg: 5.0,
                    },
                    TransformKeyframe::fitted(Micros::from_secs(2)),
                ],
            }),
            ..VideoItem::plain("c")
        },
    ];
    dsl.tracks.text = vec![
        TextItem {
            id: "t-title".into(),
            anchor: Anchor::at("title"),
            duration_us: Micros::from_secs(1),
            content: "新疆之旅".into(),
            role: Some(TextRole::Title),
            transcript_segment_id: None,
            style: None,
        },
        TextItem {
            id: "t-caption".into(),
            anchor: Anchor {
                slot_id: "c".into(),
                offset_us: Some(200_000),
            },
            duration_us: Micros::from_secs(1),
            content: "快进两倍".into(),
            role: Some(TextRole::Caption),
            transcript_segment_id: None,
            style: Some(TextStyle {
                primary_color: Some("#FFE066".into()),
                background_color: Some("#00000099".into()),
                ..TextStyle::default()
            }),
        },
    ];
    if media_dir().join("beat-120.wav").exists() {
        dsl.tracks.audio.push(AudioItem {
            id: "music".into(),
            anchor: Anchor::at("title"),
            kind: AudioKind::Bgm,
            duration_us: None,
            asset_path: Some(media_dir().join("beat-120.wav").display().to_string()),
            asset_in_us: None,
            gain_db: Some(-6.0),
            muted: None,
            fade_in_us: Some(Micros::from_millis(300)),
            fade_out_us: Some(Micros::from_millis(500)),
            ducking: None,
        });
    }
    (dsl, graph)
}

#[test]
fn a_piece_renders_whole_and_reads_back() {
    let Some(media) = backend() else { return };
    let (dsl, graph) = piece(media.as_ref());
    let keep = std::env::var_os("VB_RENDER_OUT").map(PathBuf::from);
    let dir = keep
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("vb-render-{}", std::process::id())));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("piece.mp4");
    let _ = std::fs::remove_file(&dest);

    // VB_RENDER_SHORT=720 renders larger, for looking at the text.
    let short = std::env::var("VB_RENDER_SHORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180);
    let options = RenderOptions::for_dsl(&dsl, &graph, short, None, VideoCodec::H264, 0.2);
    assert_eq!(options.height, short);
    let mut calls = 0;
    let report = render(
        &dsl,
        &graph,
        media.as_ref(),
        &dest,
        &options,
        &mut |_, _| {
            calls += 1;
            true
        },
    )
    .expect("the piece renders");
    let total = dsl.resolve_timeline().total_duration_us;
    assert_eq!(report.duration, total);
    assert!(calls as u64 >= report.frames);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // What a player sees: the length the timeline says, picture and sound.
    let info = media.probe(&dest).expect("the piece probes");
    assert_eq!((info.width, info.height), (options.width, options.height));
    assert!(info.has_audio, "the piece has a sound track");
    let diff = info.duration.0.abs_diff(total.0);
    assert!(
        diff < 100_000,
        "{} long, the timeline says {}",
        info.duration,
        total
    );

    // The middle of the title card is black with white text; the clip after
    // it is the bars, which are bright.
    let title = media
        .rgb_frame(&dest, Micros::from_millis(500), 320, 180)
        .unwrap();
    let corner = &title.rgb[0..3];
    assert!(corner.iter().all(|v| *v < 30), "title corner {corner:?}");
    let ink = title
        .rgb
        .chunks_exact(3)
        .filter(|p| p.iter().all(|v| *v > 200))
        .count();
    assert!(ink > 100, "the title's text is drawn ({ink} white pixels)");

    // The sound under the clip with the click is not silent.
    let sound = media.decode_audio(&dest, 16_000, None).unwrap();
    let loud = sound.samples.iter().filter(|s| s.abs() > 0.05).count();
    assert!(loud > 1_000, "{loud} loud samples");

    if let Some(dir) = keep {
        for s in 0..=(total.0 / 1_000_000) {
            let at = Micros::from_secs(s).min(Micros(total.0 - 50_000));
            let still = dir.join(format!("second-{s}.png"));
            let _ = media.extract_still(&dest, at, &still);
        }
        eprintln!("kept {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn one_frame_is_what_the_render_draws_there() {
    let Some(media) = backend() else { return };
    let (dsl, graph) = piece(media.as_ref());
    // Halfway through the dissolve from the bars into the photograph.
    let tl = dsl.resolve_timeline();
    let b = tl.slot("b").unwrap();
    let at = b.start_us + Micros(b.incoming_overlap_us.0 / 2);
    let rgba = frame(&dsl, &graph, media.as_ref(), at, 320, 180).expect("a frame");
    assert_eq!(rgba.len(), 320 * 180 * 4);
    assert!(
        rgba.chunks_exact(4).all(|p| p[3] == 255),
        "the frame is opaque"
    );
    if let Some(dir) = std::env::var_os("VB_RENDER_OUT").map(PathBuf::from) {
        write_png(&dir.join("frame-dissolve.png"), &rgba, 320, 180);
    }
}

#[test]
fn a_stopped_render_leaves_nothing_behind() {
    let Some(media) = backend() else { return };
    let (dsl, graph) = piece(media.as_ref());
    let dir = std::env::temp_dir().join(format!("vb-render-stop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("stopped.mp4");
    let options = RenderOptions::for_dsl(&dsl, &graph, 180, None, VideoCodec::H264, 0.2);
    let result = render(
        &dsl,
        &graph,
        media.as_ref(),
        &dest,
        &options,
        &mut |done, _| done < 10,
    );
    assert!(matches!(result, Err(vb_render::RenderError::Cancelled)));
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) {
    // A minimal PNG through the media backend would need an encoder; a
    // portable pixmap is enough to look at.
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for px in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    let _ = std::fs::write(path.with_extension("ppm"), ppm);
}

// ---------------------------------------------------------------------------
// Lossless export
// ---------------------------------------------------------------------------

/// Only cuts: two stretches of one file, end to end, as filmed.
fn plain_cut(media: &dyn MediaBackend) -> (EditingDsl, FootageGraph) {
    let mut graph = FootageGraph::new("lossless-test");
    graph.sources.push(source(media, "bars", "sample.mp4"));
    graph
        .sources
        .push(source(media, "click", "sample-with-audio.mp4"));
    let mut dsl = EditingDsl::new("无损");
    dsl.meta.aspect_ratio = Some("16:9".into());
    dsl.slots = vec![
        clip("a", 0, "bars", 2.3, 4.0, 1.0),
        clip("b", 1, "bars", 13.0, 15.5, 1.0),
    ];
    (dsl, graph)
}

#[test]
fn a_cut_of_only_cuts_leaves_without_being_re_encoded() {
    let Some(media) = backend() else { return };
    if !media.can_copy() {
        eprintln!("skipping: this backend cannot join files");
        return;
    }
    let (dsl, graph) = plain_cut(media.as_ref());
    let plan = vb_render::lossless::plan(&dsl, &graph, media.as_ref()).expect("lossless");
    // The first clip starts at the keyframe before 2.3 s and ends on one;
    // the second starts on one and ends inside a group of B-frames, so it
    // runs on two frames to where its frames are whole.
    assert_eq!(plan.parts[0].part.from, Micros::from_secs(2));
    assert_eq!(plan.parts[0].extra, Micros::from_millis(300));
    let two_frames = 66_667;
    assert!(
        plan.parts[1].extra.0.abs_diff(two_frames) <= 1,
        "{}",
        plan.parts[1].extra
    );
    assert!(plan.extra.0.abs_diff(300_000 + two_frames) <= 1);
    assert!(plan.total.0.abs_diff(4_500_000 + two_frames) <= 1);
    assert!(plan.audio);

    let dir = std::env::temp_dir().join(format!("vb-lossless-{}", std::process::id()));
    let dest = dir.join("无损.mp4");
    let started = std::time::Instant::now();
    let report = vb_render::lossless::export(&plan, media.as_ref(), &dest, &mut |_| true).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // As long as the plan said.
    assert!(
        report.duration.0.abs_diff(plan.total.0) < 1_000,
        "{} against {}",
        report.duration,
        plan.total
    );
    assert_eq!((report.width, report.height), (1280, 720));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whatever_changes_the_picture_or_sound_is_named_as_the_reason() {
    use vb_render::lossless::{Blocker, plan};
    let Some(media) = backend() else { return };
    if !media.can_copy() {
        return;
    }
    let (base, graph) = plain_cut(media.as_ref());
    let reasons = |dsl: &EditingDsl| plan(dsl, &graph, media.as_ref()).unwrap_err();

    let mut d = base.clone();
    d.transitions.push(Transition {
        from_slot_id: "a".into(),
        to_slot_id: "b".into(),
        kind: TransitionType::Xfade,
        duration_us: Micros::from_millis(500),
    });
    assert_eq!(reasons(&d), [Blocker::Transition]);

    let mut d = base.clone();
    d.slots[1] = clip("b", 1, "bars", 13.0, 15.0, 2.0);
    assert_eq!(reasons(&d), [Blocker::Speed("b".into())]);

    let mut d = base.clone();
    d.tracks.video.push(VideoItem {
        crop: Some(vb_contracts::editing_dsl::Crop {
            x: 0.1,
            y: 0.0,
            w: 0.8,
            h: 1.0,
        }),
        ..VideoItem::plain("a")
    });
    assert_eq!(reasons(&d), [Blocker::Picture("a".into())]);

    let mut d = base.clone();
    d.meta.aspect_ratio = Some("9:16".into());
    assert_eq!(reasons(&d), [Blocker::Canvas]);

    // Another file, with sound and another size: not one track.
    let mut d = base.clone();
    d.slots[1] = clip("b", 1, "click", 1.0, 3.0, 1.0);
    assert!(reasons(&d).contains(&Blocker::Mixed));

    // A clip muted and the other not.
    let mut d = base.clone();
    d.tracks.audio.push(AudioItem {
        id: "s".into(),
        anchor: Anchor::at("a"),
        kind: AudioKind::Source,
        duration_us: None,
        asset_path: None,
        asset_in_us: None,
        gain_db: None,
        muted: Some(true),
        fade_in_us: None,
        fade_out_us: None,
        ducking: None,
    });
    assert_eq!(reasons(&d), [Blocker::Sound]);

    // The renderer's own test piece: title, photo, speed, text, music...
    let (piece, piece_graph) = piece(media.as_ref());
    let all = plan(&piece, &piece_graph, media.as_ref()).unwrap_err();
    for expected in [
        Blocker::Transition,
        Blocker::Text,
        Blocker::NotFootage("title".into()),
    ] {
        assert!(all.contains(&expected), "{expected:?} in {all:?}");
    }
}
