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

//! What a video is of: the video butler's categories.
//!
//! A video is looked at in a few frames. Each frame's scene labels -- Apple
//! Vision's classifier on Apple platforms, SigLIP 2 against its table
//! elsewhere -- and the faces found in all of them decide which of a fixed
//! set of categories it belongs to: study, live-stream clips, children,
//! electronics, transport, food, gatherings, scenery, animals; and, for
//! ordering a year's memories, whether someone is a subject of it.
//!
//! # What this is
//!
//! Rules over labels, not a model trained to sort videos. A label vocabulary
//! is a tree of everyday things, and each category is a set of its labels
//! with a confidence a frame must reach. That is honest about its limits: a
//! category is as good as the labels under it, and two have nothing
//! dependable under them. *Study* is books, documents, handwriting and
//! whiteboards in the picture. *Live-stream clips* have no label at all: a
//! clip is one when it was recorded off a screen -- its name says so, or
//! Vision reads its frames as screenshots -- and someone is on that screen.
//! Both were set without real footage of either to check them on, and say
//! so where the app shows them.
//!
//! Faces decide the rest of what labels cannot: whether someone is the
//! subject or a passer-by is how large their face is in the picture.

use serde::{Deserialize, Serialize};
use vb_infer::ScoredLabel;

/// A category a video can be filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// Someone is a subject of it. Not shown as a category of its own --
    /// faces are, person by person -- but a year's memories start with it.
    People,
    Study,
    Livestream,
    Children,
    Electronics,
    Transport,
    Food,
    Gathering,
    Scenery,
    Animals,
}

impl Category {
    /// The categories the video butler shows, in the order it shows them.
    pub const SMART: [Category; 9] = [
        Category::Study,
        Category::Livestream,
        Category::Children,
        Category::Electronics,
        Category::Transport,
        Category::Food,
        Category::Gathering,
        Category::Scenery,
        Category::Animals,
    ];

    pub const ALL: [Category; 10] = [
        Category::People,
        Category::Study,
        Category::Livestream,
        Category::Children,
        Category::Electronics,
        Category::Transport,
        Category::Food,
        Category::Gathering,
        Category::Scenery,
        Category::Animals,
    ];

    /// As the Footage Graph and the API name it.
    pub fn id(self) -> &'static str {
        match self {
            Category::People => "people",
            Category::Study => "study",
            Category::Livestream => "livestream",
            Category::Children => "children",
            Category::Electronics => "electronics",
            Category::Transport => "transport",
            Category::Food => "food",
            Category::Gathering => "gathering",
            Category::Scenery => "scenery",
            Category::Animals => "animals",
        }
    }

    pub fn parse(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn zh(self) -> &'static str {
        match self {
            Category::People => "人物",
            Category::Study => "学习",
            Category::Livestream => "直播切片",
            Category::Children => "儿童",
            Category::Electronics => "电子产品",
            Category::Transport => "交通",
            Category::Food => "食物",
            Category::Gathering => "多人聚会",
            Category::Scenery => "风景",
            Category::Animals => "动物",
        }
    }

    pub fn en(self) -> &'static str {
        match self {
            Category::People => "People",
            Category::Study => "Study",
            Category::Livestream => "Live-stream clips",
            Category::Children => "Children",
            Category::Electronics => "Electronics",
            Category::Transport => "Transport",
            Category::Food => "Food",
            Category::Gathering => "Gatherings",
            Category::Scenery => "Scenery",
            Category::Animals => "Animals",
        }
    }

    /// Categories whose rules had no real footage to be set against.
    pub fn unverified(self) -> bool {
        matches!(self, Category::Study | Category::Livestream)
    }
}

/// Whose labels these are: each adapter has its own words and its own
/// scale of confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vocabulary {
    /// Apple Vision's classifier: about 1,300 identifiers in a tree.
    AppleVision,
    /// SigLIP 2 against `vb-onnx/data/scene-labels.tsv`, with each label's
    /// share of its kind as the confidence.
    Siglip,
}

