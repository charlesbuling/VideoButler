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
use vb_contracts::Blake3Hash;
use vb_contracts::editing_dsl::{
    Anchor, AudioItem, AudioKind, BackgroundKind, Slot, SlotKind, SourceRef, TextItem, Transform,
    TransformKeyframe, Transition,
};
use vb_contracts::footage_graph::{MediaKind, Shot, Source};

fn source(id: &str, secs: u64, still: bool) -> Source {
    Source {
        id: id.into(),
        path: format!("/footage/{id}.mp4"),
        content_hash: Blake3Hash::of_str(id),
        media_kind: still.then_some(MediaKind::Image),
        duration_us: Micros::from_secs(secs),
        fps_num: Some(30),
        fps_den: Some(1),
        width: Some(1920),
        height: Some(1080),
        video_codec: None,
        audio_codec: None,
        has_audio: Some(!still),
        created_at: None,
        rotation: None,
        categories: Vec::new(),
    }
}

fn graph() -> FootageGraph {
    let mut g = FootageGraph::new("edit-test");
    g.sources.push(source("v1", 60, false));
    g.sources.push(source("v2", 30, false));
    g.sources.push(source("p1", 10, true));
    g.shots.push(Shot {
        id: "shot-a".into(),
        source_id: "v1".into(),
        start_us: Micros::from_secs(0),
        end_us: Micros::from_secs(20),
        shot_change_confidence: None,
        keyframes: vec![],
        description: None,
        entity_refs: vec![],
        actions: vec![],
        audio: None,
        tags: vec![],
        segments: Vec::new(),
        timeline: Vec::new(),
    });
    g
}

fn clip(id: &str, order: u32, source: &str, in_s: u64, out_s: u64) -> Slot {
    Slot {
        id: id.into(),
        order,
        kind: SlotKind::Clip,
        duration_us: Micros::from_secs(out_s - in_s),
        source_ref: Some(SourceRef {
            source_id: source.into(),
            shot_id: None,
            in_us: Micros::from_secs(in_s),
            out_us: Micros::from_secs(out_s),
        }),
        speed: None,
        reason: Some("chosen".into()),
        score: None,
    }
}

fn text(id: &str, slot: &str, offset_ms: i64, dur_ms: u64) -> TextItem {
    TextItem {
        id: id.into(),
        anchor: Anchor {
            slot_id: slot.into(),
            offset_us: (offset_ms != 0).then_some(offset_ms * 1000),
        },
        duration_us: Micros::from_millis(dur_ms),
        content: id.into(),
        role: None,
        transcript_segment_id: None,
        style: None,
    }
}

fn music(id: &str, slot: &str, secs: u64) -> AudioItem {
    AudioItem {
        id: id.into(),
        anchor: Anchor::at(slot),
        kind: AudioKind::Bgm,
        duration_us: Some(Micros::from_secs(secs)),
        asset_path: Some("/music/song.mp3".into()),
        asset_in_us: None,
        gain_db: None,
        muted: None,
        fade_in_us: None,
        fade_out_us: None,
        ducking: None,
    }
}

/// a: v1 2-6s, b: v2 10-14s, c: v1 30-36s; a dissolve from b to c; music
/// under the whole piece; a caption on b.
fn cut() -> EditingDsl {
    let mut d = EditingDsl::new("trip");
    d.slots = vec![
        clip("a", 0, "v1", 2, 6),
        clip("b", 1, "v2", 10, 14),
        clip("c", 2, "v1", 30, 36),
    ];
    d.transitions.push(Transition {
        from_slot_id: "b".into(),
        to_slot_id: "c".into(),
        kind: TransitionType::Dissolve,
        duration_us: Micros::from_millis(500),
    });
    d.tracks.text.push(text("cap-b", "b", 1000, 2000));
    d.tracks.audio.push(music("song", "a", 13));
    tidy(&mut d, &graph());
    d
}

fn total(d: &EditingDsl) -> Micros {
    d.resolve_timeline().total_duration_us
}

fn start_of(d: &EditingDsl, slot: &str) -> Micros {
    d.resolve_timeline().slot(slot).unwrap().start_us
}

