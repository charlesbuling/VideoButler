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

//! Subsystem A: the Prior Knowledge Base.
//!
//! Two things live here. **Templates** are declarative editing styles, each a
//! JSON file of parameters, added and retired without touching inference code.
//! **Mined priors** are statistics pulled from the footage itself, so a cut can
//! follow the rhythm of the material rather than a fixed rule.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use vb_contracts::footage_graph::{MinedPriors, ShotLengthPrior};
use vb_contracts::{FootageGraph, Micros};

/// What a template is for. A finished cut blends one of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateKind {
    /// Pacing and cut rhythm.
    Rhythm,
    /// Emotional tone: colour, music, transition feel.
    Emotion,
    /// Content-specific structure, e.g. travel, interview, product.
    Content,
}

/// A declarative editing style.
///
/// Deliberately data, not code. Adding a style is dropping a JSON file into
/// the prior store; nothing recompiles and no inference path changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub name: String,
    pub kind: TemplateKind,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub params: TemplateParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateParams {
    /// How long the finished piece should be.
    pub target_duration_us: Micros,
    /// Shortest a clip may be before it reads as a glitch.
    pub min_slot_us: Micros,
    /// Longest a clip may run before the pace sags.
    pub max_slot_us: Micros,
    pub transition: String,
    pub transition_duration_us: Micros,
    /// Snap cuts to the mined beat grid when one was found.
    pub beat_align: bool,
    pub title_card: bool,
    pub title_card_duration_us: Micros,
    /// Put words on screen: a caption saying what a moment shows, where a
    /// vision model has written one. What was said is transcribed onto the
    /// screen only when the template's extras ask for it
    /// (`speechSubtitles`), as an interview's do.
    pub subtitles: bool,
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for TemplateParams {
    fn default() -> Self {
        TemplateParams {
            target_duration_us: Micros::from_secs(30),
            min_slot_us: Micros::from_millis(1_500),
            max_slot_us: Micros::from_secs(6),
            transition: "cut".into(),
            transition_duration_us: Micros::ZERO,
            beat_align: false,
            title_card: false,
            title_card_duration_us: Micros::from_secs(2),
            subtitles: true,
            extra: BTreeMap::new(),
        }
    }
}

/// The template registry.
///
/// Ships with built-ins compiled in, so the pipeline works with no assets
/// directory, and loads user templates from disk on top of them.
#[derive(Debug, Clone, Default)]
pub struct PriorStore {
    templates: BTreeMap<String, Template>,
}

/// Built-in templates, embedded so an offline first run has something to cut
/// with before any asset directory exists.
const BUILTIN_TEMPLATES: &str = include_str!("../../../prior-store/builtin.json");

impl PriorStore {
    /// The registry with built-ins only.
    pub fn with_builtins() -> Self {
        let mut store = PriorStore::default();
        match serde_json::from_str::<Vec<Template>>(BUILTIN_TEMPLATES) {
            Ok(templates) => {
                for t in templates {
                    store.templates.insert(t.id.clone(), t);
                }
            }
            Err(e) => {
                // Built-ins are compiled in, so this can only mean the file was
                // edited into an invalid state. Loud, but not fatal: a user
                // template directory may still supply something usable.
                tracing::error!(error = %e, "built-in templates failed to parse");
            }
        }
        store
    }

    /// Add every `*.json` in a directory. Later files override earlier ids, so
    /// a user template can replace a built-in by reusing its id.
    pub fn load_dir(&mut self, dir: &Path) -> Result<usize, PriorError> {
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut loaded = 0;
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| PriorError::Io {
                path: dir.display().to_string(),
                source: e,
            })?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        entries.sort();

        for path in entries {
            // builtin.json is compiled in already; skip it so a checked-out
            // source tree does not load it twice.
            if path.file_name().is_some_and(|n| n == "builtin.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| PriorError::Io {
                path: path.display().to_string(),
                source: e,
            })?;
            // One malformed user template must not take down the registry.
            match serde_json::from_str::<Template>(&text) {
                Ok(t) => {
                    self.templates.insert(t.id.clone(), t);
                    loaded += 1;
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping unreadable template");
                }
            }
        }
        Ok(loaded)
    }

    pub fn get(&self, id: &str) -> Option<&Template> {
        self.templates.get(id)
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Template> {
        self.templates.values()
    }

    pub fn of_kind(&self, kind: TemplateKind) -> impl Iterator<Item = &Template> {
        self.templates.values().filter(move |t| t.kind == kind)
    }

    /// Pick a template for this footage.
    ///
    /// Speech-led material gets a template tagged `dialogue`, because cutting
    /// dialogue to a music rhythm chops sentences in half. Otherwise the
    /// mined beat grid decides between an upbeat and a calm rhythm. Falls back
    /// to any rhythm template, then to any template at all, so this always
    /// returns something when the store is non-empty.
    pub fn match_template(&self, graph: &FootageGraph) -> Option<&Template> {
        let has_speech = !graph.transcript.is_empty();
        let bpm = graph
            .priors
            .as_ref()
            .and_then(|p| p.beat_grid.as_ref())
            .map(|b| b.bpm);

        let wanted = if has_speech {
            "dialogue"
        } else if bpm.is_some_and(|b| b >= 110.0) {
            "fast"
        } else {
            "calm"
        };

        self.of_kind(TemplateKind::Rhythm)
            .find(|t| t.tags.iter().any(|tag| tag == wanted))
            .or_else(|| self.of_kind(TemplateKind::Rhythm).next())
            .or_else(|| self.templates.values().next())
    }
}

