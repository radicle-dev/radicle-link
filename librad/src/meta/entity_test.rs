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

use std::str::FromStr;

use futures_await_test::async_test;
use sodiumoxide::crypto::sign::ed25519::Seed;

use super::{
    entity::*,
    user::{User, UserData},
};

use crate::{
    hash::Hash,
    keys::{PublicKey, SecretKey},
    peer::PeerId,
    uri::{Path, Protocol, RadUrn},
};

lazy_static! {
    pub static ref EMPTY_HASH: Hash = Hash::hash(&[]);
    pub static ref EMPTY_URI: RadUrn =
        RadUrn::new(EMPTY_HASH.to_owned(), Protocol::Git, Path::new());
}

const SEED: Seed = Seed([
    20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81, 181,
    134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
]);

fn new_key_from_seed(seed_value: u8) -> SecretKey {
    let mut seed = SEED;
    seed.0[0] = seed_value;
    SecretKey::from_seed(&seed)
}

fn peer_from_key(key: &SecretKey) -> PeerId {
    PeerId::from(key.public())
}

lazy_static! {
    pub static ref K1: SecretKey = new_key_from_seed(1);
    pub static ref K2: SecretKey = new_key_from_seed(2);
    pub static ref K3: SecretKey = new_key_from_seed(3);
    pub static ref K4: SecretKey = new_key_from_seed(4);
    pub static ref K5: SecretKey = new_key_from_seed(5);
}

lazy_static! {
    pub static ref D1: PeerId = peer_from_key(&K1);
    pub static ref D2: PeerId = peer_from_key(&K2);
    pub static ref D3: PeerId = peer_from_key(&K3);
    pub static ref D4: PeerId = peer_from_key(&K4);
    pub static ref D5: PeerId = peer_from_key(&K5);
}

fn peer_key_string(peer: &PeerId) -> PublicKey {
    peer.device_key().clone()
}

lazy_static! {
    pub static ref D1K: PublicKey = peer_key_string(&D1);
    pub static ref D2K: PublicKey = peer_key_string(&D2);
    pub static ref D3K: PublicKey = peer_key_string(&D3);
    pub static ref D4K: PublicKey = peer_key_string(&D4);
    pub static ref D5K: PublicKey = peer_key_string(&D5);
}

struct EmptyResolver {}

#[async_trait]
impl Resolver<User<Draft>> for EmptyResolver {
    async fn resolve(&self, uri: &RadUrn) -> Result<User<Draft>, Error> {
        Err(Error::ResolutionFailed(uri.to_owned()))
    }
    async fn resolve_revision(&self, uri: &RadUrn, revision: u64) -> Result<User<Draft>, Error> {
        Err(Error::RevisionResolutionFailed(uri.to_owned(), revision))
    }
}

#[derive(Debug, Clone, Default)]
pub struct UserHistory {
    pub revisions: Vec<User<Draft>>,
}

impl UserHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn check_user(
        user: &User<Draft>,
    ) -> Result<User<Verified>, HistoryVerificationError> {
        let history = Self {
            revisions: vec![user.clone()],
        };
        history.check().await
    }

    pub async fn check(&self) -> Result<User<Verified>, HistoryVerificationError> {
        let mut previous = None;
        for current in self.revisions.iter() {
            let current = current.clone();
            let current_revision = current.revision();
            let current = current.check_signatures().map_err(|err| {
                HistoryVerificationError::ErrorAtRevision {
                    revision: current_revision,
                    error: err,
                }
            })?;
            let current = current.check_signatures_ownership(self).map_err(|err| {
                HistoryVerificationError::ErrorAtRevision {
                    revision: current_revision,
                    error: err,
                }
            })?;
            let current = current.check_update(&previous).map_err(|err| {
                HistoryVerificationError::UpdateError {
                    revision: current_revision,
                    error: err,
                }
            })?;
            previous = Some(current);
        }
        match previous {
            Some(user) => Ok(user),
            None => Err(HistoryVerificationError::EmptyHistory),
        }
    }
}

impl EntityKeyOwnershipStore for UserHistory {
    fn check_ownership(
        &self,
        _key: &PublicKey,
        _uri: &RadUrn,
        _revision: EntityRevision,
        _time: EntityTimestamp,
    ) -> bool {
        true
    }
}

