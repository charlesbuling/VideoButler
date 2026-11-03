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

//! The CLI backend's lossless half: keyframes from ffprobe's packet flags,
//! a fingerprint from its description of the streams, and stretches cut and
//! joined by ffmpeg with every packet copied.

use super::{FfmpegCli, rotation_of};
use crate::MediaError;
use crate::backend::{CopyPart, MediaBackend};
use std::path::Path;
use vb_contracts::Micros;

/// Seconds as ffmpeg reads them, to the microsecond.
fn seconds(us: u64) -> String {
    format!("{}.{:06}", us / 1_000_000, us % 1_000_000)
}

/// A time ffprobe printed, in seconds, as microseconds; none for `N/A`.
fn micros(text: &str) -> Option<i64> {
    let s: f64 = text.trim().parse().ok()?;
    s.is_finite().then(|| (s * 1e6).round() as i64)
}

/// The hex digits of ffprobe's `-show_data` dump: `00000000: 0164 001f ...`
/// lines, the ASCII column left out.
fn dump_hex(dump: &str) -> String {
    dump.lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(_, rest)| {
            rest.chars()
                .take(39)
                .filter(|c| c.is_ascii_hexdigit())
                .collect::<String>()
        })
        .collect()
}

impl FfmpegCli {
    /// When the container's clock starts: what the concat demuxer's in and
    /// out points are counted from.
    fn start_time(&self, path: &Path) -> Result<Micros, MediaError> {
        let args = vec![
            "-v".into(),
            "error".into(),
            "-show_entries".into(),
            "format=start_time".into(),
            "-of".into(),
            "csv=p=0".into(),
            path.display().to_string(),
        ];
        let out = self.run(&self.ffprobe.clone(), &args)?;
        Ok(micros(&String::from_utf8_lossy(&out))
            .map(|us| Micros(us.max(0) as u64))
            .unwrap_or(Micros::ZERO))
    }