fn ok(d: &mut EditingDsl, op: Op) -> Option<Selection> {
    let s = apply(d, &graph(), &op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    check(d, &graph()).expect("valid after every edit");
    s
}

#[test]
fn ops_read_from_the_front_ends_json() {
    let op: Op = serde_json::from_str(r#"{"op":"split","atUs":5000000}"#).unwrap();
    assert_eq!(
        op,
        Op::Split {
            slot_id: None,
            at_us: 5_000_000
        }
    );
    let op: Op = serde_json::from_str(
        r#"{"op":"transition","fromSlotId":"a","kind":"dissolve","durationUs":400000}"#,
    )
    .unwrap();
    assert_eq!(
        op,
        Op::Transition {
            from_slot_id: "a".into(),
            kind: TransitionType::Dissolve,
            duration_us: 400_000
        }
    );
    let op: Op = serde_json::from_str(
        r#"{"op":"setMusic","path":"/m.mp3","assetDurationUs":9,"loop":true}"#,
    )
    .unwrap();
    assert!(matches!(op, Op::SetMusic { looped: true, .. }));
}

#[test]
fn a_split_makes_two_clips_of_the_same_footage() {
    let mut d = cut();
    let before = total(&d);
    // 5.5s is 1.5s into b (which starts at 4s).
    let sel = ok(
        &mut d,
        Op::Split {
            slot_id: None,
            at_us: 5_500_000,
        },
    );
    let Some(Selection::Slot(second)) = sel else {
        panic!("selects the second half")
    };
    assert_eq!(d.slots.len(), 4);
    let b = d.slot("b").unwrap();
    let b2 = d.slot(&second).unwrap();
    assert_eq!(
        b.source_ref.as_ref().unwrap().out_us,
        Micros::from_millis(11_500)
    );
    assert_eq!(
        b2.source_ref.as_ref().unwrap().in_us,
        Micros::from_millis(11_500)
    );
    assert_eq!(b.duration_us + b2.duration_us, Micros::from_secs(4));
    assert_eq!(total(&d), before, "a split changes nothing on screen");
    // The dissolve into c now leaves from the second half.
    assert_eq!(d.transitions[0].from_slot_id, second);
    // The caption started 1s into b, before the split: it stays on b.
    assert_eq!(d.tracks.text[0].anchor.slot_id, "b");
}

#[test]
fn text_after_the_split_point_goes_with_the_second_half() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Split {
            slot_id: None,
            at_us: 4_500_000,
        },
    );
    let t = &d.tracks.text[0];
    assert_ne!(t.anchor.slot_id, "b");
    assert_eq!(t.anchor.offset_us, Some(500_000));
    let tl = d.resolve_timeline();
    assert_eq!(
        tl.resolve_anchor(&t.anchor),
        Some(Micros::from_secs(5)),
        "same moment on screen"
    );
}

#[test]
fn a_split_at_a_clips_edge_is_refused_and_changes_nothing() {
    let mut d = cut();
    let before = d.clone();
    let e = apply(
        &mut d,
        &graph(),
        &Op::Split {
            slot_id: None,
            at_us: 4_050_000,
        },
    )
    .unwrap_err();
    assert!(matches!(e, EditError::NothingToSplit(_)));
    assert_eq!(d, before);
}

#[test]
fn trimming_the_start_keeps_the_caption_on_its_footage() {
    let mut d = cut();
    // b from 10s to 10.5s: the caption at 1s into b is now 0.5s in.
    ok(
        &mut d,
        Op::Trim {
            slot_id: "b".into(),
            in_us: Some(10_500_000),
            out_us: None,
        },
    );
    assert_eq!(d.slot("b").unwrap().duration_us, Micros::from_millis(3_500));
    assert_eq!(d.tracks.text[0].anchor.offset_us, Some(500_000));
}

#[test]
fn trimming_away_the_footage_a_caption_was_on_removes_it() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Trim {
            slot_id: "b".into(),
            in_us: Some(13_200_000),
            out_us: None,
        },
    );
    assert!(d.tracks.text.is_empty());
}

#[test]
fn a_trim_past_the_footage_is_refused() {
    let mut d = cut();
    let e = apply(
        &mut d,
        &graph(),
        &Op::Trim {
            slot_id: "b".into(),
            in_us: None,
            out_us: Some(31_000_000),
        },
    )
    .unwrap_err();
    assert!(matches!(e, EditError::OutsideFootage(_)));
}

#[test]
fn a_trimmed_clip_outside_its_shot_drops_the_shot_name() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Trim {
            slot_id: "a".into(),
            in_us: None,
            out_us: Some(6_000_000),
        },
    );
    assert_eq!(
        d.slot("a")
            .unwrap()
            .source_ref
            .as_ref()
            .unwrap()
            .shot_id
            .as_deref(),
        Some("shot-a")
    );
    ok(
        &mut d,
        Op::Trim {
            slot_id: "a".into(),
            in_us: None,
            out_us: Some(25_000_000),
        },
    );
    assert_eq!(
        d.slot("a").unwrap().source_ref.as_ref().unwrap().shot_id,
        None
    );
}

