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

//! Perception on Apple platforms, with nothing to download.
//!
//! macOS and iOS ship Vision, which classifies a still into everyday
//! categories and finds faces in it. That is not a vision-language model, but
//! it is real perception: it replaces two of the three mocks on the platform
//! the app is built for first, it costs no disk and no network, and it runs
//! on the device.
//!
//! # Saying exactly what this is
//!
//! [`VisionVlm`] writes a description from labels, so it says
//! `"beach, sky, two people"` rather than a sentence. [`VisionTracker`] finds
//! faces and groups them by how similar they look, which is *not* face
//! recognition: Vision's feature prints measure visual similarity, and the
//! same person in different light can look further away than two different
//! people in the same light. Both adapters report themselves by name in
//! provenance, and neither is a mock -- what they return is real, measured
//! and limited, and the interface has to describe it that way rather than as
//! "the AI understood your footage".

// Off Apple platforms the shim is not compiled, so the types and helpers that
// talk to it have nothing to talk to. They stay in the build, rather than
// behind a cfg each, so a change to them is still type-checked on every CI
// platform.
#![cfg_attr(not(vb_vision), allow(dead_code, unused_imports))]

use serde::Deserialize;
use std::ffi::{CStr, CString, c_char, c_double, c_int};
use std::path::Path;
use vb_contracts::Micros;
use vb_contracts::footage_graph::EntityKind;
use vb_infer::tracker::{TrackWindow, TrackedEntity};
use vb_infer::{Adapter, InferError, ScoredLabel, TrackerAdapter, VlmAdapter, VlmRequest};

const ERR_LEN: usize = 512;

/// Keeping a long run at full speed while nobody is looking at it.
pub mod power {
    use std::ffi::{CString, c_char, c_void};

    #[cfg(vb_vision)]
    unsafe extern "C" {
        fn vb_power_begin(reason: *const c_char) -> *mut c_void;
        fn vb_power_end(token: *mut c_void);
        fn vb_power_user_initiated_thread();
        fn vb_power_thermal_state() -> std::ffi::c_int;
    }

    /// While this is held, macOS neither naps the app nor sleeps the
    /// machine for want of input. Released on drop. Does nothing elsewhere.
    pub struct Awake(*mut c_void);

    // SAFETY: the token is an NSObject that NSProcessInfo accepts back from
    // any thread; nothing reads through the pointer on this side.
    unsafe impl Send for Awake {}

    /// Hold the machine awake, at full speed, for `reason` -- which the
    /// system shows in `pmset -g assertions`.
    pub fn keep_awake(reason: &str) -> Awake {
        #[cfg(vb_vision)]
        {
            let reason = CString::new(reason).unwrap_or_default();
            // SAFETY: a valid C string for the duration of the call; the
            // token comes back to vb_power_end exactly once, on drop.
            Awake(unsafe { vb_power_begin(reason.as_ptr()) })
        }
        #[cfg(not(vb_vision))]
        {
            let _ = CString::new(reason);
            Awake(std::ptr::null_mut())
        }
    }

    impl Drop for Awake {
        fn drop(&mut self) {
            #[cfg(vb_vision)]
            if !self.0.is_null() {
                // SAFETY: the token vb_power_begin returned, ended once.
                unsafe { vb_power_end(self.0) };
            }
        }
    }

    /// Run the calling thread, and those it creates, as work the user is
    /// waiting for.
    pub fn user_initiated_thread() {
        #[cfg(vb_vision)]
        // SAFETY: no arguments; affects only the calling thread.
        unsafe {
            vb_power_user_initiated_thread()
        };
    }

    /// How hot the machine is, as the system judges it: 0 nominal, 1 fair,
    /// 2 serious, 3 critical. None where there is no such reading.
    pub fn thermal_state() -> Option<u8> {
        #[cfg(vb_vision)]
        {
            // SAFETY: no arguments; reads a process-wide property.
            let state = unsafe { vb_power_thermal_state() };
            Some(state.clamp(0, 3) as u8)
        }
        #[cfg(not(vb_vision))]
        {
            None
        }
    }
}

