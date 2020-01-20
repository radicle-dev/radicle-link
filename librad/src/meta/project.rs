use std::collections::HashSet;
use std::path::PathBuf;

use hex::ToHex;
use nonempty::NonEmpty;
use serde::{Deserialize, Serialize};
use serde_json;

use crate::keys::device::{Key, Signature};
use crate::{
    meta::{
        common::{Label, Url, RAD_VERSION},
        serde_helpers,
    },
    peer::PeerId,
};

pub const DEFAULT_BRANCH: &str = "master";

pub fn default_branch() -> String {
    DEFAULT_BRANCH.into()
}

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Cannot serialize project metadata")]
    SerializationFailed(serde_json::error::Error),
    #[fail(display = "Signature already present")]
    SignatureAlreadyPresent,
    #[fail(display = "Signature from non maintainer")]
    SignatureFromNonMaintainer,
    #[fail(display = "Signature missing")]
    SignatureMissing,
    #[fail(display = "Signature decoding failed")]
    SignatureDecodingFailed,
    #[fail(display = "Signature verification failed")]
    SignatureVerificationFailed,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct ProjectSignature {
    pub key: PeerId,
    pub sig: String,
}

impl ProjectSignature {
    pub fn verify_data(&self, data: &[u8]) -> Result<(), Error> {
        let sig =
            Signature::from_hex_string(&self.sig).map_err(|_| Error::SignatureDecodingFailed)?;
        if sig.verify(data, self.key.device_key()) {
            Ok(())
        } else {
            Err(Error::SignatureVerificationFailed)
        }
    }
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

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub signatures: Vec<ProjectSignature>,
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
            signatures: vec![],
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
        xs.sort();
        xs.dedup();
        self.maintainers = NonEmpty::from_slice(&xs).unwrap();
    }

    pub fn canonical_text_contents(&self) -> serde_json::Result<String> {
        let mut data = self.clone();
        data.signatures.clear();
        // TODO: actually canonicalize the output format (use a different serializer)
        let data = serde_json::to_string(&data)?;
        Ok(data)
    }

    pub fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        match serde_json::to_string(&self) {
            Ok(s) => Ok(s.into_bytes()),
            Err(err) => Err(Error::SerializationFailed(err)),
        }
    }

    pub fn sign(&self, key: &Key) -> Result<Signature, Error> {
        Ok(key.sign(&self.canonical_data()?))
    }

    pub fn build_signature(&self, key: &Key) -> Result<ProjectSignature, Error> {
        let signature = self.sign(key)?;
        Ok(ProjectSignature {
            key: PeerId::from(key.clone()),
            sig: signature.encode_hex_upper(),
        })
    }

    pub fn add_signature(&mut self, key: &Key) -> Result<(), Error> {
        let id = PeerId::from(key.clone());
        if self.signatures.iter().any(|s| s.key == id) {
            Err(Error::SignatureAlreadyPresent)
        } else if !self.maintainers.iter().any(|m| m == &id) {
            Err(Error::SignatureFromNonMaintainer)
        } else {
            self.signatures.push(self.build_signature(key)?);
            Ok(())
        }
    }

    pub fn verify_signature(&self, signature: &ProjectSignature) -> Result<(), Error> {
        signature.verify_data(&self.canonical_data()?)
    }

    pub fn verify_signatures(&self) -> Result<(), Error> {
        let data = self.canonical_data()?;
        for s in self.signatures.iter() {
            s.verify_data(&data)?
        }
        Ok(())
    }

    pub fn check_signatures_against_maintainers(&self) -> Result<(), Error> {
        let mut maintainers = HashSet::<&PeerId>::new();
        for m in self.maintainers.iter() {
            maintainers.insert(m);
        }

        for s in self.signatures.iter() {
            let m = maintainers.take(&s.key);
            match m {
                Some(_) => {}
                None => return Err(Error::SignatureFromNonMaintainer),
            }
        }

        if maintainers.len() > 0 {
            return Err(Error::SignatureMissing);
        }

        Ok(())
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
    use sodiumoxide::crypto::sign::Seed;

    use crate::keys::device;

    fn gen_project() -> impl Strategy<Value = Project> {
        (
            any::<u64>(),
            proptest::option::of(".*"),
            proptest::option::of(".*"),
            ".*",
            proptest::collection::vec(Just(PeerId::from(device::Key::new().public())), 1..32),
            proptest::collection::vec(gen_relation(), 0..16),
            proptest::collection::vec(gen_project_signature(), 0..16),
        )
            .prop_map(
                |(revision, name, description, branch, maintainers, rel, signatures)| Project {
                    rad_version: RAD_VERSION,
                    revision,
                    name,
                    description,
                    default_branch: branch,
                    maintainers: NonEmpty::from_slice(&maintainers).unwrap(),
                    rel,
                    signatures,
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

    fn gen_project_signature() -> impl Strategy<Value = ProjectSignature> {
        prop_oneof![".*".prop_map(|sig| ProjectSignature {
            key: PeerId::from(device::Key::new().public()),
            sig,
        }),]
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

    const SEED: Seed = Seed([
        20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81,
        181, 134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
    ]);
    const CREATED_AT: u64 = 1_576_843_598;
    fn new_peer(seed_value: u8) -> PeerId {
        let mut seed = SEED;
        seed.0[0] = seed_value;
        let created_at = std::time::SystemTime::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(CREATED_AT))
            .expect("SystemTime overflow o.O");
        let key = device::Key::from_seed(&seed, created_at);
        PeerId::from(key.public())
    }

    #[test]
    fn test_dedup_maintainers() {
        let founder = new_peer(42);
        let mut prj = Project::new("foo", &founder);
        let m1 = new_peer(1);
        let m2 = new_peer(2);
        prj.add_maintainer(&m1);
        prj.add_maintainer(&m2);
        assert_eq!(3, prj.maintainers.len());
        prj.add_maintainer(&m1);
        assert_eq!(3, prj.maintainers.len());
    }
}
