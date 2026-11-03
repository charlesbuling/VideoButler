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

//! Tempo and beat phase, from audio.
//!
//! "Cut on the beat" is a request about the music, so it has to be answered by
//! listening to the music rather than by picking a number. This finds the
//! tempo and where the beats fall; the narrative layer decides what to do
//! with them.
//!
//! The method is the standard one: spectral flux gives an onset envelope,
//! autocorrelation of that envelope gives the period, and the phase is the
//! offset whose beats collect the most onset energy. No model is involved,
//! which is what lets it run offline on any device.
//!
//! What it does *not* do is claim more than it found. A grid with a low
//! confidence is reported with that confidence, and a caller that had nothing
//! to listen to must record its grid as fixed rather than detected.

use vb_contracts::Micros;

/// The rate audio is analysed at.
///
/// Onsets are a broadband event, so the top octave contributes little and
/// decoding at a low rate makes the whole analysis several times cheaper. A
/// four-minute song is about 2.6 M samples here.
pub const ANALYSIS_RATE: u32 = 11_025;

/// Samples per analysis window, and the step between them.
///
/// 512 at 11 kHz is 46 ms, long enough to resolve the low end of a drum kit;
/// a 128-sample hop puts the envelope at 86 frames a second, which places an
/// onset to within 12 ms. The resolution matters more than it looks: the
/// period is measured in these frames, and an error of half a frame is a
/// tempo error that shows as drift by the end of a minute-long cut.
const WINDOW: usize = 512;
const HOP: usize = 128;

/// The tempo range searched, in beats per minute.
///
/// Wider than most music needs on purpose: the octave errors this range
/// allows (a 75 BPM track read as 150) are handled by the prior below rather
/// than by refusing to look.
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;

/// The tempo a listener assumes when the music is ambiguous, and how wide
/// that assumption is in octaves. Tempo autocorrelation peaks at every
/// multiple of the true period, so without a prior the answer is arbitrary
/// among them. 120 BPM is the usual centre.
const PRIOR_CENTRE_BPM: f64 = 120.0;
const PRIOR_WIDTH: f64 = 0.9;

/// How finely the tempo search steps, in BPM. At 0.25 the worst rounding is
/// an eighth of a beat over four minutes, which stays inside a frame.
const BPM_STEP: f64 = 0.25;

/// A tempo and where its beats fall.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatDetection {
    pub bpm: f64,
    /// The first beat, from the start of the samples analysed.
    pub first_beat: Micros,
    /// How strongly the envelope agreed with this period, 0 to 1. Low means
    /// the audio has no steady pulse -- speech, wind, a quiet room -- and a
    /// caller should say so rather than cut to it.
    pub confidence: f64,
}

impl BeatDetection {
    pub fn beat(&self) -> Micros {
        Micros((60_000_000.0 / self.bpm).round() as u64)
    }
}

/// Find the tempo of mono audio.
///
/// Returns `None` when there is too little audio to judge, or when nothing
/// periodic is there at all.
pub fn detect(samples: &[f32], sample_rate: u32) -> Option<BeatDetection> {
    if sample_rate == 0 {
        return None;
    }
    let envelope = onset_envelope(samples);
    // Four seconds is the least that can show a tempo: at 60 BPM it holds
    // four beats, and three intervals is the fewest that can disagree.
    let per_second = sample_rate as f64 / HOP as f64;
    if envelope.len() < (per_second * 4.0) as usize {
        return None;
    }

    let (coarse, confidence) = tempo(&envelope, per_second)?;
    // The correlation peak is broad, because a lag one frame out still
    // matches most onsets. A beat grid laid across the whole envelope is not:
    // a period slightly off walks away from the onsets by the end, so scoring
    // grids is what turns a rough period into an exact one.
    let period = refine_on_grid(&envelope, coarse);
    let bpm = 60.0 * per_second / period;

    let phase = phase_of(&envelope, period);

    // A frame stands for the middle of its window, not its start. The flux
    // rises as soon as a sound enters the window, so without this every beat
    // is reported half a window early -- 23 ms here, which is enough to see
    // as a cut landing just before the hit.
    let at_frame = |frame: f64| (frame * HOP as f64 + WINDOW as f64 / 2.0) / sample_rate as f64;

    Some(BeatDetection {
        bpm,
        first_beat: Micros((at_frame(phase) * 1e6).round() as u64),
        confidence,
    })
}