impl Vocabulary {
    /// By the adapter that wrote the labels.
    pub fn of_adapter(name: &str) -> Option<Vocabulary> {
        match name {
            "AppleVision" => Some(Vocabulary::AppleVision),
            "SigLIP2" => Some(Vocabulary::Siglip),
            _ => None,
        }
    }
}

/// A face followed through a video's frames.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSeen {
    /// The largest it was, as a share of the picture's area.
    pub area: f64,
    /// Which of the frames it is in, by index.
    pub frames: Vec<usize>,
}

/// The smallest face, as a share of the picture, that makes someone a
/// subject of the video rather than someone who happened to be in it: about
/// a fifteenth of the frame's width and height. A group photographed from a
/// couple of metres passes; the people behind a tourist do not.
pub const SUBJECT_FACE: f64 = 0.004;

/// A face this large means the video is about that person, whatever is
/// behind them: a selfie in front of a mountain is not a landscape.
const PORTRAIT_FACE: f64 = 0.02;

impl FaceSeen {
    pub fn is_subject(&self) -> bool {
        self.area >= SUBJECT_FACE
    }
}

/// What was seen of one video.
#[derive(Debug, Clone)]
pub struct ClipLook {
    /// Each sampled frame's labels.
    pub frames: Vec<Vec<ScoredLabel>>,
    pub vocabulary: Option<Vocabulary>,
    /// Faces, when a tracker that measures them ran. `None` when none did,
    /// which is not the same as finding nobody.
    pub faces: Option<Vec<FaceSeen>>,
    /// It was recorded off a screen: a screen recorder's name for it, or a
    /// screenshot's for a photo.
    pub screen_recording: bool,
    /// A photograph, looked at once: one picture, not a recording.
    pub still: bool,
}

/// A file name a screenshot is given: macOS's 截屏 and Screenshot (屏幕快照
/// and Screen Shot before), Android's Screenshot_…, Windows' 屏幕截图, and
/// the tools people take them with.
pub fn named_as_screenshot(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["截屏", "截图", "屏幕截图", "屏幕快照"]
        .iter()
        .any(|w| name.contains(w))
        || ["screenshot", "screen shot", "screen_shot", "snipaste"]
            .iter()
            .any(|w| lower.contains(w))
}

/// A file name a screen recorder gives: macOS's 录屏 and Screen Recording,
/// Android's Screenrecorder and screen-…, and the rest.
pub fn named_as_screen_recording(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["录屏", "屏幕录制", "录制视频", "直播"]
        .iter()
        .any(|w| name.contains(w))
        || [
            "screen recording",
            "screen_recording",
            "screenrecord",
            "screen-record",
            "screen record",
            "screencapture",
            "screen_capture",
            "screen-",
            "rpreplay",
        ]
        .iter()
        .any(|w| lower.contains(w))
}

struct Rule {
    labels: &'static [&'static str],
    /// A frame counts when one of the labels reaches this.
    at: f64,
    /// And this many frames must count, out of however many were looked at
    /// (at least one, at most all).
    frames: usize,
}

