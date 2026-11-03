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

//! Where footage was filmed, with no network.
//!
//! A phone writes a GPS fix into every clip; this says what it is: the
//! country, the province (China's, for now), and the city it is near, in
//! Chinese and English. It also holds the world map the app draws the memory
//! map from, so the place a memory is named after and the region it lights
//! up come from one set of outlines.
//!
//! The outlines and places are Natural Earth's 1:10m layers, public domain,
//! simplified by `scripts/make-world-map.py` into `data/`. Countries are
//! from the China point-of-view layer. That is not a reviewed map, and a map
//! shown in an app published in mainland China needs a 审图号 it does not
//! have; PRD.md lists it with the licences as a release blocker.
//!
//! Nothing here reads a file or the network: both data files are compiled in.

use serde::Serialize;
use std::sync::OnceLock;

const WORLD: &str = include_str!("../data/world.json");
const PLACES: &str = include_str!("../data/places.tsv");

/// The world map as the app draws it, verbatim: regions with their names,
/// a label point and their outlines in hundredths of a degree.
pub fn world_json() -> &'static str {
    WORLD
}

/// A country, or a province of one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// ISO 3166: `CN`, `FR`, or `CN-XJ` for a province.
    pub id: String,
    pub province: bool,
    /// The country a province belongs to.
    pub parent: Option<String>,
    pub zh: String,
    pub en: String,
    /// Where its name goes on a map: longitude, latitude.
    pub label: (f64, f64),
    #[serde(skip)]
    rings: Vec<Ring>,
}

#[derive(Debug, Clone)]
struct Ring {
    points: Vec<(f64, f64)>,
    /// West, south, east, north.
    bounds: (f64, f64, f64, f64),
}

impl Ring {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        let (w, s, e, n) = self.bounds;
        if lon < w || lon > e || lat < s || lat > n {
            return false;
        }
        let mut inside = false;
        let pts = &self.points;
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[j];
            if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// How far a point is from this outline, in degrees as drawn: enough to
    /// tell a beach just outside a simplified coast from the open sea.
    fn distance(&self, lon: f64, lat: f64) -> f64 {
        let (w, s, e, n) = self.bounds;
        let outside = (w - lon)
            .max(lon - e)
            .max(0.0)
            .max((s - lat).max(lat - n).max(0.0));
        if outside > 1.0 {
            return outside;
        }
        let pts = &self.points;
        let mut best = f64::MAX;
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            best = best.min(segment_distance((lon, lat), pts[j], pts[i]));
            j = i;
        }
        best
    }
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let t = if len == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt()
}

impl Region {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        self.rings.iter().any(|r| r.contains(lon, lat))
    }

    fn distance(&self, lon: f64, lat: f64) -> f64 {
        self.rings
            .iter()
            .map(|r| r.distance(lon, lat))
            .fold(f64::MAX, f64::min)
    }

    /// The name in the language asked for.
    pub fn name(&self, zh: bool) -> &str {
        if zh { &self.zh } else { &self.en }
    }
}

/// A populated place: a city or a town.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub en: String,
    /// Empty where Natural Earth has no Chinese name for it.
    pub zh: String,
    pub lat: f64,
    pub lon: f64,
    pub population: u64,
    /// ISO 3166 alpha-2; Taiwan, Hong Kong and Macao are `CN`.
    pub country: String,
}

impl Place {
    /// The name in the language asked for; a place with no Chinese name
    /// keeps its English one.
    pub fn name(&self, zh: bool) -> &str {
        if zh && !self.zh.is_empty() {
            &self.zh
        } else {
            &self.en
        }
    }
}

/// What a point is in.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Located<'w> {
    pub country: Option<&'w Region>,
    pub province: Option<&'w Region>,
    /// The notable place it is near, when there is one within
    /// [`NEAR_PLACE_KM`].
    pub city: Option<&'w Place>,
}

impl Located<'_> {
    /// The finest region that holds the point: the province where there
    /// is one, else the country. What the map lights up.
    pub fn region(&self) -> Option<&Region> {
        self.province.or(self.country)
    }

    pub fn country_code(&self) -> Option<&str> {
        self.country.map(|c| c.id.as_str())
    }
}

/// How near a city must be for a point to count as at it.
pub const NEAR_PLACE_KM: f64 = 40.0;

/// A point this far outside every outline, in degrees, is at sea: a
/// simplified coast is a kilometre or two off, not this.
const COAST_SLACK: f64 = 0.25;

/// The world's regions and places, read once.
pub struct World {
    pub regions: Vec<Region>,
    pub places: Vec<Place>,
}