/// The emotional register a user asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mood {
    Happy,
    Sad,
    Humor,
}

impl Mood {
    /// The tag an emotion template carries to say it serves this mood.
    pub fn tag(self) -> &'static str {
        match self {
            Mood::Happy => "happy",
            Mood::Sad => "sad",
            Mood::Humor => "humor",
        }
    }
}

/// What the footage is about, which decides the cut's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Topic {
    Dance,
    Travel,
    Growth,
    /// 主体衔接: one action carried across every cut.
    Continuity,
    /// A year in review: its people, then its scenery, then its food, then
    /// the rest, each from January to December.
    Annual,
    /// The same group of people -- a couple, a family, a club -- over time.
    Together,
}

impl Topic {
    pub fn tag(self) -> &'static str {
        match self {
            Topic::Dance => "dance",
            Topic::Travel => "travel",
            Topic::Growth => "growth",
            Topic::Continuity => "continuity",
            Topic::Annual => "annual",
            Topic::Together => "together",
        }
    }
}

/// A style as a user states it: a mood, a topic, and whether cuts land on the
/// beat. Every part is optional; what is left out comes from the other parts
/// or from the footage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<Mood>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<Topic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_sync: Option<bool>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        self.mood.is_none() && self.topic.is_none() && self.beat_sync.is_none()
    }
}

impl PriorStore {
    /// The template a style asks for.
    ///
    /// Blended rather than picked, because a mood and a topic answer
    /// different questions: the topic decides the piece's structure (how
    /// long, whether it opens on a title, whether speech is subtitled), the
    /// mood decides its pacing (how long each clip holds, how clips join).
    /// The beat setting overrides both, and cuts on the beat are hard cuts,
    /// because a dissolve smears the one moment the cut was placed for.
    ///
    /// Falls back to the footage-matched template for whatever the style
    /// leaves out, so this returns `None` only for an empty store.
    pub fn template_for_style(&self, style: &Style, graph: &FootageGraph) -> Option<Template> {
        let tagged = |kind: TemplateKind, tag: &str| {
            self.of_kind(kind)
                .find(|t| t.tags.iter().any(|x| x == tag))
                .cloned()
        };
        let topic = style
            .topic
            .and_then(|t| tagged(TemplateKind::Content, t.tag()));
        let mood = style
            .mood
            .and_then(|m| tagged(TemplateKind::Emotion, m.tag()));

        let structure = match (&topic, &mood) {
            (Some(t), _) => t.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => self.match_template(graph)?.clone(),
        };
        let pacing = mood.clone().unwrap_or_else(|| structure.clone());

        let mut params = structure.params.clone();
        params.min_slot_us = pacing.params.min_slot_us;
        params.max_slot_us = pacing.params.max_slot_us;
        params.transition = pacing.params.transition.clone();
        params.transition_duration_us = pacing.params.transition_duration_us;
        // Mood extras (colour, tempo) layered over the topic's (selection),
        // so neither loses the keys only it sets.
        for (key, value) in &pacing.params.extra {
            params.extra.insert(key.clone(), value.clone());
        }
        if let Some(beat) = style.beat_sync {
            params.beat_align = beat;
        } else if mood.is_some() && topic.is_none() {
            params.beat_align = pacing.params.beat_align;
        }
        if params.beat_align {
            params.transition = "cut".into();
            params.transition_duration_us = Micros::ZERO;
        }

        let mut id = String::from("style");
        let mut name = Vec::new();
        let mut tags = Vec::new();
        if let Some(m) = style.mood {
            id.push('.');
            id.push_str(m.tag());
            name.push(pacing.name.clone());
            tags.push(m.tag().to_string());
        }
        if let Some(t) = style.topic {
            id.push('.');
            id.push_str(t.tag());
            name.push(structure.name.clone());
            tags.push(t.tag().to_string());
        }
        if style.mood.is_none() && style.topic.is_none() {
            id = structure.id.clone();
            name.push(structure.name.clone());
            tags.extend(structure.tags.iter().cloned());
        }
        if params.beat_align {
            id.push_str(".beat");
            name.push("on the beat".into());
            tags.push("beat".into());
        }

        Some(Template {
            id,
            name: name.join(", "),
            kind: if topic.is_some() {
                TemplateKind::Content
            } else if mood.is_some() {
                TemplateKind::Emotion
            } else {
                structure.kind
            },
            tags,
            params,
        })
    }
}