#[test]
fn deleting_a_clip_takes_its_caption_and_keeps_the_music_in_place() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Delete {
            slot_id: "a".into(),
        },
    );
    assert!(d.slot("a").is_none());
    assert_eq!(d.tracks.text.len(), 1, "the caption was on b");
    // The music started at the beginning and still does, anchored to b now.
    let song = d.tracks.audio.iter().find(|a| a.id == "song").unwrap();
    assert_eq!(song.anchor.slot_id, "b");
    assert_eq!(
        d.resolve_timeline().resolve_anchor(&song.anchor),
        Some(Micros::ZERO)
    );
    // And no longer than the piece.
    assert!(song.duration_us.unwrap() <= total(&d));
    let mut d = cut();
    ok(
        &mut d,
        Op::Delete {
            slot_id: "b".into(),
        },
    );
    assert!(d.tracks.text.is_empty());
    assert!(d.transitions.is_empty(), "the dissolve joined b");
}

#[test]
fn the_last_slot_stays() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Delete {
            slot_id: "a".into(),
        },
    );
    ok(
        &mut d,
        Op::Delete {
            slot_id: "b".into(),
        },
    );
    assert_eq!(
        apply(
            &mut d,
            &graph(),
            &Op::Delete {
                slot_id: "c".into()
            }
        ),
        Err(EditError::LastSlot)
    );
}

#[test]
fn moving_a_clip_keeps_its_caption_and_breaks_transitions_that_no_longer_join() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Move {
            slot_id: "b".into(),
            to_index: 0,
        },
    );
    let order: Vec<&str> = d.ordered_slots().iter().map(|s| s.id.as_str()).collect();
    assert_eq!(order, ["b", "a", "c"]);
    assert!(d.transitions.is_empty(), "b and c are no longer neighbours");
    assert_eq!(d.tracks.text[0].anchor.slot_id, "b");
    // The music still starts at the start, under whichever clip is there.
    let song = &d.tracks.audio[0];
    assert_eq!(
        d.resolve_timeline().resolve_anchor(&song.anchor),
        Some(Micros::ZERO)
    );
    assert_eq!(start_of(&d, "b"), Micros::ZERO);
}

#[test]
fn twice_the_speed_is_half_the_length_and_keeps_the_caption_on_its_words() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Speed {
            slot_id: "b".into(),
            speed: 2.0,
        },
    );
    let b = d.slot("b").unwrap();
    assert_eq!(b.duration_us, Micros::from_secs(2));
    assert_eq!(b.speed, Some(2.0));
    let t = &d.tracks.text[0];
    assert_eq!(t.anchor.offset_us, Some(500_000));
    assert_eq!(t.duration_us, Micros::from_secs(1));
    // Back to normal speed is no speed at all in the manifest.
    ok(
        &mut d,
        Op::Speed {
            slot_id: "b".into(),
            speed: 1.0,
        },
    );
    assert_eq!(d.slot("b").unwrap().speed, None);
}

#[test]
fn a_speed_out_of_range_and_a_photographs_speed_are_refused() {
    let mut d = cut();
    assert!(matches!(
        apply(
            &mut d,
            &graph(),
            &Op::Speed {
                slot_id: "b".into(),
                speed: 9.0
            }
        ),
        Err(EditError::BadSpeed(_))
    ));
    ok(
        &mut d,
        Op::Insert {
            source_id: "p1".into(),
            in_us: 0,
            out_us: 3_000_000,
            at_index: Some(1),
        },
    );
    let photo = d.slots[1].id.clone();
    assert!(matches!(
        apply(
            &mut d,
            &graph(),
            &Op::Speed {
                slot_id: photo.clone(),
                speed: 2.0
            }
        ),
        Err(EditError::PhotoHasNoSpeed(_))
    ));
    // A photograph is held as long as asked, up to what the graph allows.
    ok(
        &mut d,
        Op::SetDuration {
            slot_id: photo.clone(),
            duration_us: 5_000_000,
        },
    );
    assert_eq!(d.slot(&photo).unwrap().duration_us, Micros::from_secs(5));
}

