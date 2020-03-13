use hex::{decode, encode};
use multihash::{Multihash, Sha2_256};
use std::collections::{HashMap, HashSet};
//use serde::{Deserialize, Serialize};
//use serde_json;
use crate::{
    keys::device::{Key, PublicKey, Signature},
    peer::PeerId,
};

pub mod data;

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Serialization failed")]
    SerializationFailed(serde_json::error::Error),

    #[fail(display = "Invalid UTF8")]
    InvalidUtf8(std::string::FromUtf8Error),

    #[fail(display = "Invalid hex")]
    InvalidHex(String),

    #[fail(display = "Invalid hash")]
    InvalidHash(String),

    #[fail(display = "Signature already present")]
    SignatureAlreadyPresent(PublicKey),

    #[fail(display = "Key not present")]
    KeyNotPresent(PublicKey),

    #[fail(display = "User not present")]
    UserNotPresent(RadicleUri),

    #[fail(display = "User key not present")]
    UserKeyNotPresent(RadicleUri, PublicKey),

    #[fail(display = "Signature missing")]
    SignatureMissing,

    #[fail(display = "Signature decoding failed")]
    SignatureDecodingFailed,

    #[fail(display = "Signature verification failed")]
    SignatureVerificationFailed,

    #[fail(display = "Resolution failed")]
    ResolutionFailed(String),
}

#[derive(Clone, Debug)]
pub struct RadicleUri {
    hash: Multihash,
    /* pub repo: Multihash,
     * pub root: Multihash,
     * pub branch: String,
     * pub file: String, */
}

impl RadicleUri {
    pub fn new(hash: Multihash) -> Self {
        Self { hash }
    }
    pub fn hash(&self) -> &Multihash {
        &self.hash
    }

    pub fn from_str(s: &str) -> Result<Self, Error> {
        let bytes = decode(s).map_err(|_| Error::InvalidHex(s.to_owned()))?;
        let hash = Multihash::from_bytes(bytes).map_err(|_| Error::InvalidHash(s.to_owned()))?;
        Ok(Self { hash })
    }
}

impl ToString for RadicleUri {
    fn to_string(&self) -> String {
        encode(self.hash.to_vec())
    }
}

#[derive(Clone, Debug)]
pub enum Signatory {
    User(RadicleUri),
    OwnedKey,
}

#[derive(Clone, Debug)]
pub struct EntitySignature {
    pub by: Signatory,
    pub sig: Signature,
}

pub trait Resolver<E>
where
    E: Entity,
{
    fn resolve(&self, uri: &RadicleUri) -> Result<E, Error>;
}

pub trait RevisionsResolver<E>
where
    E: Entity,
{
    fn resolve_revisions(&self, uri: &RadicleUri) -> Box<dyn Iterator<Item = &E>>;
}

pub trait Entity {
    fn name(&self) -> &str;
    fn revision(&self) -> u64;

    fn canonical_data(&self) -> Result<Vec<u8>, Error>;

    fn uri(&self) -> &RadicleUri;
    fn signatures(&self) -> &HashMap<PublicKey, EntitySignature>;
    fn signatures_mut(&mut self) -> &mut HashMap<PublicKey, EntitySignature>;

    fn keys_count(&self) -> usize {
        0
    }
    fn has_key(&self, _key: &PublicKey) -> bool {
        false
    }
    fn keys<'a>(&'a self) -> Box<dyn Iterator<Item = PublicKey> + 'a> {
        Box::new(std::iter::empty())
    }

    fn certifiers_count(&self) -> usize {
        0
    }
    fn has_certifier(&self, _key: &RadicleUri) -> bool {
        false
    }
    fn certifiers(&self) -> Box<dyn Iterator<Item = RadicleUri>> {
        Box::new(std::iter::empty())
    }

    fn compute_hash(&self) -> Result<Multihash, Error> {
        Ok(Sha2_256::digest(&self.canonical_data()?))
    }

    fn check_user_key(
        &self,
        user_uri: &RadicleUri,
        key: &PublicKey,
        resolver: &impl Resolver<User>,
    ) -> Result<(), Error> {
        if !self.has_key(key) {
            return Err(Error::KeyNotPresent(key.to_owned()));
        }
        let user = resolver.resolve(user_uri)?;
        if !user.has_key(key) {
            return Err(Error::UserKeyNotPresent(
                user_uri.to_owned(),
                key.to_owned(),
            ));
        }
        Ok(())
    }

    fn check_key(
        &self,
        key: &PublicKey,
        by: &Signatory,
        resolver: &impl Resolver<User>,
    ) -> Result<(), Error> {
        match by {
            Signatory::OwnedKey => {
                if !self.has_key(key) {
                    return Err(Error::KeyNotPresent(key.to_owned()));
                }
            },
            Signatory::User(uri) => {
                let user = resolver.resolve(&uri)?;
                if !user.has_key(key) {
                    return Err(Error::UserKeyNotPresent(uri.to_owned(), key.to_owned()));
                }
            },
        }
        Ok(())
    }

    fn compute_signature(&self, key: &Key) -> Result<Signature, Error> {
        Ok(key.sign(&self.canonical_data()?))
    }

    fn sign(
        &mut self,
        key: &Key,
        by: &Signatory,
        resolver: &impl Resolver<User>,
    ) -> Result<(), Error> {
        let public_key = key.public();
        self.check_key(&public_key, by, resolver)?;
        let signature = EntitySignature {
            by: by.to_owned(),
            sig: self.compute_signature(key)?,
        };
        self.signatures_mut().insert(public_key, signature);
        Ok(())
    }

    fn check_signature(
        &mut self,
        key: &PublicKey,
        by: &Signatory,
        signature: &Signature,
        resolver: &impl Resolver<User>,
    ) -> Result<(), Error> {
        self.check_key(key, by, resolver)?;
        if signature.verify(&self.canonical_data()?, key) {
            Ok(())
        } else {
            Err(Error::SignatureVerificationFailed)
        }
    }
}

pub struct User {
    pub name: String,
    pub revision: u64,

    pub devices: HashSet<PeerId>,

    pub uri: RadicleUri,
    pub signatures: HashMap<PublicKey, EntitySignature>,
}

impl Entity for User {
    fn name(&self) -> &str {
        &self.name
    }
    fn revision(&self) -> u64 {
        self.revision
    }

    fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        unimplemented!()
    }

    fn uri(&self) -> &RadicleUri {
        &self.uri
    }
    fn signatures(&self) -> &HashMap<PublicKey, EntitySignature> {
        &self.signatures
    }
    fn signatures_mut(&mut self) -> &mut HashMap<PublicKey, EntitySignature> {
        &mut self.signatures
    }

    fn keys_count(&self) -> usize {
        self.devices.len()
    }
    fn has_key(&self, key: &PublicKey) -> bool {
        self.devices.contains(&PeerId::from(key.to_owned()))
    }
    fn keys<'a>(&'a self) -> Box<dyn Iterator<Item = PublicKey> + 'a> {
        Box::new(self.devices.iter().map(|id| id.device_key().to_owned()))
    }

    fn certifiers_count(&self) -> usize {
        0
    }
    fn has_certifier(&self, _key: &RadicleUri) -> bool {
        false
    }
    fn certifiers(&self) -> Box<dyn Iterator<Item = RadicleUri>> {
        Box::new(std::iter::empty())
    }
}

#[cfg(test)]
pub mod test;
