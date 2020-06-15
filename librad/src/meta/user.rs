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

use crate::{
    keys::PublicKey,
    meta::{
        common::{EmailAddr, Label, Url},
        entity::{
            data::{EntityData, EntityInfoExt, EntityKind},
            Draft,
            Entity,
            Error,
        },
        profile::{Geo, ProfileImage, UserProfile},
        serde_helpers,
    },
};

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct UserInfo {
    // Marker so `EntityInfo` can deserialize correctly
    user: (),

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serde_helpers::urltemplate::serialize_opt",
        deserialize_with = "serde_helpers::urltemplate::deserialize_opt"
    )]
    pub largefiles: Option<UrlTemplate>,
}

impl Default for UserInfo {
    fn default() -> Self {
        Self {
            user: (),
            profile: None,
            largefiles: None,
        }
    }
}

impl EntityInfoExt for UserInfo {
    fn kind(&self) -> EntityKind {
        EntityKind::User
    }

    fn check_invariants<T>(&self, data: &EntityData<T>) -> Result<(), Error> {
        // Require at least one signing key
        if data.keys.is_empty() {
            return Err(Error::InvalidData("Missing keys".to_owned()));
        }
        Ok(())
    }
}

impl UserInfo {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type UserData = EntityData<UserInfo>;

impl UserData {
    fn profile_ref_mut(&mut self) -> Result<&mut ProfileRef, Error> {
        match &mut self.info.profile {
            Some(p) => Ok(p),
            None => Err(Error::BuilderError("Missing user profile")),
        }
    }

    fn user_profile_mut(&mut self) -> Result<&mut UserProfile, Error> {
        match self.profile_ref_mut()? {
            ProfileRef::UserProfile(p) => Ok(p),
            ProfileRef::Url(_) => Err(Error::BuilderError("Missing user profile")),
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

    pub fn remove_profile_url(mut self, label: &str) -> Result<Self, Error> {
        self.user_profile_mut()?.urls.remove(label);
        Ok(self)
    }

    pub fn set_largefiles(mut self, largefiles: Option<UrlTemplate>) -> Self {
        self.info.largefiles = largefiles;
        self
    }
}

pub type User<ST> = Entity<UserInfo, ST>;

impl<ST> User<ST>
where
    ST: Clone,
{
    pub fn create(name: String, key: PublicKey) -> Result<User<Draft>, Error> {
        UserData::default()
            .set_name(name)
            .set_revision(1)
            .add_key(key)
            .build()
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

    use crate::meta::{
        entity::GenericDraftEntity,
        entity_test::K1,
        profile::tests::gen_user_profile,
    };

    pub fn gen_profile_ref() -> impl Strategy<Value = ProfileRef> {
        prop_oneof![
            gen_user_profile().prop_map(ProfileRef::UserProfile),
            Just(ProfileRef::Url(Url::parse("ipfs://Qmdeadbeef").unwrap())),
        ]
    }

    pub fn gen_largefiles() -> impl Strategy<Value = Option<UrlTemplate>> {
        prop_oneof![
            Just(None),
            Just(Some(UrlTemplate::from(
                "https://git-lfs.github.com/{SHA512}"
            ))),
        ]
    }

    pub fn gen_user() -> impl Strategy<Value = User<Draft>> {
        (proptest::option::of(gen_profile_ref()), gen_largefiles()).prop_map(
            |(profile, largefiles)| {
                UserData::default()
                    .set_name("foo".to_owned())
                    .set_revision(1)
                    .add_key(K1.public())
                    .set_profile_ref(profile)
                    .set_largefiles(largefiles)
                    .build()
                    .unwrap()
            },
        )
    }

    proptest! {
        #[test]
        fn prop_user_serde(user in gen_user()) {
            let ser = serde_json::to_string(&user).unwrap();
            let user_de = serde_json::from_str(&ser).unwrap();
            assert_eq!(user, user_de);

            let generic_de = GenericDraftEntity::from_json_str(&ser).unwrap();
            let generic_ser = generic_de.to_json_string().unwrap();
            assert_eq!(ser, generic_ser);
        }
    }
}