fn vision_rule(category: Category) -> Option<Rule> {
    Some(match category {
        Category::Animals => Rule {
            labels: &[
                "animal",
                "mammal",
                "canine",
                "dog",
                "cat",
                "feline",
                "kitten",
                "bird",
                "fish",
                "reptile",
                "insect",
                "arthropods",
                "ungulates",
                "rodent",
                "cetacean",
                "marsupial",
                "raptor",
                "poultry",
                "horse",
                "cow",
                "sheep",
                "goat",
                "camel",
                "pig",
                "rabbit",
                "deer",
                "donkey",
                "llama",
                "elephant",
                "giraffe",
                "zebra",
                "bear",
                "panda",
                "lion",
                "tiger",
                "penguin",
                "dolphin",
                "butterfly",
                "hamster",
                "parrot",
                "swan",
                "pigeon",
                "gull",
            ],
            at: 0.4,
            frames: 1,
        },
        Category::Food => Rule {
            labels: &[
                "food",
                "dessert",
                "baked_goods",
                "fruit",
                "vegetable",
                "meat",
                "seafood",
                "shellfish_prepared",
                "drink",
                "coffee",
                "tea_drink",
                "juice",
                "cocktail",
                "beer",
                "wine",
                "cake",
                "cake_regular",
                "birthday_cake",
                "pastry",
                "bread",
                "pasta",
                "pizza",
                "rice",
                "soup",
                "salad",
                "sandwich",
                "ramen",
                "dumpling",
                "sushi",
                "hamburger",
                "fries",
                "ice_cream",
                "frozen_dessert",
                "chocolate",
                "candy",
                "cookie",
                "kebab",
                "grilled_chicken",
                "steak",
                "bubble_tea",
            ],
            at: 0.4,
            frames: 1,
        },
        Category::Transport => Rule {
            labels: &[
                "vehicle",
                "car",
                "automobile",
                "bus",
                "train",
                "train_real",
                "train_station",
                "railroad",
                "airplane",
                "aircraft",
                "airport",
                "helicopter",
                "boat",
                "watercraft",
                "cruise_ship",
                "sailboat",
                "yacht",
                "speedboat",
                "motorcycle",
                "bicycle",
                "truck",
                "suv",
                "van",
                "tramway",
                "streetcar",
                "monorail",
                "cableway",
                // Not `conveyance`: Vision files doors, windows, stairs and
                // escalators under it, and on 2,555 real videos it put 145
                // corridors and doorways in transport on its own.
                "engine_vehicle",
                "dashboard",
                "car_seat",
                "semi_truck",
                "police_car",
                "firetruck",
                "ambulance",
                "limousine",
                "rickshaw",
                "scooter",
                "sportscar",
                "convertible",
                "jeep",
            ],
            at: 0.5,
            frames: 1,
        },
        Category::Electronics => Rule {
            labels: &[
                "consumer_electronics",
                "computer",
                "laptop",
                "computer_monitor",
                "computer_keyboard",
                "computer_mouse",
                "computer_tower",
                "phone",
                "television",
                "headphones",
                "speakers_music",
                "gamepad",
                "joystick",
                "videogame",
                "circuit_board",
                "printer",
                "drone_machine",
                "keypad",
                "stereo",
                "camera",
            ],
            at: 0.45,
            frames: 1,
        },
        Category::Study => Rule {
            labels: &[
                "book",
                "bookshelf",
                // Not `document` itself: Vision gives it to every
                // screenshot and subtitled clip, at the same confidence as
                // what is under it. A real page comes with one of these.
                "printed_page",
                "handwriting",
                "whiteboard",
                "chalkboard",
                "classroom",
                "library",
                "office_supplies",
                "pen",
                "calculator",
                "diagram",
                "chart",
                "flipchart",
                "sticky_note",
                "magazine",
                "newspaper",
                "microscope",
                "lab_coat",
            ],
            at: 0.45,
            frames: 2,
        },
        Category::Children => Rule {
            labels: &[
                "baby",
                "child",
                "crib",
                "stroller",
                "high_chair",
                "pacifier",
                "diaper",
                "bib",
                "playground",
                "swing_playground",
                "slide_toy",
                "stuffed_animals",
                "blocks",
                "tricycle",
                "seesaw",
            ],
            at: 0.35,
            frames: 1,
        },
        Category::Gathering => Rule {
            labels: &[
                "celebration",
                "birthday_cake",
                "wedding",
                "wedding_cake",
                "ceremony",
                "concert",
                "nightclub",
                "karaoke",
                "parade",
                "carnival",
                "graduation",
                "conference",
            ],
            at: 0.45,
            frames: 1,
        },
        Category::Scenery => Rule {
            labels: &[
                "sky",
                "blue_sky",
                "mountain",
                "hill",
                "lake",
                "ocean",
                "beach",
                "shore",
                "river",
                "creek",
                "waterfall",
                "waterways",
                "water_body",
                "forest",
                "jungle",
                "desert",
                "sand_dune",
                "canyon",
                "cliff",
                "glacier",
                "snow",
                "iceberg",
                "island",
                "sunset_sunrise",
                "night_sky",
                "aurora",
                "rainbow",
                "cityscape",
                "grass",
                "rice_field",
                "vineyard",
                "orchard",
                "volcano",
                "wetland",
                "coral_reef",
                "underwater",
                "land",
                "celestial_body",
            ],
            at: 0.5,
            frames: 2,
        },
        Category::People | Category::Livestream => return None,
    })
}

