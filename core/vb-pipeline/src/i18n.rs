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

//! The language the engine speaks to a user in.
//!
//! Most of what the engine returns is data, and the app words it. Some of it
//! is sentences only the engine can write -- why a run fell back to a
//! steady tempo, which models ran, what the self-test found -- and those
//! follow the language the user chose in the app. The CLI stays English.
//!
//! Two languages, written out side by side at the point each sentence is
//! made, rather than a catalogue keyed by id: there are few enough of them
//! that seeing both versions together is what keeps them saying the same
//! thing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    /// From a settings or request value: `zh`, `zh-CN`, `zh-Hans` and the
    /// like are Chinese; anything else is English.
    pub fn parse(value: Option<&str>) -> Lang {
        match value {
            Some(v) if v.to_ascii_lowercase().starts_with("zh") => Lang::Zh,
            _ => Lang::En,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    /// Pick the sentence for this language.
    pub fn t<'a>(self, en: &'a str, zh: &'a str) -> &'a str {
        match self {
            Lang::En => en,
            Lang::Zh => zh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_is_recognised_in_its_usual_spellings() {
        for value in ["zh", "zh-CN", "zh_Hans", "ZH"] {
            assert_eq!(Lang::parse(Some(value)), Lang::Zh, "{value}");
        }
        assert_eq!(Lang::parse(Some("en-GB")), Lang::En);
        assert_eq!(Lang::parse(None), Lang::En);
    }

    #[test]
    fn a_sentence_is_picked_by_language() {
        assert_eq!(Lang::Zh.t("Done", "完成"), "完成");
        assert_eq!(Lang::En.t("Done", "完成"), "Done");
    }
}
