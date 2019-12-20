use pgp;
use serde::{Deserialize, Serialize};
use urltemplate::UrlTemplate;

use crate::meta::common::{Url, RAD_VERSION};
use crate::meta::profile::UserProfile;
use crate::meta::serde_helpers;

#[derive(Deserialize, Serialize, Debug, Default, PartialEq)]
pub struct Contributor {
    rad_version: u8,

    revision: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serde_helpers::urltemplate::serialize_opt",
        deserialize_with = "serde_helpers::urltemplate::deserialize_opt"
    )]
    pub largefiles: Option<UrlTemplate>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serde_helpers::pgp_fingerprint::serialize_opt",
        deserialize_with = "serde_helpers::pgp_fingerprint::deserialize_opt"
    )]
    pub signing_key: Option<pgp::Fingerprint>,
}

impl Contributor {
    pub fn new() -> Self {
        Self {
            rad_version: RAD_VERSION,
            revision: 0,
            profile: None,
            largefiles: None,
            signing_key: None,
        }
    }

    pub fn rad_version(&self) -> u8 {
        self.rad_version
    }

    pub fn revision(&self) -> u64 {
        self.revision
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
        (any::<u64>(), proptest::option::of(gen_profile_ref())).prop_map(|(revision, profile)| {
            let largefiles = Some(UrlTemplate::from("https://git-lfs.github.com/{SHA512}"));
            let signing_key = Some(
                pgp::Fingerprint::from_hex("3E8877C877274692975189F5D03F6F865226FE8B").unwrap(),
            );

            Contributor {
                rad_version: RAD_VERSION,
                revision,
                profile,
                largefiles,
                signing_key,
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