/// The world, parsed on first use.
pub fn world() -> &'static World {
    static WORLD_DATA: OnceLock<World> = OnceLock::new();
    WORLD_DATA.get_or_init(World::parse)
}

impl World {
    fn parse() -> World {
        let value: serde_json::Value =
            serde_json::from_str(WORLD).expect("the compiled-in world map is JSON");
        let scale = value["scale"].as_f64().unwrap_or(100.0);
        let mut regions = Vec::new();
        for r in value["regions"].as_array().into_iter().flatten() {
            let label = r["label"].as_array();
            let coord = |i: usize| {
                label
                    .and_then(|l| l.get(i))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    / scale
            };
            let rings = r["rings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|ring| {
                    let flat: Vec<f64> = ring
                        .as_array()?
                        .iter()
                        .filter_map(|v| v.as_f64())
                        .map(|v| v / scale)
                        .collect();
                    let points: Vec<(f64, f64)> =
                        flat.chunks_exact(2).map(|p| (p[0], p[1])).collect();
                    if points.len() < 3 {
                        return None;
                    }
                    let bounds = points.iter().fold(
                        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
                        |(w, s, e, n), (x, y)| (w.min(*x), s.min(*y), e.max(*x), n.max(*y)),
                    );
                    Some(Ring { points, bounds })
                })
                .collect();
            regions.push(Region {
                id: r["id"].as_str().unwrap_or_default().to_string(),
                province: r["kind"] == "province",
                parent: r["parent"].as_str().map(str::to_string),
                zh: r["zh"].as_str().unwrap_or_default().to_string(),
                en: r["en"].as_str().unwrap_or_default().to_string(),
                label: (coord(0), coord(1)),
                rings,
            });
        }
        let places = PLACES
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|line| {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 6 {
                    return None;
                }
                Some(Place {
                    en: f[0].to_string(),
                    zh: f[1].to_string(),
                    lat: f[2].parse().ok()?,
                    lon: f[3].parse().ok()?,
                    population: f[4].parse().unwrap_or(0),
                    country: f[5].to_string(),
                })
            })
            .collect();
        World { regions, places }
    }

    pub fn region(&self, id: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    /// The country, province and city a point is in or near.
    pub fn locate(&self, lat: f64, lon: f64) -> Located<'_> {
        let inside = |province: bool| {
            self.regions
                .iter()
                .filter(|r| r.province == province)
                .find(|r| r.contains(lon, lat))
                .or_else(|| {
                    // Just off a simplified coast: the nearest outline.
                    self.regions
                        .iter()
                        .filter(|r| r.province == province)
                        .map(|r| (r, r.distance(lon, lat)))
                        .filter(|(_, d)| *d < COAST_SLACK)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(r, _)| r)
                })
        };
        let province = inside(true);
        let country = match province.and_then(|p| p.parent.as_deref()) {
            Some(parent) => self.region(parent),
            None => inside(false),
        };
        Located {
            country,
            province,
            city: self.near(lat, lon, NEAR_PLACE_KM),
        }
    }

    /// The place a point is at: the most populous within `km`, so a point
    /// in a city's suburbs is in the city and not in the village beside it.
    pub fn near(&self, lat: f64, lon: f64, km: f64) -> Option<&Place> {
        self.places
            .iter()
            .filter(|p| {
                (p.lat - lat).abs() < km / 100.0 && distance_km(lat, lon, p.lat, p.lon) <= km
            })
            .max_by_key(|p| p.population)
    }
}

/// Great-circle distance, in kilometres.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0 * 2.0 * a.sqrt().min(1.0).asin()
}