fn siglip_rule(category: Category) -> Option<Rule> {
    // SigLIP's confidence is a label's share of its kind; 0.3 is the share
    // its labeller says a label at.
    let at = 0.3;
    Some(match category {
        Category::Animals => Rule {
            labels: &[
                "dog",
                "cat",
                "horse",
                "sheep",
                "cow",
                "camel",
                "bird",
                "fish",
                "zoo",
                "aquarium",
                "riding a horse",
            ],
            at,
            frames: 1,
        },
        Category::Food => Rule {
            labels: &[
                "food",
                "cake",
                "fruit",
                "noodles",
                "barbecue",
                "drinks",
                "eating",
                "cooking",
                "restaurant",
                "cafe",
            ],
            at,
            frames: 1,
        },
        Category::Transport => Rule {
            labels: &[
                "car",
                "bus",
                "train",
                "train station",
                "bicycle",
                "motorcycle",
                "airport",
                "airplane cabin",
                "view from a plane",
                "car interior",
                "view from a car",
                "boat",
                "driving",
                "cycling",
                "parking lot",
            ],
            at,
            frames: 1,
        },
        Category::Electronics => Rule {
            labels: &["screen"],
            at,
            frames: 1,
        },
        Category::Study => Rule {
            labels: &["book", "reading", "classroom"],
            at,
            frames: 2,
        },
        Category::Children => Rule {
            labels: &[
                "baby",
                "toddler",
                "child",
                "children",
                "playground",
                "toys",
                "crawling",
            ],
            at,
            frames: 1,
        },
        Category::Gathering => Rule {
            labels: &[
                "group of people",
                "family",
                "birthday party",
                "wedding",
                "festival",
                "crowd",
            ],
            at,
            frames: 1,
        },
        Category::Scenery => Rule {
            labels: &[
                "beach",
                "sea",
                "lake",
                "river",
                "waterfall",
                "mountains",
                "snowy mountains",
                "hills",
                "grassland",
                "desert",
                "forest",
                "farmland",
                "flower field",
                "garden",
                "park",
                "sunset",
                "sunrise",
                "night sky",
                "blue sky",
                "snow",
                "city at night",
                "village",
            ],
            at,
            frames: 2,
        },
        Category::People => Rule {
            labels: &[
                "baby",
                "toddler",
                "child",
                "children",
                "teenager",
                "couple",
                "family",
                "group of people",
                "elderly person",
                "man",
                "woman",
                "selfie",
                "portrait",
            ],
            at,
            frames: 1,
        },
        Category::Livestream => return None,
    })
}

impl ClipLook {
    /// How many frames reach the rule.
    fn frames_meeting(&self, rule: &Rule) -> usize {
        self.frames
            .iter()
            .filter(|labels| {
                labels
                    .iter()
                    .any(|l| l.confidence >= rule.at && rule.labels.contains(&l.label.as_str()))
            })
            .count()
    }

