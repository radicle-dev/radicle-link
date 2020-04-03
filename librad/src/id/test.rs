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

use futures_await_test::async_test;

use super::{
    entity::*,
    uri::{RadicleUri, EMPTY_URI},
    user::{User, UserData},
};
use crate::{keys::device::Key, peer::PeerId};
use async_trait::async_trait;
use lazy_static::lazy_static;
use sodiumoxide::crypto::sign::ed25519::Seed;

const SEED: Seed = Seed([
    20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81, 181,
    134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
]);
const CREATED_AT: u64 = 1_576_843_598;

fn new_key_from_seed(seed_value: u8) -> Key {
    let mut seed = SEED;
    seed.0[0] = seed_value;
    let created_at = std::time::SystemTime::UNIX_EPOCH
        .checked_add(std::time::Duration::from_secs(CREATED_AT))
        .expect("SystemTime overflow o.O");
    Key::from_seed(&seed, created_at)
}

fn peer_from_key(key: &Key) -> PeerId {
    PeerId::from(key.public())
}

lazy_static! {
    static ref K1: Key = new_key_from_seed(1);
    static ref K2: Key = new_key_from_seed(2);
    static ref K3: Key = new_key_from_seed(3);
    static ref K4: Key = new_key_from_seed(4);
    static ref K5: Key = new_key_from_seed(5);
}

lazy_static! {
    pub static ref D1: PeerId = peer_from_key(&K1);
    pub static ref D2: PeerId = peer_from_key(&K2);
    pub static ref D3: PeerId = peer_from_key(&K3);
    pub static ref D4: PeerId = peer_from_key(&K4);
    pub static ref D5: PeerId = peer_from_key(&K5);
}

fn peer_key_string(peer: &PeerId) -> String {
    peer.device_key().to_bs58()
}

lazy_static! {
    pub static ref D1K: String = peer_key_string(&D1);
    pub static ref D2K: String = peer_key_string(&D2);
    pub static ref D3K: String = peer_key_string(&D3);
    pub static ref D4K: String = peer_key_string(&D4);
    pub static ref D5K: String = peer_key_string(&D5);
}

struct EmptyResolver {}

#[async_trait]
impl Resolver<User> for EmptyResolver {
    async fn resolve(&self, uri: &RadicleUri) -> Result<User, Error> {
        Err(Error::UserNotPresent(uri.to_owned()))
    }
}

static EMPTY_RESOLVER: EmptyResolver = EmptyResolver {};

struct UserHistory {
    pub revisions: Vec<User>,
}

impl UserHistory {
    fn new() -> Self {
        Self { revisions: vec![] }
    }
}

impl RevisionsResolver<User, <std::vec::Vec<User> as IntoIterator>::IntoIter, Vec<User>>
    for UserHistory
{
    fn resolve_revisions(&self, _uri: &RadicleUri) -> std::boxed::Box<Vec<User>> {
        Box::new(self.revisions.iter().rev().cloned().collect())
    }
}

fn new_user(name: &str, revision: u64, devices: &[&'static str]) -> Result<User, Error> {
    let mut data = UserData::default()
        .set_name(name.to_owned())
        .set_revision(revision);
    for s in devices.into_iter() {
        data = data.add_key((*s).to_owned());
    }
    data.build()
}

#[async_test]
async fn test_user_signatures() {
    // Keep signing the user while adding devices
    let mut user = new_user("foo", 1, &[&*D1K]).unwrap();

    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sig1 = user.compute_signature(&K1).unwrap();

    let mut user = user.to_builder().add_key((*D2K).clone()).build().unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let sig2 = user.compute_signature(&K1).unwrap();

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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data4 = user.canonical_data().unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data5 = user.canonical_data().unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    let data6 = user.canonical_data().unwrap();

    assert_eq!(&data3, &data4);
    assert_eq!(&data3, &data5);
    assert_eq!(&data3, &data6);

    // Check signatures collection contents
    assert_eq!(3, user.signatures().len());
    assert!(user.signatures().contains_key(&D1.device_key()));
    assert!(user.signatures().contains_key(&D2.device_key()));
    assert!(user.signatures().contains_key(&D3.device_key()));

    // Check signature verification
    let data = user.canonical_data().unwrap();
    for (k, s) in user.signatures().iter() {
        assert!(s.sig.verify(&data, k));
    }
}

#[async_test]
async fn test_user_verification() {
    // A new user is not valid because no key has signed it
    let mut user = new_user("foo", 1, &[&*D1K]).unwrap();
    assert!(matches!(
        user.check_validity(&EMPTY_RESOLVER).await,
        Err(Error::SignatureMissing)
    ));
    assert!(!user.is_valid(&EMPTY_RESOLVER).await);
    // Adding the signature fixes it
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER).await, Ok(())));
    assert!(user.is_valid(&EMPTY_RESOLVER).await);
    // Adding keys without signatures invalidates it
    let mut user = user
        .to_data()
        .clear_hash()
        .add_key((*D2K).clone())
        .add_key((*D3K).clone())
        .build()
        .unwrap();
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER).await, Err(_)));
    // Adding the missing signatures does not fix it: D1 signed a previous
    // revision
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(
        user.check_validity(&EMPTY_RESOLVER).await,
        Err(Error::SignatureVerificationFailed)
    ));
    // Cannot sign a project twice with the same key
    assert!(matches!(
        user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER).await,
        Err(Error::SignatureAlreadyPresent(_))
    ));
    // Removing the signature and re adding it fixes the project
    let mut user = user
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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(user.is_valid(&EMPTY_RESOLVER).await);
    // Removing a maintainer invalidates it again
    let user = user
        .to_data()
        .clear_hash()
        .remove_key(&*D1K)
        .build()
        .unwrap();
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER).await, Err(_)));
}

#[async_test]
async fn test_project_update() {
    // Empty history is invalid
    let mut history = UserHistory::new();
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Err(HistoryVerificationError::EmptyHistory)
    ));

    // History with invalid user is invalid
    let user = new_user("foo", 1, &[&*D1K]).unwrap();
    history.revisions.push(user);

    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Err(HistoryVerificationError::ErrorAtRevision {
            revision: 1,
            error: Error::SignatureMissing,
        })
    ));

    // History with single valid user is valid
    history
        .revisions
        .last_mut()
        .unwrap()
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Ok(())
    ));

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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Ok(())
    ));

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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Ok(())
    ));
    let mut user = history
        .revisions
        .last()
        .unwrap()
        .to_builder()
        .add_key((*D3K).clone())
        .set_parent(history.revisions.last().unwrap())
        .build()
        .unwrap();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Ok(())
    ));

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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K4, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    user.sign(&K5, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
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
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .await
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history).await,
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoPreviousQuorum,
        })
    ));
}
