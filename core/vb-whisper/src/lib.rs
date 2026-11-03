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

//! Speech recognition on the device, with whisper.cpp.
//!
//! This is the first real model behind an adapter, and the reason the model
//! library exists: the weights are downloaded once into the library and every
//! project after that transcribes with them, offline.
//!
//! The audio comes from the same media backend the rest of the pipeline uses,
//! so the linked-libav build and the CLI build hear the same thing, and a file
//! that analyses on one transcribes on the other.
//!
//! What it produces is real and imperfect. Timestamps are whisper's segment
//! boundaries, which drift by a few hundred milliseconds on fast speech;
//! segments whisper itself thinks are probably not speech are dropped rather
//! than turned into subtitles, because a subtitle reading "[Music]" over a
//! holiday montage is worse than none.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use vb_contracts::Micros;
use vb_infer::{Adapter, AsrAdapter, InferError, Utterance};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// The rate whisper.cpp expects its input at. Anything else transcribes as
/// chipmunks or molasses.
const WHISPER_RATE: u32 = 16_000;

/// Segments whisper rates as probably not speech. Above this they are dropped.
const NO_SPEECH_LIMIT: f32 = 0.6;

/// How much audio is transcribed from one file. An hour-long clip is not a
/// clip anyone subtitles in full from a rough cut, and the cap keeps a single
/// long file from holding a run for the length of the file.
const MAX_AUDIO: Micros = Micros(30 * 60 * 1_000_000);

pub struct WhisperAsr {
    model: PathBuf,
    /// Loaded on first use and kept, because loading takes longer than
    /// transcribing a short clip and a project is several clips.
    context: OnceLock<Result<WhisperContext, String>>,
    /// whisper.cpp states are not safe to share; one transcription at a time.
    busy: Mutex<()>,
    /// A language to force, or `None` to let whisper detect it. Detection is
    /// right for a holiday with speech in two languages; forcing is right for
    /// a user who knows and has short clips that detection guesses wrong.
    pub language: Option<String>,
    /// Write Chinese in simplified characters. On by default because the
    /// editor this exports to first is the mainland one; a user in Taiwan or
    /// Hong Kong turns it off.
    pub simplified_chinese: bool,
}

impl WhisperAsr {
    pub fn new(model: impl Into<PathBuf>) -> WhisperAsr {
        WhisperAsr {
            model: model.into(),
            context: OnceLock::new(),
            busy: Mutex::new(()),
            language: None,
            simplified_chinese: true,
        }
    }

    pub fn model_path(&self) -> &Path {
        &self.model
    }

    fn context(&self) -> Result<&WhisperContext, InferError> {
        self.context
            .get_or_init(|| {
                // whisper.cpp logs every layer it loads to stderr. The app has
                // no terminal and the CLI user did not ask, so route it
                // through tracing instead.
                whisper_rs::install_logging_hooks();
                let path = self
                    .model
                    .to_str()
                    .ok_or_else(|| format!("{} is not valid UTF-8", self.model.display()))?;
                WhisperContext::new_with_params(path, WhisperContextParameters::default())
                    .map_err(|e| format!("could not load {}: {e}", self.model.display()))
            })
            .as_ref()
            .map_err(|reason| InferError::Unavailable {
                adapter: "WhisperCpp",
                reason: reason.clone(),
            })
    }
}

impl Adapter for WhisperAsr {
    fn name(&self) -> &'static str {
        "WhisperCpp"
    }
}

