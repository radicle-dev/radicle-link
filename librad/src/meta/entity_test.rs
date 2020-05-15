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
use async_trait::async_trait;
use futures_await_test::async_test;
use lazy_static::lazy_static;
use sodiumoxide::crypto::sign::ed25519::Seed;
use std::str::FromStr;

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
    static ref K1: SecretKey = new_key_from_seed(1);
    static ref K2: SecretKey = new_key_from_seed(2);
    static ref K3: SecretKey = new_key_from_seed(3);
    static ref K4: SecretKey = new_key_from_seed(4);
    static ref K5: SecretKey = new_key_from_seed(5);
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
impl Resolver<User<Unknown>> for EmptyResolver {
    async fn resolve(&self, uri: &RadUrn) -> Result<User<Unknown>, Error> {
        Err(Error::ResolutionFailed(uri.to_owned()))
    }
    async fn resolve_revision(&self, uri: &RadUrn, revision: u64) -> Result<User<Unknown>, Error> {
        Err(Error::RevisionResolutionFailed(uri.to_owned(), revision))
    }
}

static EMPTY_RESOLVER: EmptyResolver = EmptyResolver {};

#[derive(Debug, Clone)]
struct UserHistory {
    pub revisions: Vec<User<Signed>>,
}

impl UserHistory {
    fn new() -> Self {
        Self { revisions: vec![] }
    }

    async fn check(&self) -> Result<User<Verified>, HistoryVerificationError> {
        let history = self.clone();
        match self.revisions.last().cloned() {
            Some(user) => user.check_history_status(&history, &EMPTY_RESOLVER).await,
            None => Err(HistoryVerificationError::EmptyHistory),
        }
    }
}

#[async_trait]
impl Resolver<User<Signed>> for UserHistory {
    async fn resolve(&self, uri: &RadUrn) -> Result<User<Signed>, Error> {
        match self.revisions.last() {
            Some(user) => Ok(user.to_owned()),
            None => Err(Error::ResolutionFailed(uri.to_owned())),
        }
    }
    async fn resolve_revision(&self, uri: &RadUrn, revision: u64) -> Result<User<Signed>, Error> {
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

fn new_user(name: &str, revision: u64, devices: &[&'static PublicKey]) -> Result<User<Unknown>, Error> {
    let mut data = UserData::default()
        .set_name(name.to_owned())
        .set_revision(revision);
    for k in devices.iter() {
        data = data.add_key((*k).to_owned());
    }
    data.build()
}

#[async_test]
async fn test_user_signatures() {
    // Keep signing the user while adding devices
    let user = new_user("foo", 1, &[&*D1K]).unwrap();

    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sig1 = sign1.compute_signature(&K1).unwrap();

    let user = sign1.to_builder().add_key((*D2K).clone()).build().unwrap();
    let sign2 = user
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sig2 = sign2.compute_signature(&K1).unwrap();

    assert_ne!(&sig1, &sig2);
}

#[async_test]
async fn test_adding_user_signatures() {
    let user = new_user("foo", 1, &[&*D1K]).unwrap();

    // Check that canonical data changes while adding devices
    let data1 = user.canonical_data().unwrap();
    let user = user.to_builder().add_key((*D2K).clone()).build().unwrap();
    let data2 = user.canonical_data().unwrap();
    let user = user.to_builder().add_key((*D3K).clone()).build().unwrap();
    let data3 = user.canonical_data().unwrap();
    assert_ne!(&data1, &data2);
    assert_ne!(&data1, &data3);
    assert_ne!(&data2, &data3);

    // Check that canonical data does not change manipulating signatures
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data4 = sign1.canonical_data().unwrap();
    let sign2 = sign1
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data5 = sign2.canonical_data().unwrap();
    let sign3 = sign2
        .sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data6 = sign3.canonical_data().unwrap();

    assert_eq!(&data3, &data4);
    assert_eq!(&data3, &data5);
    assert_eq!(&data3, &data6);

    // Check signatures collection contents
    assert_eq!(3, sign3.signatures().len());
    assert!(sign3.signatures().contains_key(&D1.device_key()));
    assert!(sign3.signatures().contains_key(&D2.device_key()));
    assert!(sign3.signatures().contains_key(&D3.device_key()));

    // Check signature verification
    let data = sign3.canonical_data().unwrap();
    for (k, s) in sign3.signatures().iter() {
        assert!(s.sig.verify(&data, k));
    }
}

#[async_test]
async fn test_user_verification() {
    // A new user is structurally valid but it is not signed
    let user = new_user("foo", 1, &[&*D1K]).unwrap();
    assert!(matches!(
        user.clone().check_signatures(&EMPTY_RESOLVER).await,
        Err(Error::SignatureMissing)
    ));

    // Adding the signature fixes it
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(
        sign1.clone().check_signatures(&EMPTY_RESOLVER).await,
        Ok(_)
    ));

    // Adding keys (any mutation would do) invalidates the signature
    let user = sign1
        .to_data()
        .clear_hash()
        .clear_root_hash()
        .add_key((*D2K).clone())
        .add_key((*D3K).clone())
        .build()
        .unwrap();
    assert!(matches!(
        user.clone().check_signatures(&EMPTY_RESOLVER).await,
        Err(Error::SignatureVerificationFailed)
    ));

    // Adding the missing signatures does not fix it: D1 signed a previous
    // revision
    let sign2 = user
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign3 = sign2
        .sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(
        sign3.clone().check_signatures(&EMPTY_RESOLVER).await,
        Err(Error::SignatureVerificationFailed)
    ));

    // Cannot sign a project twice with the same key
    assert!(matches!(
        sign3
            .clone()
            .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
            .await,
        Err(Error::SignatureAlreadyPresent(_))
    ));

    // Removing the signature and re adding it fixes it
    let user = sign3
        .to_data()
        .clear_hash()
        .map(|mut u| {
            if let Some(s) = &mut u.signatures {
                s.remove(&*D1K);
            }
            u
        })
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(
        sign1.clone().check_signatures(&EMPTY_RESOLVER).await,
        Ok(_)
    ));

