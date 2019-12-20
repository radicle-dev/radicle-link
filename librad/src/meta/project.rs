use std::path::PathBuf;

use nonempty::NonEmpty;
use serde::{Deserialize, Serialize};

use crate::meta::common::{Label, Url, RAD_VERSION};
use crate::meta::serde_helpers;
use crate::peer::PeerId;

pub const DEFAULT_BRANCH: &str = "master";

pub fn default_branch() -> String {
    DEFAULT_BRANCH.into()
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct Project {
    rad_version: u8,

    revision: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default = "default_branch")]
    pub default_branch: String,

    #[serde(
        serialize_with = "serde_helpers::nonempty::serialize",
        deserialize_with = "serde_helpers::nonempty::deserialize"
    )]
    maintainers: NonEmpty<PeerId>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rel: Vec<Relation>,
}

impl Project {
    pub fn new(name: &str, founder: &PeerId) -> Self {
        Self {
            rad_version: RAD_VERSION,
            revision: 0,
            name: Some(name.to_string()),
            description: None,
            default_branch: DEFAULT_BRANCH.into(),
            maintainers: NonEmpty::new(founder.clone()),
            rel: vec![],
        }
    }

    pub fn rad_version(&self) -> u8 {
        self.rad_version
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_rel(&mut self, rel: Relation) {
        self.rel.push(rel)
    }

    pub fn add_rels(&mut self, rels: &mut Vec<Relation>) {
        self.rel.append(rels)
    }

    pub fn add_maintainer(&mut self, maintainer: &PeerId) {
        self.maintainers.push(maintainer.clone());
        self.dedup_maintainers();
    }

    pub fn add_maintainers(&mut self, maintainers: &mut Vec<PeerId>) {
        self.maintainers.append(maintainers);
        self.dedup_maintainers();
    }

    fn dedup_maintainers(&mut self) {
        let mut xs: Vec<PeerId> = self.maintainers.iter().cloned().collect();
        xs.dedup();
        self.maintainers = NonEmpty::from_slice(&xs).unwrap();
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub enum Relation {
    Tag(Label),
    Label(Label, String),
    Url(Label, Url),
    Path(Label, PathBuf),
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use proptest::prelude::*;
    use serde_json;

    use crate::keys::device;

    fn gen_project() -> impl Strategy<Value = Project> {
        (
            any::<u64>(),
            proptest::option::of(".*"),
            proptest::option::of(".*"),
            ".*",
            proptest::collection::vec(Just(PeerId::from(device::Key::new().public())), 1..32),
            proptest::collection::vec(gen_relation(), 0..16),
        )
            .prop_map(
                |(revision, name, description, branch, maintainers, rel)| Project {
                    rad_version: RAD_VERSION,
                    revision,
                    name,
                    description,
                    default_branch: branch,
                    maintainers: NonEmpty::from_slice(&maintainers).unwrap(),
                    rel,
                },
            )
    }

    fn gen_relation() -> impl Strategy<Value = Relation> {
        prop_oneof![
            ".*".prop_map(Relation::Tag),
            (".*", ".*").prop_map(|(l, v)| Relation::Label(l, v)),
            ".*".prop_map(|l| Relation::Url(l, Url::parse("https://acme.com/x/y").unwrap())),
            (".*", prop::collection::vec(".*", 1..32))
                .prop_map(|(l, xs)| Relation::Path(l, xs.iter().collect())),
        ]
    }

    proptest! {
        #[test]
        fn prop_relation_serde(rel in gen_relation()) {
            let rel_de = serde_json::to_string(&rel)
                .and_then(|ser| serde_json::from_str(&ser))
                .unwrap();
            assert_eq!(rel, rel_de)
        }

        #[test]
        fn prop_project_serde(proj in gen_project()) {
            let proj_de = serde_json::to_string(&proj)
                .and_then(|ser| serde_json::from_str(&ser))
                .unwrap();
            assert_eq!(proj, proj_de)
        }
    }
}
