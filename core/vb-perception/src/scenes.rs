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

//! Grouping segments into scenes, across every source.
//!
//! A day of travel footage is a few dozen places filmed in a few hundred
//! stretches: the airport in four clips, the market street in one long walk,
//! the lake from three spots. A cut that moves from place to place needs to
//! know which stretches are the same place, whichever file they came from.
//!
//! This runs after the per-source analyses are merged, because a scene spans
//! sources and each source is analysed and cached alone. It needs no media,
//! only the segments' signatures, so it costs nothing to re-run.

use crate::timeline::view_distance;
use vb_contracts::FootageGraph;
use vb_contracts::footage_graph::Scene;

/// Average view distance under which two groups are one scene.
///
/// Set against the user's footage: the same airport hall from four clips
/// sits under 0.01 of each other, a walk down one market street under 0.03,
/// and different places from 0.035 up.
pub const SAME_SCENE: f64 = 0.03;

/// Neighbouring stretches of one take look alike for a reason: the camera
/// kept running. Their distance counts for this much less.
const CONTINUITY: f64 = 0.75;

/// How much capture time says about place. Footage from the same few
/// minutes is very likely the same place, whatever the framing did to its
/// colours; footage from hours apart is very likely not, however alike two
/// blue skies look.
fn time_factor(seconds_apart: Option<i64>) -> f64 {
    match seconds_apart.map(i64::abs) {
        Some(t) if t <= 5 * 60 => 0.7,
        Some(t) if t <= 30 * 60 => 0.85,
        Some(t) if t <= 3 * 3600 => 1.0,
        Some(_) => 1.3,
        None => 1.0,
    }
}

pub use vb_contracts::time::capture_seconds;

/// Group the graph's segments into scenes and label each segment with its
/// scene. Replaces any scenes already there.
pub fn attach_scenes(graph: &mut FootageGraph) {
    // Every segment with a signature takes part, usable or not: an unusable
    // stretch still says where the camera was.
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (s, shot) in graph.shots.iter().enumerate() {
        for (g, segment) in shot.segments.iter().enumerate() {
            if !segment.signature.is_empty() {
                items.push((s, g));
            }
        }
    }
    graph.scenes.clear();
    if items.is_empty() {
        return;
    }

    let signature = |i: usize| &graph.shots[items[i].0].segments[items[i].1].signature;
    let source = |i: usize| graph.shots[items[i].0].source_id.as_str();
    // When each stretch was filmed, where the source says when it began.
    let filmed: Vec<Option<i64>> = items
        .iter()
        .map(|&(s, g)| {
            let shot = &graph.shots[s];
            let began = graph
                .source(&shot.source_id)
                .and_then(|src| src.created_at.as_deref())
                .and_then(capture_seconds)?;
            Some(began + (shot.segments[g].start_us.0 / 1_000_000) as i64)
        })
        .collect();
    let n = items.len();

    // Pairwise distances: how the views differ, discounted for the next
    // stretch of the same take, and weighed by how far apart in time.
    let mut distance = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in i + 1..n {
            let mut d = view_distance(signature(i), signature(j));
            if j == i + 1 && source(i) == source(j) {
                d *= CONTINUITY;
            }
            let apart = match (filmed[i], filmed[j]) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            };
            d *= time_factor(apart);
            distance[i][j] = d;
            distance[j][i] = d;
        }
    }

    // Average-linkage agglomeration: repeatedly join the two closest groups
    // until none are closer than SAME_SCENE. Ties go to the earliest pair,
    // so the result does not depend on anything but the footage.
    let mut groups: Vec<Option<Vec<usize>>> = (0..n).map(|i| Some(vec![i])).collect();
    let mut linkage = distance.clone();
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for a in 0..n {
            if groups[a].is_none() {
                continue;
            }
            for b in a + 1..n {
                if groups[b].is_none() {
                    continue;
                }
                let d = linkage[a][b];
                if d < SAME_SCENE && best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((a, b, d));
                }
            }
        }
        let Some((a, b, _)) = best else { break };
        let (size_a, size_b) = (
            groups[a].as_ref().map_or(0, Vec::len) as f64,
            groups[b].as_ref().map_or(0, Vec::len) as f64,
        );
        let merged = groups[b].take().unwrap_or_default();
        if let Some(group) = groups[a].as_mut() {
            group.extend(merged);
        }
        // Lance-Williams update for average linkage.
        for k in 0..n {
            if k == a || groups[k].is_none() {
                continue;
            }
            let d = (size_a * linkage[a][k] + size_b * linkage[b][k]) / (size_a + size_b);
            linkage[a][k] = d;
            linkage[k][a] = d;
        }
    }

    // Scenes numbered in the order they first appear in the footage.
    let mut scenes: Vec<Vec<usize>> = groups.into_iter().flatten().collect();
    for group in scenes.iter_mut() {
        group.sort_unstable();
    }
    scenes.sort_by_key(|g| g[0]);

    for (k, members) in scenes.iter().enumerate() {
        let id = format!("scene-{:03}", k + 1);
        let mut segment_ids = Vec::with_capacity(members.len());
        let mut labels: Vec<String> = Vec::new();
        for &i in members {
            let (s, g) = items[i];
            let segment = &mut graph.shots[s].segments[g];
            segment.scene_id = Some(id.clone());
            segment_ids.push(segment.id.clone());
            if let Some(d) = &segment.description {
                labels.extend(d.text.split(',').map(|t| t.trim().to_string()));
            }
        }
        graph.scenes.push(Scene {
            id,
            label: common_words(&labels),
            segment_ids,
        });
    }
}

