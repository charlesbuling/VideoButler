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

//! Captions: a sentence for the screen about what each moment shows.
//!
//! What people say in holiday footage is rarely worth reading back -- a
//! shout across a car park, a word lost to the wind -- and speech
//! recognition writes it down wrong more often than not. What the viewer
//! wants on screen is what the picture shows. So a general vision-language
//! model looks at the keyframe of each moment the cut gives a caption to
//! (`vb_narrative::caption_plan`) and writes one short sentence in the
//! viewer's language; the sentence goes into the graph as the segment's
//! `caption`, and the cut is composed again with it.
//!
//! Chinese comes out simplified whatever the model chose, and anything that
//! is not text a caption should carry -- quotation marks, stray symbols, a
//! second line of explanation -- is taken off. Captions are cached by the
//! clip's content, the moment, the model, the prompt and the language.

use crate::{Cache, Lang};
use std::path::Path;
use vb_contracts::footage_graph::Description;
use vb_contracts::{Blake3Hash, EditingDsl, FootageGraph, Micros};
use vb_infer::{VlmAdapter, VlmRequest};
use vb_media::MediaBackend;

/// Part of every cached caption's key: change it whenever the same frame
/// could be captioned differently -- the prompt, the clean-up.
pub const CAPTION_VERSION: &str = "caption-2";

/// Enough for one short sentence and nothing after it.
const MAX_TOKENS: u32 = 48;

/// The longest caption kept, in characters: a line a viewer reads in the
/// time a caption is up.
const LONGEST_ZH: usize = 20;
const LONGEST_EN: usize = 64;

pub fn prompt(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => {
            "这是视频里的一个画面。请用一句简洁自然的简体中文，像视频字幕那样，说出画面里正在发生什么，\
             不超过十六个字。只写这一句话本身：不要引号，不要解释，不要写画面里看不出来的内容。"
        }
        Lang::En => {
            "This is a frame from a video. In one short, natural sentence of at most ten words, \
             say what is happening in it, like an on-screen caption. Write only the sentence: no \
             quotation marks, no explanation, nothing the picture does not show."
        }
    }
}

/// The model's answer, as a caption: its first line, simplified when
/// Chinese, without quotation marks, symbols or a closing full stop, and
/// short enough to read. `None` when nothing usable is left.
pub fn clean(answer: &str, lang: Lang) -> Option<String> {
    let line = answer
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        // A model that prefixes its answer with a label.
        .trim_start_matches("字幕：")
        .trim_start_matches("字幕:")
        .trim_start_matches("Caption:")
        .trim();
    let line = match lang {
        Lang::Zh => fast2s::convert(line),
        Lang::En => line.to_string(),
    };
    let kept: String = line
        .chars()
        .filter(|c| keep(*c, lang))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = kept
        .trim_matches(|c: char| {
            matches!(
                c,
                '。' | '.' | '，' | ',' | '、' | '；' | ';' | '：' | ':' | ' ' | '-' | '—'
            )
        })
        .to_string();
    let limit = match lang {
        Lang::Zh => LONGEST_ZH,
        Lang::En => LONGEST_EN,
    };
    let short = shorten(&trimmed, limit, lang);
    (short.chars().count() >= 2).then_some(short)
}

/// Letters, digits and the punctuation a caption uses; not quotation marks,
/// brackets, emoji or symbols.
fn keep(c: char, lang: Lang) -> bool {
    if c.is_alphanumeric() {
        // Han, kana, Latin letters and digits; Latin only in moderation in
        // a Chinese caption, where it is usually a brand or a sign.
        return true;
    }
    match lang {
        Lang::Zh => matches!(c, '，' | '、' | '！' | '？' | '·' | ' ' | '。' | '：'),
        Lang::En => matches!(c, ' ' | ',' | '!' | '?' | '\'' | '-' | '.' | ':'),
    }
}

/// At most `limit` characters, cut at a word or a comma where there is one.
fn shorten(text: &str, limit: usize, lang: Lang) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let head: String = text.chars().take(limit).collect();
    let cut = match lang {
        Lang::Zh => head.rfind(['，', '、', '！', '？']),
        Lang::En => head.rfind([' ', ',']),
    };
    match cut {
        Some(at) if at > head.len() / 2 => head[..at]
            .trim_end_matches([',', '，', '、', ' '])
            .to_string(),
        _ => head,
    }
}