    pub(super) fn read_keyframes(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
    ) -> Result<Vec<Micros>, MediaError> {
        let start = self.start_time(path)?;
        let args = vec![
            "-v".into(),
            "error".into(),
            "-select_streams".into(),
            "v:0".into(),
            // From the keyframe at or before `from`, which a seek lands on,
            // to a second past `to`, since the interval leaves its end out
            // and `to` itself counts.
            "-read_intervals".into(),
            format!(
                "{}%{}",
                seconds(start.0 + from.0),
                seconds(start.0 + to.0 + 1_000_000)
            ),
            "-show_entries".into(),
            "packet=pts_time,flags".into(),
            "-of".into(),
            "csv=p=0".into(),
            path.display().to_string(),
        ];
        let out = self.run(&self.ffprobe.clone(), &args)?;
        let mut times: Vec<Micros> = String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|line| {
                let (pts, flags) = line.split_once(',')?;
                if !flags.starts_with('K') {
                    return None;
                }
                let at = micros(pts)? - start.0 as i64;
                (at >= from.0 as i64 && at <= to.0 as i64).then_some(Micros(at as u64))
            })
            .collect();
        times.sort();
        times.dedup();
        Ok(times)
    }

    pub(super) fn fingerprint(&self, path: &Path) -> Result<String, MediaError> {
        let args = vec![
            "-v".into(),
            "error".into(),
            "-show_streams".into(),
            "-show_data".into(),
            "-of".into(),
            "json".into(),
            path.display().to_string(),
        ];
        let out = self.run(&self.ffprobe.clone(), &args)?;
        let json: serde_json::Value =
            serde_json::from_slice(&out).map_err(|e| MediaError::Parse {
                what: "ffprobe streams",
                detail: e.to_string(),
            })?;
        let streams = json["streams"].as_array().cloned().unwrap_or_default();
        let first = |kind: &str| {
            streams.iter().find(|s| {
                s["codec_type"] == kind && s["disposition"]["attached_pic"].as_i64() != Some(1)
            })
        };
        let field = |s: &serde_json::Value, k: &str| match &s[k] {
            serde_json::Value::Null => "-".to_string(),
            serde_json::Value::String(v) => v.clone(),
            v => v.to_string(),
        };
        let header = |s: &serde_json::Value| {
            vb_contracts::Blake3Hash::of_str(&dump_hex(s["extradata"].as_str().unwrap_or("")))
                .to_hex()[..16]
                .to_string()
        };
        let video = first("video").ok_or(MediaError::NoVideoStream)?;
        let mut print = format!(
            "v:{}",
            [
                "codec_name",
                "profile",
                "level",
                "width",
                "height",
                "pix_fmt",
                "color_range",
                "color_space",
                "color_transfer",
                "color_primaries",
                "sample_aspect_ratio",
                "field_order",
            ]
            .iter()
            .map(|k| field(video, k))
            .collect::<Vec<_>>()
            .join("/")
        );
        print.push_str(&format!("/rot{}/x{}", rotation_of(video), header(video)));
        match first("audio") {
            Some(audio) => print.push_str(&format!(
                "|a:{}/{}/{}/{}/x{}",
                field(audio, "codec_name"),
                field(audio, "profile"),
                field(audio, "sample_rate"),
                field(audio, "channels"),
                header(audio)
            )),
            None => print.push_str("|a:none"),
        }
        Ok(print)
    }

    /// What a copy of a stretch keeps of its picture (see [`kept`]), and how
    /// long it lasts with its sound, read from ffprobe's packet list.
    fn measure(&self, path: &Path, from: Micros, to: Micros) -> Result<(Kept, i64), MediaError> {
        let start = self.start_time(path)?;
        let args = vec![
            "-v".into(),
            "error".into(),
            "-read_intervals".into(),
            // The interval ends where a packet is shown, and a frame decoded
            // before `to` can be shown a little after it.
            format!(
                "{}%+{}",
                seconds(start.0 + from.0),
                seconds(to.0.saturating_sub(from.0) + 2_000_000)
            ),
            "-show_entries".into(),
            "stream=index,codec_type,avg_frame_rate,r_frame_rate:stream_disposition=attached_pic:\
             packet=stream_index,pts_time,dts_time,duration_time"
                .into(),
            "-of".into(),
            "json".into(),
            path.display().to_string(),
        ];
        let out = self.run(&self.ffprobe.clone(), &args)?;
        let json: serde_json::Value =
            serde_json::from_slice(&out).map_err(|e| MediaError::Parse {
                what: "ffprobe packets",
                detail: e.to_string(),
            })?;
        let time = |v: &serde_json::Value| v.as_str().and_then(micros);
        let streams = json["streams"].as_array().cloned().unwrap_or_default();
        let first = |kind: &str| {
            streams.iter().find(|s| {
                s["codec_type"] == kind && s["disposition"]["attached_pic"].as_i64() != Some(1)
            })
        };
        let video = first("video").ok_or(MediaError::NoVideoStream)?;
        let (vi, ai) = (
            video["index"].as_i64(),
            first("audio").and_then(|a| a["index"].as_i64()),
        );
        let frame = [&video["avg_frame_rate"], &video["r_frame_rate"]]
            .iter()
            .find_map(|r| {
                let (n, d) = r.as_str()?.split_once('/')?;
                let (n, d): (i64, i64) = (n.parse().ok()?, d.parse().ok()?);
                (n > 0 && d > 0).then(|| 1_000_000 * d / n)
            })
            .unwrap_or(33_333);
        let start = start.0 as i64;
        let packets = json["packets"].as_array().cloned().unwrap_or_default();
        let picture: Vec<Packet> = packets
            .iter()
            .filter(|p| p["stream_index"].as_i64() == vi)
            .filter_map(|p| {
                let pts = time(&p["pts_time"])? - start;
                Some(Packet {
                    pts,
                    dts: time(&p["dts_time"]).map_or(pts, |d| d - start),
                })
            })
            .collect();
        let (from, to) = (from.0 as i64, to.0 as i64);
        let kept = kept(&picture, from, to, frame);
        // The sound under the picture, packet by whole packet, as the linked
        // backend keeps it: the next stretch starts after it.
        let sound_end = packets
            .iter()
            .filter(|p| ai.is_some() && p["stream_index"].as_i64() == ai)
            .filter_map(|p| {
                let pts = time(&p["pts_time"])? - start;
                ((from..from + kept.end).contains(&pts))
                    .then(|| pts + time(&p["duration_time"]).unwrap_or(0) - from)
            })
            .max()
            .unwrap_or(0);
        Ok((kept, kept.end.max(sound_end)))
    }

    pub(super) fn measure_copy(
        &self,
        path: &Path,
        from: Micros,
        to: Micros,
    ) -> Result<Micros, MediaError> {
        let (kept, _) = self.measure(path, from, to)?;
        Ok(Micros(from.0 + kept.end.max(0) as u64))
    }

    /// LosslessCut's two steps: each stretch cut into a file of its own by
    /// copying, then the whole files joined by the concat demuxer -- whole
    /// files, because given in and out points it ends a stretch when any
    /// stream reaches the out point, and a phone stores its sound ahead of
    /// its picture. Each stretch keeps what the linked backend keeps: its
    /// picture from its keyframe (`-ss` before the input lands there) to
    /// where its frames are whole after its end ([`kept`]), which the
    /// picture's own input ends at by decode time; its sound from its start
    /// to where the picture ends, from a second input of the same file. The
    /// list says how long each file lasts, because the files' own lengths
    /// come from packet durations some cameras spend on decoding rather
    /// than showing.
    pub(super) fn join(
        &self,
        parts: &[CopyPart],
        dest: &Path,
        audio: bool,
        progress: &mut dyn FnMut(f64) -> bool,
    ) -> Result<(), MediaError> {
        let first = parts.first().ok_or_else(|| MediaError::Parse {
            what: "parts to join",
            detail: "there are none".into(),
        })?;
        let hevc = self.probe(&first.path)?.video_codec.as_deref() == Some("hevc");
        let stem = dest
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "join".into());
        let work = dest.with_file_name(format!(".{stem}.parts"));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| MediaError::Io {
            path: work.display().to_string(),
            source: e,
        })?;
        let result = (|| {
            let mut list = String::from("ffconcat version 1.0\n");
            let mut pieces = 0;
            for (i, part) in parts.iter().enumerate() {
                let (kept, length) = self.measure(&part.path, part.from, part.to)?;
                if kept.count == 0 {
                    // Nothing of it is shown, as in the linked backend.
                    continue;
                }
                let piece = work.join(format!("{i:04}.mp4"));
                let mut args: Vec<String> = vec![
                    "-hide_banner".into(),
                    "-nostats".into(),
                    "-v".into(),
                    "error".into(),
                    "-y".into(),
                    "-ss".into(),
                    seconds(part.from.0),
                ];
                if let Some(cut) = kept.cut.filter(|c| *c > 0) {
                    args.extend(["-t".into(), seconds(cut as u64)]);
                }
                args.extend(["-i".into(), part.path.display().to_string()]);
                if audio {
                    args.extend([
                        "-ss".into(),
                        seconds(part.from.0),
                        "-t".into(),
                        seconds(kept.end.max(1) as u64),
                        "-i".into(),
                        part.path.display().to_string(),
                    ]);
                }
                args.extend(["-map".into(), "0:v:0".into()]);
                if audio {
                    args.extend(["-map".into(), "1:a:0?".into()]);
                }
                if kept.cut.is_some_and(|c| c <= 0) {
                    // A stretch of a frame or two, which a decode time
                    // cannot end: counted out instead.
                    args.extend(["-frames:v".into(), kept.count.to_string()]);
                }
                args.extend([
                    // Left to itself ffmpeg copies the sound the seek lands
                    // before the stretch's start, which then plays at the
                    // seam; the linked backend starts it at the start.
                    "-copypriorss".into(),
                    "0".into(),
                    "-c".into(),
                    "copy".into(),
                    "-f".into(),
                    "mp4".into(),
                    piece.display().to_string(),
                ]);
                self.run(&self.ffmpeg.clone(), &args)?;
                pieces += 1;
                list.push_str(&format!(
                    "file 'file:{}'\nduration {}\n",
                    piece.to_string_lossy().replace('\'', "'\\''"),
                    seconds(length.max(1) as u64)
                ));
                if !progress(0.9 * (i + 1) as f64 / parts.len() as f64) {
                    return Err(stopped());
                }
            }
            if pieces == 0 {
                return Err(MediaError::Parse {
                    what: "parts to join",
                    detail: "none of them shows a frame".into(),
                });
            }
            let list_path = work.join("parts.ffconcat");
            std::fs::write(&list_path, list).map_err(|e| MediaError::Io {
                path: list_path.display().to_string(),
                source: e,
            })?;
            let mut args: Vec<String> = vec![
                "-hide_banner".into(),
                "-nostats".into(),
                "-v".into(),
                "error".into(),
                "-y".into(),
                "-f".into(),
                "concat".into(),
                "-safe".into(),
                "0".into(),
                // The parts share one codec header: their packets go in as
                // they are, as the linked backend writes them.
                "-auto_convert".into(),
                "0".into(),
                "-i".into(),
                list_path.display().to_string(),
            ];
            args.extend(["-map".into(), "0:v:0".into()]);
            if audio {
                args.extend(["-map".into(), "0:a:0?".into()]);
            }
            args.extend(["-c".into(), "copy".into()]);
            if hevc {
                // QuickTime and Photos play HEVC in MP4 only as hvc1.
                args.extend(["-tag:v".into(), "hvc1".into()]);
            }
            args.extend([
                "-movflags".into(),
                "+faststart".into(),
                dest.display().to_string(),
            ]);
            self.run(&self.ffmpeg.clone(), &args)?;
            progress(1.0);
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&work);
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }
}

