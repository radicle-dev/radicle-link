// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, ops::Index as _, str::FromStr};

use it_helpers::{fixed::TestProject, testnet};
use lazy_static::lazy_static;
use librad::{
    collaborative_objects::{
        CollaborativeObject,
        EntryContents,
        History,
        NewObjectSpec,
        TypeName,
        UpdateObjectSpec,
    },
    git::{
        identities,
        tracking,
        types::{Namespace, Reference},
    },
    identities::{delegation::Direct, git::Urn, payload},
    SecretKey,
};
use test_helpers::logging;

macro_rules! assert_state {
    ($object: expr, $expected_state: expr) => {
        let state = realize_state($object);
        assert_eq!(&state, &$expected_state);
    };
}

lazy_static! {
    static ref TYPENAME: TypeName = FromStr::from_str("xyz.radicle.testobject").unwrap();
    static ref KEY_ONE: SecretKey = SecretKey::from_seed([
        100, 107, 14, 43, 237, 25, 113, 215, 236, 197, 160, 60, 169, 174, 81, 58, 143, 74, 42, 201,
        122, 252, 143, 21, 82, 225, 111, 252, 12, 186, 4, 154
    ]);
    static ref KEY_TWO: SecretKey = SecretKey::from_seed([
        153, 72, 253, 68, 81, 29, 234, 67, 15, 241, 138, 59, 180, 75, 76, 113, 103, 189, 174, 200,
        244, 183, 138, 215, 98, 231, 103, 194, 0, 53, 124, 119
    ]);
}

fn config() -> testnet::Config {
    testnet::Config {
        num_peers: nonzero!(2usize),
        min_connected: 2,
        bootstrap: testnet::Bootstrap::from_env(),
    }
}

#[test]
fn collab_object_crud() {
    logging::init();

    let peer1_cache_dir = tempfile::TempDir::new().unwrap();
    let peer1_cache_path = peer1_cache_dir.path().to_path_buf();
    let peer2_cache_dir = tempfile::TempDir::new().unwrap();
    let peer2_cache_path = peer2_cache_dir.path().to_path_buf();

    let net = testnet::run(config()).unwrap();
    net.enter(async {
        let peer1 = net.peers().index(0);
        let peer2 = net.peers().index(1);

        let proj = peer1
            .using_storage(TestProject::create)
            .await
            .unwrap()
            .unwrap();

        let local_id_1 = {
            let urn = proj.project.urn();
            let peer2_id = peer2.peer_id();
            peer1
                .using_storage(move |storage| {
                    let id = identities::local::load(storage, urn.clone())
                        .expect("local ID should have been created by TestProject::create")
                        .unwrap();
                    id.link(storage, &urn).unwrap();
                    assert!(tracking::track(
                        storage,
                        &urn,
                        Some(peer2_id),
                        tracking::Config::default(),
                        tracking::policy::Track::Any
                    )
                    .unwrap()
                    .is_ok());
                    id
                })
                .await
                .unwrap()
        };

        let local_id_2 = peer2
            .using_storage(|storage| {
                let peer_id = storage.peer_id();
                let person = identities::person::create(
                    storage,
                    payload::Person {
                        name: "peer2".into(),
                    },
                    Direct::new(*peer_id.as_public_key()),
                )
                .unwrap();
                identities::local::load(storage, person.urn())
                    .unwrap()
                    .unwrap()
            })
            .await
            .unwrap();

        // Add peer2 as a maintainer
        peer1
            .using_storage({
                let urn = proj.project.urn();
                let owner = proj.owner.clone();
                let peer_id = peer2.peer_id();
                let key = *peer_id.as_public_key();
                move |storage| -> Result<(), anyhow::Error> {
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        librad::identities::delegation::Indirect::try_from_iter(
                            vec![either::Either::Left(key), either::Either::Right(owner)]
                                .into_iter(),
                        )
                        .unwrap(),
                    )?;
                    identities::project::verify(storage, &urn)?;
                    Ok(())
                }
            })
            .await
            .unwrap()
            .unwrap();

        proj.pull(peer1, peer2).await.unwrap();

        peer2
            .using_storage({
                let urn = proj.project.urn();
                let peer_id = peer1.peer_id();
                let rad =
                    Urn::try_from(Reference::rad_id(Namespace::from(&urn)).with_remote(peer_id))
                        .unwrap();
                move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
                    let project = identities::project::get(&storage, &rad)?.unwrap();
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        project.delegations().clone(),
                    )?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?)
                }
            })
            .await
            .unwrap()
            .unwrap();

        let object = {
            let urn = proj.project.urn();
            let local_id_1 = local_id_1.clone();
            let cache_path = peer1_cache_path.clone();
            // Create a collaborative object
            peer1
                .using_storage(move |storage| {
                    let collabs = storage.collaborative_objects(Some(cache_path));
                    collabs
                        .create(
                            &local_id_1,
                            &urn,
                            NewObjectSpec {
                                history: init_history(),
                                message: Some("first change".to_string()),
                                typename: TYPENAME.clone(),
                            },
                        )
                        .unwrap()
                })
                .await
                .unwrap()
        };

        assert_state!(
            &object,
            serde_json::json!({
                "items": []
            })
        );

        // Update the object
        {
            let urn = proj.project.urn();
            let id = *(object.id());
            let history = object.history().clone();
            let cache_path = peer1_cache_path.clone();
            peer1
                .using_storage(move |storage| {
                    let collabs = storage.collaborative_objects(Some(cache_path));
                    collabs
                        .update(
                            &local_id_1,
                            &urn,
                            UpdateObjectSpec {
                                typename: TYPENAME.clone(),
                                message: Some("add first item".to_string()),
                                object_id: id,
                                changes: add_item(&history, "peer 1 item"),
                            },
                        )
                        .unwrap();
                })
                .await
                .unwrap();
        }

        {
            let urn = proj.project.urn();
            let id = *object.id();
            let cache_path = peer1_cache_path.clone();
            let object = peer1
                .using_storage(move |s| {
                    s.collaborative_objects(Some(cache_path))
                        .retrieve(&urn, &TYPENAME, &id)
                        .unwrap()
                })
                .await
                .unwrap()
                .unwrap();
            assert_state!(
                &object,
                serde_json::json!({
                    "items": ["peer 1 item"],
                })
            );
        }

        proj.pull(peer1, peer2).await.unwrap();

        // Check that peer 2
        // sees the same object state
        let peer2_object = {
            let urn = proj.project.urn();
            let id = *object.id();
            peer2
                .using_storage(move |s| {
                    s.collaborative_objects(Some(peer2_cache_dir.path().to_path_buf()))
                        .retrieve(&urn, &TYPENAME, &id)
                        .unwrap()
                })
                .await
                .unwrap()
                .unwrap()
        };
        assert_state!(
            &peer2_object,
            serde_json::json!({
                "items": ["peer 1 item"],
            })
        );

        // Update in peer 2
        let updated_peer_2_object = {
            let urn = proj.project.urn();
            let id = *object.id();
            let history = peer2_object.history().clone();
            let cache_path = peer2_cache_path.clone();
            peer2
                .using_storage({
                    let local_id_2 = local_id_2.clone();
                    move |storage| {
                        storage
                            .collaborative_objects(Some(cache_path))
                            .update(
                                &local_id_2,
                                &urn,
                                UpdateObjectSpec {
                                    typename: TYPENAME.clone(),
                                    object_id: id,
                                    changes: add_item(&history, "peer 2 item"),
                                    message: Some("peer 2 change".to_string()),
                                },
                            )
                            .unwrap();
                        let result = storage
                            .collaborative_objects(None)
                            .retrieve(&urn, &TYPENAME, &id)
                            .unwrap()
                            .unwrap();
                        result
                    }
                })
                .await
                .unwrap()
        };

        assert_state!(
            &updated_peer_2_object,
            serde_json::json!({
                "items": ["peer 1 item", "peer 2 item"],
            })
        );

        proj.pull(peer2, peer1).await.unwrap();

        let peer1_after_pull = {
            let urn = proj.project.urn();
            let id = *object.id();
            peer1
                .using_storage(move |storage| {
                    let result = storage
                        .collaborative_objects(Some(peer1_cache_dir.path().to_path_buf()))
                        .retrieve(&urn, &TYPENAME, &id)
                        .unwrap()
                        .unwrap();
                    result
                })
                .await
                .unwrap()
        };

        assert_state!(
            &peer1_after_pull,
            serde_json::json!({
                "items": ["peer 1 item", "peer 2 item"],
            })
        );

        let peer1_all_objects = peer1
            .using_storage({
                let urn = proj.project.urn();
                let cache_path = peer1_cache_path.clone();
                move |storage| {
                    storage
                        .collaborative_objects(Some(cache_path))
                        .list(&urn, &TYPENAME)
                        .unwrap()
                }
            })
            .await
            .unwrap();

        assert_eq!(peer1_all_objects.len(), 1);
    })
}

