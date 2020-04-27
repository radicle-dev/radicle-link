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

use crate::meta::{
    common::{EmailAddr, Label, Url},
    entity::{data::EntityData, Entity, Error},
    profile::{Geo, ProfileImage, UserProfile},
    serde_helpers,
};

#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
pub struct ContributorInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serde_helpers::urltemplate::serialize_opt",
        deserialize_with = "serde_helpers::urltemplate::deserialize_opt"
    )]
    pub largefiles: Option<UrlTemplate>,
}

impl ContributorInfo {
    pub fn new() -> Self {
        Self {
            profile: None,
            largefiles: None,
        }
    }
}

pub type ContributorData = EntityData<ContributorInfo>;

impl ContributorData {
    pub fn new() -> Self {
        let result = Self::default();
        result
    }

    fn profile_ref_mut(&mut self) -> Result<&mut ProfileRef, Error> {
        match &mut self.info.profile {
            Some(p) => Ok(p),
            None => Err(Error::MissingRootHash),
        }
    }

    fn user_profile_mut(&mut self) -> Result<&mut UserProfile, Error> {
        match self.profile_ref_mut()? {
            ProfileRef::UserProfile(p) => Ok(p),
            ProfileRef::Url(_) => Err(Error::MissingRootHash),
        }
    }

    pub fn clear_profile(mut self) -> Self {
        self.info.profile = None;
        self
    }

    pub fn set_profile_ref(mut self, profile_ref: Option<ProfileRef>) -> Self {
        self.info.profile = profile_ref;
        self
    }

    pub fn set_profile_url(mut self, url: Url) -> Self {
        self.info.profile = Some(ProfileRef::Url(url));
        self
    }

    pub fn set_profile(mut self, profile: UserProfile) -> Self {
        self.info.profile = Some(ProfileRef::UserProfile(profile));
        self
    }

    pub fn new_profile(mut self, nick: String) -> Self {
        self.info.profile = Some(ProfileRef::UserProfile(UserProfile::new(nick)));
        self
    }

    pub fn set_profile_nick(mut self, nick: String) -> Result<Self, Error> {
        self.user_profile_mut()?.nick = nick;
        Ok(self)
    }

    pub fn set_profile_name(mut self, name: String) -> Result<Self, Error> {
        self.user_profile_mut()?.name = Some(name);
        Ok(self)
    }

    pub fn set_profile_image(mut self, img: ProfileImage) -> Result<Self, Error> {
        self.user_profile_mut()?.img = Some(img);
        Ok(self)
    }

    pub fn set_profile_bio(mut self, bio: String) -> Result<Self, Error> {
        self.user_profile_mut()?.bio = Some(bio);
        Ok(self)
    }

    pub fn set_profile_email(mut self, email: EmailAddr) -> Result<Self, Error> {
        self.user_profile_mut()?.email = Some(email);
        Ok(self)
    }

    pub fn set_profile_geo(mut self, geo: Geo) -> Result<Self, Error> {
        self.user_profile_mut()?.geo = Some(geo);
        Ok(self)
    }

    pub fn add_profile_url(mut self, label: Label, url: Url) -> Result<Self, Error> {
        self.user_profile_mut()?.urls.insert(label, url);
        Ok(self)
    }

    pub fn remove_profile_url(mut self, label: &Label) -> Result<Self, Error> {
        self.user_profile_mut()?.urls.remove(label);
        Ok(self)
    }

    pub fn set_largefiles(mut self, largefiles: Option<UrlTemplate>) -> Self {
        self.info.largefiles = largefiles;
        self
    }

    pub fn build(self) -> Result<Contributor, Error> {
        // FIXME: require at least one key (unify build methods and invariant checks)
        Contributor::from_data(self)
    }
}

pub type Contributor = Entity<ContributorInfo>;

impl Contributor {
    // FIXME: require at least one key!
    pub fn new() -> Result<Self, Error> {
        ContributorData::new().build()
    }

    pub fn profile(&self) -> &Option<ProfileRef> {
        &self.info().profile
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

            ContributorData::new()
                .set_name("foo".to_owned())
                .set_revision(1)
                .set_profile_ref(profile)
                .set_largefiles(largefiles)
                .build()
                .unwrap()
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