/// The clock a place keeps, as minutes east of UTC, for reading the day a
/// clip was filmed from a camera's UTC time: the country's standard time
/// where one zone covers it (all of China is UTC+8, Xinjiang included),
/// otherwise the longitude's hour. Summer time is not applied; the day and
/// the month are what this is for, and an hour does not move them often.
pub fn zone_minutes(country: Option<&str>, lon: f64) -> i32 {
    const ZONES: &[(&str, i32)] = &[
        ("CN", 480),
        ("HK", 480),
        ("MO", 480),
        ("TW", 480),
        ("SG", 480),
        ("MY", 480),
        ("PH", 480),
        ("JP", 540),
        ("KR", 540),
        ("KP", 540),
        ("TH", 420),
        ("VN", 420),
        ("KH", 420),
        ("LA", 420),
        ("MM", 390),
        ("IN", 330),
        ("LK", 330),
        ("NP", 345),
        ("BD", 360),
        ("BT", 360),
        ("PK", 300),
        ("UZ", 300),
        ("MV", 300),
        ("AE", 240),
        ("OM", 240),
        ("IR", 210),
        ("SA", 180),
        ("QA", 180),
        ("TR", 180),
        ("IL", 120),
        ("JO", 180),
        ("EG", 120),
        ("GR", 120),
        ("FI", 120),
        ("ZA", 120),
        ("KE", 180),
        ("GB", 0),
        ("IE", 0),
        ("PT", 0),
        ("IS", 0),
        ("MA", 60),
        ("FR", 60),
        ("DE", 60),
        ("IT", 60),
        ("ES", 60),
        ("CH", 60),
        ("AT", 60),
        ("NL", 60),
        ("BE", 60),
        ("CZ", 60),
        ("PL", 60),
        ("HU", 60),
        ("DK", 60),
        ("NO", 60),
        ("SE", 60),
        ("HR", 60),
        ("NZ", 720),
        ("AR", -180),
        ("CL", -240),
        ("PE", -300),
        ("CO", -300),
        ("CU", -300),
    ];
    match country.and_then(|c| ZONES.iter().find(|(code, _)| *code == c)) {
        Some((_, minutes)) => *minutes,
        None => ((lon / 15.0).round() as i32).clamp(-12, 14) * 60,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trips_clips_are_in_xinjiang_and_its_departure_in_los_angeles() {
        let w = world();
        // South of Urumqi.
        let at = w.locate(43.3372, 87.4084);
        assert_eq!(at.province.map(|p| p.id.as_str()), Some("CN-XJ"));
        assert_eq!(at.province.map(|p| p.zh.as_str()), Some("新疆"));
        assert_eq!(at.country_code(), Some("CN"));
        assert_eq!(at.country.map(|c| c.zh.as_str()), Some("中国"));
        // Los Angeles: a country, no province, and the city.
        let at = w.locate(34.0522, -118.2437);
        assert_eq!(at.country_code(), Some("US"));
        assert!(at.province.is_none());
        assert_eq!(at.city.map(|c| c.name(true)), Some("洛杉矶"));
    }

    #[test]
    fn places_abroad_are_their_countries() {
        let w = world();
        let paris = w.locate(48.8566, 2.3522);
        assert_eq!(paris.country_code(), Some("FR"));
        assert!(paris.province.is_none(), "only China has provinces here");
        assert_eq!(paris.city.map(|c| c.en.as_str()), Some("Paris"));
        assert_eq!(w.locate(35.6762, 139.6503).country_code(), Some("JP"));
        // Taipei is in Taiwan province, Hong Kong a region of its own.
        assert_eq!(
            w.locate(25.033, 121.5654).province.map(|p| p.id.as_str()),
            Some("CN-TW")
        );
        assert_eq!(
            w.locate(22.2783, 114.1747).province.map(|p| p.id.as_str()),
            Some("CN-HK")
        );
    }

    #[test]
    fn a_beach_just_off_the_simplified_coast_is_still_on_land() {
        // Sanya's Yalong Bay, a few hundred metres from the water.
        let at = world().locate(18.2294, 109.6390);
        assert_eq!(at.province.map(|p| p.id.as_str()), Some("CN-HI"));
        // The middle of the Pacific is nowhere.
        let sea = world().locate(0.0, -150.0);
        assert!(sea.country.is_none() && sea.province.is_none());
    }

    #[test]
    fn distances_are_great_circle_kilometres() {
        // Los Angeles to San Francisco is about 559 km.
        let d = distance_km(34.0522, -118.2437, 37.7749, -122.4194);
        assert!((540.0..580.0).contains(&d), "{d}");
        assert!(distance_km(1.0, 1.0, 1.0, 1.0) < 1e-9);
    }

    #[test]
    fn all_of_china_keeps_beijing_time() {
        assert_eq!(zone_minutes(Some("CN"), 81.8), 480);
        assert_eq!(zone_minutes(Some("FR"), 2.3), 60);
        // A country of many zones goes by longitude: Los Angeles.
        assert_eq!(zone_minutes(Some("US"), -118.2), -480);
        assert_eq!(zone_minutes(None, 139.7), 540);
    }

    #[test]
    fn the_map_every_front_end_draws_is_the_one_located_against() {
        let value: serde_json::Value = serde_json::from_str(world_json()).unwrap();
        let regions = value["regions"].as_array().unwrap();
        assert_eq!(regions.len(), world().regions.len());
        assert!(world().places.len() > 7000);
        // Every province names its country, and that country is on the map.
        for r in &world().regions {
            if let Some(parent) = &r.parent {
                assert!(
                    world().region(parent).is_some(),
                    "{} has no {}",
                    r.id,
                    parent
                );
            }
        }
    }
}