fn init_history() -> EntryContents {
    let mut backend = automerge::Backend::new();
    let mut frontend = automerge::Frontend::new();
    let (_, change) = frontend
        .change::<_, _, automerge::InvalidChangeRequest>(None, |d| {
            d.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("items"),
                automerge::Value::List(Vec::new()),
            ))?;
            Ok(())
        })
        .unwrap();
    backend.apply_local_change(change.unwrap()).unwrap();
    let bytes = backend
        .get_changes(&[])
        .iter()
        .flat_map(|c| c.raw_bytes().to_vec())
        .collect();
    EntryContents::Automerge(bytes)
}

fn add_item<I: Into<automerge::Value>>(history: &History, item: I) -> EntryContents {
    let (mut frontend, mut backend) = evaluate_history(history);
    let (_, change) = frontend
        .change::<_, _, automerge::InvalidChangeRequest>(None, |d| {
            let num_items = match d.value_at_path(&automerge::Path::root().key("items")) {
                Some(automerge::Value::List(items)) => items.len() as u32,
                _ => panic!("no items in doc"),
            };
            d.add_change(automerge::LocalChange::insert(
                automerge::Path::root().key("items").index(num_items),
                item.into(),
            ))
            .unwrap();
            Ok(())
        })
        .unwrap();
    let (_, change) = backend.apply_local_change(change.unwrap()).unwrap();
    EntryContents::Automerge(change.raw_bytes().to_vec())
}

//fn assert_state(object: &CollaborativeObject, expected_state:
// serde_json::Value) { let state = realize_state(object);
//assert_eq!(&state, &expected_state);
//}
fn realize_state(object: &CollaborativeObject) -> serde_json::Value {
    let (mut frontend, _) = evaluate_history(object.history());
    frontend.state().to_json()
}

fn evaluate_history(history: &History) -> (automerge::Frontend, automerge::Backend) {
    let backend = history.traverse(
        automerge::Backend::new(),
        |mut backend, change| match change.contents() {
            librad::collaborative_objects::EntryContents::Automerge(bytes) => {
                let change = automerge::Change::from_bytes(bytes.clone()).unwrap();
                backend.apply_changes(vec![change]).unwrap();
                std::ops::ControlFlow::Continue(backend)
            },
        },
    );
    let mut frontend = automerge::Frontend::new();
    let patch = backend.get_patch().unwrap();
    frontend.apply_patch(patch).unwrap();
    (frontend, backend)
}