/// Spectral flux: how much energy appeared since the last window.
///
/// Only increases count. A note starting is energy appearing; a note ending
/// is not an onset, and counting it as one puts beats on the offbeat.
fn onset_envelope(samples: &[f32]) -> Vec<f64> {
    if samples.len() < WINDOW {
        return Vec::new();
    }
    let window: Vec<f64> = (0..WINDOW)
        .map(|i| {
            // Hann, so the spectrum of a steady tone stays put instead of
            // smearing across bins as the window slides over it.
            0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / WINDOW as f64).cos()
        })
        .collect();

    let bins = WINDOW / 2;
    let mut previous = vec![0.0f64; bins];
    let mut envelope = Vec::with_capacity(samples.len() / HOP);
    let mut re = vec![0.0f64; WINDOW];
    let mut im = vec![0.0f64; WINDOW];

    let mut start = 0;
    while start + WINDOW <= samples.len() {
        for i in 0..WINDOW {
            re[i] = samples[start + i] as f64 * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);

        let mut flux = 0.0;
        for bin in 0..bins {
            // Log magnitude: loudness is multiplicative, so a quiet passage's
            // onsets would otherwise contribute nothing beside a loud one's.
            let magnitude = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt();
            let magnitude = (1.0 + 1000.0 * magnitude).ln();
            let rise = magnitude - previous[bin];
            if rise > 0.0 {
                flux += rise;
            }
            previous[bin] = magnitude;
        }
        envelope.push(flux);
        start += HOP;
    }

    normalise(envelope)
}

/// Centre the envelope on its local average and keep only what rises above
/// it, so a loud chorus does not outvote a quiet verse of the same tempo.
fn normalise(envelope: Vec<f64>) -> Vec<f64> {
    // About half a second, which is long enough to average over a beat and
    // short enough to follow a change in dynamics.
    let radius = 10usize;
    let mut out = Vec::with_capacity(envelope.len());
    for i in 0..envelope.len() {
        let from = i.saturating_sub(radius);
        let to = (i + radius + 1).min(envelope.len());
        let local: f64 = envelope[from..to].iter().sum::<f64>() / (to - from) as f64;
        out.push((envelope[i] - local).max(0.0));
    }
    // Spread each onset over its neighbours. An onset lands between frames,
    // and without this a period that is not a whole number of frames matches
    // itself poorly and its own double well.
    let mut spread = out.clone();
    for i in 0..out.len() {
        let before = if i > 0 { out[i - 1] } else { 0.0 };
        let after = if i + 1 < out.len() { out[i + 1] } else { 0.0 };
        spread[i] = 0.25 * before + 0.5 * out[i] + 0.25 * after;
    }
    let mut out = spread;

    let peak = out.iter().cloned().fold(0.0, f64::max);
    if peak > 0.0 {
        for value in &mut out {
            *value /= peak;
        }
    }
    out
}

