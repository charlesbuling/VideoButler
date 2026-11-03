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

//! The finished piece's sound: each clip's own, at its speed with its
//! pitch kept, crossfaded where a transition joins two clips; and the
//! music, with its fades, ducked under speech when the cut asks.
//!
//! Mixed a video frame's worth at a time, in order, each voice read
//! straight through from where it starts, so a piece of any length mixes
//! in a few frames' memory.

use crate::layers::{Content, Layout};
use std::path::PathBuf;
use vb_contracts::Micros;
use vb_contracts::editing_dsl::TransitionType;
use vb_media::{MediaBackend, SampleReader};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

/// A cut with no transition still fades each clip's sound in and out over
/// this long, so the join does not click.
const EDGE: Micros = Micros(10_000);

/// The shape of a fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ramp {
    /// Straight from silence to full over the fade.
    Linear,
    /// Silent for the first half, then straight up: the sound half of a
    /// fade through black.
    SecondHalf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Clip,
    Music,
}

struct Voice {
    role: Role,
    path: PathBuf,
    /// Where it starts and ends in the piece, in samples.
    start: u64,
    end: u64,
    /// Where its reading starts in the file, and stops.
    from: Micros,
    to: Option<Micros>,
    tempo: f64,
    gain: f32,
    fade_in: (u64, Ramp),
    fade_out: (u64, Ramp),
    reader: Option<Box<dyn SampleReader>>,
    /// The next sample of the piece this voice has read up to.
    pos: u64,
    failed: bool,
    duck: Option<(f32, f32)>,
}

impl Voice {
    fn envelope(&self, s: u64) -> f32 {
        let ramp = |into: u64, (len, shape): (u64, Ramp)| -> f32 {
            if len == 0 || into >= len {
                return 1.0;
            }
            let p = into as f32 / len as f32;
            match shape {
                Ramp::Linear => p,
                Ramp::SecondHalf => ((p - 0.5) * 2.0).max(0.0),
            }
        };
        ramp(s - self.start, self.fade_in) * ramp(self.end - 1 - s, self.fade_out)
    }
}

/// What a voice could not do, said once.
pub struct Mixer<'a> {
    media: &'a dyn MediaBackend,
    voices: Vec<Voice>,
    /// How far down the music is ducked now, as a gain.
    duck_level: f32,
    pub warnings: Vec<String>,
}

pub(crate) fn samples_at(t: Micros) -> u64 {
    (t.0 as u128 * SAMPLE_RATE as u128 / 1_000_000) as u64
}

fn ramp_for(transition: Option<(TransitionType, Micros)>) -> (u64, Ramp) {
    match transition {
        Some((TransitionType::FadeToBlack, overlap)) => (samples_at(overlap), Ramp::SecondHalf),
        Some((TransitionType::Cut, _)) | None => (samples_at(EDGE), Ramp::Linear),
        Some((_, overlap)) => (samples_at(overlap), Ramp::Linear),
    }
}