#[test]
fn a_clips_sound_is_kept_only_when_it_differs() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Volume {
            slot_id: "a".into(),
            gain_db: Some(-6.0),
            muted: None,
        },
    );
    let item = d
        .tracks
        .audio
        .iter()
        .find(|a| a.kind == AudioKind::Source)
        .unwrap();
    assert_eq!(item.gain_db, Some(-6.0));
    ok(
        &mut d,
        Op::Volume {
            slot_id: "a".into(),
            gain_db: Some(0.0),
            muted: Some(false),
        },
    );
    assert!(d.tracks.audio.iter().all(|a| a.kind != AudioKind::Source));
    ok(&mut d, Op::MuteAll { muted: true });
    assert_eq!(
        d.tracks
            .audio
            .iter()
            .filter(|a| a.kind == AudioKind::Source && a.muted == Some(true))
            .count(),
        3
    );
    ok(&mut d, Op::MuteAll { muted: false });
    assert!(d.tracks.audio.iter().all(|a| a.kind != AudioKind::Source));
}

#[test]
fn transitions_are_set_capped_and_removed() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Transition {
            from_slot_id: "a".into(),
            kind: TransitionType::FadeToBlack,
            duration_us: 9_000_000,
        },
    );
    let t = d
        .transitions
        .iter()
        .find(|t| t.from_slot_id == "a")
        .unwrap();
    // Half of the shorter neighbour, 4s.
    assert_eq!(t.duration_us, Micros::from_secs(2));
    ok(
        &mut d,
        Op::Transition {
            from_slot_id: "a".into(),
            kind: TransitionType::Cut,
            duration_us: 0,
        },
    );
    assert!(d.transitions.iter().all(|t| t.from_slot_id != "a"));
    assert!(matches!(
        apply(
            &mut d,
            &graph(),
            &Op::Transition {
                from_slot_id: "c".into(),
                kind: TransitionType::Xfade,
                duration_us: 500_000
            }
        ),
        Err(EditError::NoNextSlot(_))
    ));
    ok(
        &mut d,
        Op::TransitionAll {
            kind: TransitionType::Xfade,
            duration_us: 300_000,
        },
    );
    assert_eq!(d.transitions.len(), 2);
    assert!(total(&d) < Micros::from_secs(14));
}

#[test]
fn a_placement_is_one_held_keyframe_and_fitted_is_none() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Transform {
            slot_id: "a".into(),
            scale: 1.5,
            x: 0.1,
            y: -0.05,
            rotation_deg: 370.0,
        },
    );
    let item = d.tracks.video.iter().find(|v| v.slot_id == "a").unwrap();
    let k = item.transform.as_ref().unwrap().keyframes[0];
    assert_eq!(item.transform.as_ref().unwrap().keyframes.len(), 1);
    assert!((k.rotation_deg - 10.0).abs() < 1e-9);
    ok(
        &mut d,
        Op::Transform {
            slot_id: "a".into(),
            scale: 1.0,
            x: 0.0,
            y: 0.0,
            rotation_deg: 0.0,
        },
    );
    assert!(d.tracks.video.is_empty(), "a fitted picture needs no item");
}

#[test]
fn adjustments_and_backgrounds_apply_to_one_clip_or_all() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Adjust {
            slot_id: "a".into(),
            brightness: 0.1,
            contrast: 1.0,
            saturation: 1.2,
            all: false,
        },
    );
    assert_eq!(d.tracks.video.len(), 1);
    ok(
        &mut d,
        Op::Adjust {
            slot_id: "a".into(),
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            all: true,
        },
    );
    assert!(d.tracks.video.is_empty(), "neutral everywhere is nothing");
    ok(
        &mut d,
        Op::Background {
            slot_id: None,
            background: Some(vb_contracts::editing_dsl::Background {
                kind: BackgroundKind::Blur,
                color: None,
            }),
        },
    );
    assert_eq!(d.tracks.video.len(), 3);
    ok(
        &mut d,
        Op::Background {
            slot_id: Some("b".into()),
            background: None,
        },
    );
    assert_eq!(d.tracks.video.len(), 2);
}

