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

use super::*;

use std::str::FromStr;

use futures_await_test::async_test;

use crate::{
    hash::Hash,
    meta::{entity::Draft, Project, User},
    test::{ConstResolver, WithTmpDir},
    uri::{self, RadUrn},
};

type TmpStorage = WithTmpDir<Storage>;

fn storage() -> TmpStorage {
    WithTmpDir::new(|path| {
        let paths = Paths::from_root(path)?;
        Storage::init(&paths, SecretKey::new())
    })
    .unwrap()
}

fn urn_from_idref(refname: &str) -> Option<RadUrn> {
    refname
        .strip_suffix("/refs/rad/id")
        .and_then(|namespace_root| {
            namespace_root
                .split('/')
                .next_back()
                .and_then(|namespace| Hash::from_str(namespace).ok())
                .map(|hash| RadUrn::new(hash, uri::Protocol::Git, uri::Path::empty()))
        })
}

#[test]
fn test_tracking_read_after_write() {
    let store = storage();
    let urn = RadUrn {
        id: Hash::hash(b"lala"),
        proto: uri::Protocol::Git,
        path: uri::Path::empty(),
    };
    let peer = PeerId::from(SecretKey::new());

    store.track(&urn, &peer).unwrap();
    let tracked = store.tracked(&urn).unwrap().next();
    assert_eq!(tracked, Some(peer))
}

#[test]
fn test_idempotent_tracking() {
    let store = storage();
    let urn = RadUrn {
        id: Hash::hash(b"lala"),
        proto: uri::Protocol::Git,
        path: uri::Path::empty(),
    };
    let peer = PeerId::from(SecretKey::new());

    store.track(&urn, &peer).unwrap();

    // Attempting to track again does not fail
    store.track(&urn, &peer).unwrap();

    let tracked = store.tracked(&urn).unwrap().next();
    assert_eq!(tracked, Some(peer))
}

#[test]
fn test_untrack() {
    let store = storage();
    let urn = RadUrn {
        id: Hash::hash(b"lala"),
        proto: uri::Protocol::Git,
        path: uri::Path::empty(),
    };
    let peer = PeerId::from(SecretKey::new());

    store.track(&urn, &peer).unwrap();
    store.untrack(&urn, &peer).unwrap();

    assert!(store.tracked(&urn).unwrap().next().is_none())
}

#[async_test]
async fn test_all_metadata_heads() {
    let store = storage();

    // Create signed and verified user
    let mut user = User::<Draft>::create("user".to_owned(), store.key.public()).unwrap();
    user.sign_owned(&store.key).unwrap();
    let user_resolver = ConstResolver::new(user.clone());
    let verified_user = user
        .clone()
        .check_history_status(&user_resolver, &user_resolver)
        .await
        .unwrap();

    // Create and sign two projects
    let mut project_foo = Project::<Draft>::create("foo".to_owned(), user.urn()).unwrap();
    let mut project_bar = Project::<Draft>::create("bar".to_owned(), user.urn()).unwrap();
    project_foo
        .sign_by_user(&store.key, &verified_user)
        .unwrap();
    project_bar
        .sign_by_user(&store.key, &verified_user)
        .unwrap();

    // Store the three entities in their respective namespaces
    println!("poop");
    store.create_repo(&user).unwrap();
    println!("piep");
    store.create_repo(&project_foo).unwrap();
    store.create_repo(&project_bar).unwrap();

    let mut ids = HashSet::new();
    let mut urns = HashMap::new();
    ids.insert(user.hash());
    ids.insert(project_foo.hash());
    ids.insert(project_bar.hash());

    // Iterate ove all namespaces
    let all_metadata_heads =
        References::from_globs(&store.backend, &["refs/namespaces/*/refs/rad/id"])
            .unwrap()
            .peeled()
            .filter_map(|(refname, oid)| {
                urn_from_idref(&refname).and_then(|urn| {
                    store
                        .backend
                        .find_commit(oid)
                        .map(|commit| (urn, commit))
                        .ok()
                })
            });
    for (urn, commit) in all_metadata_heads {
        // Check that we found one of our IDs
        let id = &urn.id;
        assert!(ids.contains(id));

        // Check that we can use the URN to find the same commit
        let commit_from_urn = Reference::rad_id(urn.id.clone())
            .find(&store.backend)
            .unwrap()
            .target()
            .unwrap();
        assert_eq!(commit_from_urn, commit.id());

        // Bookkeeping for more tests
        ids.remove(id);
        urns.insert(id.to_owned(), urn);
    }

    // Check that we found all the entities that we saved
    assert!(ids.is_empty());

    // Pull out user blob and deserialize
    assert_eq!(user, store.metadata(&user.urn()).unwrap());
    let generic_user = store.some_metadata(&user.urn()).unwrap();
    assert_eq!(generic_user.kind(), user.kind());
    assert_eq!(generic_user.hash(), user.hash());

    // Pull out foo blob and deserialize
    assert_eq!(project_foo, store.metadata(&project_foo.urn()).unwrap());
    let generic_foo = store.some_metadata(&project_foo.urn()).unwrap();
    assert_eq!(generic_foo.kind(), project_foo.kind());
    assert_eq!(generic_foo.hash(), project_foo.hash());

    // Check user commit history length
    let user_history = {
        let rad_id = Reference::rad_id(user.urn().id);

        let mut revwalk = store.backend.revwalk().unwrap();
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL).unwrap();
        revwalk.simplify_first_parent().unwrap();
        revwalk.push_ref(&rad_id.to_string()).unwrap();

        revwalk.collect::<Vec<_>>()
    };
    assert_eq!(user_history.len(), 1);
}
