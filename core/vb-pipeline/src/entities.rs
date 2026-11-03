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

//! The album: who and what keeps turning up in a user's footage.
//!
//! Every run finds subjects in its clips. This gathers them across runs and
//! across projects, so the second project over the same family holiday shows
//! the same faces as the first, already named if the user named them.
//!
//! # What this is and is not
//!
//! Subjects are grouped by how alike they look, using the appearance
//! signature the tracker recorded. That is a similarity measure, not
//! recognition: the same person in evening light can land in a second group,
//! and two people can land in one. So nothing here is presented as an
//! identity the system knows. The user names a group; the name sticks to it;
//! and a group the user has named is never merged into another automatically,
//! because overruling a person's own label with a distance threshold is how
//! an album starts lying.

use crate::library::Library;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use vb_contracts::FootageGraph;
use vb_contracts::footage_graph::{Entity, EntityKind, signature_distance};

/// How alike two signatures must be to be treated as one subject.
///
/// Chosen to group rather than to identify. A split group is a nuisance the
/// user can ignore; a merged one puts two people under one name.
const SAME_SUBJECT: f32 = 0.55;

/// The same, for AuraFace's 512-number face vectors, which measure whose
/// face it is rather than how the picture looks. Set by looking at the
/// closest pairs of a real trip's faces: below 0.62 nearly every pair is one
/// person, sunglasses and masks included; between 0.65 and 0.75 about half
/// are two.
const SAME_FACE: f32 = 0.62;

/// AuraFace's vectors are this long; Apple Vision's feature prints are not,
/// so the two never match each other.
const FACE_ID_LEN: usize = 512;

/// How many sightings beside the first a subject keeps to be matched by: a
/// face in sunglasses and the same face without them sit further apart than
/// either does from its own kind. Each was itself matched to the first, so
/// none can carry the subject away to someone else.
const EXEMPLARS: usize = 6;

fn same_subject(signature: &[f64]) -> f32 {
    if signature.len() == FACE_ID_LEN {
        SAME_FACE
    } else {
        SAME_SUBJECT
    }
}

/// Where a subject was seen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub project_id: String,
    pub project_name: String,
    /// The clip, as the user would recognise it.
    pub source_path: String,
    pub shots: usize,
    pub at: String,
}

/// One subject in the album.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub id: String,
    /// What the user called this subject. Absent until they say.
    #[serde(default)]
    pub label: Option<String>,
    pub kind: EntityKind,
    /// A still of the subject, absolute.
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// The appearance this group is matched against. Taken from the first
    /// sighting and left alone, so the group cannot drift into another
    /// subject one merge at a time.
    #[serde(default)]
    pub signature: Vec<f64>,
    /// Later sightings it is also matched by, each close to the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exemplars: Vec<Vec<f64>>,
    #[serde(default)]
    pub appearances: Vec<Appearance>,
    pub first_seen_at: String,
    pub updated_at: String,
}

impl Subject {
    pub fn shots(&self) -> usize {
        self.appearances.iter().map(|a| a.shots).sum()
    }