#[test]
fn a_keyed_placement_is_cut_with_its_clip() {
    let mut d = cut();
    d.tracks.video.push(vb_contracts::editing_dsl::VideoItem {
        transform: Some(Transform {
            keyframes: vec![
                TransformKeyframe {
                    scale: 1.0,
                    ..TransformKeyframe::fitted(Micros::ZERO)
                },
                TransformKeyframe {
                    scale: 2.0,
                    ..TransformKeyframe::fitted(Micros::from_secs(4))
                },
            ],
        }),
        ..vb_contracts::editing_dsl::VideoItem::plain("a")
    });
    ok(
        &mut d,
        Op::Split {
            slot_id: None,
            at_us: 2_000_000,
        },
    );
    let first = d.tracks.video.iter().find(|v| v.slot_id == "a").unwrap();
    let ks = &first.transform.as_ref().unwrap().keyframes;
    assert!(
        (ks.last().unwrap().scale - 1.5).abs() < 1e-9,
        "eased halfway at the cut"
    );
    let second = d.tracks.video.iter().find(|v| v.slot_id != "a").unwrap();
    let ks = &second.transform.as_ref().unwrap().keyframes;
    assert!((ks[0].scale - 1.5).abs() < 1e-9 && (ks.last().unwrap().scale - 2.0).abs() < 1e-9);
}

#[test]
fn footage_can_be_added_from_the_project_but_not_invented() {
    let mut d = cut();
    let sel = ok(
        &mut d,
        Op::Insert {
            source_id: "v2".into(),
            in_us: 0,
            out_us: 2_000_000,
            at_index: Some(0),
        },
    );
    assert_eq!(sel, Some(Selection::Slot(d.ordered_slots()[0].id.clone())));
    assert_eq!(total(&d), Micros::from_millis(15_500));
    assert!(matches!(
        apply(
            &mut d,
            &graph(),
            &Op::Insert {
                source_id: "nope".into(),
                in_us: 0,
                out_us: 1_000_000,
                at_index: None
            }
        ),
        Err(EditError::NoSuchSource(_))
    ));
    assert!(matches!(
        apply(
            &mut d,
            &graph(),
            &Op::Insert {
                source_id: "v2".into(),
                in_us: 29_000_000,
                out_us: 31_000_000,
                at_index: None
            }
        ),
        Err(EditError::OutsideFootage(_))
    ));
    let sel = ok(
        &mut d,
        Op::Duplicate {
            slot_id: "c".into(),
        },
    );
    let Some(Selection::Slot(copy)) = sel else {
        panic!()
    };
    assert_eq!(
        d.slot(&copy).unwrap().source_ref,
        d.slot("c").unwrap().source_ref
    );
}

#[test]
fn text_is_added_where_the_playhead_is_and_moves_by_its_place_on_screen() {
    let mut d = cut();
    let sel = ok(
        &mut d,
        Op::AddText {
            at_us: 9_000_000,
            content: "终点".into(),
            duration_us: None,
            role: None,
            style: None,
        },
    );
    let Some(Selection::Text(id)) = sel else {
        panic!()
    };
    let t = d.tracks.text.iter().find(|t| t.id == id).unwrap();
    // 9s is 1.5s into c, which starts at 7.5s after the dissolve.
    assert_eq!(t.anchor.slot_id, "c");
    assert_eq!(t.anchor.offset_us, Some(1_500_000));
    assert_eq!(t.duration_us, DEFAULT_TEXT);
    ok(
        &mut d,
        Op::MoveText {
            id: id.clone(),
            at_us: 1_000_000,
        },
    );
    let t = d.tracks.text.iter().find(|t| t.id == id).unwrap();
    assert_eq!(
        (t.anchor.slot_id.as_str(), t.anchor.offset_us),
        ("a", Some(1_000_000))
    );
    ok(
        &mut d,
        Op::UpdateText {
            id: id.clone(),
            content: Some("起点".into()),
            style: None,
            duration_us: Some(1_000_000),
        },
    );
    assert_eq!(
        d.tracks.text.iter().find(|t| t.id == id).unwrap().content,
        "起点"
    );
    assert_eq!(
        apply(
            &mut d,
            &graph(),
            &Op::UpdateText {
                id: id.clone(),
                content: Some("  ".into()),
                style: None,
                duration_us: None
            }
        ),
        Err(EditError::EmptyText)
    );
    ok(&mut d, Op::DeleteText { id: id.clone() });
    assert!(d.tracks.text.iter().all(|t| t.id != id));
}

#[test]
fn text_never_runs_past_the_end() {
    let mut d = cut();
    ok(
        &mut d,
        Op::AddText {
            at_us: 13_000_000,
            content: "x".into(),
            duration_us: Some(10_000_000),
            role: None,
            style: None,
        },
    );
    let t = d.tracks.text.last().unwrap();
    let start = d.resolve_timeline().resolve_anchor(&t.anchor).unwrap();
    assert!(start + t.duration_us <= total(&d));
}

