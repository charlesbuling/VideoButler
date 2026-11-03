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

//! Memories: the video butler's collection read for the stories in it.
//!
//! Four kinds, each found from when and where every video was filmed and
//! every photo taken, and who is its subject -- numbers the catalogue
//! already holds -- and each made a project of its own, cut in the
//! background:
//!
//! - **Trips**: a few days of footage filmed together away from home, named
//!   after where most of it was filmed: "2023年9月到新疆的旅行回忆".
//! - **Growing up**: one person filmed over months or years, in order.
//! - **Years**: every video of a year, January to December, cut by time or
//!   by kind -- people, then scenery, then food, then the rest.
//! - **Groups**: the same people together, two or more, in video after
//!   video: a couple, a family, a club.
//!
//! # How a trip is told from home
//!
//! Home is the place filmed on the most days, provided those days are
//! spread over two months or more: footage that keeps coming back to one
//! place is where someone lives; a single week of footage is a trip, wherever
//! it was. Clips filmed away from home within a day and a half of each other
//! are one trip. Clips with no location join the trip they were filmed in
//! the middle of -- a drone or an action camera writes no GPS -- for up to
//! four days after its last located clip, and home clips within a day of
//! leaving or coming back are its departure and return.
//!
//! Who someone is comes from the album, which groups faces by likeness: a
//! person split into two groups grows up twice, and two people grouped as
//! one are one. The memory says who by their faces, and the user's names.

use crate::hosting::{self, Entry};
use crate::library::Library;
use crate::{Lang, entities};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use vb_media::GeoPoint;

/// A clip this far from home is away from it.
const HOME_KM: f64 = 60.0;
/// Home is where most was filmed this many days either side of a clip, or,
/// where little was filmed then, [`HOME_FAR`].
const HOME_NEAR: i64 = 91;
const HOME_FAR: i64 = 365;
/// A trip lasts this many days at most; longer is living somewhere.
const MAX_TRIP_DAYS: i64 = 45;
/// Clips of one trip are filmed no further apart than this.
const TRIP_GAP: i64 = 36 * 3600;
/// Clips with no location join a trip for this long after its last located
/// clip.
const UNLOCATED_TAIL: i64 = 4 * 86_400;
/// Home clips this close to the first or last clip away are the trip's
/// departure and return.
const DEPARTURE: i64 = 24 * 3600;
/// Longer than this is not a clip someone filmed but a film someone saved.
const LONGEST_CLIP_US: u64 = 30 * 60 * 1_000_000;
/// The most videos a memory's project is made from. A year of footage is
/// hundreds of clips and a cut holds a few dozen: a project of this many,
/// spread over the memory, analyses in minutes rather than hours.
pub const PROJECT_CLIPS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Travel,
    Growth,
    Annual,
    Group,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Travel => "travel",
            Kind::Growth => "growth",
            Kind::Annual => "annual",
            Kind::Group => "group",
        }
    }

    pub fn parse(id: &str) -> Option<Kind> {
        [Kind::Travel, Kind::Growth, Kind::Annual, Kind::Group]
            .into_iter()
            .find(|k| k.id() == id)
    }
}

/// One memory, as found.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    pub kind: Kind,
    /// The local day of its first and last video, `2023-09-29`.
    pub start: String,
    pub end: String,
    /// The year and month a pin shows.
    pub year: i32,
    pub month: u32,
    /// Its videos and photos, in the order they were filmed.
    pub videos: Vec<String>,
    /// How many of those are photos.
    #[serde(default)]
    pub photos: usize,
    /// Who it is about: one person growing up, the people of a group.
    #[serde(default)]
    pub subjects: Vec<String>,
    /// Where it was, for its name: a city, a province or a country.
    #[serde(default)]
    pub place_zh: Option<String>,
    #[serde(default)]
    pub place_en: Option<String>,
    /// Where its pin goes; none when none of its videos says where.
    #[serde(default)]
    pub pin: Option<GeoPoint>,
    /// The regions its videos were filmed in, which the map lights.
    #[serde(default)]
    pub regions: Vec<String>,
    /// Where each of its located videos was, for the map.
    #[serde(default)]
    pub points: Vec<GeoPoint>,
}

/// What became of a memory's project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemoryState {
    pub project_id: Option<String>,
    /// The videos and style it was last cut from, hashed: a memory whose
    /// footage has not changed is not cut again.
    pub footprint: Option<String>,
    /// A year's order: `category` (the default) or `time`.
    pub order: Option<String>,
    /// `pending`, `done`, `failed`, or `dismissed` when the user deleted the
    /// project, which is not made again unless asked for.
    pub status: Option<String>,
    pub error: Option<String>,
    pub at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Book {
    pub memories: Vec<Memory>,
    pub states: BTreeMap<String, MemoryState>,
    pub found_at: Option<String>,
}

fn book_path(library: &Library) -> std::path::PathBuf {
    hosting::dir(library).join("memories.json")
}