#[async_trait]
impl Resolver<User<Draft>> for UserHistory {
    async fn resolve(&self, uri: &RadUrn) -> Result<User<Draft>, Error> {
        match self.revisions.last() {
            Some(user) => Ok(user.to_owned()),
            None => Err(Error::ResolutionFailed(uri.to_owned())),
        }
    }
    async fn resolve_revision(&self, uri: &RadUrn, revision: u64) -> Result<User<Draft>, Error> {
        if revision >= 1 && revision <= self.revisions.len() as u64 {
            Ok(self.revisions[revision as usize - 1].clone())
        } else {
            Err(Error::RevisionResolutionFailed(uri.to_owned(), revision))
        }
    }
}

#[test]
fn test_valid_uri() {
    let u1 = RadUrn::new((*EMPTY_HASH).to_owned(), Protocol::Git, Path::new());
    let s = u1.to_string();
    let u2 = RadUrn::from_str(&s).unwrap();
    assert_eq!(u1, u2);
}

fn new_user(
    name: &str,
    revision: u64,
    devices: &[&'static PublicKey],
) -> Result<User<Draft>, Error> {
    let mut data = UserData::new(EntityTimestamp::current_time())
        .set_name(name.to_owned())
        .set_revision(revision);
    for k in devices.iter() {
        data = data.add_key((*k).to_owned());
    }
    data.build()
}

#[test]
fn test_self_signatures() {
    // Keep signing the user while adding keys
    let mut user = new_user("foo", 1, &[&*D1K]).unwrap();

    user.sign_owned(&K1).unwrap();
    let sig1 = user.keys().get(&K1.public()).unwrap().clone().unwrap();

    // Cannot sign with a not-owned key
    assert!(matches!(user.sign_owned(&K2), Err(Error::KeyNotPresent(_))));

    let mut user = user.to_builder().add_key((*D2K).clone()).build().unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K2).unwrap();
    let sig2 = user.keys().get(&K1.public()).unwrap().clone().unwrap();

    assert_ne!(&sig1, &sig2);
}

#[async_test]
async fn test_adding_user_signatures() {
    let user = new_user("foo", 1, &[&*D1K]).unwrap();

    // Check that canonical data changes while adding devices
    let data1 = user.canonical_data().unwrap();
    let user = user.to_builder().add_key((*D2K).clone()).build().unwrap();
    let data2 = user.canonical_data().unwrap();
    let mut user = user.to_builder().add_key((*D3K).clone()).build().unwrap();
    let data3 = user.canonical_data().unwrap();
    assert_ne!(&data1, &data2);
    assert_ne!(&data1, &data3);
    assert_ne!(&data2, &data3);

    // Check that canonical data does not change manipulating signatures
    user.sign_owned(&K1).unwrap();
    let data4 = user.canonical_data().unwrap();
    user.sign_owned(&K2).unwrap();
    let data5 = user.canonical_data().unwrap();
    user.sign_owned(&K3).unwrap();
    let data6 = user.canonical_data().unwrap();

    assert_eq!(&data3, &data4);
    assert_eq!(&data3, &data5);
    assert_eq!(&data3, &data6);

    // Check signatures presence
    assert!(user.keys().get(&D1.device_key()).unwrap().is_some());
    assert!(user.keys().get(&D2.device_key()).unwrap().is_some());
    assert!(user.keys().get(&D3.device_key()).unwrap().is_some());
}