impl AsrAdapter for WhisperAsr {
    fn transcribe(&self, path: &Path) -> Result<Vec<Utterance>, InferError> {
        let media = vb_media::default_backend();
        let audio = match media.decode_audio(path, WHISPER_RATE, Some(MAX_AUDIO)) {
            Ok(audio) => audio,
            // A clip with no sound has nothing to say, which is not an error.
            Err(vb_media::MediaError::NoAudioStream) => return Ok(Vec::new()),
            Err(e) => {
                return Err(InferError::Failed {
                    adapter: "WhisperCpp",
                    detail: e.to_string(),
                });
            }
        };
        // Under a second is not worth loading a model for, and whisper
        // hallucinates freely on near-empty input.
        if audio.samples.len() < WHISPER_RATE as usize {
            return Ok(Vec::new());
        }

        let context = self.context()?;
        let _one_at_a_time = self.busy.lock().map_err(|_| InferError::Failed {
            adapter: "WhisperCpp",
            detail: "a previous transcription panicked".into(),
        })?;
        let mut state = context.create_state().map_err(|e| InferError::Failed {
            adapter: "WhisperCpp",
            detail: e.to_string(),
        })?;

        // The language is settled before transcribing rather than left to
        // whisper, for one reason: Chinese. Left alone, whisper writes it in
        // traditional characters about as often as simplified, and a subtitle
        // track in the wrong script is one a mainland user has to retype. A
        // prompt in simplified Chinese steers it, but a prompt in any one
        // language would also pull a clip in another language towards it --
        // so the prompt is given only once the audio is known to be Chinese.
        let language = match self.language.clone() {
            Some(forced) => forced,
            None => detect_language(&mut state, &audio.samples).unwrap_or_else(|| "auto".into()),
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(threads());
        params.set_language(Some(&language));
        if language == "zh" {
            params.set_initial_prompt(SIMPLIFIED_CHINESE_PROMPT);
        }
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        // Shorter segments make better subtitles: a line a viewer can read
        // before the cut takes it away.
        params.set_max_len(42);
        params.set_split_on_word(true);
        params.set_suppress_blank(true);

        state
            .full(params, &audio.samples)
            .map_err(|e| InferError::Failed {
                adapter: "WhisperCpp",
                detail: e.to_string(),
            })?;

        let language = whisper_rs::get_lang_str(state.full_lang_id_from_state())
            .map(str::to_string)
            .or(Some(language));
        let offset = audio.start;

        let mut utterances = Vec::new();
        for segment in state.as_iter() {
            if segment.no_speech_probability() > NO_SPEECH_LIMIT {
                continue;
            }
            let text = match segment.to_str_lossy() {
                Ok(text) => text.trim().to_string(),
                Err(_) => continue,
            };
            // The prompt above steers larger models; small ones still write
            // traditional characters, so the script is also settled here,
            // deterministically, from OpenCC's tables.
            let text = if self.simplified_chinese && language.as_deref() == Some("zh") {
                fast2s::convert(&text)
            } else {
                text
            };
            if text.is_empty() || is_annotation(&text) {
                continue;
            }
            // Centiseconds from whisper, microseconds everywhere else.
            let start = offset + Micros(segment.start_timestamp().max(0) as u64 * 10_000);
            let end = offset + Micros(segment.end_timestamp().max(0) as u64 * 10_000);
            if end <= start {
                continue;
            }
            utterances.push(Utterance {
                start,
                end,
                text,
                speaker: None,
                language: language.clone(),
                confidence: Some(f64::from(1.0 - segment.no_speech_probability())),
            });
        }
        Ok(utterances)
    }
}

/// Steers whisper to simplified characters. It reads as a transcript's own
/// opening line ("what follows is Mandarin, written in simplified Chinese"),
/// which is the form of prompt whisper was trained to continue.
const SIMPLIFIED_CHINESE_PROMPT: &str = "以下是普通话的句子，使用简体中文。";

/// The language of the first thirty seconds, from one encoder pass.
///
/// Far cheaper than a transcription, and enough: a clip that switches
/// language after half a minute is rare in home footage, and the fallback is
/// whisper's own detection, not a wrong answer.
fn detect_language(state: &mut whisper_rs::WhisperState, samples: &[f32]) -> Option<String> {
    let head = &samples[..samples.len().min(30 * WHISPER_RATE as usize)];
    let threads = threads() as usize;
    state.pcm_to_mel(head, threads).ok()?;
    let (id, _) = state.lang_detect(0, threads).ok()?;
    whisper_rs::get_lang_str(id).map(str::to_string)
}

/// Whisper's way of saying "not speech": `[Music]`, `(applause)`, `♪`. Kept
/// out of subtitles, where they read as the software narrating the soundtrack.
fn is_annotation(text: &str) -> bool {
    let trimmed = text.trim();
    let bracketed = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'));
    let only_symbols = trimmed
        .chars()
        .all(|c| !c.is_alphanumeric() || matches!(c, '♪' | '♫'));
    bracketed || only_symbols
}

/// Most of the machine, and never all of it: a run happens while the user is
/// using the computer.
fn threads() -> i32 {
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    available.saturating_sub(2).clamp(1, 8) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_are_not_subtitles() {
        assert!(is_annotation("[Music]"));
        assert!(is_annotation("(applause)"));
        assert!(is_annotation(" ♪ ♪ "));
        assert!(!is_annotation("Let's go to the beach"));
        assert!(!is_annotation("我们出发吧"));
    }

    #[test]
    fn the_script_converter_turns_traditional_into_simplified() {
        // What whisper's base model actually wrote for a simplified-Chinese
        // voice, and what a mainland subtitle track needs instead.
        assert_eq!(
            fast2s::convert("我們明天早上出發去海邊,天氣很好,大家都很開心。"),
            "我们明天早上出发去海边,天气很好,大家都很开心。"
        );
        // Latin text passes through untouched.
        assert_eq!(fast2s::convert("Good morning"), "Good morning");
    }

    #[test]
    fn a_missing_model_is_unavailable_rather_than_a_crash() {
        let asr = WhisperAsr::new("/definitely/not/a/model.bin");
        let Err(err) = asr.context() else {
            panic!("there is no model there, so loading must fail");
        };
        assert!(matches!(err, InferError::Unavailable { .. }), "got {err}");
        assert!(!asr.is_mock());
        assert_eq!(asr.name(), "WhisperCpp");
    }

    #[test]
    fn a_clip_without_sound_has_nothing_to_say() {
        // The fixture has no audio track; that is an empty transcript, not an
        // error the pipeline would warn about.
        let clip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-media/sample.mp4");
        if !clip.exists() || !vb_media::default_backend().is_available() {
            return;
        }
        let asr = WhisperAsr::new("/definitely/not/a/model.bin");
        assert!(asr.transcribe(&clip).unwrap().is_empty());
    }

    #[test]
    fn a_real_model_transcribes_real_speech() {
        // Needs a downloaded model and a clip with speech in it, which CI has
        // neither of, so it runs only when both are named.
        let (Ok(model), Ok(clip)) = (
            std::env::var("VB_WHISPER_MODEL"),
            std::env::var("VB_WHISPER_CLIP"),
        ) else {
            return;
        };
        let utterances = WhisperAsr::new(model)
            .transcribe(Path::new(&clip))
            .expect("transcription runs");
        println!("{utterances:#?}");
        assert!(!utterances.is_empty(), "no speech was found in {clip}");
        for pair in utterances.windows(2) {
            assert!(pair[0].start <= pair[1].start, "segments out of order");
        }
    }
}