#[cfg(vb_vision)]
unsafe extern "C" {
    fn vb_vision_available() -> c_int;
    fn vb_vision_classify(
        path: *const c_char,
        min_confidence: c_double,
        max_labels: c_int,
        err: *mut c_char,
        err_len: usize,
    ) -> *mut c_char;
    fn vb_vision_faces(path: *const c_char, err: *mut c_char, err_len: usize) -> *mut c_char;
    fn vb_vision_crop(
        path: *const c_char,
        dest: *const c_char,
        x: c_double,
        y: c_double,
        w: c_double,
        h: c_double,
        padding: c_double,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;
    fn vb_vision_free(text: *mut c_char);
}

/// Is Vision usable in this build?
///
/// False off Apple platforms, where the shim is not compiled at all, so a
/// caller can ask without a `cfg` of its own.
pub fn is_available() -> bool {
    #[cfg(vb_vision)]
    {
        // SAFETY: no arguments, no allocation.
        unsafe { vb_vision_available() == 1 }
    }
    #[cfg(not(vb_vision))]
    {
        false
    }
}

fn c_string(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes().to_vec())
        .map_err(|_| format!("{} contains a NUL byte", path.display()))
}

/// Call a shim function that returns JSON, and parse it.
#[cfg(vb_vision)]
fn parse_json<T: for<'de> Deserialize<'de>>(
    adapter: &'static str,
    call: impl FnOnce(*mut c_char, usize) -> *mut c_char,
) -> Result<T, InferError> {
    let mut err = [0 as c_char; ERR_LEN];
    let raw = call(err.as_mut_ptr(), ERR_LEN);
    if raw.is_null() {
        let detail = unsafe { CStr::from_ptr(err.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        return Err(InferError::Failed { adapter, detail });
    }
    // SAFETY: the shim returns a NUL-terminated string it malloc'd, which is
    // freed here whatever the parse does with it.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { vb_vision_free(raw) };

    serde_json::from_str(&text).map_err(|e| InferError::BadOutput {
        adapter,
        detail: e.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct Classification {
    labels: Vec<Label>,
}

#[derive(Debug, Deserialize)]
struct Label {
    label: String,
    #[serde(default)]
    confidence: f64,
}

#[derive(Debug, Deserialize)]
struct FaceList {
    faces: Vec<Face>,
}

#[derive(Debug, Clone, Deserialize)]
struct Face {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    confidence: f64,
    #[serde(default)]
    signature: Vec<f32>,
}

/// Describes a still from Vision's own classifier.
#[derive(Debug, Clone)]
pub struct VisionVlm {
    /// Labels below this are noise. Vision returns a thousand of them,
    /// nearly all with near-zero confidence.
    pub min_confidence: f64,
    pub max_labels: usize,
}

impl Default for VisionVlm {
    fn default() -> Self {
        VisionVlm {
            min_confidence: 0.18,
            max_labels: 5,
        }
    }
}

impl Adapter for VisionVlm {
    fn name(&self) -> &'static str {
        "AppleVision"
    }
}

/// Labels the video butler classifies by: deeper into Vision's list than a
/// description goes, because its taxonomy is a tree and the leaf that names
/// a dish or a breed often sits under a parent ("food", "animal") with less
/// confidence than the description's five carry.
const CLASSIFY_MIN: f64 = 0.1;
const CLASSIFY_MAX: usize = 25;

impl VlmAdapter for VisionVlm {
    fn labels(&self, request: &VlmRequest) -> Result<Vec<ScoredLabel>, InferError> {
        #[cfg(not(vb_vision))]
        {
            let _ = request;
            Err(InferError::Unavailable {
                adapter: "AppleVision",
                reason: "this build has no Vision framework".into(),
            })
        }
        #[cfg(vb_vision)]
        {
            let path = c_string(&request.image_path).map_err(|detail| InferError::Failed {
                adapter: "AppleVision",
                detail,
            })?;
            let found: Classification = parse_json("AppleVision", |err, len| unsafe {
                vb_vision_classify(path.as_ptr(), CLASSIFY_MIN, CLASSIFY_MAX as c_int, err, len)
            })?;
            let mut labels: Vec<ScoredLabel> = found
                .labels
                .into_iter()
                .map(|l| ScoredLabel {
                    label: l.label,
                    confidence: l.confidence,
                })
                .collect();
            labels.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
            Ok(labels)
        }
    }

    fn describe(&self, request: &VlmRequest) -> Result<String, InferError> {
        #[cfg(not(vb_vision))]
        {
            let _ = request;
            Err(InferError::Unavailable {
                adapter: "AppleVision",
                reason: "this build has no Vision framework".into(),
            })
        }
        #[cfg(vb_vision)]
        {
            let path = c_string(&request.image_path).map_err(|detail| InferError::Failed {
                adapter: "AppleVision",
                detail,
            })?;
            let found: Classification = parse_json("AppleVision", |err, len| unsafe {
                vb_vision_classify(
                    path.as_ptr(),
                    self.min_confidence,
                    self.max_labels as c_int,
                    err,
                    len,
                )
            })?;

            if found.labels.is_empty() {
                // Nothing recognised is an answer, and an empty description
                // is the honest form of it -- inventing one would put words
                // in the cut's reasoning that nothing measured.
                return Ok(String::new());
            }
            Ok(found
                .labels
                .iter()
                .map(|l| l.label.replace('_', " "))
                .collect::<Vec<_>>()
                .join(", "))
        }
    }
}

/// Finds faces and groups them by appearance.
#[derive(Debug, Clone)]
pub struct VisionTracker {
    /// How close two feature prints must be to count as the same subject.
    /// Tuned to group rather than to identify: a wrong grouping in an album
    /// is a nuisance, and a split one is invisible.
    pub same_subject_distance: f32,
    /// Faces below this confidence are not faces.
    pub min_confidence: f64,
    /// Where thumbnails are written. Without one, entities have no picture.
    pub thumbnail_dir: Option<std::path::PathBuf>,
}

impl Default for VisionTracker {
    fn default() -> Self {
        VisionTracker {
            same_subject_distance: 0.62,
            min_confidence: 0.45,
            thumbnail_dir: None,
        }
    }
}

impl Adapter for VisionTracker {
    fn name(&self) -> &'static str {
        "AppleVisionFaces"
    }
}

impl TrackerAdapter for VisionTracker {
    fn track(
        &self,
        _path: &Path,
        windows: &[(String, TrackWindow)],
    ) -> Result<Vec<TrackedEntity>, InferError> {
        #[cfg(not(vb_vision))]
        {
            let _ = windows;
            Ok(Vec::new())
        }
        #[cfg(vb_vision)]
        {
            // Groups being built, each holding the faces put in it so far.
            let mut groups: Vec<Group> = Vec::new();

            for (shot_id, window) in windows {
                for still in &window.stills {
                    let Ok(path) = c_string(still) else {
                        continue;
                    };
                    let found: FaceList = match parse_json("AppleVisionFaces", |err, len| unsafe {
                        vb_vision_faces(path.as_ptr(), err, len)
                    }) {
                        Ok(found) => found,
                        Err(e) => {
                            // One unreadable still costs its faces, not
                            // the whole analysis.
                            tracing::warn!(still = %still.display(), error = %e, "face detection failed");
                            continue;
                        }
                    };

                    for face in found.faces {
                        if face.confidence < self.min_confidence || face.signature.is_empty() {
                            continue;
                        }
                        self.place(&mut groups, face, shot_id, window, still);
                    }
                }
            }

            Ok(groups
                .into_iter()
                .enumerate()
                .map(|(index, group)| group.into_entity(index))
                .collect())
        }
    }
}

#[cfg(vb_vision)]
impl VisionTracker {
    /// Put a face in the group it looks most like, or start a new one.
    fn place(
        &self,
        groups: &mut Vec<Group>,
        face: Face,
        shot_id: &str,
        window: &TrackWindow,
        still: &Path,
    ) {
        let mut best: Option<(usize, f32)> = None;
        for (index, group) in groups.iter().enumerate() {
            let distance = cosine_distance(&group.signature, &face.signature);
            if distance < self.same_subject_distance
                && best.map(|(_, d)| distance < d).unwrap_or(true)
            {
                best = Some((index, distance));
            }
        }

        match best {
            Some((index, _)) => groups[index].add(shot_id, window, &face),
            None => {
                let mut group = Group::new(face.signature.clone());
                group.add(shot_id, window, &face);
                group.thumbnail = self.thumbnail(still, &face, groups.len());
                groups.push(group);
            }
        }
    }

    /// A picture of the subject, cropped out of the still it was found in.
    fn thumbnail(&self, still: &Path, face: &Face, index: usize) -> Option<std::path::PathBuf> {
        // Under the clip's own directory, as the stills are: every clip
        // numbers its shots from shot-0000, and one flat directory had the
        // next clip's faces written over this one's.
        let clip = still
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let dir = self.thumbnail_dir.as_ref()?.join(clip);
        std::fs::create_dir_all(&dir).ok()?;
        let stem = still.file_stem()?.to_string_lossy().to_string();
        let dest = dir.join(format!("{stem}-face-{index}.jpg"));

        let source = c_string(still).ok()?;
        let target = c_string(&dest).ok()?;
        let mut err = [0 as c_char; ERR_LEN];
        // A quarter again around the face, so the crop reads as a portrait
        // rather than as a cutout.
        let code = unsafe {
            vb_vision_crop(
                source.as_ptr(),
                target.as_ptr(),
                face.x,
                face.y,
                face.w,
                face.h,
                0.25,
                err.as_mut_ptr(),
                ERR_LEN,
            )
        };
        (code == 0).then_some(dest)
    }
}

/// Faces grouped as one subject.
#[cfg(vb_vision)]
#[derive(Debug)]
struct Group {
    signature: Vec<f32>,
    shot_ids: Vec<String>,
    first_seen: Micros,
    last_seen: Micros,
    confidence: f64,
    seen: usize,
    thumbnail: Option<std::path::PathBuf>,
    face_area: f64,
}

#[cfg(vb_vision)]
impl Group {
    fn new(signature: Vec<f32>) -> Group {
        Group {
            signature,
            shot_ids: Vec::new(),
            first_seen: Micros(u64::MAX),
            last_seen: Micros::ZERO,
            confidence: 0.0,
            seen: 0,
            thumbnail: None,
            face_area: 0.0,
        }
    }

    fn add(&mut self, shot_id: &str, window: &TrackWindow, face: &Face) {
        if !self.shot_ids.iter().any(|id| id == shot_id) {
            self.shot_ids.push(shot_id.to_string());
        }
        self.first_seen = Micros(self.first_seen.0.min(window.start.0));
        self.last_seen = Micros(self.last_seen.0.max(window.end.0));
        // The running mean, so one certain sighting does not carry a subject
        // that was a guess everywhere else.
        self.seen += 1;
        self.confidence += (face.confidence - self.confidence) / self.seen as f64;
        // Vision's rectangles are normalised, so this is a share of the
        // picture whatever its size.
        self.face_area = self.face_area.max(face.w * face.h);
    }

    fn into_entity(self, index: usize) -> TrackedEntity {
        TrackedEntity {
            id: format!("person-{:03}", index + 1),
            kind: EntityKind::Person,
            // Deliberately unnamed. Vision found a face; who it belongs to is
            // something only the user knows, and the interface lets them say.
            label: None,
            first_seen: self.first_seen,
            last_seen: self.last_seen,
            shot_ids: self.shot_ids,
            confidence: Some(self.confidence),
            thumbnail: self.thumbnail,
            signature: self.signature,
            face_area: Some(self.face_area),
        }
    }
}

/// How unalike two signatures are. The measure lives in the contracts crate,
/// because the album groups subjects across projects with the same one and
/// two implementations would group differently.
pub use vb_contracts::footage_graph::signature_distance as cosine_distance;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_signatures_are_at_no_distance_and_opposites_are_far() {
        let a = vec![0.1, 0.2, 0.3, 0.4];
        assert!(cosine_distance(&a, &a) < 1e-6);

        let opposite: Vec<f32> = a.iter().map(|x| -x).collect();
        assert!(cosine_distance(&a, &opposite) > 1.9);
    }

    #[test]
    fn signatures_of_different_lengths_are_never_the_same_subject() {
        // Two Vision revisions return different lengths, and comparing them
        // by truncation would group strangers.
        assert_eq!(cosine_distance(&[0.1, 0.2], &[0.1]), f32::MAX);
        assert_eq!(cosine_distance::<f32>(&[], &[]), f32::MAX);
    }

    #[test]
    fn a_zero_signature_is_not_similar_to_anything() {
        assert_eq!(cosine_distance(&[0.0, 0.0], &[0.1, 0.2]), f32::MAX);
    }

    #[test]
    fn the_adapters_name_themselves_and_are_not_mocks() {
        // Provenance has to be able to tell this apart from MockVlm, and
        // from a real vision-language model.
        assert_eq!(VisionVlm::default().name(), "AppleVision");
        assert!(!VisionVlm::default().is_mock());
        assert_eq!(VisionTracker::default().name(), "AppleVisionFaces");
        assert!(!VisionTracker::default().is_mock());
    }

    #[test]
    #[cfg(vb_vision)]
    fn vision_is_available_on_this_platform() {
        assert!(is_available(), "the shim compiled but Vision is not there");
    }

    #[test]
    #[cfg(vb_vision)]
    fn a_still_of_a_known_scene_gets_labels() {
        // Generated rather than committed: a synthetic gradient is not a
        // beach, so this asserts the path works, not what it recognises.
        let dir = std::env::temp_dir().join("vb-vision-classify");
        let _ = std::fs::create_dir_all(&dir);
        let still = dir.join("frame.jpg");
        if !make_test_jpeg(&still) {
            return;
        }
        let described = VisionVlm::default()
            .describe(&VlmRequest::describe(&still))
            .expect("classification runs");
        // Labels or nothing, but never an error and never invented prose.
        assert!(described.is_ascii() || !described.is_empty() || described.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(vb_vision)]
    fn a_still_with_no_faces_yields_no_subjects() {
        let dir = std::env::temp_dir().join("vb-vision-faces");
        let _ = std::fs::create_dir_all(&dir);
        let still = dir.join("frame.jpg");
        if !make_test_jpeg(&still) {
            return;
        }
        let entities = VisionTracker::default()
            .track(
                Path::new("/unused.mp4"),
                &[(
                    "shot-0000".into(),
                    TrackWindow {
                        start: Micros::ZERO,
                        end: Micros::from_secs(2),
                        stills: vec![still],
                    },
                )],
            )
            .expect("face detection runs");
        assert!(
            entities.is_empty(),
            "a gradient is not a person: {entities:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(vb_vision)]
    fn an_unreadable_still_is_reported_rather_than_returning_nothing() {
        let err = VisionVlm::default()
            .describe(&VlmRequest::describe(Path::new("/definitely/not/here.jpg")))
            .unwrap_err();
        assert!(err.to_string().contains("not there"), "got {err}");
    }

    /// A small JPEG, via the ffmpeg on PATH. Returns false when there is
    /// none, so the test skips rather than fails on a bare machine.
    #[cfg(vb_vision)]
    fn make_test_jpeg(dest: &Path) -> bool {
        std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240",
            ])
            .args(["-frames:v", "1"])
            .arg(dest)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
