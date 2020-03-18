// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::meta::common::{EmailAddr, Label, Url};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct UserProfile {
    pub nick: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img: Option<ProfileImage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailAddr>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo: Option<Geo>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub urls: HashMap<Label, Url>,
}

impl UserProfile {
    pub fn new(nick: &str) -> Self {
        Self {
            nick: nick.to_string(),
            name: None,
            img: None,
            bio: None,
            email: None,
            geo: None,
            urls: HashMap::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum ProfileImage {
    Path(PathBuf),
    Url(Url),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum Geo {
    LatLon(f32, f32),
    Earth,
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use proptest::prelude::*;
    use serde_json;

    pub fn gen_geo() -> impl Strategy<Value = Geo> {
        prop_oneof![
            Just(Geo::Earth),
            (any::<f32>(), any::<f32>()).prop_map(|(lat, lon)| Geo::LatLon(lat, lon)),
        ]
    }

    pub fn gen_path(max_length: usize) -> impl Strategy<Value = PathBuf> {
        proptest::collection::vec("[[:print:]&&[^ /.'\"]]{1,32}", 0..max_length)
            .prop_map(|xs| xs.iter().collect())
    }

    pub fn gen_profile_image() -> impl Strategy<Value = ProfileImage> {
        prop_oneof![
            gen_path(32).prop_map(ProfileImage::Path),
            Just(ProfileImage::Url(Url::parse("ipfs://Qmdeadbeef").unwrap()))
        ]
    }

    pub fn gen_addr_spec() -> impl Strategy<Value = EmailAddr> {
        Just(EmailAddr::parse("leboeuf@acme.org").expect("Invalid EmailAddr"))
    }

    pub fn gen_user_profile() -> impl Strategy<Value = UserProfile> {
        (
            ".*",
            proptest::option::of(".*"),
            proptest::option::of(gen_profile_image()),
            proptest::option::of(".*"),
            proptest::option::of(gen_addr_spec()),
            proptest::option::of(gen_geo()),
        )
            .prop_map(|(nick, name, img, bio, email, geo)| UserProfile {
                nick,
                name,
                img,
                bio,
                email,
                geo,
                urls: HashMap::default(),
            })
    }

    proptest! {
        #[test]
        fn prop_user_profile_serde(profile in gen_user_profile()) {
            let ser = serde_json::to_string(&profile).unwrap();
            let profile_de = serde_json::from_str(&ser).unwrap();
            assert_eq!(profile, profile_de)
        }
    }
}