/// The period, in envelope frames, that the onsets repeat at.
///
/// Autocorrelation at fractional lags, weighted by a log-normal tempo prior.
///
/// Two details matter more than they look. The lag is fractional because a
/// real tempo is not a whole number of frames, and at whole lags a period of
/// 18.4 frames matches *better* at 36.9 than at 18 -- which is how a 140 BPM
/// track gets reported as 70. And the prior resolves what is left: a pulse
/// correlates just as well at half and double its tempo, and only an
/// assumption about what a listener would tap can choose between them.
fn tempo(envelope: &[f64], frames_per_second: f64) -> Option<(f64, f64)> {
    let longest = (60.0 * frames_per_second / MIN_BPM).ceil();
    if longest as usize >= envelope.len() / 2 {
        return None;
    }

    let mut best = (0.0f64, f64::MIN);
    let mut scores = Vec::new();
    let mut bpm = MIN_BPM;
    while bpm <= MAX_BPM {
        let lag = 60.0 * frames_per_second / bpm;
        let weighted = correlation(envelope, lag) * prior(bpm);
        scores.push(weighted);
        if weighted > best.1 {
            best = (lag, weighted);
        }
        bpm += BPM_STEP;
    }

    let (lag, peak) = best;
    if peak <= 0.0 {
        return None;
    }

    // How far the winning period stands above the others. A steady pulse
    // stands well clear; a field recording barely does, and the caller is
    // told so rather than handed a tempo that is really noise.
    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let confidence = if mean > 0.0 {
        (1.0 - mean / peak).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Some((lag, confidence))
}

/// Autocorrelation at a fractional lag, with the envelope interpolated
/// between frames.
fn correlation(envelope: &[f64], lag: f64) -> f64 {
    let first = lag.ceil() as usize;
    if first >= envelope.len() {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in first..envelope.len() {
        sum += envelope[i] * sample_at(envelope, i as f64 - lag);
    }
    sum / (envelope.len() - first) as f64
}

/// The envelope between two frames.
fn sample_at(envelope: &[f64], at: f64) -> f64 {
    if at < 0.0 {
        return 0.0;
    }
    let low = at.floor() as usize;
    if low + 1 >= envelope.len() {
        return envelope.get(low).copied().unwrap_or(0.0);
    }
    let fraction = at - low as f64;
    envelope[low] * (1.0 - fraction) + envelope[low + 1] * fraction
}

/// How likely a listener is to tap at this tempo.
fn prior(bpm: f64) -> f64 {
    let octaves = (bpm / PRIOR_CENTRE_BPM).log2();
    (-0.5 * (octaves / PRIOR_WIDTH).powi(2)).exp()
}

/// Sharpen the period by scoring whole beat grids.
///
/// Searched within a tenth of the estimate, which is well inside the
/// neighbouring octave, so this refines the tempo rather than changing it.
fn refine_on_grid(envelope: &[f64], period: f64) -> f64 {
    let step = 0.02;
    let mut best = (period, f64::MIN);
    let mut candidate = period * 0.9;
    while candidate <= period * 1.1 {
        let score = grid_score(envelope, candidate, phase_of(envelope, candidate));
        // Longer periods lay fewer beats and so collect less; comparing the
        // average per beat is what keeps this from drifting slow.
        let beats = (envelope.len() as f64 / candidate).max(1.0);
        let average = score / beats;
        if average > best.1 {
            best = (candidate, average);
        }
        candidate += step;
    }
    best.0
}

/// How much onset energy a grid of this period and phase collects.
fn grid_score(envelope: &[f64], period: f64, phase: f64) -> f64 {
    let mut sum = 0.0;
    let mut beat = phase;
    while beat < envelope.len() as f64 {
        sum += sample_at(envelope, beat);
        beat += period;
    }
    sum
}

/// Where the first beat falls, in envelope frames.
///
/// The offset whose beats collect the most onset energy, searched in quarter
/// frames -- about 6 ms, which is finer than a cut can express, and cheap
/// because there is only ever one beat's worth of offsets to try.
fn phase_of(envelope: &[f64], period: f64) -> f64 {
    let step = 0.25;
    let mut best = (0.0f64, f64::MIN);
    let mut offset = 0.0;
    while offset < period {
        let mut sum = 0.0;
        let mut beat = offset;
        while beat < envelope.len() as f64 {
            sum += sample_at(envelope, beat);
            beat += period;
        }
        if sum > best.1 {
            best = (offset, sum);
        }
        offset += step;
    }
    best.0
}

/// In-place iterative radix-2 FFT.
///
/// Written out rather than pulled in: it is thirty lines, the transform is
/// the only piece of signal processing here that is not specific to this
/// problem, and a dependency in the perception crate is a dependency in every
/// build of the engine on every platform.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());

    // Bit-reversal permutation.
    let mut target = 0usize;
    for position in 0..n {
        if target > position {
            re.swap(target, position);
            im.swap(target, position);
        }
        let mut mask = n >> 1;
        while mask > 0 && target & mask != 0 {
            target &= !mask;
            mask >>= 1;
        }
        target |= mask;
    }

    let mut span = 1;
    while span < n {
        let step = -std::f64::consts::PI / span as f64;
        for start in (0..n).step_by(span * 2) {
            for offset in 0..span {
                let angle = step * offset as f64;
                let (sin, cos) = angle.sin_cos();
                let i = start + offset;
                let j = i + span;
                let tre = re[j] * cos - im[j] * sin;
                let tim = re[j] * sin + im[j] * cos;
                re[j] = re[i] - tre;
                im[j] = im[i] - tim;
                re[i] += tre;
                im[i] += tim;
            }
        }
        span *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click track: one short burst every `beat_ms`, as a metronome makes.
    fn clicks(bpm: f64, seconds: f64, offset_ms: f64) -> Vec<f32> {
        let rate = ANALYSIS_RATE as f64;
        let period = 60.0 / bpm;
        let count = (seconds * rate) as usize;
        (0..count)
            .map(|i| {
                let t = i as f64 / rate - offset_ms / 1000.0;
                if t < 0.0 {
                    return 0.0;
                }
                let since = t % period;
                // A 1 kHz burst decaying over about 25 ms.
                (0.6 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin() * (-40.0 * since).exp())
                    as f32
            })
            .collect()
    }

    #[test]
    fn a_click_track_reports_its_own_tempo() {
        for bpm in [90.0, 100.0, 120.0, 140.0] {
            let found = detect(&clicks(bpm, 12.0, 0.0), ANALYSIS_RATE)
                .unwrap_or_else(|| panic!("no tempo found at {bpm} BPM"));
            assert!(
                (found.bpm - bpm).abs() < 2.0,
                "{bpm} BPM was read as {:.1}",
                found.bpm
            );
            assert!(found.confidence > 0.3, "confidence {}", found.confidence);
        }
    }

    #[test]
    fn the_first_beat_is_found_where_the_first_click_is() {
        // Cutting on a grid that is half a beat out is worse than not cutting
        // to it at all, so the phase matters as much as the tempo.
        let found = detect(&clicks(120.0, 12.0, 250.0), ANALYSIS_RATE).unwrap();
        let beat = found.beat().0 as f64;
        let error = (found.first_beat.0 as f64 - 250_000.0).abs();
        // Any beat is as good as the first, so a whole-beat error is correct.
        let off_grid = (error % beat).min(beat - error % beat);
        assert!(off_grid < 40_000.0, "first beat off by {off_grid} us");
    }

    #[test]
    fn silence_yields_no_tempo_rather_than_a_confident_guess() {
        let silence = vec![0.0f32; ANALYSIS_RATE as usize * 8];
        match detect(&silence, ANALYSIS_RATE) {
            None => {}
            Some(found) => assert!(
                found.confidence < 0.2,
                "silence reported {:.0} BPM at confidence {:.2}",
                found.bpm,
                found.confidence
            ),
        }
    }

    #[test]
    fn noise_is_reported_with_low_confidence() {
        // A caller must be able to tell "there is a pulse here" from "this is
        // the best fit to something with no pulse at all".
        let mut seed = 12345u64;
        let noise: Vec<f32> = (0..ANALYSIS_RATE as usize * 10)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((seed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) as f32
            })
            .collect();
        let steady = detect(&clicks(120.0, 10.0, 0.0), ANALYSIS_RATE).unwrap();
        if let Some(found) = detect(&noise, ANALYSIS_RATE) {
            assert!(
                found.confidence < steady.confidence,
                "noise ({:.2}) looked as periodic as a click track ({:.2})",
                found.confidence,
                steady.confidence
            );
        }
    }

    #[test]
    fn too_little_audio_is_declined() {
        assert!(detect(&clicks(120.0, 1.0, 0.0), ANALYSIS_RATE).is_none());
        assert!(detect(&[], ANALYSIS_RATE).is_none());
    }

    #[test]
    fn the_transform_matches_a_direct_computation() {
        // The FFT is the one piece here not specific to this problem, so it
        // is checked against the definition rather than against itself.
        let n = 16;
        let signal: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 3.0 * i as f64 / n as f64).sin())
            .collect();
        let mut re = signal.clone();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);

        for k in 0..n {
            let (mut dre, mut dim) = (0.0, 0.0);
            for (t, x) in signal.iter().enumerate() {
                let angle = -2.0 * std::f64::consts::PI * k as f64 * t as f64 / n as f64;
                dre += x * angle.cos();
                dim += x * angle.sin();
            }
            assert!((re[k] - dre).abs() < 1e-9, "bin {k} real");
            assert!((im[k] - dim).abs() < 1e-9, "bin {k} imaginary");
        }
    }
}
