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

use serde::{Deserialize, Serialize};
use urltemplate::UrlTemplate;

use crate::meta::{common::Url, profile::UserProfile, serde_helpers};

#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
pub struct Contributor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serde_helpers::urltemplate::serialize_opt",
        deserialize_with = "serde_helpers::urltemplate::deserialize_opt"
    )]
    pub largefiles: Option<UrlTemplate>,
}

impl Contributor {
    pub fn new() -> Self {
        Self {
            profile: None,
            largefiles: None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum ProfileRef {
    UserProfile(UserProfile),
    Url(Url),
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use proptest::prelude::*;
    use serde_json;

    use crate::meta::profile::tests::gen_user_profile;

    pub fn gen_profile_ref() -> impl Strategy<Value = ProfileRef> {
        prop_oneof![
            gen_user_profile().prop_map(ProfileRef::UserProfile),
            Just(ProfileRef::Url(Url::parse("ipfs://Qmdeadbeef").unwrap())),
        ]
    }

    pub fn gen_contributor() -> impl Strategy<Value = Contributor> {
        proptest::option::of(gen_profile_ref()).prop_map(|profile| {
            let largefiles = Some(UrlTemplate::from("https://git-lfs.github.com/{SHA512}"));

            Contributor {
                profile,
                largefiles,
            }
        })
    }

    proptest! {
        #[test]
        fn prop_contributor_serde(contrib in gen_contributor()) {
            let ser = serde_json::to_string(&contrib).unwrap();
            let contrib_de = serde_json::from_str(&ser).unwrap();
            assert_eq!(contrib, contrib_de)
        }
    }
}