pub fn load(library: &Library) -> Book {
    std::fs::read_to_string(book_path(library))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(library: &Library, book: &Book) -> Result<(), String> {
    let path = book_path(library);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(book).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Time, as the camera's clock read
// ---------------------------------------------------------------------------

/// A video placed in time and space.
#[derive(Debug, Clone)]
struct Dated<'a> {
    entry: &'a Entry,
    /// Seconds since the epoch, UTC.
    at: i64,
    /// Its day on the clock where it was filmed.
    day: i64,
    point: Option<GeoPoint>,
    region: Option<String>,
    country: Option<String>,
}

/// The local offset a file name implies: phones and cameras name a video
/// by their own clock (`VID_20260214_035333`, `dji_mimo_20260706_213502`)
/// while the file says when in UTC. The whole quarter-hour between the two,
/// when they agree to within two minutes of one.
pub fn offset_from_name(name: &str, utc: i64) -> Option<i32> {
    let chars: Vec<char> = name.chars().collect();
    for start in 0..chars.len() {
        // Eight digits of a date, an optional separator, six of a time.
        let take = |from: usize, n: usize| -> Option<String> {
            let s: String = chars.get(from..from + n)?.iter().collect();
            s.chars().all(|c| c.is_ascii_digit()).then_some(s)
        };
        let Some(date) = take(start, 8) else { continue };
        if start > 0 && chars[start - 1].is_ascii_digit() {
            continue;
        }
        let after = start + 8;
        let clock = take(after, 6).or_else(|| {
            matches!(chars.get(after), Some('_' | '-' | ' ' | 'T')).then(|| take(after + 1, 6))?
        });
        let Some(clock) = clock else { continue };
        let text = format!(
            "{}-{}-{}T{}:{}:{}Z",
            &date[0..4],
            &date[4..6],
            &date[6..8],
            &clock[0..2],
            &clock[2..4],
            &clock[4..6]
        );
        let Some(local) = vb_contracts::time::capture_seconds(&text) else {
            continue;
        };
        let apart = local - utc;
        let quarters = (apart as f64 / 900.0).round() as i64;
        if quarters.abs() <= 14 * 4 && (apart - quarters * 900).abs() <= 120 {
            return Some((quarters * 15) as i32);
        }
    }
    None
}

/// Days since the epoch to a calendar date (Howard Hinnant's algorithm).
pub fn civil(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

fn date_text(day: i64) -> String {
    let (y, m, d) = civil(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Every video a memory can be made of, placed in time: filmed (the
/// camera's clock, not the file's), not a film saved off the internet, and
/// once each however many copies there are.
fn date_all(entries: &[Entry]) -> Vec<Dated<'_>> {
    let world = vb_geo::world();
    let mut seen = std::collections::HashSet::new();
    let mut dated: Vec<Dated> = Vec::new();
    for entry in entries {
        if entry.error.is_some() || entry.duration_us > LONGEST_CLIP_US {
            continue;
        }
        let Some(at) = entry
            .captured_at
            .as_deref()
            .and_then(vb_contracts::time::capture_seconds)
        else {
            continue;
        };
        if !entry.fingerprint.is_empty() && !seen.insert(entry.fingerprint.clone()) {
            continue;
        }
        let (region, country) = match entry.location {
            Some(p) => {
                let here = world.locate(p.lat, p.lon);
                (
                    here.region().map(|r| r.id.clone()),
                    here.country_code().map(str::to_string),
                )
            }
            None => (None, None),
        };
        dated.push(Dated {
            entry,
            at,
            day: 0,
            point: entry.location,
            region,
            country,
        });
    }
    dated.sort_by_key(|d| (d.at, d.entry.path.clone()));

    // The clock each was filmed by: its own name's, else the zone where it
    // was, else the nearest located clip's within three days, else the one
    // most of the located clips keep.
    let zone = |d: &Dated| {
        d.point
            .map(|p| vb_geo::zone_minutes(d.country.as_deref(), p.lon))
    };
    let mut common: HashMap<i32, usize> = HashMap::new();
    for d in &dated {
        if let Some(z) = zone(d) {
            *common.entry(z).or_insert(0) += 1;
        }
    }
    let usual = common
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(0, |(z, _)| z);
    let offsets: Vec<i32> = (0..dated.len())
        .map(|i| {
            let d = &dated[i];
            offset_from_name(&d.entry.name(), d.at)
                .or_else(|| zone(d))
                .or_else(|| {
                    dated
                        .iter()
                        .filter(|o| o.point.is_some() && (o.at - d.at).abs() <= 3 * 86_400)
                        .min_by_key(|o| (o.at - d.at).abs())
                        .and_then(zone)
                })
                .unwrap_or(usual)
        })
        .collect();
    for (d, offset) in dated.iter_mut().zip(offsets) {
        d.day = (d.at + offset as i64 * 60).div_euclid(86_400);
    }
    dated
}

// ---------------------------------------------------------------------------
// Places
// ---------------------------------------------------------------------------

/// Points within [`HOME_KM`] of each other, grouped greedily.
struct Spot {
    lat: f64,
    lon: f64,
    members: Vec<usize>,
}

fn spots(dated: &[Dated], among: impl Iterator<Item = usize>) -> Vec<Spot> {
    let mut spots: Vec<Spot> = Vec::new();
    for i in among {
        let Some(p) = dated[i].point else { continue };
        match spots
            .iter_mut()
            .find(|s| vb_geo::distance_km(s.lat, s.lon, p.lat, p.lon) <= HOME_KM)
        {
            Some(spot) => {
                let n = spot.members.len() as f64;
                spot.lat = (spot.lat * n + p.lat) / (n + 1.0);
                spot.lon = (spot.lon * n + p.lon) / (n + 1.0);
                spot.members.push(i);
            }
            None => spots.push(Spot {
                lat: p.lat,
                lon: p.lon,
                members: vec![i],
            }),
        }
    }
    spots
}

/// A place filmed on these days is lived in, not visited: three days or
/// more, spanning two months or more.
fn lived(days: &[i64]) -> bool {
    match (days.first(), days.last()) {
        (Some(first), Some(last)) => days.len() >= 3 && last - first >= 60,
        _ => false,
    }
}

/// The days of sorted `days` from `from` to `to`, both included.
fn days_in(days: &[i64], from: i64, to: i64) -> &[i64] {
    let start = days.partition_point(|&d| d < from);
    let end = days.partition_point(|&d| d <= to).max(start);
    &days[start..end]
}

/// Whether each clip was filmed at home, as far as the footage says. Home is
/// where someone lives and comes back to, and people move:
///
/// - A clip is filmed where someone lived when it was filmed at a place
///   lived in at the time: within three months either side, or a year where
///   little was filmed -- or at the place lived in on the most days overall.
/// - Those clips, in the order filmed, fall into runs at one place. A run of
///   six weeks or less between two runs at another place, which was lived in
///   at least as much around then, is a visit: a weekend back in the city
///   lived in before is a trip there. The rest is home -- a year of footage
///   from the old city is not a string of trips, nor is the last weekend in
///   it before the move.
fn at_home(dated: &[Dated]) -> Vec<bool> {
    let spots = spots(dated, 0..dated.len());
    let mut spot_of: Vec<Option<usize>> = vec![None; dated.len()];
    let days: Vec<Vec<i64>> = spots
        .iter()
        .enumerate()
        .map(|(s, spot)| {
            for &i in &spot.members {
                spot_of[i] = Some(s);
            }
            let days: BTreeSet<i64> = spot.members.iter().map(|&i| dated[i].day).collect();
            days.into_iter().collect()
        })
        .collect();
    let longest = (0..spots.len())
        .filter(|&s| lived(&days[s]))
        .max_by_key(|&s| days[s].len());
    let mut home: Vec<bool> = dated
        .iter()
        .zip(&spot_of)
        .map(|(d, spot)| {
            spot.is_some_and(|s| {
                longest == Some(s)
                    || [HOME_NEAR, HOME_FAR]
                        .iter()
                        .any(|&w| lived(days_in(&days[s], d.day - w, d.day + w)))
            })
        })
        .collect();

    let mut runs: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, spot) in spot_of.iter().enumerate() {
        let Some(s) = *spot else { continue };
        if !home[i] {
            continue;
        }
        match runs.last_mut() {
            Some((at, clips)) if *at == s => clips.push(i),
            _ => runs.push((s, vec![i])),
        }
    }
    // How much a place was lived in around a run: its days within three
    // months of it.
    let lived_in = |s: usize, first: i64, last: i64| {
        days_in(&days[s], first - HOME_NEAR, last + HOME_NEAR).len()
    };
    let mut visits = Vec::new();
    for k in 1..runs.len().saturating_sub(1) {
        let (at, clips) = &runs[k];
        let (before, after) = (runs[k - 1].0, runs[k + 1].0);
        let first = dated[clips[0]].day;
        let last = clips.last().map_or(first, |&i| dated[i].day);
        if before == after
            && last - first <= MAX_TRIP_DAYS
            && lived_in(before, first, last) >= lived_in(*at, first, last)
        {
            visits.extend(clips.iter().copied());
        }
    }
    for i in visits {
        home[i] = false;
    }
    home
}

/// The place most of a memory's videos were filmed at, for its pin.
fn main_spot(dated: &[Dated], members: &[usize]) -> Option<GeoPoint> {
    spots(dated, members.iter().copied())
        .into_iter()
        .max_by_key(|s| s.members.len())
        .map(|s| GeoPoint {
            lat: s.lat,
            lon: s.lon,
        })
}

fn regions_of(dated: &[Dated], members: &[usize]) -> Vec<String> {
    let set: BTreeSet<String> = members
        .iter()
        .filter_map(|&i| dated[i].region.clone())
        .collect();
    set.into_iter().collect()
}

fn points_of(dated: &[Dated], members: &[usize]) -> Vec<GeoPoint> {
    let mut seen = std::collections::HashSet::new();
    members
        .iter()
        .filter_map(|&i| dated[i].point)
        .filter(|p| {
            seen.insert((
                (p.lat * 100.0).round() as i64,
                (p.lon * 100.0).round() as i64,
            ))
        })
        .collect()
}

/// What a trip is named after: the city most of it was filmed near, else
/// the province (or country) most of it was in, else its two main ones. Never
/// the country of home: a trip from Los Angeles to New York is not a trip to
/// the United States.
fn name_trip(
    dated: &[Dated],
    away: &[usize],
    home: Option<(f64, f64)>,
) -> (String, String, String) {
    let world = vb_geo::world();
    let home_country =
        home.and_then(|(lat, lon)| world.locate(lat, lon).country_code().map(str::to_string));
    let share = |key: &dyn Fn(usize) -> Option<String>| -> Option<(String, f64)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for &i in away {
            if let Some(k) = key(i) {
                *counts.entry(k).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|(k, n)| (k, n as f64 / away.len().max(1) as f64))
    };
    let city = |i: usize| {
        let p = dated[i].point?;
        world
            .near(p.lat, p.lon, vb_geo::NEAR_PLACE_KM)
            .map(|c| format!("{}\t{}", c.name(true), c.en))
    };
    if let Some((names, s)) = share(&city)
        && s >= 0.7
    {
        let (zh, en) = names.split_once('\t').unwrap_or((&names, &names));
        return (format!("city:{en}"), zh.to_string(), en.to_string());
    }
    let named = |id: &str| {
        world
            .region(id)
            .map_or((id.to_string(), id.to_string()), |r| {
                (r.zh.clone(), r.en.clone())
            })
    };
    if let Some((region, s)) = share(&|i| dated[i].region.clone())
        && s >= 0.7
    {
        let (zh, en) = named(&region);
        return (region, zh, en);
    }
    if let Some((country, s)) = share(&|i| dated[i].country.clone())
        && s >= 0.7
        && home_country.as_deref() != Some(country.as_str())
    {
        let (zh, en) = named(&country);
        return (country, zh, en);
    }
    let mut counts: Vec<(String, usize)> = {
        let mut m: HashMap<String, usize> = HashMap::new();
        for &i in away {
            if let Some(r) = &dated[i].region {
                *m.entry(r.clone()).or_insert(0) += 1;
            }
        }
        m.into_iter().collect()
    };
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let top: Vec<(String, String)> = counts.iter().take(2).map(|(id, _)| named(id)).collect();
    (
        counts
            .iter()
            .take(2)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
            .join("+"),
        top.iter()
            .map(|(zh, _)| zh.clone())
            .collect::<Vec<_>>()
            .join("、"),
        top.iter()
            .map(|(_, en)| en.clone())
            .collect::<Vec<_>>()
            .join(" and "),
    )
}

// ---------------------------------------------------------------------------
// Finding them
// ---------------------------------------------------------------------------

fn memory(dated: &[Dated], kind: Kind, id: String, members: &[usize]) -> Memory {
    let first = members.iter().map(|&i| dated[i].day).min().unwrap_or(0);
    let last = members.iter().map(|&i| dated[i].day).max().unwrap_or(0);
    let (year, month, _) = civil(first);
    Memory {
        id,
        kind,
        start: date_text(first),
        end: date_text(last),
        year,
        month,
        videos: members
            .iter()
            .map(|&i| dated[i].entry.path.clone())
            .collect(),
        photos: members
            .iter()
            .filter(|&&i| dated[i].entry.is_photo())
            .count(),
        subjects: Vec::new(),
        place_zh: None,
        place_en: None,
        pin: main_spot(dated, members),
        regions: regions_of(dated, members),
        points: points_of(dated, members),
    }
}

fn trips(dated: &[Dated]) -> Vec<Memory> {
    #[derive(PartialEq, Clone, Copy)]
    enum Where {
        Home,
        Away,
        Unknown,
    }
    let wheres: Vec<Where> = dated
        .iter()
        .zip(at_home(dated))
        .map(|(d, home)| match (d.point, home) {
            (None, _) => Where::Unknown,
            (Some(_), true) => Where::Home,
            (Some(_), false) => Where::Away,
        })
        .collect();

    // Runs of clips away, with the unlocated clips filmed among them.
    let mut chains: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut last_away: Option<i64> = None;
    for (i, d) in dated.iter().enumerate() {
        let last = current.last().map(|&j| dated[j].at);
        let close = |current: &mut Vec<usize>, chains: &mut Vec<Vec<usize>>| {
            if !current.is_empty() {
                chains.push(std::mem::take(current));
            }
        };
        match wheres[i] {
            Where::Away => {
                if last.is_some_and(|t| d.at - t > TRIP_GAP) {
                    close(&mut current, &mut chains);
                }
                current.push(i);
                last_away = Some(d.at);
            }
            Where::Unknown => {
                if last.is_some_and(|t| d.at - t <= TRIP_GAP)
                    && last_away.is_some_and(|a| d.at - a <= UNLOCATED_TAIL)
                {
                    current.push(i);
                }
            }
            Where::Home => {
                if last_away.is_none_or(|a| d.at - a > DEPARTURE) {
                    close(&mut current, &mut chains);
                }
            }
        }
    }
    if !current.is_empty() {
        chains.push(current);
    }

    let mut out = Vec::new();
    for chain in chains {
        let away: Vec<usize> = chain
            .iter()
            .copied()
            .filter(|&i| wheres[i] == Where::Away)
            .collect();
        let (Some(&first), Some(&last)) = (away.first(), away.last()) else {
            continue;
        };
        // Unlocated clips after the last one away are kept only up to it
        // and the tail; before the first, never.
        let mut members: Vec<usize> = chain
            .iter()
            .copied()
            .filter(|&i| dated[i].at >= dated[first].at)
            .collect();
        // Departure and return, filmed at home.
        for (i, d) in dated.iter().enumerate() {
            if wheres[i] == Where::Home
                && ((d.at < dated[first].at && dated[first].at - d.at <= DEPARTURE)
                    || (d.at > dated[last].at && d.at - dated[last].at <= DEPARTURE))
            {
                members.push(i);
            }
        }
        members.sort_by_key(|&i| (dated[i].at, i));
        members.dedup();
        // Days away, not counting the departure and the return: a day out
        // with a clip at home the evening before is still a day out.
        let days: BTreeSet<i64> = members
            .iter()
            .filter(|&&i| wheres[i] != Where::Home)
            .map(|&i| dated[i].day)
            .collect();
        let span = days.last().unwrap_or(&0) - days.first().unwrap_or(&0);
        if away.len() < 3 || days.len() < 2 || span > MAX_TRIP_DAYS {
            continue;
        }
        // Named from the home it left: the last clip at home before it.
        let left = (0..first)
            .rev()
            .find(|&j| wheres[j] == Where::Home)
            .and_then(|j| dated[j].point)
            .map(|p| (p.lat, p.lon));
        let (key, zh, en) = name_trip(dated, &away, left);
        let start = members.iter().map(|&i| dated[i].day).min().unwrap_or(0);
        let mut m = memory(
            dated,
            Kind::Travel,
            format!("travel-{}-{}", date_text(start).replace('-', ""), key),
            &members,
        );
        // The pin goes where the trip went, not to the airport it left from.
        m.pin = main_spot(dated, &away);
        m.place_zh = Some(zh);
        m.place_en = Some(en);
        out.push(m);
    }

    // Trips nothing says the place of: a run of clips with no location at
    // all, a day and a half apart at most, with a drone flown on two days
    // or more of it. A phone backup can lose every fix -- Android zeroes
    // them in the copy -- and a drone is flown on holiday: on one such
    // backup a week away was 202 clips with no location, drone footage on
    // seven of its days, and the one other drone flight in years was a
    // single afternoon.
    let taken: std::collections::HashSet<usize> = out
        .iter()
        .flat_map(|m| m.videos.iter())
        .filter_map(|v| dated.iter().position(|d| &d.entry.path == v))
        .collect();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for (i, d) in dated.iter().enumerate() {
        let apart = run.last().is_some_and(|&j| d.at - dated[j].at > TRIP_GAP);
        if (wheres[i] != Where::Unknown || taken.contains(&i) || apart) && !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
        if wheres[i] == Where::Unknown && !taken.contains(&i) {
            run.push(i);
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    for run in runs {
        let days: BTreeSet<i64> = run.iter().map(|&i| dated[i].day).collect();
        let flown: BTreeSet<i64> = run
            .iter()
            .filter(|&&i| flown(dated[i].entry))
            .map(|&i| dated[i].day)
            .collect();
        let span = days.last().unwrap_or(&0) - days.first().unwrap_or(&0);
        if run.len() < 3 || flown.len() < 2 || span > MAX_TRIP_DAYS {
            continue;
        }
        let (first, last) = (run[0], run[run.len() - 1]);
        let mut members = run.clone();
        for (i, d) in dated.iter().enumerate() {
            if wheres[i] == Where::Home
                && ((d.at < dated[first].at && dated[first].at - d.at <= DEPARTURE)
                    || (d.at > dated[last].at && d.at - dated[last].at <= DEPARTURE))
            {
                members.push(i);
            }
        }
        members.sort_by_key(|&i| (dated[i].at, i));
        let start = members.iter().map(|&i| dated[i].day).min().unwrap_or(0);
        let mut m = memory(
            dated,
            Kind::Travel,
            format!("travel-{}-unplaced", date_text(start).replace('-', "")),
            &members,
        );
        // Where it went is unknown, so it has no pin and lights nothing:
        // the departure filmed at home is not where the trip was.
        m.pin = None;
        m.regions = Vec::new();
        m.points = Vec::new();
        out.push(m);
    }
    out.sort_by(|a, b| a.start.cmp(&b.start));
    out
}

/// Filmed by a drone, as DJI Fly names what it saves.
fn flown(entry: &Entry) -> bool {
    entry.name().to_lowercase().starts_with("dji_fly_")
}

fn months(dated: &[Dated], members: &[usize]) -> usize {
    members
        .iter()
        .map(|&i| {
            let (y, m, _) = civil(dated[i].day);
            (y, m)
        })
        .collect::<BTreeSet<_>>()
        .len()
}

/// Labels that name a child, and an adult, in either labeller's words.
const CHILD_LABELS: [&str; 6] = ["baby", "toddler", "child", "children", "teen", "teenager"];
const ADULT_LABELS: [&str; 4] = ["adult", "man", "woman", "elderly person"];

/// The strongest of `names` among a frame's labels, 0 when none is there.
fn strongest(labels: &[vb_infer::ScoredLabel], names: &[&str]) -> f64 {
    labels
        .iter()
        .filter(|l| names.contains(&l.label.as_str()))
        .map(|l| l.confidence)
        .fold(0.0, f64::max)
}

/// The labels of each frame `subject` is a subject of with no other
/// subject in it: the frames whose labels are about them.
fn frames_alone<'a>(
    subject: &'a str,
    look: &'a hosting::Look,
) -> impl Iterator<Item = &'a [vb_infer::ScoredLabel]> + 'a {
    let subjects: Vec<&hosting::SeenFace> = look
        .faces
        .iter()
        .filter(|f| f.area >= vb_perception::categories::SUBJECT_FACE)
        .collect();
    let mut frames = Vec::new();
    for face in subjects
        .iter()
        .filter(|f| f.subject.as_deref() == Some(subject))
    {
        for &frame in &face.frames {
            let alone = !subjects
                .iter()
                .any(|other| !std::ptr::eq(*other, *face) && other.frames.contains(&frame));
            if alone && let Some(labels) = look.frames.get(frame) {
                frames.push(labels.as_slice());
            }
        }
    }
    frames.into_iter()
}

/// Whether someone is a child, as far as the labels say. Nothing on the
/// device estimates age, but a frame someone is alone in is labelled for
/// them: a child when the frames that read as a child outnumber those that
/// read as an adult. On 2,555 real videos the three children read so in
/// 152 of 169 frames, 179 of 180 and 61 of 66, and no adult in more than a
/// fifth of theirs.
fn is_child(subject: &str, dated: &[Dated], members: &[usize]) -> bool {
    let (mut child, mut adult) = (0usize, 0usize);
    for &i in members {
        for labels in frames_alone(subject, &dated[i].entry.look) {
            let (c, a) = (
                strongest(labels, &CHILD_LABELS),
                strongest(labels, &ADULT_LABELS),
            );
            if c > a {
                child += 1;
            } else if a > c {
                adult += 1;
            }
        }
    }
    child >= 2 && child > adult
}

/// Whether a video's own frames say the face filed as `subject` there is
/// a grown-up's: every frame it is alone in reads as an adult and as no
/// child at all. A parent at the edge of a child's frame makes it read as
/// an adult as well, but beside a baby or a child; a frame with no child in
/// its words is someone else's. On 2,555 real videos this left out the two
/// strangers filed with a baby whose frames were theirs alone -- a woman at
/// karaoke and a live-stream host, both in 2020 -- and nothing else of
/// the two children's 246 dated videos.
fn reads_as_grown_up(subject: &str, look: &hosting::Look) -> bool {
    let mut any = false;
    for labels in frames_alone(subject, look) {
        if strongest(labels, &CHILD_LABELS) > 0.0 || strongest(labels, &ADULT_LABELS) <= 0.0 {
            return false;
        }
        any = true;
    }
    any
}

/// How far apart, in days, two sightings of a child may be and still be one
/// stretch of their growing up.
const CHILDHOOD_GAP: i64 = 365;

/// The sightings a child's growing-up memory is made of: those their own
/// frames do not contradict, in the one stretch of time that holds the
/// most of them. The face model files a stranger with a child now and
/// then, and such a sighting stands apart in time: on 2,555 real videos a
/// baby filmed through 2025 and 2026 had been seen at a gathering in 2021
/// (another little girl) and at a dinner in 2023 (a man), each more than a
/// year from the rest, while a child filmed over months is filmed again
/// within the year.
fn childhood(subject: &str, dated: &[Dated], members: &[usize]) -> Vec<usize> {
    let mut kept: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&i| !reads_as_grown_up(subject, &dated[i].entry.look))
        .collect();
    kept.sort_by_key(|&i| (dated[i].day, i));
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for i in kept {
        match runs.last_mut() {
            Some(run) if dated[i].day - dated[run[run.len() - 1]].day <= CHILDHOOD_GAP => {
                run.push(i)
            }
            _ => runs.push(vec![i]),
        }
    }
    // The longest; of two as long, the later.
    runs.into_iter()
        .max_by_key(|run| (run.len(), dated[run[run.len() - 1]].day))
        .unwrap_or_default()
}

fn growing_up(dated: &[Dated]) -> Vec<Memory> {
    let mut by_subject: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, d) in dated.iter().enumerate() {
        for subject in d.entry.subjects() {
            by_subject.entry(subject).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (subject, members) in by_subject {
        let members = childhood(subject, dated, &members);
        let first = members.iter().map(|&i| dated[i].day).min().unwrap_or(0);
        let last = members.iter().map(|&i| dated[i].day).max().unwrap_or(0);
        if members.len() < 5
            || months(dated, &members) < 3
            || last - first < 90
            || !is_child(subject, dated, &members)
        {
            continue;
        }
        let mut m = memory(dated, Kind::Growth, format!("growth-{subject}"), &members);
        m.subjects = vec![subject.to_string()];
        out.push(m);
    }
    out
}

fn years(dated: &[Dated]) -> Vec<Memory> {
    let mut by_year: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for (i, d) in dated.iter().enumerate() {
        by_year.entry(civil(d.day).0).or_default().push(i);
    }
    let mut out = Vec::new();
    for (year, members) in by_year {
        let days: BTreeSet<i64> = members.iter().map(|&i| dated[i].day).collect();
        if members.len() < 6 || days.len() < 3 {
            continue;
        }
        let mut m = memory(dated, Kind::Annual, format!("annual-{year}"), &members);
        m.year = year;
        out.push(m);
    }
    out
}

fn groups(dated: &[Dated]) -> Vec<Memory> {
    let sets: Vec<BTreeSet<&str>> = dated
        .iter()
        .map(|d| d.entry.subjects().into_iter().collect())
        .collect();
    let mut candidates: BTreeSet<Vec<&str>> = BTreeSet::new();
    let many: Vec<&BTreeSet<&str>> = sets.iter().filter(|s| s.len() >= 2).collect();
    for (a, set) in many.iter().enumerate() {
        candidates.insert(set.iter().copied().collect());
        for other in &many[a + 1..] {
            let both: Vec<&str> = set.intersection(other).copied().collect();
            if both.len() >= 2 {
                candidates.insert(both);
            }
        }
    }
    let support = |group: &[&str]| -> Vec<usize> {
        (0..dated.len())
            .filter(|&i| group.iter().all(|s| sets[i].contains(s)))
            .collect()
    };
    let mut kept: Vec<(Vec<&str>, Vec<usize>)> = candidates
        .into_iter()
        .filter(|g| g.len() <= 8)
        .map(|g| {
            let members = support(&g);
            (g, members)
        })
        .filter(|(_, members)| {
            let days: BTreeSet<i64> = members.iter().map(|&i| dated[i].day).collect();
            members.len() >= 3 && days.len() >= 2
        })
        .collect();
    // A group always seen with one more person is that larger group.
    let closed: Vec<bool> = kept
        .iter()
        .map(|(g, members)| {
            !kept.iter().any(|(other, theirs)| {
                other.len() > g.len()
                    && g.iter().all(|s| other.contains(s))
                    && theirs.len() == members.len()
            })
        })
        .collect();
    let mut i = 0;
    kept.retain(|_| {
        i += 1;
        closed[i - 1]
    });
    kept.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    kept.truncate(12);
    kept.into_iter()
        .map(|(group, members)| {
            let mut m = memory(
                dated,
                Kind::Group,
                format!("group-{}", group.join("+")),
                &members,
            );
            m.subjects = group.iter().map(|s| s.to_string()).collect();
            m
        })
        .collect()
}

/// Every memory in the collection.
pub fn find(entries: &[Entry]) -> Vec<Memory> {
    let dated = date_all(entries);
    let mut all = trips(&dated);
    all.extend(years(&dated));
    all.extend(growing_up(&dated));
    all.extend(groups(&dated));
    all
}

/// Find the memories again and keep what became of each.
pub fn refresh(library: &Library) -> Result<Book, String> {
    let catalogue = hosting::load(library);
    let mut book = load(library);
    book.memories = find(&catalogue.entries);
    book.found_at = Some(vb_contracts::now_rfc3339());
    save(library, &book)?;
    Ok(book)
}

// ---------------------------------------------------------------------------
// Their names
// ---------------------------------------------------------------------------

fn month_name(m: u32) -> &'static str {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][(m.clamp(1, 12) - 1) as usize]
}

/// Who a memory is about, by the names the user gave; the unnamed by how
/// many they are.
fn people(memory: &Memory, album: &entities::Album, lang: Lang) -> Option<String> {
    let names: Vec<String> = memory
        .subjects
        .iter()
        .filter_map(|id| album.subjects.iter().find(|s| &s.id == id)?.label.clone())
        .collect();
    if names.is_empty() || names.len() < memory.subjects.len() {
        return None;
    }
    Some(match lang {
        Lang::Zh => match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{}和{last}", rest.join("、")),
            _ => names.join(""),
        },
        Lang::En => match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => names.join(""),
        },
    })
}

pub fn title(memory: &Memory, album: &entities::Album, lang: Lang) -> String {
    let who = people(memory, album, lang);
    match (memory.kind, lang) {
        (Kind::Travel, Lang::Zh) => match memory.place_zh.as_deref() {
            Some(place) => format!("{}年{}月到{place}的旅行回忆", memory.year, memory.month),
            None => format!("{}年{}月的旅行回忆", memory.year, memory.month),
        },
        (Kind::Travel, Lang::En) => match memory.place_en.as_deref() {
            Some(place) => format!(
                "Trip to {place}, {} {}",
                month_name(memory.month),
                memory.year
            ),
            None => format!("A trip, {} {}", month_name(memory.month), memory.year),
        },
        (Kind::Growth, Lang::Zh) => format!("{}的成长记忆", who.as_deref().unwrap_or("TA")),
        (Kind::Growth, Lang::En) => match who {
            Some(name) => format!("{name} growing up"),
            None => "Growing up".to_string(),
        },
        (Kind::Annual, Lang::Zh) => format!("{}年度回忆", memory.year),
        (Kind::Annual, Lang::En) => format!("{} in review", memory.year),
        (Kind::Group, Lang::Zh) => match who {
            Some(names) => format!("{names}的共同回忆"),
            None => format!("{}人同框的回忆", memory.subjects.len()),
        },
        (Kind::Group, Lang::En) => match who {
            Some(names) => format!("{names} together"),
            None => format!("{} people together", memory.subjects.len()),
        },
    }
}

// ---------------------------------------------------------------------------
// Their projects
// ---------------------------------------------------------------------------

/// `n` of `items`, spread evenly over them, first and last included.
fn spread<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    if items.len() <= n || n == 0 {
        return items.to_vec();
    }
    if n == 1 {
        return vec![items[items.len() / 2].clone()];
    }
    (0..n)
        .map(|k| items[(k * (items.len() - 1) + (n - 1) / 2) / (n - 1)].clone())
        .collect()
}