    fn meets(&self, rule: &Rule) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        let needed = rule.frames.clamp(1, self.frames.len());
        self.frames_meeting(rule) >= needed
    }

    fn rule(&self, category: Category) -> Option<Rule> {
        match self.vocabulary? {
            Vocabulary::AppleVision => vision_rule(category),
            Vocabulary::Siglip => siglip_rule(category),
        }
    }

    fn label_at(&self, label: &str, at: f64) -> usize {
        self.frames
            .iter()
            .filter(|labels| {
                labels
                    .iter()
                    .any(|l| l.label == label && l.confidence >= at)
            })
            .count()
    }

    /// The subjects -- faces large enough not to be passers-by -- in the
    /// frame that holds the most of them.
    pub fn most_subjects_at_once(&self) -> usize {
        let Some(faces) = &self.faces else {
            return 0;
        };
        (0..self.frames.len().max(1))
            .map(|frame| {
                faces
                    .iter()
                    .filter(|f| f.is_subject() && f.frames.contains(&frame))
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    fn largest_face(&self) -> f64 {
        self.faces
            .iter()
            .flatten()
            .map(|f| f.area)
            .fold(0.0, f64::max)
    }

    /// Someone is a subject of this video.
    pub fn has_people(&self) -> bool {
        match &self.faces {
            Some(faces) => faces.iter().any(FaceSeen::is_subject),
            // No face tracker ran: a label that names people is the best
            // there is.
            None => self.rule(Category::People).is_some_and(|r| self.meets(&r)),
        }
    }

    /// Recorded off a screen, by its name or by how its frames read: a live
    /// stream, a film, a call. Its faces are people on a screen, which the
    /// video butler counts and never files as the user's people.
    pub fn off_a_screen(&self) -> bool {
        self.screen_recording
            || (self.vocabulary == Some(Vocabulary::AppleVision)
                && self.label_at("screenshot", 0.5) * 2 > self.frames.len())
    }
}

/// The categories a video belongs to, `People` among them when someone is
/// its subject. Nothing when nothing looked at it.
pub fn classify(look: &ClipLook) -> Vec<Category> {
    if look.vocabulary.is_none() && look.faces.is_none() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let people = look.has_people();
    if people {
        found.push(Category::People);
    }
    let screen = look.off_a_screen();
    for category in Category::SMART {
        let yes = match category {
            // A recording: a screenshot of someone is a picture of a screen,
            // not a clip of a stream.
            Category::Livestream => !look.still && screen && (people || look.largest_face() > 0.0),
            // A screen recording is what is on the screen, not scenery.
            Category::Scenery => {
                !screen
                    && look.largest_face() < PORTRAIT_FACE
                    && look.rule(category).is_some_and(|r| look.meets(&r))
            }
            // Nor study: Vision files a screenshot under `document` too, at
            // the same confidence, and on 2,555 real videos 221 of the 257
            // it called study were a phone's screen recorded.
            Category::Study => !screen && look.rule(category).is_some_and(|r| look.meets(&r)),
            Category::Gathering => {
                look.most_subjects_at_once() >= 3
                    || (look.rule(category).is_some_and(|r| look.meets(&r))
                        && (look.faces.is_none() || look.most_subjects_at_once() >= 2))
            }
            _ => look.rule(category).is_some_and(|r| look.meets(&r)),
        };
        if yes {
            found.push(category);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, f64)]) -> Vec<ScoredLabel> {
        pairs
            .iter()
            .map(|(l, c)| ScoredLabel {
                label: (*l).to_string(),
                confidence: *c,
            })
            .collect()
    }

    fn look(frames: Vec<Vec<ScoredLabel>>, faces: Vec<FaceSeen>) -> ClipLook {
        ClipLook {
            frames,
            vocabulary: Some(Vocabulary::AppleVision),
            faces: Some(faces),
            screen_recording: false,
            still: false,
        }
    }

    #[test]
    fn a_landscape_with_nobody_in_it_is_scenery() {
        let frame = labels(&[("outdoor", 0.9), ("sky", 0.8), ("mountain", 0.7)]);
        let c = classify(&look(vec![frame.clone(), frame.clone(), frame], vec![]));
        assert_eq!(c, vec![Category::Scenery]);
    }

    #[test]
    fn a_selfie_in_front_of_a_mountain_is_people_not_scenery() {
        let frame = labels(&[("sky", 0.8), ("mountain", 0.7)]);
        let me = FaceSeen {
            area: 0.12,
            frames: vec![0, 1, 2],
        };
        let c = classify(&look(vec![frame.clone(), frame.clone(), frame], vec![me]));
        assert_eq!(c, vec![Category::People]);
    }

    #[test]
    fn passers_by_do_not_make_a_video_about_people() {
        let frame = labels(&[("cityscape", 0.8), ("sky", 0.6)]);
        let crowd = (0..5)
            .map(|_| FaceSeen {
                area: 0.0008,
                frames: vec![0, 1],
            })
            .collect();
        let c = classify(&look(vec![frame.clone(), frame.clone(), frame], crowd));
        assert_eq!(c, vec![Category::Scenery]);
    }

    #[test]
    fn three_people_in_one_frame_are_a_gathering() {
        let faces = (0..3)
            .map(|_| FaceSeen {
                area: 0.01,
                frames: vec![1],
            })
            .collect();
        let c = classify(&look(vec![vec![], vec![], vec![]], faces));
        assert!(c.contains(&Category::Gathering), "{c:?}");
        assert!(c.contains(&Category::People));
    }

    #[test]
    fn three_people_in_different_frames_are_not() {
        let faces = (0..3)
            .map(|i| FaceSeen {
                area: 0.01,
                frames: vec![i],
            })
            .collect();
        let c = classify(&look(vec![vec![], vec![], vec![]], faces));
        assert!(!c.contains(&Category::Gathering), "{c:?}");
    }

    #[test]
    fn one_frame_of_food_files_it_under_food() {
        let c = classify(&look(
            vec![labels(&[("food", 0.8), ("dessert", 0.6)]), vec![], vec![]],
            vec![],
        ));
        assert_eq!(c, vec![Category::Food]);
    }

    #[test]
    fn study_needs_more_than_one_glimpse_of_a_book() {
        let book = labels(&[("book", 0.6)]);
        assert!(classify(&look(vec![book.clone(), vec![], vec![]], vec![])).is_empty());
        let c = classify(&look(vec![book.clone(), book, vec![]], vec![]));
        assert_eq!(c, vec![Category::Study]);
    }

    #[test]
    fn a_phones_screen_recorded_is_not_study() {
        // As Vision labels a recorded chat or shopping app: a screenshot,
        // and a document at the same confidence.
        let screen = labels(&[("document", 0.93), ("screenshot", 0.93)]);
        let c = classify(&look(vec![screen.clone(), screen.clone(), screen], vec![]));
        assert!(!c.contains(&Category::Study), "{c:?}");
        // Nor is a saved clip with subtitles, which reads as a faint one.
        let subtitled = labels(&[("people", 0.86), ("document", 0.47), ("screenshot", 0.47)]);
        let c = classify(&look(
            vec![subtitled.clone(), subtitled.clone(), subtitled],
            vec![],
        ));
        assert!(!c.contains(&Category::Study), "{c:?}");
        // A printed form filmed with the camera still is.
        let form = labels(&[("document", 0.79), ("printed_page", 0.79)]);
        let c = classify(&look(vec![form.clone(), form, vec![]], vec![]));
        assert_eq!(c, vec![Category::Study]);
    }

    #[test]
    fn a_doorway_is_not_transport() {
        // Vision files doors and windows under `conveyance`.
        let door = labels(&[
            ("structure", 0.65),
            ("conveyance", 0.65),
            ("portal", 0.65),
            ("window", 0.65),
        ]);
        let c = classify(&look(vec![door.clone(), door.clone(), door], vec![]));
        assert!(!c.contains(&Category::Transport), "{c:?}");
        let car = labels(&[("vehicle", 0.8), ("car", 0.8)]);
        let c = classify(&look(vec![car, vec![], vec![]], vec![]));
        assert_eq!(c, vec![Category::Transport]);
    }

    #[test]
    fn a_screen_recording_of_someone_is_a_live_stream_clip() {
        let mut clip = look(
            vec![labels(&[("screenshot", 0.7)]), vec![], vec![]],
            vec![FaceSeen {
                area: 0.05,
                frames: vec![0],
            }],
        );
        clip.screen_recording = true;
        let c = classify(&clip);
        assert!(c.contains(&Category::Livestream), "{c:?}");
        // The same recording with nobody on screen is not.
        clip.faces = Some(vec![]);
        assert!(!classify(&clip).contains(&Category::Livestream));
    }

    #[test]
    fn a_screenshot_of_someone_is_off_a_screen_and_not_a_live_stream_clip() {
        // A video call or a chat with a face in it, captured as a picture.
        let mut shot = look(
            vec![labels(&[("people", 0.8)])],
            vec![FaceSeen {
                area: 0.05,
                frames: vec![0],
            }],
        );
        shot.still = true;
        shot.screen_recording = named_as_screenshot("Screenshot_20250102-101500.png");
        assert!(shot.off_a_screen());
        let c = classify(&shot);
        assert!(!c.contains(&Category::Livestream), "{c:?}");
    }

    #[test]
    fn screenshots_names_are_known() {
        assert!(named_as_screenshot("截屏2026-10-17 10.02.03.png"));
        assert!(named_as_screenshot("Screenshot 2026-10-17 at 10.02.03.png"));
        assert!(named_as_screenshot("Screenshot_20250102-101500_WeChat.jpg"));
        assert!(named_as_screenshot("屏幕截图 2026-10-17 100203.png"));
        assert!(named_as_screenshot("Snipaste_2026-10-17_10-02-03.png"));
        assert!(!named_as_screenshot("IMG_20230929_183143.jpg"));
        assert!(!named_as_screenshot("P30929-183143.JPG"));
    }

    #[test]
    fn recorders_names_are_known() {
        assert!(named_as_screen_recording("录屏2026-09-24 10.23.03.mov"));
        assert!(named_as_screen_recording(
            "Screen Recording 2026-01-02 at 10.00.mov"
        ));
        assert!(named_as_screen_recording(
            "Screenrecorder-2025-01-01-10-00-00-123.mp4"
        ));
        assert!(named_as_screen_recording("RPReplay_Final1690000000.MP4"));
        assert!(!named_as_screen_recording("V30929-184730.mp4"));
        assert!(!named_as_screen_recording("dji_mimo_20260706_213502_0.mp4"));
    }

    #[test]
    fn nothing_looked_means_no_categories() {
        let clip = ClipLook {
            frames: vec![],
            vocabulary: None,
            faces: None,
            screen_recording: false,
            still: false,
        };
        assert!(classify(&clip).is_empty());
    }

    #[test]
    fn siglip_labels_file_footage_too() {
        let clip = ClipLook {
            frames: vec![
                labels(&[("lake", 0.6), ("couple", 0.5)]),
                labels(&[("lake", 0.5)]),
                labels(&[("horse", 0.7)]),
            ],
            vocabulary: Some(Vocabulary::Siglip),
            faces: None,
            screen_recording: false,
            still: false,
        };
        let c = classify(&clip);
        assert_eq!(
            c,
            vec![Category::People, Category::Scenery, Category::Animals]
        );
    }

    #[test]
    fn every_category_round_trips_its_id() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.id()), Some(c));
        }
    }
}
