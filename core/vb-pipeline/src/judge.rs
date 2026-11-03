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

//! The composition judge: a second look at the moments a cut might use.
//!
//! The composition model takes seconds a frame -- slow beside the rest of
//! perception -- so it never sees every segment. After a first cut, the
//! moments it could have used are shortlisted: every clip it did use, and the
//! best segment of each of the strongest scenes, three times as many as the
//! cut holds. The judge rates their keyframes, the ratings go into the graph,
//! and the cut is made again. This is the development doc's second pass
//! (§6.1): the cheap readings choose what the expensive model looks at.
//!
//! Ratings are cached by the clip's content, the moment and the model, so a
//! second run of the same footage asks the model nothing.

use crate::Cache;
use std::collections::HashMap;
use std::path::Path;
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros};
use vb_infer::{VlmAdapter, VlmRequest};
use vb_media::MediaBackend;

/// How many moments the judge looks at, as a multiple of the clips in the
/// first cut, never fewer than [`SHORTLIST_MIN`] and never more than
/// [`SHORTLIST_MAX`]: at three seconds a frame, a long cut's full shortlist
/// took seven minutes, most of them on moments ranked too low to be used.
pub const SHORTLIST_FACTOR: usize = 3;
pub const SHORTLIST_MIN: usize = 24;
pub const SHORTLIST_MAX: usize = 60;

/// PhotoFramer's composition prompt, word for word as its authors evaluated
/// it: the model was trained to this wording and answers best to it.
pub const PROMPT: &str = "How would you rate the composition of this image? First output the \
    thinking process in <think> </think> tags and then output the final answer in <answer> \
    </answer> tags. The final answer should be a float between 1 and 5, rounded to two decimal \
    places, with 1 representing very poor quality and 5 representing excellent quality. Return \
    the final answer in JSON format with the following keys: \"rating\": The score.";

/// Enough for the model's reasoning and its answer.
const MAX_TOKENS: u32 = 320;

/// Part of every cached rating's key. Change it whenever a rating of the same
/// frame by the same model could come out differently: the prompt, how the
/// frame is read, how the answer is parsed.
const RATING_VERSION: &str = "photoframer-prompt-1;image-tokens-1024;upright-stills";

/// A 1-5 rating from the model's answer: the one in its `<answer>` when it
/// gave one, otherwise the last number in range it wrote.
pub fn parse_rating(text: &str) -> Option<f64> {
    let answer = text
        .split_once("<answer>")
        .map(|(_, rest)| rest.split("</answer>").next().unwrap_or(rest))
        .unwrap_or(text);
    let numbers: Vec<f64> = answer
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter_map(|s| s.trim_matches('.').parse::<f64>().ok())
        .filter(|v| (1.0..=5.0).contains(v))
        .collect();
    numbers.last().copied()
}

/// The segments the judge should look at, as (shot, segment) indices.
///
/// Every segment the first cut used, then the best segment of each scene,
/// strongest scenes first, until there are `size`.
pub fn shortlist(graph: &FootageGraph, cut: &EditingDsl, size: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for slot in &cut.slots {
        let Some(r) = &slot.source_ref else { continue };
        let Some(shot) = graph
            .shots
            .iter()
            .position(|s| Some(&s.id) == r.shot_id.as_ref())
        else {
            continue;
        };
        if let Some(seg) = graph.shots[shot].segments.iter().position(|g| {
            g.usable()
                .is_some_and(|(from, to)| r.in_us >= from && r.in_us < to)
        }) && !out.contains(&(shot, seg))
        {
            out.push((shot, seg));
        }
    }

    // The best usable segment of each scene, by the readings the first cut
    // went on.
    let mut best: HashMap<String, ((usize, usize), f64)> = HashMap::new();
    for (i, shot) in graph.shots.iter().enumerate() {
        for (j, seg) in shot.segments.iter().enumerate() {
            if seg.usable().is_none() {
                continue;
            }
            let clear = seg
                .occlusion
                .map_or(1.0, |o| (1.0 - 2.0 * o).max(0.0).powi(2));
            let value = seg.quality.unwrap_or(0.0) * clear;
            let scene = seg.scene_id.clone().unwrap_or_else(|| seg.id.clone());
            let entry = best.entry(scene).or_insert(((i, j), value));
            if value > entry.1 {
                *entry = ((i, j), value);
            }
        }
    }
    let mut scenes: Vec<((usize, usize), f64)> = best.into_values().collect();
    scenes.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (at, _) in scenes {
        if out.len() >= size {
            break;
        }
        if !out.contains(&at) {
            out.push(at);
        }
    }
    out
}