/// The videos a memory's project is made from: all of them when they fit,
/// else [`PROJECT_CLIPS`] spread over its time -- and for a year spread over
/// its sections too, so each of people, scenery, food and the rest is in
/// the cut in proportion, none left out.
///
/// Photos fill what the videos cannot: a memory of forty videos or more is
/// cut from videos as it was before photos were filed, and one of fewer
/// takes every video and photos spread over its time for the rest. The cut
/// holds a photo three seconds at most and never lets photos crowd out the
/// videos, so more photos than that would be analysed for nothing.
pub fn project_videos(memory: &Memory, catalogue: &hosting::Catalogue) -> Vec<String> {
    let entries: HashMap<&str, &Entry> = catalogue
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let (photos, videos): (Vec<String>, Vec<String>) = memory
        .videos
        .iter()
        .cloned()
        .partition(|p| entries.get(p.as_str()).is_some_and(|e| e.is_photo()));
    if videos.len() < PROJECT_CLIPS {
        let mut chosen = videos;
        chosen.extend(spread(&photos, PROJECT_CLIPS - chosen.len()));
        let order: HashMap<&String, usize> = memory
            .videos
            .iter()
            .enumerate()
            .map(|(i, p)| (p, i))
            .collect();
        chosen.sort_by_key(|p| order.get(p).copied().unwrap_or(usize::MAX));
        return chosen;
    }
    if memory.kind != Kind::Annual || videos.len() <= PROJECT_CLIPS {
        return spread(&videos, PROJECT_CLIPS);
    }
    let section = |path: &String| -> usize {
        let Some(entry) = entries.get(path.as_str()) else {
            return 3;
        };
        ["people", "scenery", "food"]
            .iter()
            .position(|c| entry.is_in(c))
            .unwrap_or(3)
    };
    let mut sections: Vec<Vec<String>> = vec![Vec::new(); 4];
    for path in &videos {
        sections[section(path)].push(path.clone());
    }
    let total = videos.len() as f64;
    let mut quotas: Vec<usize> = sections
        .iter()
        .map(|s| {
            if s.is_empty() {
                0
            } else {
                ((s.len() as f64 / total * PROJECT_CLIPS as f64).round() as usize)
                    .max(4.min(s.len()))
            }
        })
        .collect();
    while quotas.iter().sum::<usize>() > PROJECT_CLIPS {
        let largest = (0..4).max_by_key(|&k| quotas[k]).unwrap_or(0);
        quotas[largest] -= 1;
    }
    let mut chosen: Vec<String> = sections
        .iter()
        .zip(&quotas)
        .flat_map(|(s, &q)| spread(s, q))
        .collect();
    let order: HashMap<&String, usize> = memory
        .videos
        .iter()
        .enumerate()
        .map(|(i, p)| (p, i))
        .collect();
    chosen.sort_by_key(|p| order.get(p).copied().unwrap_or(usize::MAX));
    chosen
}