/// Mine statistics out of the footage.
///
/// These are the quantifiable patterns a cut can follow without any model
/// being involved: how long the source shots actually run, which is a better
/// guide to natural pacing than a fixed constant.
pub fn mine_shot_priors(graph: &FootageGraph) -> MinedPriors {
    let mut lengths: Vec<u64> = graph.shots.iter().map(|s| s.duration().0).collect();
    if lengths.is_empty() {
        return MinedPriors::default();
    }
    lengths.sort_unstable();

    let sum: u128 = lengths.iter().map(|l| *l as u128).sum();
    let mean = (sum / lengths.len() as u128) as u64;

    MinedPriors {
        shot_length: Some(ShotLengthPrior {
            mean_us: Some(Micros(mean)),
            median_us: Some(Micros(percentile(&lengths, 0.50))),
            p10_us: Some(Micros(percentile(&lengths, 0.10))),
            p90_us: Some(Micros(percentile(&lengths, 0.90))),
        }),
        beat_grid: None,
        transition_frequency: None,
    }
}

/// Nearest-rank percentile over a sorted slice.
fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Debug, thiserror::Error)]
pub enum PriorError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::Blake3Hash;
    use vb_contracts::footage_graph::{BeatGrid, Shot, Source, TranscriptSegment};

    fn graph_with_shots(durations_s: &[u64]) -> FootageGraph {
        let mut g = FootageGraph::new("test");
        g.sources.push(Source {
            id: "src".into(),
            path: "/x.mp4".into(),
            content_hash: Blake3Hash::of_str("x"),
            media_kind: None,
            duration_us: Micros::from_secs(durations_s.iter().sum::<u64>()),
            fps_num: Some(30),
            fps_den: Some(1),
            width: Some(1920),
            height: Some(1080),
            video_codec: None,
            audio_codec: None,
            has_audio: Some(false),
            created_at: None,
            rotation: None,
            categories: Vec::new(),
        });
        let mut cursor = Micros::ZERO;
        for (i, d) in durations_s.iter().enumerate() {
            let end = cursor + Micros::from_secs(*d);
            g.shots.push(Shot {
                id: format!("shot-{i}"),
                source_id: "src".into(),
                start_us: cursor,
                end_us: end,
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
            cursor = end;
        }
        g
    }

    #[test]
    fn the_builtin_registry_is_not_empty() {
        let store = PriorStore::with_builtins();
        assert!(
            !store.is_empty(),
            "built-in templates must parse, or an offline first run has nothing to cut with"
        );
    }

    #[test]
    fn every_builtin_template_has_a_sane_duration_window() {
        for t in PriorStore::with_builtins().iter() {
            assert!(
                t.params.min_slot_us < t.params.max_slot_us,
                "template {} has min >= max slot duration",
                t.id
            );
            assert!(
                t.params.target_duration_us > t.params.max_slot_us,
                "template {} targets a total shorter than one clip",
                t.id
            );
        }
    }

    #[test]
    fn speech_selects_a_dialogue_template() {
        let mut g = graph_with_shots(&[5, 5]);
        g.transcript.push(TranscriptSegment {
            id: "tr".into(),
            source_id: "src".into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(2),
            text: "hello".into(),
            speaker: None,
            language: None,
            confidence: None,
        });
        let store = PriorStore::with_builtins();
        let t = store.match_template(&g).unwrap();
        assert!(
            t.tags.iter().any(|x| x == "dialogue"),
            "speech-led footage picked {} instead of a dialogue template",
            t.id
        );
    }

    #[test]
    fn a_fast_beat_grid_selects_a_fast_template() {
        let mut g = graph_with_shots(&[5, 5]);
        g.priors = Some(MinedPriors {
            beat_grid: Some(BeatGrid {
                bpm: 128.0,
                first_beat_us: None,
                confidence: None,
                origin: None,
                asset_path: None,
                asset_duration_us: None,
            }),
            ..Default::default()
        });
        let store = PriorStore::with_builtins();
        let t = store.match_template(&g).unwrap();
        assert!(t.tags.iter().any(|x| x == "fast"), "picked {}", t.id);
    }

    #[test]
    fn mined_percentiles_describe_the_actual_shot_lengths() {
        let g = graph_with_shots(&[2, 4, 6, 8, 10]);
        let p = mine_shot_priors(&g);
        let sl = p.shot_length.unwrap();
        assert_eq!(sl.mean_us, Some(Micros::from_secs(6)));
        assert_eq!(sl.median_us, Some(Micros::from_secs(6)));
        assert_eq!(sl.p10_us, Some(Micros::from_secs(2)));
        assert_eq!(sl.p90_us, Some(Micros::from_secs(10)));
    }

    #[test]
    fn a_topic_sets_the_structure_and_a_mood_sets_the_pacing() {
        let store = PriorStore::with_builtins();
        let g = graph_with_shots(&[5, 5]);
        let style = Style {
            mood: Some(Mood::Sad),
            topic: Some(Topic::Travel),
            beat_sync: Some(false),
        };
        let t = store.template_for_style(&style, &g).unwrap();
        let travel = store.get("content.travel-diary").unwrap();
        let sad = store.get("emotion.sad-tender").unwrap();
        assert_eq!(
            t.params.target_duration_us,
            travel.params.target_duration_us
        );
        assert_eq!(t.params.title_card, travel.params.title_card);
        assert_eq!(t.params.min_slot_us, sad.params.min_slot_us);
        assert_eq!(t.params.max_slot_us, sad.params.max_slot_us);
        assert_eq!(t.params.transition, sad.params.transition);
        assert!(!t.params.beat_align);
        assert_eq!(t.id, "style.sad.travel");
    }

    #[test]
    fn cutting_on_the_beat_forces_hard_cuts() {
        // A dissolve smears the moment the cut was placed on.
        let store = PriorStore::with_builtins();
        let g = graph_with_shots(&[5, 5]);
        let style = Style {
            mood: Some(Mood::Sad),
            topic: None,
            beat_sync: Some(true),
        };
        let t = store.template_for_style(&style, &g).unwrap();
        assert!(t.params.beat_align);
        assert_eq!(t.params.transition, "cut");
        assert_eq!(t.params.transition_duration_us, Micros::ZERO);
        assert!(t.id.ends_with(".beat"));
    }

    #[test]
    fn every_mood_and_topic_has_a_template() {
        let store = PriorStore::with_builtins();
        let g = graph_with_shots(&[5, 5]);
        for mood in [Mood::Happy, Mood::Sad, Mood::Humor] {
            for topic in [
                Topic::Dance,
                Topic::Travel,
                Topic::Growth,
                Topic::Continuity,
            ] {
                let style = Style {
                    mood: Some(mood),
                    topic: Some(topic),
                    beat_sync: None,
                };
                let t = store.template_for_style(&style, &g).unwrap();
                assert!(
                    t.tags.iter().any(|x| x == mood.tag())
                        && t.tags.iter().any(|x| x == topic.tag()),
                    "{mood:?} + {topic:?} blended into {} without one of them",
                    t.id
                );
                assert!(t.params.min_slot_us < t.params.max_slot_us);
            }
        }
    }

    #[test]
    fn an_empty_style_falls_back_to_the_footage_match() {
        let store = PriorStore::with_builtins();
        let g = graph_with_shots(&[5, 5]);
        let t = store.template_for_style(&Style::default(), &g).unwrap();
        assert_eq!(t.id, store.match_template(&g).unwrap().id);
    }

    #[test]
    fn mining_empty_footage_yields_nothing_rather_than_panicking() {
        let g = FootageGraph::new("empty");
        assert!(mine_shot_priors(&g).shot_length.is_none());
    }
}