/// What a pass of the judge did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Judged {
    /// Ratings the model gave in this run.
    pub asked: usize,
    /// Ratings served from the cache.
    pub remembered: usize,
    /// Moments it could not rate: no still, or an answer with no rating.
    pub failed: usize,
}

/// Rate the shortlisted segments' keyframes, writing each rating into the
/// graph as `composition` (0..1).
#[allow(clippy::too_many_arguments)]
pub fn judge(
    graph: &mut FootageGraph,
    list: &[(usize, usize)],
    vlm: &dyn VlmAdapter,
    media: &dyn MediaBackend,
    work_dir: &Path,
    cache: &Cache,
    progress: &dyn Fn(usize, usize),
    cancelled: &dyn Fn() -> bool,
) -> Judged {
    let mut done = Judged::default();
    for (n, &(i, j)) in list.iter().enumerate() {
        if cancelled() {
            break;
        }
        progress(n, list.len());
        let shot = &graph.shots[i];
        let seg = &shot.segments[j];
        let Some(source) = graph.sources.iter().find(|s| s.id == shot.source_id) else {
            done.failed += 1;
            continue;
        };
        let keyframe = seg
            .keyframe_id
            .as_ref()
            .and_then(|id| shot.keyframes.iter().find(|k| &k.id == id));
        let at = keyframe.map(|k| k.at_us).unwrap_or_else(|| {
            let (from, to) = seg.usable().unwrap_or((seg.start_us, seg.end_us));
            Micros((from.0 + to.0) / 2)
        });
        // By the frame itself -- the clip's content and the moment in it --
        // not the segment's id, which is numbered across the project and
        // shifts when a clip is added.
        let key = Blake3Hash::of_parts(&[
            b"composition",
            RATING_VERSION.as_bytes(),
            vlm.name().as_bytes(),
            source.content_hash.to_hex().as_bytes(),
            at.0.to_string().as_bytes(),
        ]);
        let rating = match cache.get(&key).and_then(|v| v.trim().parse::<f64>().ok()) {
            Some(rating) => {
                done.remembered += 1;
                Some(rating)
            }
            None => {
                let still = match keyframe.and_then(|k| k.path.as_ref()) {
                    Some(path) => work_dir.join(path),
                    None => {
                        let dest = work_dir.join("judge").join(format!("{}.jpg", seg.id));
                        match media.extract_still(Path::new(&source.path), at, &dest) {
                            Ok(path) => path,
                            Err(e) => {
                                tracing::warn!(segment = %seg.id, error = %e, "no still to judge");
                                done.failed += 1;
                                continue;
                            }
                        }
                    }
                };
                let request = VlmRequest {
                    image_path: still,
                    prompt: PROMPT.to_string(),
                    max_tokens: MAX_TOKENS,
                };
                match vlm.describe(&request).map(|answer| parse_rating(&answer)) {
                    Ok(Some(rating)) => {
                        done.asked += 1;
                        let _ = cache.put(&key, &rating.to_string());
                        Some(rating)
                    }
                    Ok(None) => {
                        tracing::warn!(segment = %seg.id, "the judge gave no rating");
                        done.failed += 1;
                        None
                    }
                    Err(e) => {
                        tracing::warn!(segment = %seg.id, error = %e, "the judge failed");
                        done.failed += 1;
                        None
                    }
                }
            }
        };
        if let Some(rating) = rating {
            let c = ((rating - 1.0) / 4.0).clamp(0.0, 1.0);
            graph.shots[i].segments[j].composition = Some((c * 1000.0).round() / 1000.0);
        }
    }
    progress(list.len(), list.len());
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rating_comes_from_the_answer() {
        let text = "<think>The subject sits on a third, 5 people, rated 2 by some.</think> \
                    <answer>{\"rating\": 3.25}</answer>";
        assert_eq!(parse_rating(text), Some(3.25));
        assert_eq!(parse_rating("{\"rating\": 2.00}"), Some(2.0));
        assert_eq!(parse_rating("<answer>no idea</answer>"), None);
        assert_eq!(parse_rating("<answer>{\"rating\": 7.5}</answer>"), None);
    }
}
