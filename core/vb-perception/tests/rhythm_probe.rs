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

//! What the rhythm reading makes of an analysed project's poses: every
//! segment read densely enough, its tempo, how clearly, its beats, and
//! where its people stand idle, watch what they hold, or keep no beat. For
//! setting the reading against takes a person has watched:
//!
//! `VB_PROBE_GRAPH=<project.footage-graph.json> cargo test --release -p
//! vb-perception --test rhythm_probe -- --nocapture`

use vb_contracts::FootageGraph;

#[test]
fn rhythm_probe() {
    let Ok(path) = std::env::var("VB_PROBE_GRAPH") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("a footage graph");
    let graph: FootageGraph = serde_json::from_str(&text).expect("a footage graph");
    for shot in &graph.shots {
        let source = graph.source(&shot.source_id).expect("its source");
        let name = std::path::Path::new(&source.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let aspect = source
            .display_size()
            .map(|(w, h)| w as f64 / h.max(1) as f64)
            .unwrap_or(16.0 / 9.0);
        for segment in &shot.segments {
            if segment.poses.len() < 60 {
                continue;
            }
            let span = segment.poses.last().unwrap().at_us.as_secs_f64()
                - segment.poses[0].at_us.as_secs_f64();
            let Some(r) = vb_perception::rhythm::read(&segment.poses, aspect) else {
                println!("{name:24} {span:6.1}s  no reading");
                continue;
            };
            let total = |spans: &[vb_contracts::footage_graph::Span]| -> f64 {
                spans
                    .iter()
                    .map(|s| (s.end_us.0 - s.start_us.0) as f64 / 1e6)
                    .sum::<f64>()
                    + 0.0
            };
            let gaps: Vec<f64> = r
                .beats_us
                .windows(2)
                .map(|w| (w[1].0 - w[0].0) as f64 / 1e6)
                .collect();
            let regular = if gaps.is_empty() {
                0.0
            } else {
                let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
                (gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len() as f64).sqrt()
            };
            let moving = vb_perception::rhythm::movement_of(&segment.poses, aspect);
            let mut values: Vec<f64> = moving.iter().map(|m| m.1).collect();
            values.sort_by(f64::total_cmp);
            let q = |f: f64| values[((values.len() - 1) as f64 * f) as usize];
            let beat = if r.bpm > 0.0 { 60.0 / r.bpm } else { 0.6 };
            let idle_at = |still: f64| -> f64 {
                let (mut total, mut open) = (0.0, None::<f64>);
                for w in moving.windows(2) {
                    if w[0].1 < still {
                        open.get_or_insert(w[0].0);
                    } else if let Some(a) = open.take()
                        && w[0].0 - a > 1.5 * beat
                    {
                        total += w[0].0 - a;
                    }
                }
                100.0 * total / span.max(1e-6)
            };
            println!(
                "{name:24}         movement p10 {:.2} p25 {:.2} p50 {:.2}; idle share at <0.2 {:4.1}%  <0.3 {:4.1}%  <0.4 {:4.1}%  <0.5 {:4.1}%",
                q(0.1),
                q(0.25),
                q(0.5),
                idle_at(0.2),
                idle_at(0.3),
                idle_at(0.4),
                idle_at(0.5)
            );
            println!(
                "{name:24} {span:6.1}s  bpm {:6.1}  clarity {:.2}  beats {:3} (gap sd {:.3}s)  idle {:5.1}s  watching {:5.1}s  unrhythmic {:5.1}s",
                r.bpm,
                r.confidence.unwrap_or(0.0),
                r.beats_us.len(),
                regular,
                total(&r.idle),
                total(&r.watching),
                total(&r.unrhythmic)
            );
            let at = |spans: &[vb_contracts::footage_graph::Span]| -> String {
                spans
                    .iter()
                    .map(|s| {
                        format!(
                            "{:.1}-{:.1}",
                            s.start_us.as_secs_f64(),
                            s.end_us.as_secs_f64()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!("{name:24}         idle {}", at(&r.idle));
            println!("{name:24}         watching {}", at(&r.watching));
        }
    }
}