/// The words the segments' descriptions share most, three at most, for a
/// scene's name. None when nothing was described.
fn common_words(words: &[String]) -> Option<String> {
    let mut counts: Vec<(String, usize, usize)> = Vec::new();
    for (order, word) in words.iter().enumerate() {
        if word.is_empty() {
            continue;
        }
        match counts.iter_mut().find(|(w, _, _)| w == word) {
            Some(entry) => entry.1 += 1,
            None => counts.push((word.clone(), 1, order)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    let top: Vec<String> = counts.into_iter().take(3).map(|(w, _, _)| w).collect();
    (!top.is_empty()).then(|| top.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Micros;
    use vb_contracts::footage_graph::{Description, Segment, Shot};

    fn segment(id: &str, signature: Vec<f64>, text: Option<&str>) -> Segment {
        Segment {
            id: id.into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(5),
            usable_start_us: Some(Micros::from_secs(1)),
            usable_end_us: Some(Micros::from_secs(4)),
            scene_id: None,
            quality: Some(0.7),
            camera: None,
            keyframe_id: None,
            description: text.map(|t| Description {
                text: t.into(),
                model: None,
                confidence: None,
            }),
            signature,
            occlusion: None,
            composition: None,
            ..Default::default()
        }
    }

    fn shot(id: &str, source: &str, segments: Vec<Segment>) -> Shot {
        Shot {
            id: id.into(),
            source_id: source.into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(10),
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: vec![],
            actions: vec![],
            audio: None,
            tags: vec![],
            segments,
            timeline: vec![],
        }
    }

    /// A signature that is `v` everywhere: views are as alike as their `v`s.
    fn view(v: f64) -> Vec<f64> {
        vec![v; 44]
    }

    #[test]
    fn the_same_place_from_different_clips_is_one_scene() {
        let mut g = FootageGraph::new("t");
        g.shots = vec![
            shot(
                "a",
                "src-1",
                vec![segment("a0", view(0.20), Some("water, sky"))],
            ),
            shot("b", "src-2", vec![segment("b0", view(0.60), Some("sand"))]),
            shot(
                "c",
                "src-3",
                vec![segment("c0", view(0.205), Some("water, boat"))],
            ),
        ];
        attach_scenes(&mut g);
        assert_eq!(g.scenes.len(), 2);
        let scene = |s: usize| g.shots[s].segments[0].scene_id.clone().unwrap();
        assert_eq!(scene(0), scene(2), "the lake from two clips");
        assert_ne!(scene(0), scene(1));
        // Numbered in order of appearance, named for what they share.
        assert_eq!(scene(0), "scene-001");
        assert_eq!(g.scenes[0].label.as_deref(), Some("water, sky, boat"));
        assert_eq!(g.scenes[0].segment_ids, vec!["a0", "c0"]);
    }

    #[test]
    fn capture_times_parse_as_phones_write_them() {
        assert_eq!(capture_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            capture_seconds("2023-09-29T10:47:54.000000Z"),
            Some(1_695_984_474)
        );
        assert_eq!(
            capture_seconds("2023-09-29T18:47:54+08:00"),
            capture_seconds("2023-09-29T10:47:54Z")
        );
        assert_eq!(capture_seconds("yesterday"), None);
    }

    #[test]
    fn alike_views_hours_apart_stay_apart_and_minutes_apart_join() {
        let mut g = FootageGraph::new("t");
        let source = |id: &str, at: &str| vb_contracts::Source {
            id: id.into(),
            path: format!("/{id}.mp4"),
            content_hash: vb_contracts::Blake3Hash::of_str(id),
            media_kind: None,
            duration_us: Micros::from_secs(10),
            fps_num: None,
            fps_den: None,
            width: None,
            height: None,
            video_codec: None,
            audio_codec: None,
            has_audio: None,
            created_at: Some(at.into()),
            rotation: None,
            categories: Vec::new(),
        };
        g.sources = vec![
            source("src-1", "2023-10-01T10:00:00Z"),
            source("src-2", "2023-10-01T10:03:00Z"),
            source("src-3", "2023-10-03T16:00:00Z"),
        ];
        // Views 0.035 apart: a new place by colour alone, but filmed three
        // minutes after the first. The third is as far from the second as
        // the second from the first, and filmed days later.
        g.shots = vec![
            shot("a", "src-1", vec![segment("a0", view(0.300), None)]),
            shot("b", "src-2", vec![segment("b0", view(0.335), None)]),
            shot("c", "src-3", vec![segment("c0", view(0.370), None)]),
        ];
        attach_scenes(&mut g);
        let scene = |s: usize| g.shots[s].segments[0].scene_id.clone().unwrap();
        assert_eq!(scene(0), scene(1), "minutes apart, one place");
        assert_ne!(scene(1), scene(2), "days apart, another place");
    }

    #[test]
    fn every_segment_with_a_signature_gets_a_scene() {
        let mut g = FootageGraph::new("t");
        g.shots = vec![shot(
            "a",
            "src-1",
            vec![
                segment("a0", view(0.1), None),
                segment("a1", view(0.5), None),
                segment("a2", Vec::new(), None),
            ],
        )];
        attach_scenes(&mut g);
        assert!(g.shots[0].segments[0].scene_id.is_some());
        assert!(g.shots[0].segments[1].scene_id.is_some());
        assert!(
            g.shots[0].segments[2].scene_id.is_none(),
            "nothing to compare"
        );
        assert_eq!(g.scenes.len(), 2);
    }
}