    /// How many videos the subject is in: a clip seen by the video butler
    /// and again in a project is one clip.
    pub fn clips(&self) -> usize {
        let mut paths: Vec<&str> = self
            .appearances
            .iter()
            .map(|a| a.source_path.as_str())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

/// The project id the video butler's sightings are recorded under. Not a
/// project: they come and go with the videos the butler keeps, not with
/// any project.
pub const HOSTED: &str = "hosting";

/// The next subject id, past every one in use: counting the subjects gave
/// a forgotten subject's successor the id of one still there.
fn next_id(album: &Album) -> String {
    let highest = album
        .subjects
        .iter()
        .filter_map(|s| s.id.strip_prefix("sub-")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("sub-{:04}", highest + 1)
}

/// Fold the faces the video butler found in one video into the album, in
/// place of what it found there before. Returns, for each face, the subject
/// it was filed under; a face with no signature has none.
pub fn fold_hosted(
    album: &mut Album,
    path: &str,
    faces: &[vb_infer::TrackedEntity],
) -> Vec<Option<String>> {
    let now = vb_contracts::now_rfc3339();
    for subject in album.subjects.iter_mut() {
        subject
            .appearances
            .retain(|a| !(a.project_id == HOSTED && a.source_path == path));
    }
    let mut filed = Vec::with_capacity(faces.len());
    for face in faces {
        if face.signature.is_empty() {
            filed.push(None);
            continue;
        }
        let signature: Vec<f64> = face.signature.iter().map(|v| *v as f64).collect();
        let appearance = Appearance {
            project_id: HOSTED.to_string(),
            project_name: String::new(),
            source_path: path.to_string(),
            shots: face.shot_ids.len().max(1),
            at: now.clone(),
        };
        let thumbnail = face
            .thumbnail
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        let id = match match_signature(album, face.kind, &signature) {
            Some(index) => {
                let subject = &mut album.subjects[index];
                subject.updated_at = now.clone();
                if subject.thumbnail.is_none() {
                    subject.thumbnail = thumbnail;
                }
                keep_exemplar(subject, &signature);
                // Two faces of one video filed under one subject are one
                // sighting.
                if !subject
                    .appearances
                    .iter()
                    .any(|a| a.project_id == HOSTED && a.source_path == path)
                {
                    subject.appearances.push(appearance);
                }
                subject.id.clone()
            }
            None => {
                let id = next_id(album);
                album.subjects.push(Subject {
                    id: id.clone(),
                    label: None,
                    kind: face.kind,
                    thumbnail,
                    signature,
                    exemplars: Vec::new(),
                    appearances: vec![appearance],
                    first_seen_at: now.clone(),
                    updated_at: now.clone(),
                });
                id
            }
        };
        filed.push(Some(id));
    }
    filed
}

/// Take the video butler's sightings in videos it no longer keeps out of
/// the album, and every subject left with none.
pub fn drop_hosted(album: &mut Album, keep: &dyn Fn(&str) -> bool) {
    for subject in album.subjects.iter_mut() {
        subject
            .appearances
            .retain(|a| a.project_id != HOSTED || keep(&a.source_path));
    }
    album.subjects.retain(|s| !s.appearances.is_empty());
}

/// Most-seen first: an album is read for the people in it, and the one in
/// forty clips matters more than the passer-by in one.
pub fn sort(album: &mut Album) {
    album
        .subjects
        .sort_by(|a, b| b.clips().cmp(&a.clips()).then_with(|| a.id.cmp(&b.id)));
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Album {
    pub subjects: Vec<Subject>,
}

fn album_path(library: &Library) -> std::path::PathBuf {
    library.root().join("album.json")
}

pub fn load(library: &Library) -> Album {
    std::fs::read_to_string(album_path(library))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(library: &Library, album: &Album) -> Result<(), String> {
    library.ensure().map_err(|e| e.to_string())?;
    let path = album_path(library);
    let text = serde_json::to_string_pretty(album).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Fold what a run found into the album.
///
/// Subjects with no signature are skipped rather than added blind: without
/// one there is nothing to match them by, so every run would add the same
/// person again.
pub fn record_run(
    library: &Library,
    project_id: &str,
    project_name: &str,
    graph: &FootageGraph,
    work_dir: &std::path::Path,
) -> Album {
    let mut album = load(library);
    let now = vb_contracts::now_rfc3339();

    // A project run again replaces what it saw before: its clips may have
    // changed, and sightings from the last run must not linger beside the
    // new ones.
    // Emptied subjects stay until the new sightings are folded in, so a
    // person the user named keeps the name when the same project runs again.
    for subject in album.subjects.iter_mut() {
        subject.appearances.retain(|a| a.project_id != project_id);
    }

    for entity in &graph.entities {
        if entity.signature.is_empty() {
            continue;
        }
        let source_path = source_of(graph, entity);
        let shots = entity.shot_ids.len().max(1);

        match find_match(&album, entity) {
            Some(index) => {
                let subject = &mut album.subjects[index];
                subject.updated_at = now.clone();
                if subject.thumbnail.is_none() {
                    subject.thumbnail = thumbnail_path(entity, work_dir);
                }
                match subject
                    .appearances
                    .iter_mut()
                    .find(|a| a.project_id == project_id && a.source_path == source_path)
                {
                    // The same clip analysed again is the same sighting, not
                    // a new one; a re-run must not inflate the counts.
                    Some(appearance) => {
                        appearance.shots = shots;
                        appearance.at = now.clone();
                    }
                    None => subject.appearances.push(Appearance {
                        project_id: project_id.to_string(),
                        project_name: project_name.to_string(),
                        source_path,
                        shots,
                        at: now.clone(),
                    }),
                }
            }
            None => album.subjects.push(Subject {
                id: next_id(&album),
                label: None,
                kind: entity.kind,
                thumbnail: thumbnail_path(entity, work_dir),
                signature: entity.signature.clone(),
                exemplars: Vec::new(),
                appearances: vec![Appearance {
                    project_id: project_id.to_string(),
                    project_name: project_name.to_string(),
                    source_path,
                    shots,
                    at: now.clone(),
                }],
                first_seen_at: now.clone(),
                updated_at: now.clone(),
            }),
        }
    }

    album.subjects.retain(|s| !s.appearances.is_empty());

    // Most-seen first: an album is read for the people in it, and the one in
    // forty clips matters more than the passer-by in one.
    album
        .subjects
        .sort_by(|a, b| b.shots().cmp(&a.shots()).then_with(|| a.id.cmp(&b.id)));

    if let Err(e) = save(library, &album) {
        tracing::warn!(error = %e, "could not save the album");
    }
    album
}

fn find_match(album: &Album, entity: &Entity) -> Option<usize> {
    match_signature(album, entity.kind, &entity.signature)
}

fn match_signature(album: &Album, kind: EntityKind, signature: &[f64]) -> Option<usize> {
    let threshold = same_subject(signature);
    let mut best: Option<(usize, f32)> = None;
    for (index, subject) in album.subjects.iter().enumerate() {
        if subject.kind != kind {
            continue;
        }
        let distance = std::iter::once(&subject.signature)
            .chain(&subject.exemplars)
            .map(|s| signature_distance(s, signature))
            .fold(f32::MAX, f32::min);
        if distance < threshold && best.map(|(_, d)| distance < d).unwrap_or(true) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// Keep a sighting to be matched by, when it is unlike the ones kept and
/// still close to the first.
fn keep_exemplar(subject: &mut Subject, signature: &[f64]) {
    if signature.len() != FACE_ID_LEN || subject.exemplars.len() >= EXEMPLARS {
        return;
    }
    let to_first = signature_distance(&subject.signature, signature);
    let nearest = std::iter::once(&subject.signature)
        .chain(&subject.exemplars)
        .map(|s| signature_distance(s, signature))
        .fold(f32::MAX, f32::min);
    if to_first < SAME_FACE && nearest > 0.3 {
        subject.exemplars.push(signature.to_vec());
    }
}

/// The clip a subject was seen in, by the shots that reference it.
fn source_of(graph: &FootageGraph, entity: &Entity) -> String {
    graph
        .shots
        .iter()
        .find(|shot| entity.shot_ids.contains(&shot.id))
        .and_then(|shot| graph.source(&shot.source_id))
        .map(|source| source.path.clone())
        .unwrap_or_default()
}

/// A thumbnail path the app can show. Stored relative in the graph, so it is
/// resolved against the work directory here.
fn thumbnail_path(entity: &Entity, work_dir: &std::path::Path) -> Option<String> {
    entity
        .thumbnail_path
        .as_ref()
        .map(|relative| work_dir.join(relative).to_string_lossy().to_string())
}

/// Drop every sighting from a project that no longer exists, and every
/// subject left with none. The album is of the user's projects: a person
/// seen only in a deleted project is not in it any more.
pub fn prune(library: &Library) -> Album {
    let mut album = load(library);
    let projects: std::collections::HashSet<String> = crate::projects::list(library)
        .into_iter()
        .map(|p| p.id)
        .collect();
    let before: usize = album.subjects.iter().map(|s| s.appearances.len()).sum();
    for subject in album.subjects.iter_mut() {
        subject
            .appearances
            .retain(|a| a.project_id == HOSTED || projects.contains(&a.project_id));
    }
    album.subjects.retain(|s| !s.appearances.is_empty());
    let after: usize = album.subjects.iter().map(|s| s.appearances.len()).sum();
    if after != before
        && let Err(e) = save(library, &album)
    {
        tracing::warn!(error = %e, "could not save the pruned album");
    }
    album
}

/// Name a subject, or clear the name with an empty string.
pub fn rename(library: &Library, id: &str, label: Option<&str>) -> Result<Value, String> {
    let mut album = load(library);
    let subject = album
        .subjects
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("there is no subject {id}"))?;
    subject.label = label
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    subject.updated_at = vb_contracts::now_rfc3339();
    let named = one(subject);
    save(library, &album)?;
    Ok(named)
}

/// File one subject under another: the user saying two groups are one
/// person, which likeness alone could not -- the same face with and without
/// sunglasses and a mask, a baby a year apart. The name kept is the one
/// the user gave, the kept subject's first; every sighting and every face
/// the video butler filed moves across.
pub fn merge(library: &Library, from: &str, into: &str) -> Result<Value, String> {
    if from == into {
        return Err("a subject cannot be merged with itself".into());
    }
    let mut album = load(library);
    if !album.subjects.iter().any(|s| s.id == from) {
        return Err(format!("there is no subject {from}"));
    }
    if !album.subjects.iter().any(|s| s.id == into) {
        return Err(format!("there is no subject {into}"));
    }
    absorb(&mut album, from, into);
    let value = album
        .subjects
        .iter()
        .find(|s| s.id == into)
        .map(one)
        .unwrap_or(Value::Null);
    sort(&mut album);
    save(library, &album)?;
    crate::hosting::rename_subject(library, from, into)?;
    Ok(value)
}

/// File `from` under `into`, in the album alone: its sightings move across,
/// and the faces it was matched by match `into` from now on.
fn absorb(album: &mut Album, from: &str, into: &str) {
    let Some(gone) = album.subjects.iter().position(|s| s.id == from) else {
        return;
    };
    if !album.subjects.iter().any(|s| s.id == into) {
        return;
    }
    let moved = album.subjects.remove(gone);
    let kept = album
        .subjects
        .iter_mut()
        .find(|s| s.id == into)
        .expect("checked above");
    if kept.label.is_none() {
        kept.label = moved.label;
    }
    if kept.thumbnail.is_none() {
        kept.thumbnail = moved.thumbnail;
    }
    // Its first sighting, and its others, match this subject from now on.
    if moved.signature.len() == kept.signature.len() {
        kept.exemplars.push(moved.signature);
    }
    kept.exemplars.extend(
        moved
            .exemplars
            .into_iter()
            .filter(|e| e.len() == kept.signature.len()),
    );
    for appearance in moved.appearances {
        if !kept.appearances.iter().any(|a| {
            a.project_id == appearance.project_id && a.source_path == appearance.source_path
        }) {
            kept.appearances.push(appearance);
        }
    }
    kept.updated_at = vb_contracts::now_rfc3339();
}

/// How close two groups the video butler filed apart must come to be one
/// face. Groups are made one face at a time, and two can each gather
/// sightings that end up nearer each other than either was to the first;
/// nothing went back to join them. Far stricter than [`SAME_FACE`], because
/// AuraFace is least sure of exactly these faces -- babies, masks, a face
/// on a screen. On 2,555 real videos the four pairs closer than 0.47 were
/// each one person (a woman in four groups, a man in two), while from 0.50
/// up two children in one frame, a woman and a man, and a baby and a
/// stranger on a screen came as close.
const ONE_FACE: f32 = 0.48;

/// Join the groups the video butler made of one face: AuraFace subjects
/// whose kept faces come within [`ONE_FACE`], never the subjects of one
/// video together (two faces of one video are two people) and never two the
/// user named. `videos` says which videos each subject is a subject of, and
/// `changed` which subjects this pass touched -- only their pairs are
/// measured, every subject's when it is empty. Returns each subject joined
/// to another, and the one it joined.
pub fn consolidate(
    album: &mut Album,
    videos: &HashMap<String, HashSet<String>>,
    changed: &HashSet<String>,
) -> Vec<(String, String)> {
    let unit = |v: &[f64]| -> Vec<f32> {
        let norm = v
            .iter()
            .map(|x| x * x)
            .sum::<f64>()
            .sqrt()
            .max(f64::MIN_POSITIVE);
        v.iter().map(|x| (x / norm) as f32).collect()
    };
    let faces: Vec<(usize, Vec<Vec<f32>>)> = album
        .subjects
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == EntityKind::Person && s.signature.len() == FACE_ID_LEN)
        .map(|(i, s)| {
            let kept = std::iter::once(&s.signature)
                .chain(s.exemplars.iter().filter(|e| e.len() == FACE_ID_LEN))
                .map(|v| unit(v))
                .collect();
            (i, kept)
        })
        .collect();
    let mut close: Vec<(f32, usize, usize)> = Vec::new();
    for (a, (i, first)) in faces.iter().enumerate() {
        for (j, second) in &faces[a + 1..] {
            let (x, y) = (&album.subjects[*i].id, &album.subjects[*j].id);
            if !changed.is_empty() && !changed.contains(x) && !changed.contains(y) {
                continue;
            }
            let mut nearest = f32::MAX;
            for u in first {
                for v in second {
                    let dot: f32 = u.iter().zip(v).map(|(p, q)| p * q).sum();
                    nearest = nearest.min(1.0 - dot);
                }
            }
            if nearest < ONE_FACE {
                close.push((nearest, *i, *j));
            }
        }
    }
    close.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Joined closest first; each group keeps the videos of all it holds.
    let mut root: Vec<usize> = (0..album.subjects.len()).collect();
    fn find(root: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while root[r] != r {
            r = root[r];
        }
        root[i] = r;
        r
    }
    let mut seen: HashMap<usize, HashSet<String>> = HashMap::new();
    for (_, i, j) in close {
        let (a, b) = (find(&mut root, i), find(&mut root, j));
        if a == b {
            continue;
        }
        let of = |k: usize, seen: &HashMap<usize, HashSet<String>>| {
            seen.get(&k).cloned().unwrap_or_else(|| {
                videos
                    .get(&album.subjects[k].id)
                    .cloned()
                    .unwrap_or_default()
            })
        };
        let (va, vb) = (of(a, &seen), of(b, &seen));
        let (sa, sb) = (&album.subjects[a], &album.subjects[b]);
        if !va.is_disjoint(&vb) || (sa.label.is_some() && sb.label.is_some()) {
            continue;
        }
        // The one the user named, else the one seen more, keeps its id.
        let a_keeps = match (sa.label.is_some(), sb.label.is_some()) {
            (true, false) => true,
            (false, true) => false,
            _ => {
                (sa.appearances.len(), std::cmp::Reverse(&sa.id))
                    >= (sb.appearances.len(), std::cmp::Reverse(&sb.id))
            }
        };
        let (keep, join) = if a_keeps { (a, b) } else { (b, a) };
        root[join] = keep;
        seen.insert(keep, va.union(&vb).cloned().collect());
    }

    let joined: Vec<(String, String)> = (0..album.subjects.len())
        .filter_map(|i| {
            let r = find(&mut root, i);
            (r != i).then(|| (album.subjects[i].id.clone(), album.subjects[r].id.clone()))
        })
        .collect();
    for (from, into) in &joined {
        absorb(album, from, into);
    }
    joined
}

/// Forget a subject. It comes back if the footage is analysed again, which is
/// worth saying in the interface: this tidies the album, it does not delete
/// anything from the footage.
pub fn forget(library: &Library, id: &str) -> Result<(), String> {
    let mut album = load(library);
    let before = album.subjects.len();
    album.subjects.retain(|s| s.id != id);
    if album.subjects.len() == before {
        return Err(format!("there is no subject {id}"));
    }
    save(library, &album)
}

fn one(subject: &Subject) -> Value {
    json!({
        "id": subject.id,
        "label": subject.label,
        "kind": format!("{:?}", subject.kind).to_lowercase(),
        "thumbnail": subject.thumbnail,
        "clips": subject.clips(),
        "shots": subject.shots(),
        "firstSeenAt": subject.first_seen_at,
        "updatedAt": subject.updated_at,
        "appearances": subject.appearances.iter().map(|a| json!({
            "projectId": a.project_id,
            "projectName": a.project_name,
            "sourcePath": a.source_path,
            "shots": a.shots,
            "at": a.at,
        })).collect::<Vec<_>>(),
    })
}

/// The album as a front end sees it. Signatures are left out: they are
/// several kilobytes each and mean nothing outside the matcher.
pub fn listing(library: &Library) -> Value {
    listing_in(library, crate::Lang::En)
}

pub fn listing_in(library: &Library, lang: crate::Lang) -> Value {
    let album = prune(library);
    json!({
        "subjects": album.subjects.iter().map(one).collect::<Vec<_>>(),
        "grouping": lang.t(
            "Subjects are grouped by how alike they look, not recognised. \
             Rename or split them as you see fit.",
            "人物是按长相相似程度分组的，不是人脸识别。名字由你来起，分错了可以移除。",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_contracts::footage_graph::{Shot, Source};
    use vb_contracts::{Blake3Hash, Micros};

    fn scratch(name: &str) -> Library {
        let root = std::env::temp_dir().join(format!("vb-album-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let library = Library::at(root);
        library.ensure().unwrap();
        library
    }

    /// A graph with one clip and the given subjects in its only shot.
    fn graph_with(path: &str, subjects: Vec<(&str, Vec<f64>)>) -> FootageGraph {
        let mut g = FootageGraph::new("p");
        g.sources.push(Source {
            id: "src-0001".into(),
            path: path.into(),
            content_hash: Blake3Hash::of_str(path),
            media_kind: None,
            duration_us: Micros::from_secs(10),
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
        g.shots.push(Shot {
            id: "shot-0000".into(),
            source_id: "src-0001".into(),
            start_us: Micros::ZERO,
            end_us: Micros::from_secs(10),
            shot_change_confidence: None,
            keyframes: vec![],
            description: None,
            entity_refs: subjects.iter().map(|(id, _)| (*id).to_string()).collect(),
            actions: vec![],
            audio: None,
            tags: vec![],
            segments: Vec::new(),
            timeline: Vec::new(),
        });
        for (id, signature) in subjects {
            g.entities.push(Entity {
                id: id.into(),
                kind: EntityKind::Person,
                label: None,
                first_seen_us: Some(Micros::ZERO),
                last_seen_us: Some(Micros::from_secs(10)),
                shot_ids: vec!["shot-0000".into()],
                confidence: Some(0.9),
                tracker: Some("AppleVisionFaces".into()),
                thumbnail_path: Some(format!("faces/{id}.jpg")),
                signature,
            });
        }
        g
    }

    #[test]
    fn the_same_subject_in_two_projects_is_one_entry_in_the_album() {
        let library = scratch("across-projects");
        let work = library.work_dir();
        let alice = vec![1.0f64, 0.0, 0.0, 0.2];

        record_run(
            &library,
            "p1",
            "Spring",
            &graph_with("/movies/a.mp4", vec![("person-001", alice.clone())]),
            &work,
        );
        // The same person, slightly differently lit, in another project.
        let again = vec![0.98f64, 0.05, 0.0, 0.22];
        let album = record_run(
            &library,
            "p2",
            "Summer",
            &graph_with("/movies/b.mp4", vec![("person-001", again)]),
            &work,
        );

        assert_eq!(album.subjects.len(), 1, "got {:?}", album.subjects);
        let subject = &album.subjects[0];
        assert_eq!(subject.clips(), 2);
        assert_eq!(subject.appearances[0].project_name, "Spring");
        assert_eq!(subject.appearances[1].project_name, "Summer");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn two_different_people_stay_two_entries() {
        let library = scratch("two-people");
        let album = record_run(
            &library,
            "p1",
            "Party",
            &graph_with(
                "/movies/a.mp4",
                vec![
                    ("person-001", vec![1.0, 0.0, 0.0, 0.0]),
                    ("person-002", vec![0.0, 1.0, 0.0, 0.0]),
                ],
            ),
            &library.work_dir(),
        );
        assert_eq!(album.subjects.len(), 2);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn re_running_a_project_does_not_double_the_counts() {
        let library = scratch("rerun");
        let graph = graph_with("/movies/a.mp4", vec![("person-001", vec![1.0, 0.1, 0.0])]);
        record_run(&library, "p1", "Spring", &graph, &library.work_dir());
        let album = record_run(&library, "p1", "Spring", &graph, &library.work_dir());
        assert_eq!(album.subjects.len(), 1);
        assert_eq!(album.subjects[0].clips(), 1);
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn a_name_the_user_gave_survives_the_next_run() {
        let library = scratch("naming");
        let graph = graph_with("/movies/a.mp4", vec![("person-001", vec![1.0, 0.1, 0.0])]);
        let album = record_run(&library, "p1", "Spring", &graph, &library.work_dir());
        let id = album.subjects[0].id.clone();

        rename(&library, &id, Some("Mum")).unwrap();
        let after = record_run(
            &library,
            "p2",
            "Summer",
            &graph_with("/movies/b.mp4", vec![("person-001", vec![1.0, 0.1, 0.01])]),
            &library.work_dir(),
        );
        assert_eq!(after.subjects[0].label.as_deref(), Some("Mum"));

        // And clearing it works, because a wrong name is worse than none.
        rename(&library, &id, None).unwrap();
        assert!(load(&library).subjects[0].label.is_none());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn subjects_without_a_signature_are_left_out() {
        // Nothing to match them by, so every run would add them again.
        let library = scratch("no-signature");
        let album = record_run(
            &library,
            "p1",
            "Spring",
            &graph_with("/movies/a.mp4", vec![("person-001", vec![])]),
            &library.work_dir(),
        );
        assert!(album.subjects.is_empty());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn the_listing_never_carries_signatures() {
        let library = scratch("listing");
        let project = crate::projects::Project::new("Spring");
        project.save(&library).unwrap();
        record_run(
            &library,
            &project.id,
            "Spring",
            &graph_with("/movies/a.mp4", vec![("person-001", vec![1.0, 0.5, 0.25])]),
            &library.work_dir(),
        );
        let listing = listing(&library).to_string();
        assert!(listing.contains("\"clips\":1"));
        assert!(
            !listing.contains("signature"),
            "signatures are kilobytes of noise to a front end: {listing}"
        );
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn a_rerun_replaces_what_the_project_saw_before() {
        let library = scratch("rerun-replaces");
        let (alice, bob) = (vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]);
        record_run(
            &library,
            "p1",
            "Spring",
            &graph_with("/movies/a.mp4", vec![("person-001", alice.clone())]),
            &library.work_dir(),
        );
        rename(&library, "sub-0001", Some("Alice")).unwrap();
        // Run again over other footage: Alice is still there, and keeps her
        // name; the clip she was in before is not.
        let album = record_run(
            &library,
            "p1",
            "Spring",
            &graph_with(
                "/movies/b.mp4",
                vec![("person-001", alice), ("person-002", bob)],
            ),
            &library.work_dir(),
        );
        assert_eq!(album.subjects.len(), 2);
        let alice = album.subjects.iter().find(|s| s.label.is_some()).unwrap();
        assert_eq!(alice.label.as_deref(), Some("Alice"));
        assert_eq!(alice.appearances.len(), 1);
        assert_eq!(alice.appearances[0].source_path, "/movies/b.mp4");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn people_seen_only_in_deleted_projects_leave_the_album() {
        let library = scratch("prune");
        let kept = crate::projects::Project::new("Kept");
        kept.save(&library).unwrap();
        let gone = vec![0.0, 1.0, 0.0, 0.0];
        let both = vec![1.0, 0.0, 0.0, 0.0];
        record_run(
            &library,
            &kept.id,
            "Kept",
            &graph_with("/movies/a.mp4", vec![("person-001", both.clone())]),
            &library.work_dir(),
        );
        record_run(
            &library,
            "p-deleted",
            "Deleted",
            &graph_with(
                "/movies/b.mp4",
                vec![("person-001", both), ("person-002", gone)],
            ),
            &library.work_dir(),
        );
        assert_eq!(load(&library).subjects.len(), 2);

        let album = prune(&library);
        assert_eq!(album.subjects.len(), 1, "only the person in a kept project");
        assert_eq!(album.subjects[0].appearances.len(), 1);
        assert_eq!(album.subjects[0].appearances[0].project_id, kept.id);
        assert_eq!(load(&library).subjects.len(), 1, "and it is saved");
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn merging_two_groups_makes_them_one_person_who_keeps_the_name() {
        let library = scratch("merge");
        record_run(
            &library,
            "p1",
            "Trip",
            &graph_with(
                "/movies/a.mp4",
                vec![
                    ("person-001", vec![1.0, 0.0, 0.0, 0.0]),
                    ("person-002", vec![0.0, 1.0, 0.0, 0.0]),
                ],
            ),
            &library.work_dir(),
        );
        rename(&library, "sub-0002", Some("Mum")).unwrap();
        let merged = merge(&library, "sub-0002", "sub-0001").unwrap();
        assert_eq!(merged["label"], "Mum");
        let album = load(&library);
        assert_eq!(album.subjects.len(), 1);
        assert_eq!(album.subjects[0].exemplars.len(), 1);
        assert!(merge(&library, "sub-0001", "sub-0001").is_err());
        assert!(merge(&library, "sub-0009", "sub-0001").is_err());
        let _ = std::fs::remove_dir_all(library.root());
    }

    #[test]
    fn forgetting_a_subject_removes_it_and_says_when_there_was_none() {
        let library = scratch("forget");
        record_run(
            &library,
            "p1",
            "Spring",
            &graph_with("/movies/a.mp4", vec![("person-001", vec![1.0, 0.5])]),
            &library.work_dir(),
        );
        assert!(forget(&library, "sub-0001").is_ok());
        assert!(load(&library).subjects.is_empty());
        assert!(forget(&library, "sub-0001").is_err());
        let _ = std::fs::remove_dir_all(library.root());
    }

    /// An AuraFace subject whose face is `distance` (cosine) from the first
    /// axis, turned towards `axis`, seen in `videos` hosted videos. Two
    /// turned towards different axes are `1 - (1 - d1)(1 - d2)` apart.
    fn face_subject(
        id: &str,
        distance: f64,
        axis: usize,
        videos: usize,
        label: Option<&str>,
    ) -> Subject {
        let mut signature = vec![0.0; FACE_ID_LEN];
        signature[0] = 1.0 - distance;
        signature[axis] = (1.0 - (1.0 - distance).powi(2)).sqrt();
        Subject {
            id: id.into(),
            label: label.map(str::to_string),
            kind: EntityKind::Person,
            thumbnail: None,
            signature,
            exemplars: vec![],
            appearances: (0..videos)
                .map(|n| Appearance {
                    project_id: HOSTED.into(),
                    project_name: String::new(),
                    source_path: format!("/{id}/{n}.mp4"),
                    shots: 1,
                    at: String::new(),
                })
                .collect(),
            first_seen_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn seen_in(pairs: &[(&str, &[&str])]) -> HashMap<String, HashSet<String>> {
        pairs
            .iter()
            .map(|(id, videos)| {
                (
                    id.to_string(),
                    videos.iter().map(|v| v.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn groups_of_one_face_are_joined_and_the_larger_keeps_its_id() {
        let mut album = Album {
            subjects: vec![
                face_subject("sub-0001", 0.0, 1, 2, None),
                face_subject("sub-0002", 0.30, 1, 5, None),
                // Close, but no closer than strangers come: 0.55 and 0.69.
                face_subject("sub-0003", 0.55, 2, 1, None),
            ],
        };
        let videos = seen_in(&[
            ("sub-0001", &["/a.mp4", "/b.mp4"]),
            ("sub-0002", &["/c.mp4"]),
            ("sub-0003", &["/d.mp4"]),
        ]);
        let joined = consolidate(&mut album, &videos, &HashSet::new());
        assert_eq!(joined, [("sub-0001".to_string(), "sub-0002".to_string())]);
        let ids: Vec<&str> = album.subjects.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["sub-0002", "sub-0003"]);
        // Matched by the joined face from now on.
        assert_eq!(album.subjects[0].exemplars.len(), 1);
        assert_eq!(album.subjects[0].appearances.len(), 7);
    }

    #[test]
    fn two_faces_of_one_video_or_two_named_people_stay_apart() {
        let close = || {
            vec![
                face_subject("sub-0001", 0.0, 1, 3, None),
                face_subject("sub-0002", 0.20, 1, 3, None),
            ]
        };
        let mut album = Album { subjects: close() };
        let together = seen_in(&[("sub-0001", &["/a.mp4"]), ("sub-0002", &["/a.mp4"])]);
        assert!(consolidate(&mut album, &together, &HashSet::new()).is_empty());

        let mut album = Album { subjects: close() };
        album.subjects[0].label = Some("妈妈".into());
        album.subjects[1].label = Some("外婆".into());
        let apart = seen_in(&[("sub-0001", &["/a.mp4"]), ("sub-0002", &["/b.mp4"])]);
        assert!(consolidate(&mut album, &apart, &HashSet::new()).is_empty());

        // One named: the name keeps its id, whoever was seen more.
        let mut album = Album { subjects: close() };
        album.subjects[0].label = Some("妈妈".into());
        album.subjects[0].appearances.truncate(1);
        let joined = consolidate(&mut album, &apart, &HashSet::new());
        assert_eq!(joined, [("sub-0002".to_string(), "sub-0001".to_string())]);
        assert_eq!(album.subjects[0].label.as_deref(), Some("妈妈"));
    }

    #[test]
    fn only_the_groups_a_pass_touched_are_measured() {
        let mut album = Album {
            subjects: vec![
                face_subject("sub-0001", 0.0, 1, 1, None),
                face_subject("sub-0002", 0.30, 1, 1, None),
            ],
        };
        let apart = seen_in(&[("sub-0001", &["/a.mp4"]), ("sub-0002", &["/b.mp4"])]);
        let untouched: HashSet<String> = ["sub-0009".to_string()].into_iter().collect();
        assert!(consolidate(&mut album, &apart, &untouched).is_empty());
        let touched: HashSet<String> = ["sub-0002".to_string()].into_iter().collect();
        assert_eq!(consolidate(&mut album, &apart, &touched).len(), 1);
    }
}