/// A video packet as ffprobe lists it, in decoding order, in microseconds
/// of the file's own clock.
#[derive(Debug, Clone, Copy)]
struct Packet {
    pts: i64,
    dts: i64,
}

/// What a copy of a stretch keeps of its picture.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Kept {
    /// How many packets, in decoding order, from the keyframe.
    count: usize,
    /// Where the picture's input stops, in decode time from the stretch's
    /// start: halfway between the last packet kept and the first left, so
    /// a tick of rounding cannot move it. None when the file ends first.
    cut: Option<i64>,
    /// Where the frames kept end, from the stretch's start.
    end: i64,
}

/// The linked backend's rule (`join_measure` in the shim): from the
/// keyframe at `from`, in decoding order, every packet shown from `from`
/// on, up to the last one shown before `to` -- and then on until no frame
/// left out would be shown before one kept. A stretch that ends inside a
/// group of B-frames waits for the group's reference frame, shown after
/// `to`; stopping at the last frame before `to` would freeze the picture
/// over the frames between, then flash the reference frame. Carried on,
/// it ends at the same moment with every frame shown. It ends when the
/// next frame would have been shown, or a frame after the last at the
/// stretch's own rate (`frame` when it has one frame) -- never by packet
/// durations, which one phone's compressor spends on decoding: 0.1 ms for
/// each frame of a group, a quarter of a second for its last.
fn kept(packets: &[Packet], from: i64, to: i64, frame: i64) -> Kept {
    let mut frames: Vec<Packet> = Vec::new();
    let mut latest = i64::MIN;
    let mut beyond = None;
    for p in packets {
        // Decoded after the end and after every frame read is shown: from
        // here on nothing is shown before either.
        if p.dts >= to && p.dts > latest {
            beyond = Some(*p);
            break;
        }
        if p.pts >= from {
            frames.push(*p);
            latest = latest.max(p.pts);
        }
    }
    let next_shown = |n: usize| {
        frames[n..]
            .iter()
            .chain(beyond.as_ref())
            .map(|p| p.pts)
            .min()
            .unwrap_or(i64::MAX)
    };
    let mut n = frames.iter().rposition(|p| p.pts < to).map_or(0, |i| i + 1);
    let mut shown = frames[..n].iter().map(|p| p.pts).max().unwrap_or(i64::MIN);
    let first = frames[..n].iter().map(|p| p.pts).min().unwrap_or(from);
    while n > 0 && n < frames.len() && next_shown(n) <= shown {
        shown = shown.max(frames[n].pts);
        n += 1;
    }
    let end = if n == 0 {
        from
    } else {
        // The frames kept are shown one after another, none missing, so
        // their spacing is the stretch's rate.
        let step = if n > 1 {
            (shown - first) / (n as i64 - 1)
        } else {
            frame
        };
        let end = shown + step.max(1);
        let later = next_shown(n);
        if later > shown && later < end {
            later
        } else {
            end
        }
    };
    // With no packet left out the file has ended -- the listing reaches
    // two seconds past `to`, and no encoder shows a frame that long after
    // decoding it.
    let cut = match (n, frames.get(n).or(beyond.as_ref()).map(|p| p.dts)) {
        (0, _) | (_, None) => None,
        (n, Some(next)) => Some((frames[n - 1].dts + next) / 2 - from),
    };
    Kept {
        count: n,
        cut,
        end: end - from,
    }
}