/// The style a memory is cut in.
pub fn style(memory: &Memory, state: &MemoryState) -> crate::StyleChoice {
    use vb_narrative::Topic;
    crate::StyleChoice {
        topic: Some(match memory.kind {
            Kind::Travel => Topic::Travel,
            Kind::Growth => Topic::Growth,
            Kind::Annual => Topic::Annual,
            Kind::Group => Topic::Together,
        }),
        order: (memory.kind == Kind::Annual)
            .then(|| state.order.clone().unwrap_or_else(|| "category".into())),
        ..crate::StyleChoice::default()
    }
}

fn footprint(videos: &[String], style: &crate::StyleChoice) -> String {
    let style = serde_json::to_string(style).unwrap_or_default();
    let mut parts: Vec<&[u8]> = videos.iter().map(|v| v.as_bytes()).collect();
    parts.push(style.as_bytes());
    vb_contracts::Blake3Hash::of_parts(&parts).to_hex()
}

/// Cuts one memory's project: which of how many, the project's id and its
/// name, reporting as it goes.
pub type CutOne<'a> = dyn Fn(usize, usize, &str, &str) -> Result<Value, String> + 'a;

/// Make each memory's project, and cut those whose footage changed: the
/// ones named, or every one not dismissed. A memory whose project the user
/// deleted is dismissed and not made again unless named. Stops at the
/// next memory when `cancelled` says so; one that fails is recorded and the
/// rest carry on. `run` cuts one project, reporting as it goes.
pub fn generate(
    library: &Library,
    only: Option<&[String]>,
    cancelled: &dyn Fn() -> bool,
    run: &CutOne,
) -> Result<Vec<Value>, String> {
    let mut book = load(library);
    let catalogue = hosting::load(library);
    let album = entities::load(library);
    let lang = library.settings().lang();
    let editors = library.settings().default_editors.clone();
    let mut order: Vec<usize> = (0..book.memories.len()).collect();
    order.sort_by_key(|&i| {
        (
            book.memories[i].kind,
            std::cmp::Reverse(book.memories[i].start.clone()),
        )
    });
    let wanted: Vec<usize> = order
        .into_iter()
        .filter(|&i| only.is_none_or(|ids| ids.contains(&book.memories[i].id)))
        .collect();

    let mut plan: Vec<(usize, crate::Project, String)> = Vec::new();
    for &i in &wanted {
        let memory = &book.memories[i];
        let named = only.is_some();
        let state = book.states.entry(memory.id.clone()).or_default();
        if state.status.as_deref() == Some("dismissed") && !named {
            continue;
        }
        let existing = state
            .project_id
            .as_deref()
            .and_then(|id| crate::projects::get(library, id));
        if state.project_id.is_some() && existing.is_none() && !named {
            state.status = Some("dismissed".into());
            continue;
        }
        let videos = project_videos(memory, &catalogue);
        let style = style(memory, state);
        let print = footprint(&videos, &style);
        if let Some(project) = &existing
            && state.footprint.as_deref() == Some(print.as_str())
            && project.last_run.is_some()
            && !named
        {
            continue;
        }
        let mut project = existing.unwrap_or_else(|| {
            let mut p = crate::Project::new(title(memory, &album, lang));
            p.editors = editors.clone();
            p
        });
        project.name = title(memory, &album, lang);
        project.inputs = videos;
        project.style = style;
        project.memory = Some(memory.id.clone());
        project.updated_at = vb_contracts::now_rfc3339();
        project.save(library)?;
        state.project_id = Some(project.id.clone());
        state.status = Some("pending".into());
        state.error = None;
        plan.push((i, project, print));
    }
    save(library, &book)?;

    let mut done = Vec::new();
    let total = plan.len();
    for (n, (i, project, print)) in plan.into_iter().enumerate() {
        if cancelled() {
            break;
        }
        let id = book.memories[i].id.clone();
        let outcome = run(n, total, &project.id, &project.name);
        let mut fresh = load(library);
        let state = fresh.states.entry(id.clone()).or_default();
        state.at = Some(vb_contracts::now_rfc3339());
        match outcome {
            Ok(_) => {
                state.status = Some("done".into());
                state.footprint = Some(print);
                state.error = None;
                done.push(json!({ "memory": id, "projectId": project.id, "status": "done" }));
            }
            Err(e) => {
                if cancelled() {
                    state.status = Some("pending".into());
                    save(library, &fresh)?;
                    break;
                }
                state.status = Some("failed".into());
                state.error = Some(e.clone());
                done.push(json!({ "memory": id, "projectId": project.id, "status": "failed", "error": e }));
            }
        }
        save(library, &fresh)?;
    }
    Ok(done)
}

