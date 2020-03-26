use crate::{
    keys::device::{Key, PublicKey, Signature},
    peer::PeerId,
};
use multihash::{Multihash, Sha2_256};
use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
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

    #[fail(display = "Invalid data")]
    InvalidData(String),

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

#[derive(Debug, Fail)]
pub enum UpdateVerificationError {
    #[fail(display = "Non monotonic revision")]
    NonMonotonicRevision,

    #[fail(display = "Update without previous quorum")]
    NoPreviousQuorum,

    #[fail(display = "Update without current quorum")]
    NoCurrentQuorum,
}

#[derive(Debug, Fail)]
pub enum HistoryVerificationError {
    #[fail(display = "Empty history")]
    EmptyHistory,

    #[fail(display = "Error at revsion")]
    ErrorAtRevision { revision: u64, error: Error },

    #[fail(display = "Update error")]
    UpdateError {
        revision: u64,
        error: UpdateVerificationError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        let bytes = bs58::decode(s.as_bytes())
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_vec()
            .map_err(|_| Error::InvalidHex(s.to_owned()))?;
        let hash = Multihash::from_bytes(bytes).map_err(|_| Error::InvalidHash(s.to_owned()))?;
        Ok(Self { hash })
    }
}

lazy_static! {
    static ref EMPTY_HASH: Multihash = Sha2_256::digest(&[]);
    static ref EMPTY_URI: RadicleUri = RadicleUri::new(EMPTY_HASH.to_owned());
}

impl ToString for RadicleUri {
    fn to_string(&self) -> String {
        bs58::encode(&self.hash)
            .with_alphabet(bs58::alphabet::BITCOIN)
            .into_string()
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

pub trait RevisionsResolver<E, I, II>
where
    E: Entity,
    I: Iterator<Item = E>,
    II: IntoIterator<Item = E, IntoIter = I> + Sized,
{
    fn resolve_revisions(&self, uri: &RadicleUri) -> Box<II>;
}

pub trait Entity: Sized {
    fn name(&self) -> &str;
    fn revision(&self) -> u64;

    fn from_data(data: &data::EntityData) -> Result<Self, Error>;
    fn to_data(&self) -> data::EntityData;

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

    fn canonical_data(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_data().canonical_data()?)
    }
    fn to_json_writer<W>(&self, writer: W) -> Result<(), Error>
    where
        W: std::io::Write,
    {
        self.to_data().to_json_writer(writer)?;
        Ok(())
    }
    fn to_json_string(&self) -> Result<String, Error> {
        Ok(self.to_data().to_json_string()?)
    }

    fn from_json_reader<R>(r: R) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        Ok(Self::from_data(&data::EntityData::from_json_reader(r)?)?)
    }
    fn from_json_str(s: &str) -> Result<Self, Error> {
        Ok(Self::from_data(&data::EntityData::from_json_str(s)?)?)
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
        if self.signatures().contains_key(&public_key) {
            return Err(Error::SignatureAlreadyPresent(public_key.to_owned()));
        }
        self.check_key(&public_key, by, resolver)?;
        let signature = EntitySignature {
            by: by.to_owned(),
            sig: self.compute_signature(key)?,
        };
        self.signatures_mut().insert(public_key, signature);
        Ok(())
    }