impl<'a> Mixer<'a> {
    pub fn new(media: &'a dyn MediaBackend, layout: &Layout) -> Mixer<'a> {
        let mut voices = Vec::new();
        for layer in &layout.layers {
            let Content::Clip {
                path,
                in_us,
                out_us,
                speed,
                has_audio,
                ..
            } = &layer.content
            else {
                continue;
            };
            if !has_audio || layer.muted || layer.gain <= 0.0 {
                continue;
            }
            voices.push(Voice {
                role: Role::Clip,
                path: path.clone(),
                start: samples_at(layer.start),
                end: samples_at(layer.end),
                from: *in_us,
                to: Some(*out_us),
                tempo: *speed,
                gain: layer.gain,
                fade_in: ramp_for(layer.incoming),
                fade_out: ramp_for(layer.outgoing),
                reader: None,
                pos: samples_at(layer.start),
                failed: false,
                duck: None,
            });
        }
        for music in &layout.music {
            let end = samples_at(music.end);
            // Cut short by the end of the piece: at least an edge's fade.
            let fade_out = samples_at(music.fade_out).max(if music.end == layout.total {
                samples_at(EDGE)
            } else {
                0
            });
            voices.push(Voice {
                role: Role::Music,
                path: music.path.clone(),
                start: samples_at(music.start),
                end,
                from: music.asset_in,
                to: None,
                tempo: 1.0,
                gain: music.gain,
                fade_in: (samples_at(music.fade_in), Ramp::Linear),
                fade_out: (fade_out, Ramp::Linear),
                reader: None,
                pos: samples_at(music.start),
                failed: false,
                duck: music.ducking.map(|d| {
                    (
                        d.target_gain_db.map_or(0.25, crate::layers::db_to_gain),
                        d.threshold_db.unwrap_or(-30.0) as f32,
                    )
                }),
            });
        }
        Mixer {
            media,
            voices,
            duck_level: 1.0,
            warnings: Vec::new(),
        }
    }

    /// The piece's sound from sample `from` to `to`, interleaved.
    pub fn mix(&mut self, from: u64, to: u64) -> Vec<f32> {
        let frames = to.saturating_sub(from) as usize;
        let ch = CHANNELS as usize;
        let mut clips = vec![0f32; frames * ch];
        let mut music = vec![0f32; frames * ch];
        let mut ducked = None;
        for i in 0..self.voices.len() {
            let (start, end) = (self.voices[i].start, self.voices[i].end);
            if end <= from || start >= to || self.voices[i].failed {
                continue;
            }
            let a = from.max(start);
            let b = to.min(end);
            let Some(chunk) = self.read(i, a, b) else {
                continue;
            };
            let voice = &self.voices[i];
            if voice.role == Role::Music && voice.duck.is_some() {
                ducked = voice.duck;
            }
            let into = if voice.role == Role::Clip {
                &mut clips
            } else {
                &mut music
            };
            for (k, s) in (a..b).enumerate() {
                let g = voice.gain * voice.envelope(s);
                let o = ((s - from) as usize) * ch;
                for c in 0..ch {
                    into[o + c] += chunk.get(k * ch + c).copied().unwrap_or(0.0) * g;
                }
            }
        }
        // The music gives way to speech: down while the clips are loud,
        // back up slowly after.
        if let Some((target, threshold_db)) = ducked {
            let rms = (clips.iter().map(|v| v * v).sum::<f32>() / clips.len().max(1) as f32).sqrt();
            let db = 20.0 * rms.max(1e-9).log10();
            let want = if db > threshold_db { target } else { 1.0 };
            let rate = if want < self.duck_level { 0.5 } else { 0.1 };
            self.duck_level += (want - self.duck_level) * rate;
            for v in &mut music {
                *v *= self.duck_level;
            }
        }
        clips
            .iter()
            .zip(&music)
            .map(|(c, m)| (c + m).clamp(-1.0, 1.0))
            .collect()
    }

    /// Samples `a` to `b` of voice `i`, interleaved, opening it the first
    /// time and skipping ahead if it is behind.
    fn read(&mut self, i: usize, a: u64, b: u64) -> Option<Vec<f32>> {
        let ch = CHANNELS as usize;
        let voice = &mut self.voices[i];
        if voice.reader.is_none() {
            // Opened where this sample falls in the file, which is the
            // voice's start when the piece is played through.
            let into = Micros(((a - voice.start) as u128 * 1_000_000 / SAMPLE_RATE as u128) as u64);
            let from = voice.from + Micros((into.0 as f64 * voice.tempo).round() as u64);
            match self.media.read_samples(
                &voice.path,
                from,
                voice.to,
                SAMPLE_RATE,
                CHANNELS,
                voice.tempo,
            ) {
                Ok(reader) => {
                    voice.reader = Some(reader);
                    voice.pos = a;
                }
                Err(e) => {
                    voice.failed = true;
                    self.warnings.push(format!(
                        "the sound of {} could not be read: {e}",
                        voice.path.display()
                    ));
                    return None;
                }
            }
        }
        let reader = voice.reader.as_mut()?;
        if a > voice.pos {
            let skip = (a - voice.pos) as usize;
            let _ = reader.read(skip);
            voice.pos = a;
        }
        let want = (b - a) as usize;
        let mut out = Vec::with_capacity(want * ch);
        while out.len() < want * ch {
            match reader.read(want - out.len() / ch) {
                Ok(more) if !more.is_empty() => out.extend(more),
                Ok(_) => break,
                Err(e) => {
                    voice.failed = true;
                    self.warnings.push(format!(
                        "the sound of {} stopped early: {e}",
                        voice.path.display()
                    ));
                    break;
                }
            }
        }
        // A file shorter than its place in the piece ends in silence.
        out.resize(want * ch, 0.0);
        voice.pos = b;
        if voice.pos >= voice.end {
            voice.reader = None;
        }
        Some(out)
    }
}