fn stopped() -> MediaError {
    MediaError::Parse {
        what: "join",
        detail: "stopped".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dump_reads_as_its_hex_without_the_ascii() {
        let dump = "\n00000000: 0164 001f ffe1 001a 6764 001f acd9 4050  .d......gd....@P\n00000010: 05bb 01                                ...\n";
        assert_eq!(dump_hex(dump), "0164001fffe1001a6764001facd9405005bb01");
    }

    #[test]
    fn times_print_to_the_microsecond() {
        assert_eq!(seconds(12_034_567), "12.034567");
        assert_eq!(micros("5.000000"), Some(5_000_000));
        assert_eq!(micros("N/A"), None);
    }

    /// Packets in x264's order with two frames of delay, as the fixture has
    /// them (I0 P3 B1 B2 P6 B4 B5 ...), frames numbered as shown, a frame
    /// a millisecond.
    fn group(shown: &[i64]) -> Vec<Packet> {
        shown
            .iter()
            .enumerate()
            .map(|(k, n)| Packet {
                pts: n * 1000,
                dts: (k as i64 - 2) * 1000,
            })
            .collect()
    }

    #[test]
    fn a_stretch_ending_on_a_keyframe_keeps_nothing_after_it() {
        let packets = group(&[0, 3, 1, 2, 6, 4, 5, 8, 7, 9, 11, 10]);
        let k = kept(&packets, 0, 9000, 1000);
        // I9 and P11 are decoded before 9 ms but needed by nothing shown.
        assert_eq!(k.count, 9);
        assert_eq!(k.end, 9000);
        assert_eq!(k.cut, Some((6000 + 7000) / 2));
    }

    #[test]
    fn a_stretch_ending_inside_a_group_runs_on_to_its_reference_frame() {
        let packets = group(&[0, 3, 1, 2, 6, 4, 5, 9, 7, 8]);
        // Ends after frame 4: B4 needs P6, which is shown after the end,
        // so B5 is kept too rather than frame 4 held over its place.
        let k = kept(&packets, 0, 5000, 1000);
        assert_eq!(k.count, 7);
        assert_eq!(k.end, 7000);
        assert_eq!(k.cut, Some((4000 + 5000) / 2));
        // A frame before nothing is shown.
        assert_eq!(kept(&packets, 0, 0, 1000).count, 0);
    }

    #[test]
    fn a_deep_pyramid_with_bunched_decode_times_is_carried_to_its_end() {
        // One phone's footage: a reference frame eight ahead, a pyramid of
        // seven B-frames, and every packet of a group decoded within a
        // millisecond -- frames of 33 ms, decode times 0.1 ms apart.
        let order = [8, 4, 2, 1, 3, 6, 5, 7];
        let mut packets = vec![Packet {
            pts: 0,
            dts: -7_900,
        }];
        for g in 0..3i64 {
            for (k, n) in order.iter().enumerate() {
                packets.push(Packet {
                    pts: (g * 8 + n) * 33_000,
                    dts: g * 8 * 33_000 + k as i64 * 100,
                });
            }
        }
        // Out after frame 10, inside the second group (frames 9..16).
        let k = kept(&packets, 0, 11 * 33_000, 33_000);
        assert_eq!(k.count, 17, "both groups whole");
        assert_eq!(k.end, 17 * 33_000);
        let cut = k.cut.unwrap();
        assert!(cut > 8 * 33_000 + 700 && cut < 16 * 33_000, "{cut}");
    }
}