    fn check_signature(
        &self,
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

    fn check_validity(&self, resolver: &impl Resolver<User>) -> Result<(), Error> {
        let mut keys = HashSet::<PublicKey>::from_iter(self.keys());
        let mut users = HashSet::<RadicleUri>::from_iter(self.certifiers());

        for (k, s) in self.signatures() {
            self.check_signature(k, &s.by, &s.sig, resolver)?;
            match &s.by {
                Signatory::OwnedKey => {
                    keys.remove(k);
                },
                Signatory::User(user) => {
                    users.remove(&user);
                },
            }
        }
        if keys.len() > 0 || users.len() > 0 {
            Err(Error::SignatureMissing)
        } else {
            Ok(())
        }
    }

    fn is_valid(&self, resolver: &impl Resolver<User>) -> bool {
        self.check_validity(resolver).is_ok()
    }

    fn check_update(&self, previous: &Self) -> Result<(), UpdateVerificationError> {
        if self.revision() <= previous.revision() {
            return Err(UpdateVerificationError::NonMonotonicRevision);
        }

        let retained_keys = self.keys().filter(|k| previous.has_key(k)).count();
        let total_keys = self.keys_count();
        let added_keys = total_keys - retained_keys;
        let removed_keys = previous.keys_count() - retained_keys;
        let quorum_keys = total_keys / 2;

        if added_keys > quorum_keys {
            return Err(UpdateVerificationError::NoCurrentQuorum);
        } else if removed_keys > quorum_keys {
            return Err(UpdateVerificationError::NoPreviousQuorum);
        }

        let retained_certifiers = self
            .certifiers()
            .filter(|c| previous.has_certifier(c))
            .count();
        let total_certifiers = self.certifiers_count();
        let added_certifiers = total_certifiers - retained_certifiers;
        let removed_certifiers = previous.certifiers_count() - retained_certifiers;
        let quorum_certifiers = total_certifiers / 2;

        if added_certifiers > quorum_certifiers {
            return Err(UpdateVerificationError::NoCurrentQuorum);
        } else if removed_certifiers > quorum_certifiers {
            return Err(UpdateVerificationError::NoPreviousQuorum);
        }

        Ok(())
    }

    fn check_history<E, I, II>(
        uri: &RadicleUri,
        resolver: &impl Resolver<User>,
        revisions_resolver: &impl RevisionsResolver<E, I, II>,
    ) -> Result<(), HistoryVerificationError>
    where
        E: Entity,
        I: Iterator<Item = E>,
        II: IntoIterator<Item = E, IntoIter = I> + Sized,
    {
        let mut revisions = revisions_resolver.resolve_revisions(uri).into_iter();

        let current = revisions.next();
        let mut current = match current {
            None => {
                return Err(HistoryVerificationError::EmptyHistory);
            },
            Some(entity) => entity,
        };

        let revision = current.revision();
        current
            .check_validity(resolver)
            .map_err(|error| HistoryVerificationError::ErrorAtRevision { revision, error })?;

        for previous in revisions {
            let revision = current.revision();
            previous
                .check_validity(resolver)
                .map_err(|error| HistoryVerificationError::ErrorAtRevision { revision, error })?;
            current
                .check_update(&previous)
                .map_err(|error| HistoryVerificationError::UpdateError { revision, error })?;
            current = previous;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
    pub revision: u64,

    pub devices: HashSet<PeerId>,

    pub uri: RadicleUri,
    pub signatures: HashMap<PublicKey, EntitySignature>,
}

impl User {
    pub fn new(name: &str, devices: impl Iterator<Item = &'static PeerId>) -> Self {
        Self {
            name: name.to_owned(),
            revision: 1,
            devices: HashSet::from_iter(devices.cloned()),
            uri: EMPTY_URI.to_owned(),
            signatures: HashMap::new(),
        }
    }
}

impl Entity for User {
    fn name(&self) -> &str {
        &self.name
    }
    fn revision(&self) -> u64 {
        self.revision
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

    fn from_data(data: &data::EntityData) -> Result<Self, Error> {
        if let None = data.name {
            return Err(Error::InvalidData("Missing name".to_owned()));
        }
        if let None = data.uri {
            return Err(Error::InvalidData("Missing URI".to_owned()));
        }
        if let None = data.revision {
            return Err(Error::InvalidData("Missing revision".to_owned()));
        }
        if data.keys.len() == 0 {
            return Err(Error::InvalidData("Missing keys".to_owned()));
        }

        let mut devices = HashSet::new();
        for k in data.keys.iter() {
            let d = PeerId::from(PublicKey::from_bs58(k).ok_or(Error::InvalidData(k.to_owned()))?);
            devices.insert(d);
        }

        let mut signatures = HashMap::new();
        if let Some(s) = &data.signatures {
            for (k, sig) in s.iter() {
                let key = PublicKey::from_bs58(k).ok_or(Error::InvalidData(k.to_owned()))?;
                let signature = EntitySignature {
                    by: match &sig.user {
                        Some(uri) => Signatory::User(RadicleUri::from_str(&uri)?),
                        None => Signatory::OwnedKey,
                    },
                    sig: Signature::from_bs58(k).ok_or(Error::InvalidData(k.to_owned()))?,
                };
                signatures.insert(key, signature);
            }
        }

        Ok(Self {
            name: data
                .name
                .as_deref()
                .ok_or(Error::InvalidData("Missing name".to_owned()))?
                .to_owned(),
            revision: data.revision.unwrap().to_owned(),
            devices,
            uri: RadicleUri::from_str(
                &data
                    .uri
                    .as_deref()
                    .ok_or(Error::InvalidData("Missing name".to_owned()))?,
            )?,
            signatures,
        })
    }

    fn to_data(&self) -> data::EntityData {
        let mut signatures = HashMap::new();
        for (k, s) in self.signatures() {
            signatures.insert(
                k.to_bs58(),
                data::EntitySignatureData {
                    user: match &s.by {
                        Signatory::User(uri) => Some(uri.to_string()),
                        Signatory::OwnedKey => None,
                    },
                    sig: s.sig.to_bs58(),
                },
            );
        }

        let keys = HashSet::from_iter(self.keys().map(|k| k.to_bs58()));
        let owners = HashSet::from_iter(self.certifiers().map(|c| c.to_string()));

        data::EntityData {
            name: Some(self.name.to_owned()),
            revision: Some(self.revision()),
            uri: Some(self.uri().to_string()),
            signatures: Some(signatures),
            keys,
            owners,
        }
    }
}

#[cfg(test)]
pub mod test;
