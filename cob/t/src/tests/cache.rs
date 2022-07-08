// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use cob::{
    internals::{Cache, CachedChangeGraph, FileSystemCache},
    ObjectId,
};
use rand::Rng;
use std::{cell::RefCell, env::temp_dir, rc::Rc};

use crate::helpers::{random_history, random_oid};

struct CacheTestEnv {
    states: Vec<CachedChangeGraph>,
    dir: std::path::PathBuf,
    oid: ObjectId,
}

impl CacheTestEnv {
    fn new() -> CacheTestEnv {
        let states: [&str; 3] = ["one", "two", "three"];
        let graph_states: Vec<CachedChangeGraph> = states.iter().map(|s| object_state(s)).collect();

        let cache_dirname: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(30)
            .map(char::from)
            .collect();
        let cache_dir = temp_dir().join(cache_dirname);

        let the_oid: ObjectId = random_oid().into();

        CacheTestEnv {
            states: graph_states,
            dir: cache_dir,
            oid: the_oid,
        }
    }
}

#[test]
fn test_load_returns_none_if_refs_dont_match() {
    let test_env = CacheTestEnv::new();
    let mut cache = FileSystemCache::open(test_env.dir.as_path()).unwrap();
    let target_state = &test_env.states[0];
    cache
        .put(test_env.oid, Rc::new(RefCell::new(target_state.clone())))
        .unwrap();
    if let Some(reloaded) = cache.load(test_env.oid, &target_state.refs).unwrap() {
        let reloaded: &CachedChangeGraph = &reloaded.as_ref().borrow();
        assert_eq!(reloaded, target_state);
    } else {
        panic!("cache returned None");
    }
    assert!(cache
        .load(test_env.oid, &test_env.states[1].refs)
        .unwrap()
        .is_none());
    assert!(cache
        .load(test_env.oid, &test_env.states[2].refs)
        .unwrap()
        .is_none());
}

fn object_state(name: &'static str) -> CachedChangeGraph {
    let tips = [0..10].iter().map(|_| random_oid());
    let history = random_history(name);
    let urn = radicle_git_ext::Oid::from(random_oid()).into();
    CachedChangeGraph {
        refs: tips.collect(),
        history,
        typename: "some.type.name".parse().unwrap(),
        object_id: random_oid().into(),
        authorizing_identity_urn: urn,
    }
}