/// The segments the plan gives a caption to, as (shot, segment) indices.
pub fn targets(graph: &FootageGraph, cut: &EditingDsl) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for place in vb_narrative::caption_plan(cut) {
        let Some(r) = cut.slot(&place.slot_id).and_then(|s| s.source_ref.as_ref()) else {
            continue;
        };
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
    out
}

/// What a caption pass did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Captioned {
    pub asked: usize,
    pub remembered: usize,
    pub failed: usize,
}

/// Caption the targets' keyframes, writing each into the graph.
#[allow(clippy::too_many_arguments)]
pub fn caption(
    graph: &mut FootageGraph,
    list: &[(usize, usize)],
    vlm: &dyn VlmAdapter,
    media: &dyn MediaBackend,
    work_dir: &Path,
    cache: &Cache,
    lang: Lang,
    progress: &dyn Fn(usize, usize),
    cancelled: &dyn Fn() -> bool,
) -> Captioned {
    let mut done = Captioned::default();
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
        let key = Blake3Hash::of_parts(&[
            b"caption",
            CAPTION_VERSION.as_bytes(),
            vlm.name().as_bytes(),
            lang.id().as_bytes(),
            source.content_hash.to_hex().as_bytes(),
            at.0.to_string().as_bytes(),
        ]);
        let text = match cache.get(&key) {
            Some(text) => {
                done.remembered += 1;
                Some(text)
            }
            None => {
                let still = match keyframe.and_then(|k| k.path.as_ref()) {
                    Some(path) if work_dir.join(path).is_file() => work_dir.join(path),
                    _ => {
                        let dest = work_dir.join("judge").join(format!(
                            "{}-{}.jpg",
                            &source.content_hash.to_hex()[..16],
                            at.0
                        ));
                        match media.extract_still(Path::new(&source.path), at, &dest) {
                            Ok(path) => path,
                            Err(e) => {
                                tracing::warn!(segment = %seg.id, error = %e, "no still to caption");
                                done.failed += 1;
                                continue;
                            }
                        }
                    }
                };
                let request = VlmRequest {
                    image_path: still,
                    prompt: prompt(lang).to_string(),
                    max_tokens: MAX_TOKENS,
                };
                match vlm.describe(&request) {
                    Ok(answer) => match clean(&answer, lang) {
                        Some(text) => {
                            done.asked += 1;
                            let _ = cache.put(&key, &text);
                            Some(text)
                        }
                        None => {
                            tracing::warn!(segment = %seg.id, %answer, "no usable caption");
                            done.failed += 1;
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!(segment = %seg.id, error = %e, "captioning failed");
                        done.failed += 1;
                        None
                    }
                }
            }
        };
        if let Some(text) = text {
            graph.shots[i].segments[j].caption = Some(Description {
                text,
                model: Some(vlm.name().to_string()),
                confidence: None,
            });
        }
    }
    progress(list.len(), list.len());
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_comes_out_simplified_and_without_stray_marks() {
        assert_eq!(
            clean("「孩子們在超市裡挑選水果。」", Lang::Zh).as_deref(),
            Some("孩子们在超市里挑选水果")
        );
        assert_eq!(
            clean("\"一家人在海边散步\"\n这张图片展示了……", Lang::Zh).as_deref(),
            Some("一家人在海边散步")
        );
        assert_eq!(
            clean("字幕：小女孩在草地上奔跑🏃", Lang::Zh).as_deref(),
            Some("小女孩在草地上奔跑")
        );
        assert_eq!(clean("。", Lang::Zh), None);
    }

    #[test]
    fn a_long_answer_is_cut_where_a_reader_would_pause() {
        let long = clean(
            "一个穿着白色衣服的小孩在超市的肉类柜台前，好奇地看着各种各样的新鲜肉类",
            Lang::Zh,
        )
        .unwrap();
        assert!(long.chars().count() <= LONGEST_ZH, "{long}");
        assert_eq!(long, "一个穿着白色衣服的小孩在超市的肉类柜台前");

        let english = clean(
            "\"A child in a white shirt looks curiously at the meat counter of a busy supermarket, surrounded by shoppers.\"",
            Lang::En,
        )
        .unwrap();
        assert!(english.chars().count() <= LONGEST_EN, "{english}");
        assert!(!english.contains('"'));
    }
}