#[test]
fn music_set_covers_the_cut_and_loops_when_asked() {
    let mut d = cut();
    ok(
        &mut d,
        Op::SetMusic {
            path: "/m/short.mp3".into(),
            asset_duration_us: 5_000_000,
            looped: false,
            gain_db: None,
        },
    );
    let bgm: Vec<_> = d
        .tracks
        .audio
        .iter()
        .filter(|a| a.kind == AudioKind::Bgm)
        .collect();
    assert_eq!(bgm.len(), 1);
    assert_eq!(bgm[0].duration_us, Some(Micros::from_secs(5)));
    ok(
        &mut d,
        Op::SetMusic {
            path: "/m/short.mp3".into(),
            asset_duration_us: 5_000_000,
            looped: true,
            gain_db: Some(-3.0),
        },
    );
    let bgm: Vec<_> = d
        .tracks
        .audio
        .iter()
        .filter(|a| a.kind == AudioKind::Bgm)
        .collect();
    assert_eq!(bgm.len(), 3, "13.5s of cut is three passes of 5s");
    let covered: u64 = bgm.iter().map(|a| a.duration_us.unwrap().0).sum();
    assert_eq!(Micros(covered), total(&d));
    let id = bgm[0].id.clone();
    ok(
        &mut d,
        Op::UpdateMusic {
            id: id.clone(),
            gain_db: Some(-12.0),
            muted: None,
            fade_in_us: Some(1_000_000),
            fade_out_us: None,
            asset_in_us: None,
            at_us: Some(2_000_000),
            duration_us: None,
        },
    );
    let moved = d.tracks.audio.iter().find(|a| a.id == id).unwrap();
    assert_eq!(
        d.resolve_timeline().resolve_anchor(&moved.anchor),
        Some(Micros::from_secs(2))
    );
    assert_eq!(moved.gain_db, Some(-12.0));
    ok(&mut d, Op::DeleteMusic { id: None });
    assert!(d.tracks.audio.iter().all(|a| a.kind == AudioKind::Source));
}

#[test]
fn the_frame_and_the_title_change_by_name() {
    let mut d = cut();
    ok(
        &mut d,
        Op::Canvas {
            aspect_ratio: "9:16".into(),
        },
    );
    assert_eq!(d.meta.aspect_ratio.as_deref(), Some("9:16"));
    assert!(
        apply(
            &mut d,
            &graph(),
            &Op::Canvas {
                aspect_ratio: "wide".into()
            }
        )
        .is_err()
    );
    d.tracks.text.push(TextItem {
        role: Some(TextRole::Title),
        content: "trip".into(),
        ..text("t", "a", 0, 1000)
    });
    ok(
        &mut d,
        Op::Rename {
            title: "新疆".into(),
        },
    );
    assert_eq!(d.meta.title, "新疆");
    assert_eq!(
        d.tracks.text.iter().find(|t| t.id == "t").unwrap().content,
        "新疆"
    );
}

#[test]
fn change_ratio_counts_the_footage_changed_not_the_order() {
    let ai = cut();
    let mut d = ai.clone();
    assert_eq!(change_ratio(&ai, &d), 0.0);
    ok(
        &mut d,
        Op::Move {
            slot_id: "c".into(),
            to_index: 0,
        },
    );
    assert_eq!(change_ratio(&ai, &d), 0.0);
    // 14s of footage; 6s of c gone is 6/14 changed.
    ok(
        &mut d,
        Op::Delete {
            slot_id: "c".into(),
        },
    );
    assert!((change_ratio(&ai, &d) - 6.0 / 14.0).abs() < 1e-9);
    // 14s of other footage added: half of 28 shown is new.
    let mut e = ai.clone();
    ok(
        &mut e,
        Op::Insert {
            source_id: "v2".into(),
            in_us: 15_000_000,
            out_us: 29_000_000,
            at_index: None,
        },
    );
    assert!((change_ratio(&ai, &e) - 0.5).abs() < 1e-9);
}

#[test]
fn every_edit_leaves_grounding_to_be_written_afresh() {
    let mut d = cut();
    d.grounding
        .push(vb_contracts::editing_dsl::GroundingRecord {
            slot_id: "a".into(),
            status: vb_contracts::editing_dsl::GroundingStatus::Verified,
            resolved_shot_id: None,
            resolved_entity_ids: vec![],
            rejection_reason: None,
        });
    ok(&mut d, Op::Rename { title: "x".into() });
    assert!(d.grounding.is_empty());
}