#[test]
fn test_user_verification() {
    // A new user is structurally valid but it is not signed
    let mut user = new_user("foo", 1, &[&*D1K]).unwrap();
    assert!(matches!(
        user.clone().check_signatures(),
        Err(Error::SignatureMissing)
    ));

    // Adding the signature fixes it
    user.sign_owned(&K1).unwrap();
    assert!(matches!(user.clone().check_signatures(), Ok(_)));

    // Changing name (any mutation would do) invalidates the signature
    let user = user
        .to_data()
        .clear_hash()
        .clear_root_hash()
        .set_name("bar".to_owned())
        .build()
        .unwrap();
    assert!(matches!(
        user.clone().check_signatures(),
        Err(Error::SignatureVerificationFailed(_))
    ));

    // Adding the missing signatures does not fix it: D1 signed a previous
    // revision
    let mut user = user
        .to_data()
        .clear_hash()
        .clear_root_hash()
        .add_key((*D2K).clone())
        .add_key((*D3K).clone())
        .set_name("foo".to_owned())
        .build()
        .unwrap();
    user.sign_owned(&K2).unwrap();
    user.sign_owned(&K3).unwrap();
    assert!(matches!(
        user.clone().check_signatures(),
        Err(Error::SignatureVerificationFailed(_))
    ));

    // Cannot sign a project twice with the same key
    assert_eq!(
        user.clone().sign_owned(&K1),
        Err(Error::SignatureAlreadyPresent(K1.public()))
    );

    // Removing the signature and re adding it fixes it
    let mut user = user
        .to_data()
        .clear_hash()
        .remove_key(&K1.public())
        .add_key(K1.public())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    assert!(matches!(user.clone().check_signatures(), Ok(_)));

    // Removing a maintainer invalidates it again
    let user = user
        .to_data()
        .clear_hash()
        .clear_root_hash()
        .remove_key(&*D1K)
        .build()
        .unwrap();
    // TODO(finto): I tried matching on a specific error. There
    // seems to be a race condition between error cases in
    // check_signature.
    assert!(matches!(user.check_signatures(), Err(_)));
}

#[async_test]
async fn test_project_update() {
    // Empty history is invalid
    let mut history = UserHistory::new();
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::EmptyHistory)
    ));

    // History with invalid user is invalid
    let user = new_user("foo", 1, &[&*D1K]).unwrap();
    history.revisions.push(user);

    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::ErrorAtRevision {
            revision: 1,
            error: Error::SignatureMissing,
        })
    ));

    history
        .revisions
        .last_mut()
        .unwrap()
        .sign_owned(&K1)
        .unwrap();

    // History with single valid user is valid
    assert!(matches!(history.check().await, Ok(_)));

    // Having a parent but no parent hash is not ok
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .set_parent(history.revisions.last().unwrap())
        .clear_parent_hash()
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    let some_random_hash = user.to_data().hash.unwrap().to_owned();
    history.revisions.push(user);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::WrongParentHash,
        })
    ));
    history.revisions.pop();

    // Having a parent but wrong parent hash is not ok
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .set_parent(history.revisions.last().unwrap())
        .set_parent_hash(some_random_hash)
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    history.revisions.push(user);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::WrongParentHash,
        })
    ));
    history.revisions.pop();

    // Adding one key is ok
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K2).unwrap();
    history.revisions.push(user);
    assert!(matches!(history.check().await, Ok(_)));

    // Adding two keys starting from one is not ok
    history.revisions.pop();
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .add_key((*D3K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K2).unwrap();
    user.sign_owned(&K3).unwrap();
    history.revisions.push(user);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Adding two keys one by one is ok
    history.revisions.pop();
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K2).unwrap();
    history.revisions.push(user);
    assert!(matches!(history.check().await, Ok(_)));

    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D3K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K2).unwrap();
    user.sign_owned(&K3).unwrap();
    history.revisions.push(user.clone());
    assert!(matches!(history.check().await, Ok(_)));

    // Also check directly signing a user
    let verified_user = history.check().await.unwrap();
    let user2 = new_user("bar", 1, &[&*D4K]).unwrap();
    let mut user2 = user2
        .to_builder()
        .add_certifier(user.urn())
        .build()
        .unwrap();

    assert!(matches!(user2.sign_by_user(&K1, &verified_user), Ok(_)));

    // Changing two devices out of three is not ok
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .remove_key(&*D2K)
        .remove_key(&*D3K)
        .add_key((*D4K).clone())
        .add_key((*D5K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    user.sign_owned(&K4).unwrap();
    user.sign_owned(&K5).unwrap();
    history.revisions.push(user);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Removing two devices out of three is not ok
    history.revisions.pop();
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .remove_key(&*D2K)
        .remove_key(&*D3K)
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign_owned(&K1).unwrap();
    history.revisions.push(user);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoPreviousQuorum,
        })
    ));
}
