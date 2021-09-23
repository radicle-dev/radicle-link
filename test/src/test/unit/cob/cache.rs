// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use cob::{
    internals::{Cache, FileSystemCache, ThinChangeGraph},
    History,
    ObjectId,
    Schema,
    TypeName,
};
use lazy_static::lazy_static;
use rand::{seq::IteratorRandom, Rng};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
    env::temp_dir,
    rc::Rc,
};

use link_identities::git::Urn;

lazy_static! {
    static ref SCHEMA: Schema = Schema::try_from(&serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" }
        },
        "required": ["name"]
    }))
    .unwrap();
}

#[test]
fn smash_the_cache() {
    // On each thread create a cache pointing at the same directory. Each
    // thread stores a different object state under the same object
    // ID. Now repeatedly store and load objects from the object on
    // each thread. Each thread will see different states on each load but
    // they should never see a mixture of states.
    let states: [&str; 3] = ["one", "two", "three"];
    let graph_states: Vec<ObjectState> = states.iter().map(|s| object_state(s)).collect();

    let cache_dirname: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(30)
        .map(char::from)
        .collect();
    let cache_dir = temp_dir().join(cache_dirname);

    let the_oid: ObjectId = random_oid().into();

    let mut threads = Vec::new();
    // Writer threads
    for _ in 0..3 {
        let cache_dir = cache_dir.clone();
        let graph_states = graph_states.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..100 {
                let mut cache = FileSystemCache::open(cache_dir.as_path()).unwrap();
                let thin_graph = Rc::new(RefCell::new(
                    graph_states
                        .iter()
                        .choose(&mut rand::thread_rng())
                        .unwrap()
                        .into(),
                ));
                cache.put(the_oid, thin_graph.clone()).unwrap();
            }
        }));
    }

    // reader threads
    for _ in 0..10 {
        let cache_dir = cache_dir.clone();
        let graph_states = graph_states.clone();
        let mut successful_loads = 0;
        threads.push(std::thread::spawn(move || {
            for _ in 0..100 {
                let mut cache = FileSystemCache::open(cache_dir.as_path()).unwrap();
                for state in &graph_states {
                    if let Some(g) = cache.load(the_oid, &state.refs).unwrap() {
                        let objstate: ObjectState = g.into();
                        successful_loads += 1;
                        assert!(graph_states.contains(&objstate));
                    }
                }
            }
            assert!(successful_loads > 0);
        }));
    }

    for thread in threads {
        thread.join().unwrap();
    }
}

/// The same as a ThinChangeGraph, just without the ValidatedAutomerge
/// (which is not`Send`) so that it can be easily sent between threads
/// for the purposes of comparison
#[derive(PartialEq, Clone)]
struct ObjectState {
    raw_history: Vec<u8>,
    refs: BTreeSet<git2::Oid>,
    schema_commit: git2::Oid,
    schema: Schema,
    state: serde_json::Value,
    typename: TypeName,
    object_id: ObjectId,
    authorizing_identity_urn: Urn,
}

impl From<Rc<RefCell<ThinChangeGraph>>> for ObjectState {
    fn from(g: Rc<RefCell<ThinChangeGraph>>) -> Self {
        ObjectState {
            raw_history: g.borrow().history().as_bytes().to_vec(),
            refs: g.borrow().refs().clone(),
            schema_commit: g.borrow().schema_commit(),
            schema: g.borrow().schema().clone(),
            state: g.borrow().state().clone(),
            typename: g.borrow().typename().clone(),
            object_id: g.borrow().object_id(),
            authorizing_identity_urn: g.borrow().authorizing_identity_urn().clone(),
        }
    }
}

impl From<&ObjectState> for ThinChangeGraph {
    fn from(o: &ObjectState) -> Self {
        ThinChangeGraph {
            validated_history: None,
            history: History::Automerge(o.raw_history.clone()),
            refs: o.refs.clone(),
            schema_commit: o.schema_commit,
            schema: o.schema.clone(),
            state: o.state.clone(),
            typename: o.typename.clone(),
            object_id: o.object_id,
            authorizing_identity_urn: o.authorizing_identity_urn.clone(),
        }
    }
}

fn object_state(name: &'static str) -> ObjectState {
    let tips = [0..10].iter().map(|_| random_oid());
    let schema_commit = random_oid();
    let (history, state) = history(name);
    let urn = radicle_git_ext::Oid::from(random_oid()).into();
    ObjectState {
        raw_history: history,
        refs: tips.collect(),
        schema_commit,
        schema: SCHEMA.clone(),
        state,
        typename: "some.type.name".parse().unwrap(),
        object_id: random_oid().into(),
        authorizing_identity_urn: urn,
    }
}

fn history(name: &'static str) -> (Vec<u8>, serde_json::Value) {
    let mut backend = automerge::Backend::new();
    let mut frontend = automerge::Frontend::new();
    let (_, change) = frontend
        .change::<_, _, automerge::InvalidChangeRequest>(None, |d| {
            d.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("name"),
                automerge::Value::Primitive(automerge::Primitive::Str(name.into())),
            ))?;
            Ok(())
        })
        .unwrap();
    backend.apply_local_change(change.unwrap()).unwrap();
    let history = backend.save().unwrap();
    let state = frontend
        .get_value(&automerge::Path::root())
        .unwrap()
        .to_json();
    (history, state)
}

fn random_oid() -> git2::Oid {
    let oid_raw: [u8; 20] = rand::random();
    git2::Oid::from_bytes(&oid_raw).unwrap()
}

/// This test checks that we can load a cached object from a test fixture.
/// The intention is to guard against future changes to the layout of
/// cache files which would make existing caches unloadable.
#[test]
fn test_load_v1() {
    let fixture_path = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("fixtures")
        .join("cache");
    let first_tip: radicle_git_ext::Oid = "a587846c7294f53c541df546bf73cbe308b81821"
        .try_into()
        .unwrap();
    let second_tip: radicle_git_ext::Oid = "7f31041aad51476ed63e924bd1f6e527db008da8"
        .try_into()
        .unwrap();
    let mut tips: BTreeSet<git2::Oid> = BTreeSet::new();
    tips.insert(first_tip.into());
    tips.insert(second_tip.into());
    let mut cache = FileSystemCache::open(fixture_path).unwrap();
    assert!(cache
        .load(
            "hnrk86cerdksint5q4a9jr16t6511xsaytswy".parse().unwrap(),
            &tips,
        )
        .unwrap()
        .is_some());
}