// ---------------------------------------------------------------------------
// What the app shows
// ---------------------------------------------------------------------------

fn summary(
    memory: &Memory,
    state: Option<&MemoryState>,
    album: &entities::Album,
    library: &Library,
    lang: Lang,
) -> Value {
    let project = state
        .and_then(|s| s.project_id.as_deref())
        .and_then(|id| crate::projects::get(library, id));
    let thumbnails: Vec<Value> = memory
        .subjects
        .iter()
        .filter_map(|id| album.subjects.iter().find(|s| &s.id == id))
        .map(|s| json!({ "id": s.id, "label": s.label, "thumbnail": s.thumbnail }))
        .collect();
    json!({
        "id": memory.id,
        "kind": memory.kind.id(),
        "title": title(memory, album, lang),
        "start": memory.start,
        "end": memory.end,
        "year": memory.year,
        "month": memory.month,
        "label": format!("{}.{:02}", memory.year, memory.month),
        "place": if lang == Lang::Zh { memory.place_zh.clone() } else { memory.place_en.clone() },
        "pin": memory.pin,
        "regions": memory.regions,
        "points": memory.points,
        "videoCount": memory.videos.len() - memory.photos.min(memory.videos.len()),
        "photoCount": memory.photos,
        "cover": memory.videos.get(memory.videos.len() / 2),
        "people": thumbnails,
        "order": (memory.kind == Kind::Annual)
            .then(|| state.and_then(|s| s.order.clone()).unwrap_or_else(|| "category".into())),
        "projectId": project.as_ref().map(|p| p.id.clone()),
        "projectRun": project.as_ref().is_some_and(|p| p.last_run.is_some()),
        "projectClips": project.as_ref().map_or(0, |p| p.inputs.len()),
        "status": state.and_then(|s| s.status.clone()),
        "error": state.and_then(|s| s.error.clone()),
    })
}

pub fn listing(library: &Library, kind: Option<Kind>, lang: Lang) -> Value {
    let book = load(library);
    let album = entities::load(library);
    let memories: Vec<Value> = book
        .memories
        .iter()
        .filter(|m| kind.is_none_or(|k| m.kind == k))
        .map(|m| summary(m, book.states.get(&m.id), &album, library, lang))
        .collect();
    let lit: BTreeSet<&String> = book
        .memories
        .iter()
        .filter(|m| kind.is_none_or(|k| m.kind == k))
        .flat_map(|m| m.regions.iter())
        .collect();
    json!({
        "memories": memories,
        "regions": lit,
        "foundAt": book.found_at,
        "notes": [
            lang.t(
                "Places come from the GPS fix the camera wrote into each video. Videos with no \
                 location -- most drones and action cameras -- join the trip they were filmed \
                 during, and are not pinned on their own.",
                "地点来自相机写进视频的 GPS 坐标。没有定位的视频（大多数无人机、运动相机拍的）按拍摄时间归入同时段的旅行，自己不会出现在地图上。",
            ),
            lang.t(
                "A trip with no location at all -- a phone backup can lose every GPS fix -- is \
                 found by a drone flown on two days or more of it. It has no place in its name \
                 and no pin.",
                "整段都没有定位的旅行（手机备份可能把 GPS 全部清掉），靠其中至少两天飞过无人机来认出。它的名字里没有地名，地图上也没有图钉。",
            ),
            lang.t(
                "Who is in a memory comes from faces grouped by likeness, not recognised. \
                 Growing up is made for children only: someone whose frames, when alone in \
                 them, read as a child more often than as an adult.",
                "回忆里的人物来自按长相相似程度分组的人脸，不是人脸识别。成长记忆只给孩子做：单独出镜的画面多数被识别为儿童的人。",
            ),
        ],
    })
}