    // Removing a maintainer invalidates it again
    let user = sign1
        .to_data()
        .clear_hash()
        .clear_root_hash()
        .remove_key(&*D1K)
        .build()
        .unwrap();
    assert!(matches!(
        user.check_signatures(&EMPTY_RESOLVER).await,
        Err(_)
    ));
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
    /* Can't actually do this without signing the user
    let user = new_user("foo", 1, &[&*D1K]).unwrap();
    history.revisions.push(user);

    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::ErrorAtRevision {
            revision: 1,
            error: Error::SignatureMissing,
        })
    ));
    */

    let user = new_user("foo", 1, &[&*D1K])
        .unwrap()
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);

    // History with single valid user is valid
    assert!(matches!(history.check().await, Ok(_)));

    // Having a parent but no parent hash is not ok
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .set_parent(history.revisions.last().unwrap())
        .clear_parent_hash()
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let some_random_hash = sign1.to_data().hash.unwrap().to_owned();
    history.revisions.push(sign1);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::WrongParentHash,
        })
    ));
    history.revisions.pop();

    // Having a parent but wrong parent hash is not ok
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .set_parent(history.revisions.last().unwrap())
        .set_parent_hash(some_random_hash)
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign1);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::WrongParentHash,
        })
    ));
    history.revisions.pop();

    // Adding one key is ok
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign2 = sign1
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign2);
    assert!(matches!(history.check().await, Ok(_)));

    // Adding two keys starting from one is not ok
    history.revisions.pop();
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .add_key((*D3K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign2 = sign1
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign3 = sign2
        .sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign3);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Adding two keys one by one is ok
    history.revisions.pop();
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D2K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign2 = sign1
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign2);
    assert!(matches!(history.check().await, Ok(_)));

    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D3K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign2 = sign1
        .sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign3 = sign2
        .sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign3);
    assert!(matches!(history.check().await, Ok(_)));

    // Changing two devices out of three is not ok
    let user = history
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
    let sign1 = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign4 = sign1
        .sign(&K4, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sign5 = sign4
        .sign(&K5, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(sign5);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Removing two devices out of three is not ok
    history.revisions.pop();
    let user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .remove_key(&*D2K)
        .remove_key(&*D3K)
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    let signed = user
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(signed);
    assert!(matches!(
        history.check().await,
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoPreviousQuorum,
        })
    ));
}