pub fn get(library: &Library, id: &str, lang: Lang) -> Result<Value, String> {
    let book = load(library);
    let memory = book
        .memories
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("there is no memory {id}"))?;
    let album = entities::load(library);
    let catalogue = hosting::load(library);
    let entries: HashMap<&str, &Entry> = catalogue
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let mut value = summary(memory, book.states.get(id), &album, library, lang);
    value["videos"] = json!(
        memory
            .videos
            .iter()
            .filter_map(|p| entries.get(p.as_str()))
            .map(|e| hosting::video_value(e, lang))
            .collect::<Vec<_>>()
    );
    Ok(value)
}

/// Set a year's order, `category` or `time`.
pub fn set_order(library: &Library, id: &str, order: &str) -> Result<(), String> {
    if !matches!(order, "category" | "time") {
        return Err(format!("unknown order {order:?}"));
    }
    let mut book = load(library);
    if !book
        .memories
        .iter()
        .any(|m| m.id == id && m.kind == Kind::Annual)
    {
        return Err(format!("there is no year {id}"));
    }
    book.states.entry(id.to_string()).or_default().order = Some(order.to_string());
    save(library, &book)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::{Look, SeenFace};

    fn video(path: &str, at: &str, at_place: Option<(f64, f64)>, people: &[(&str, f64)]) -> Entry {
        Entry {
            path: path.into(),
            fingerprint: path.into(),
            duration_us: 20_000_000,
            captured_at: Some(at.into()),
            location: at_place.map(|(lat, lon)| GeoPoint { lat, lon }),
            look: Look {
                tracker: Some("AppleVisionFaces".into()),
                faces: people
                    .iter()
                    .map(|(s, area)| SeenFace {
                        subject: Some((*s).into()),
                        area: *area,
                        frames: vec![0],
                        picture: None,
                    })
                    .collect(),
                ..Look::default()
            },
            ..Entry::default()
        }
    }

    const LOS_ANGELES: (f64, f64) = (34.0522, -118.2437);
    const AIRPORT: (f64, f64) = (33.9416, -118.4085);

    /// Home in Los Angeles across two years, and the trip as it was filmed:
    /// the plane at the airport there, a week in Xinjiang, drone clips with
    /// no location after the phone's last.
    fn library_of_the_trip() -> Vec<Entry> {
        let mut e = vec![
            video(
                "/home/a.mp4",
                "2023-06-01T02:00:00Z",
                Some(LOS_ANGELES),
                &[],
            ),
            video(
                "/home/b.mp4",
                "2025-09-23T02:40:51Z",
                Some(LOS_ANGELES),
                &[],
            ),
            video(
                "/home/c.mp4",
                "2025-10-03T01:38:46Z",
                Some(LOS_ANGELES),
                &[],
            ),
            video(
                "/home/d.mp4",
                "2025-12-13T10:47:19Z",
                Some((34.1000, -118.3300)),
                &[],
            ),
            video(
                "/trip/V30929-191205.mp4",
                "2023-09-29T11:12:35Z",
                Some(AIRPORT),
                &[],
            ),
            video(
                "/trip/V30930-172308.mp4",
                "2023-09-30T09:23:08Z",
                Some((43.7797, 87.6164)),
                &[],
            ),
            video(
                "/trip/V31001-181040.mp4",
                "2023-10-01T10:10:40Z",
                Some((43.3372, 87.4084)),
                &[],
            ),
            video(
                "/trip/V31002-164424.mp4",
                "2023-10-02T08:44:24Z",
                Some((43.7764, 84.4523)),
                &[],
            ),
            video(
                "/trip/V31003-124026.mp4",
                "2023-10-03T04:40:26Z",
                Some((43.2173, 83.9882)),
                &[],
            ),
            video(
                "/trip/V31004-155556.mp4",
                "2023-10-04T07:55:56Z",
                Some((44.2074, 80.4121)),
                &[],
            ),
            video(
                "/dji/dji_fly_20231005_140716_0.mp4",
                "2023-10-05T06:07:16Z",
                None,
                &[],
            ),
            video(
                "/dji/dji_fly_20231006_191729_0.mp4",
                "2023-10-06T11:17:29Z",
                None,
                &[],
            ),
            // A month later at home, with no location: not the trip.
            video(
                "/dji/dji_fly_20231106_191729_0.mp4",
                "2023-11-06T11:17:29Z",
                None,
                &[],
            ),
        ];
        e.sort_by(|a, b| a.path.cmp(&b.path));
        e
    }

    #[test]
    fn the_trip_is_found_and_named_after_xinjiang() {
        let entries = library_of_the_trip();
        let found = find(&entries);
        let trips: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Travel).collect();
        assert_eq!(trips.len(), 1, "{trips:#?}");
        let trip = trips[0];
        assert_eq!(trip.place_zh.as_deref(), Some("新疆"));
        assert_eq!((trip.year, trip.month), (2023, 9));
        assert_eq!(trip.start, "2023-09-29");
        assert_eq!(trip.end, "2023-10-06");
        // The plane at the airport, the week away, and the drone clips.
        assert!(trip.videos.iter().any(|v| v.contains("191205")));
        assert!(trip.videos.iter().any(|v| v.contains("20231006")));
        assert!(!trip.videos.iter().any(|v| v.contains("20231106")));
        assert!(!trip.videos.iter().any(|v| v.starts_with("/home")));
        assert_eq!(trip.regions, ["CN-XJ", "US"]);
        let pin = trip.pin.unwrap();
        assert!(
            pin.lon < 90.0,
            "the pin is in Xinjiang, not at the airport: {pin:?}"
        );
        let album = entities::Album::default();
        assert_eq!(title(trip, &album, Lang::Zh), "2023年9月到新疆的旅行回忆");
        assert_eq!(
            title(trip, &album, Lang::En),
            "Trip to Xinjiang, September 2023"
        );
    }

    fn as_photo(mut entry: Entry) -> Entry {
        entry.path = entry.path.replace(".mp4", ".jpg");
        entry.fingerprint = entry.path.clone();
        entry.duration_us = 0;
        entry.look.still = true;
        entry
    }

    #[test]
    fn a_trip_photographed_and_not_filmed_is_still_the_trip() {
        let entries: Vec<Entry> = library_of_the_trip()
            .into_iter()
            .map(|e| {
                if e.path.starts_with("/trip") {
                    as_photo(e)
                } else {
                    e
                }
            })
            .collect();
        let photographed = entries.iter().filter(|e| e.is_photo()).count();
        let found = find(&entries);
        let trips: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Travel).collect();
        assert_eq!(trips.len(), 1, "{trips:#?}");
        assert_eq!(trips[0].place_zh.as_deref(), Some("新疆"));
        let photos_in = trips[0]
            .videos
            .iter()
            .filter(|p| p.ends_with(".jpg"))
            .count();
        assert_eq!(trips[0].photos, photos_in);
        assert_eq!(photos_in, photographed);
    }

    #[test]
    fn photos_fill_what_the_videos_cannot() {
        let memory_of = |items: &[String], photos: usize| Memory {
            id: "travel-1".into(),
            kind: Kind::Travel,
            start: String::new(),
            end: String::new(),
            year: 2023,
            month: 9,
            videos: items.to_vec(),
            photos,
            subjects: vec![],
            place_zh: None,
            place_en: None,
            pin: None,
            regions: vec![],
            points: vec![],
        };
        // In the order filmed, the videos spread among the photos.
        let catalogue_of = |videos: usize, photos: usize| {
            let total = videos + photos;
            let mut catalogue = hosting::Catalogue::default();
            for i in 0..total {
                let is_video = (i * videos) / total != ((i + 1) * videos) / total;
                let entry = video(&format!("/{i:03}.mp4"), "2023-09-30T00:00:00Z", None, &[]);
                catalogue
                    .entries
                    .push(if is_video { entry } else { as_photo(entry) });
            }
            let items: Vec<String> = catalogue.entries.iter().map(|e| e.path.clone()).collect();
            (catalogue, items)
        };

        // Ten videos among fifty photos: every video, photos for the rest.
        let (catalogue, items) = catalogue_of(10, 50);
        let chosen = project_videos(&memory_of(&items, 50), &catalogue);
        assert_eq!(chosen.len(), PROJECT_CLIPS);
        assert_eq!(
            chosen.iter().filter(|p| p.ends_with(".mp4")).count(),
            10,
            "every video is in it"
        );
        let mut sorted = chosen.clone();
        sorted.sort();
        assert_eq!(chosen, sorted, "in the order filmed");

        // Forty-five videos and twenty photos: made of videos, as before
        // photos were filed.
        let (catalogue, items) = catalogue_of(45, 20);
        let chosen = project_videos(&memory_of(&items, 20), &catalogue);
        assert_eq!(chosen.len(), PROJECT_CLIPS);
        assert!(chosen.iter().all(|p| p.ends_with(".mp4")), "{chosen:?}");
    }

    #[test]
    fn a_week_away_with_no_location_and_a_drone_is_a_trip_with_no_place() {
        let mut entries = library_of_the_trip();
        // The same week as a phone backup keeps it: every fix zeroed.
        for entry in entries.iter_mut() {
            if entry.path.starts_with("/trip") {
                entry.location = None;
            }
        }
        for (n, at) in ["2023-10-02T05:24:41Z", "2023-10-03T06:41:49Z"]
            .iter()
            .enumerate()
        {
            entries.push(video(
                &format!("/dji/dji_fly_2023100{}_0_video_low_quality.mp4", n + 2),
                at,
                None,
                &[],
            ));
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let found = find(&entries);
        let trips: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Travel).collect();
        assert_eq!(trips.len(), 1, "{trips:#?}");
        let trip = trips[0];
        assert_eq!(trip.place_zh, None);
        assert_eq!(trip.pin, None, "nothing says where it was");
        assert!(trip.regions.is_empty());
        assert!(trip.videos.iter().any(|v| v.contains("V30930")));
        let album = entities::Album::default();
        assert_eq!(title(trip, &album, Lang::Zh), "2023年9月的旅行回忆");
        assert_eq!(title(trip, &album, Lang::En), "A trip, September 2023");
    }

    #[test]
    fn a_drone_flown_one_afternoon_is_not_a_trip() {
        let mut entries = library_of_the_trip();
        entries.retain(|e| !e.path.starts_with("/trip") && !e.path.starts_with("/dji"));
        for (n, at) in [
            "2024-11-01T14:24:00Z",
            "2024-11-02T01:20:00Z",
            "2024-11-02T04:28:21Z",
            "2024-11-02T04:32:48Z",
        ]
        .iter()
        .enumerate()
        {
            let name = if n >= 2 {
                format!("/dji/dji_fly_20241102_{n}_video.mp4")
            } else {
                format!("/wx/wx_camera_{n}.mp4")
            };
            entries.push(video(&name, at, None, &[]));
        }
        assert!(find(&entries).iter().all(|m| m.kind != Kind::Travel));
    }

    #[test]
    fn a_day_out_is_not_a_trip() {
        let entries = vec![
            video("/a.mp4", "2025-05-01T02:00:00Z", Some((39.9, 116.4)), &[]),
            video("/b.mp4", "2025-05-01T04:00:00Z", Some((39.9, 116.4)), &[]),
            video("/c.mp4", "2025-05-01T06:00:00Z", Some((39.9, 116.4)), &[]),
        ];
        assert!(find(&entries).iter().all(|m| m.kind != Kind::Travel));
    }

    const SAN_FRANCISCO: (f64, f64) = (37.7749, -122.4194);

    /// Footage of someone who lived in San Francisco through 2020, moved to
    /// Los Angeles in 2021, and went back to San Francisco for a weekend in
    /// 2022: the shape of a real phone backup.
    fn library_of_a_move() -> Vec<Entry> {
        let mut e = Vec::new();
        // Life in San Francisco, filmed a weekend a month for a year: clips a day
        // apart, which taken for away from home chain into a trip a month.
        for m in 1..=12 {
            for at in ["06T18:00:00Z", "06T22:00:00Z", "07T18:00:00Z"] {
                e.push(video(
                    &format!("/sz/2020-{m:02}-{at}.mp4"),
                    &format!("2020-{m:02}-{at}"),
                    Some(SAN_FRANCISCO),
                    &[],
                ));
            }
        }
        // A weekend in Las Vegas while living there.
        for (n, at) in [
            "2020-05-02T18:00:00Z",
            "2020-05-02T22:00:00Z",
            "2020-05-03T18:00:00Z",
        ]
        .iter()
        .enumerate()
        {
            e.push(video(
                &format!("/sz/vegas-{n}.mp4"),
                at,
                Some((36.1699, -115.1398)),
                &[],
            ));
        }
        // Life in Los Angeles from 2021, filmed the same way: its first
        // weekend follows the last in San Francisco, and neither is a trip.
        for y in [2021, 2022] {
            for m in 1..=12 {
                for at in ["09T18:00:00Z", "09T22:00:00Z", "10T18:00:00Z"] {
                    e.push(video(
                        &format!("/wh/{y}-{m:02}-{at}.mp4"),
                        &format!("{y}-{m:02}-{at}"),
                        Some(LOS_ANGELES),
                        &[],
                    ));
                }
            }
        }
        // Back in San Francisco for a weekend.
        for (n, at) in [
            "2022-08-13T18:00:00Z",
            "2022-08-14T00:00:00Z",
            "2022-08-14T18:00:00Z",
        ]
        .iter()
        .enumerate()
        {
            e.push(video(
                &format!("/wh/sz-{n}.mp4"),
                at,
                Some(SAN_FRANCISCO),
                &[],
            ));
        }
        e.sort_by(|a, b| a.path.cmp(&b.path));
        e
    }

    #[test]
    fn a_year_lived_somewhere_else_is_not_a_string_of_trips() {
        let entries = library_of_a_move();
        let trips: Vec<Memory> = find(&entries)
            .into_iter()
            .filter(|m| m.kind == Kind::Travel)
            .collect();
        let places: Vec<(&str, &str)> = trips
            .iter()
            .map(|t| (t.start.as_str(), t.place_zh.as_deref().unwrap_or("")))
            .collect();
        // The weekend away from San Francisco, and the weekend back in it
        // after the move -- and nothing of the year lived there.
        assert_eq!(
            places,
            [("2020-05-02", "拉斯维加斯"), ("2022-08-13", "旧金山")],
            "{trips:#?}"
        );
    }

    #[test]
    fn a_day_out_with_a_clip_at_home_the_evening_before_is_not_a_trip() {
        let mut entries = library_of_the_trip();
        entries.push(video(
            "/home/evening.mp4",
            "2025-06-07T03:00:00Z",
            Some(LOS_ANGELES),
            &[],
        ));
        for (n, at) in [
            "2025-06-07T17:00:00Z",
            "2025-06-07T19:00:00Z",
            "2025-06-07T21:00:00Z",
        ]
        .iter()
        .enumerate()
        {
            // Riverside, 80 km from home: away, for a day.
            entries.push(video(
                &format!("/out/{n}.mp4"),
                at,
                Some((33.9533, -117.3962)),
                &[],
            ));
        }
        let trips: Vec<Memory> = find(&entries)
            .into_iter()
            .filter(|m| m.kind == Kind::Travel)
            .collect();
        assert_eq!(trips.len(), 1, "only Xinjiang: {trips:#?}");
    }

    #[test]
    fn a_trip_at_home_in_the_country_is_named_after_its_provinces_not_the_country() {
        // Only China has provinces in the gazetteer, so this home is in
        // Beijing rather than Los Angeles.
        let mut entries = library_of_the_trip();
        for entry in entries.iter_mut() {
            if entry.path.starts_with("/home") {
                entry.location = Some(GeoPoint {
                    lat: 39.9042,
                    lon: 116.4074,
                });
            }
        }
        for (n, (at, place)) in [
            ("2025-08-14T06:46:25Z", (45.7185, 126.3616)),
            ("2025-08-15T10:15:16Z", (33.2629, 114.7212)),
            ("2025-08-15T10:16:18Z", (33.1608, 114.7110)),
        ]
        .iter()
        .enumerate()
        {
            entries.push(video(&format!("/north/{n}.mp4"), at, Some(*place), &[]));
        }
        let found = find(&entries);
        let trip = found
            .iter()
            .find(|m| m.kind == Kind::Travel && m.start == "2025-08-14")
            .expect("the trip north is found");
        assert_eq!(trip.place_zh.as_deref(), Some("河南、黑龙江"));
    }

    #[test]
    fn a_trip_in_one_city_is_named_after_the_city() {
        let entries: Vec<Entry> = (0..4)
            .map(|d| {
                video(
                    &format!("/paris-{d}.mp4"),
                    &format!("2024-05-0{}T10:00:00Z", d + 1),
                    Some((48.85 + d as f64 * 0.01, 2.35)),
                    &[],
                )
            })
            .collect();
        let found = find(&entries);
        let trip = found.iter().find(|m| m.kind == Kind::Travel).unwrap();
        assert_eq!(trip.place_zh.as_deref(), Some("巴黎"));
        assert_eq!(
            title(trip, &entities::Album::default(), Lang::Zh),
            "2024年5月到巴黎的旅行回忆"
        );
    }

    /// A frame labelled as the labeller labels people.
    fn labelled(mut entry: Entry, labels: &[(&str, f64)]) -> Entry {
        entry.look.frames = vec![
            labels
                .iter()
                .map(|(label, confidence)| vb_infer::ScoredLabel {
                    label: (*label).into(),
                    confidence: *confidence,
                })
                .collect(),
        ];
        entry
    }

    const MONTHS_APART: [&str; 5] = [
        "2024-11-02T04:28:21Z",
        "2025-01-10T04:28:21Z",
        "2025-03-15T04:28:21Z",
        "2025-07-20T04:28:21Z",
        "2026-07-06T13:35:04Z",
    ];

    #[test]
    fn a_child_filmed_over_months_is_growing_up_and_a_passer_by_is_not() {
        let entries: Vec<Entry> = MONTHS_APART
            .iter()
            .enumerate()
            .map(|(i, at)| {
                labelled(
                    video(
                        &format!("/kid-{i}.mp4"),
                        at,
                        None,
                        &[("sub-0001", 0.05), ("sub-0009", 0.0005)],
                    ),
                    &[("people", 0.81), ("baby", 0.81)],
                )
            })
            .collect();
        let found = find(&entries);
        let growth: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Growth).collect();
        assert_eq!(growth.len(), 1);
        assert_eq!(growth[0].subjects, ["sub-0001"]);
        assert_eq!(growth[0].videos.len(), 5);
    }

    /// A baby filmed over months, alone in each frame.
    fn baby_filmed_over_months() -> Vec<Entry> {
        MONTHS_APART
            .iter()
            .enumerate()
            .map(|(i, at)| {
                labelled(
                    video(&format!("/kid-{i}.mp4"), at, None, &[("sub-0001", 0.05)]),
                    &[("people", 0.81), ("baby", 0.81)],
                )
            })
            .collect()
    }

    fn growth_of(entries: &[Entry]) -> Memory {
        let found = find(entries);
        let growth: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Growth).collect();
        assert_eq!(growth.len(), 1, "one child growing up");
        growth[0].clone()
    }

    #[test]
    fn a_stranger_filed_with_a_child_is_left_out_of_their_growing_up() {
        // As on the real disk: a woman's face, alone in a frame that reads
        // as an adult and as no child, filed with the baby five years
        // before the baby's own videos.
        let mut entries = baby_filmed_over_months();
        entries.push(labelled(
            video(
                "/S00303-09551513.mp4",
                "2020-03-03T02:01:09Z",
                None,
                &[("sub-0001", 0.005)],
            ),
            &[("people", 0.85), ("adult", 0.85)],
        ));
        let growth = growth_of(&entries);
        assert_eq!(growth.videos.len(), 5);
        assert!(!growth.videos.iter().any(|v| v.contains("S00303")));
        assert_eq!(growth.start, "2024-11-02");
    }

    #[test]
    fn a_parent_in_the_frame_does_not_make_it_a_grown_ups() {
        // The baby's face alone, a parent's shoulder beside it: the frame
        // reads as an adult and as a baby, and stays the baby's.
        let mut entries = baby_filmed_over_months();
        entries.push(labelled(
            video(
                "/kid-held.mp4",
                "2025-05-01T04:00:00Z",
                None,
                &[("sub-0001", 0.09)],
            ),
            &[("people", 0.9), ("adult", 0.83), ("baby", 0.73)],
        ));
        assert_eq!(growth_of(&entries).videos.len(), 6);
    }

    #[test]
    fn a_sighting_years_before_the_rest_is_not_part_of_growing_up() {
        // Filed with the baby at a gathering four years earlier, among
        // others, where the frame cannot say whose face is whose; more than
        // a year from everything else the child is in.
        let mut entries = baby_filmed_over_months();
        entries.push(labelled(
            video(
                "/wx_camera_1620093936450.mp4",
                "2021-05-04T02:05:36Z",
                None,
                &[("sub-0001", 0.006), ("sub-0007", 0.01)],
            ),
            &[("people", 0.91), ("adult", 0.91), ("child", 0.46)],
        ));
        let growth = growth_of(&entries);
        assert_eq!(growth.videos.len(), 5);
        assert_eq!(growth.start, "2024-11-02");
        // A year apart is still one childhood: the fixture's last two are
        // 351 days apart and both kept.
        assert!(growth.videos.iter().any(|v| v.ends_with("kid-4.mp4")));
    }

    #[test]
    fn a_grown_up_filmed_over_months_is_not_growing_up() {
        let mut entries: Vec<Entry> = MONTHS_APART
            .iter()
            .enumerate()
            .map(|(i, at)| {
                labelled(
                    video(&format!("/dad-{i}.mp4"), at, None, &[("sub-0002", 0.05)]),
                    &[("adult", 0.93), ("people", 0.93)],
                )
            })
            .collect();
        // Holding the baby once does not make him one: the frame is not his
        // alone.
        entries.push(labelled(
            video(
                "/dad-and-baby.mp4",
                "2025-08-01T04:00:00Z",
                None,
                &[("sub-0002", 0.05), ("sub-0001", 0.03)],
            ),
            &[("baby", 0.9)],
        ));
        assert!(find(&entries).iter().all(|m| m.kind != Kind::Growth));
    }

    #[test]
    fn a_couple_seen_together_again_and_again_is_a_group() {
        let mut entries = Vec::new();
        for d in 1..=4 {
            entries.push(video(
                &format!("/couple-{d}.mp4"),
                &format!("2023-10-0{d}T10:00:00Z"),
                None,
                &[("sub-0001", 0.03), ("sub-0002", 0.02)],
            ));
        }
        // Once with a friend: still the couple's memory, not a trio's.
        entries.push(video(
            "/three.mp4",
            "2023-10-05T10:00:00Z",
            None,
            &[("sub-0001", 0.03), ("sub-0002", 0.02), ("sub-0003", 0.02)],
        ));
        let found = find(&entries);
        let groups: Vec<&Memory> = found.iter().filter(|m| m.kind == Kind::Group).collect();
        assert_eq!(groups.len(), 1, "{groups:#?}");
        assert_eq!(groups[0].subjects, ["sub-0001", "sub-0002"]);
        assert_eq!(groups[0].videos.len(), 5);
        let mut album = entities::Album::default();
        assert_eq!(title(groups[0], &album, Lang::Zh), "2人同框的回忆");
        for (id, name) in [("sub-0001", "爸爸"), ("sub-0002", "妈妈")] {
            album.subjects.push(entities::Subject {
                id: id.into(),
                label: Some(name.into()),
                kind: vb_contracts::footage_graph::EntityKind::Person,
                thumbnail: None,
                signature: vec![],
                exemplars: vec![],
                appearances: vec![],
                first_seen_at: String::new(),
                updated_at: String::new(),
            });
        }
        assert_eq!(title(groups[0], &album, Lang::Zh), "爸爸和妈妈的共同回忆");
    }

    #[test]
    fn a_year_holds_every_video_filmed_in_it_by_the_local_clock() {
        let mut entries: Vec<Entry> = (1..=6)
            .map(|m| {
                video(
                    &format!("/2025-{m}.mp4"),
                    &format!("2025-0{m}-10T10:00:00Z"),
                    Some(LOS_ANGELES),
                    &[],
                )
            })
            .collect();
        // Half past five on New Year's Day in UTC is still the old year in
        // Los Angeles: it closes 2025, not opens 2026.
        entries.push(video(
            "/eve.mp4",
            "2026-01-01T05:30:00Z",
            Some(LOS_ANGELES),
            &[],
        ));
        let found = find(&entries);
        let year = found.iter().find(|m| m.id == "annual-2025").unwrap();
        assert_eq!(year.videos.len(), 7);
        assert!(year.videos.contains(&"/eve.mp4".to_string()));
        assert!(!found.iter().any(|m| m.id == "annual-2026"));
    }

    #[test]
    fn a_camera_names_its_files_by_its_own_clock() {
        // dji_mimo_20260706_213502: 13:35:04 UTC on the file.
        let utc = vb_contracts::time::capture_seconds("2026-07-06T13:35:04Z").unwrap();
        assert_eq!(
            offset_from_name("dji_mimo_20260706_213502_0_1783501580814_video.mp4", utc),
            Some(480)
        );
        let utc = vb_contracts::time::capture_seconds("2026-02-13T19:53:33Z").unwrap();
        assert_eq!(offset_from_name("VID_20260214_035333.mp4", utc), Some(480));
        let utc = vb_contracts::time::capture_seconds("2025-09-23T02:40:51Z").unwrap();
        assert_eq!(
            offset_from_name("VID20250923103657.mp4", utc),
            None,
            "4 minutes apart"
        );
        assert_eq!(offset_from_name("clip-a.mp4", utc), None);
    }

    #[test]
    fn a_year_too_big_for_one_cut_keeps_every_section_in_proportion() {
        let mut catalogue = hosting::Catalogue::default();
        let mut videos = Vec::new();
        for i in 0..100 {
            let category = match i % 10 {
                0..=5 => "people",
                6 | 7 => "scenery",
                8 => "food",
                _ => "transport",
            };
            let path = format!("/{i:03}.mp4");
            catalogue.entries.push(Entry {
                path: path.clone(),
                categories: vec![category.into()],
                ..Entry::default()
            });
            videos.push(path);
        }
        let memory = Memory {
            id: "annual-2025".into(),
            kind: Kind::Annual,
            start: String::new(),
            end: String::new(),
            year: 2025,
            month: 1,
            videos,
            photos: 0,
            subjects: vec![],
            place_zh: None,
            place_en: None,
            pin: None,
            regions: vec![],
            points: vec![],
        };
        let chosen = project_videos(&memory, &catalogue);
        assert_eq!(chosen.len(), PROJECT_CLIPS);
        let count = |c: &str| {
            chosen
                .iter()
                .filter(|p| catalogue.get(p).unwrap().is_in(c))
                .count()
        };
        assert_eq!(count("people"), 24);
        assert_eq!(count("scenery"), 8);
        assert_eq!(count("food"), 4);
        assert_eq!(count("transport"), 4);
        let mut sorted = chosen.clone();
        sorted.sort();
        assert_eq!(chosen, sorted, "in the order filmed");
    }

    #[test]
    fn spreading_keeps_the_first_and_the_last() {
        let items: Vec<u32> = (0..10).collect();
        assert_eq!(spread(&items, 4), [0, 3, 6, 9]);
        assert_eq!(spread(&items, 20).len(), 10);
    }
}
