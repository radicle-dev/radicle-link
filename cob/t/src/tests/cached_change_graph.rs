// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use cob::internals::CachedChangeGraph;
use std::collections::BTreeSet;

use minicbor::Decode;

use crate::helpers::random_history;

#[test]
fn test_cached_change_graph_encode_decode() {
    let commit = git2::Oid::from_str("f41a052ad0a6b8a17ddae486cf2322cc48215222").unwrap();
    let some_urn = radicle_git_ext::Oid::from(commit).into();
    let g = CachedChangeGraph {
        history: random_history("somename"),
        refs: BTreeSet::new(),
        object_id: commit.into(),
        typename: "some.type.name".parse().unwrap(),
        authorizing_identity_urn: some_urn,
    };
    let mut output: Vec<u8> = Vec::new();
    minicbor::encode(&g, &mut output).unwrap();
    let decoded = CachedChangeGraph::decode(&mut minicbor::Decoder::new(&output)).unwrap();
    assert_eq!(g, decoded);
}

#[test]
fn test_oids_minicbor_roundtrip() {
    let mut oids: BTreeSet<radicle_git_ext::Oid> = BTreeSet::new();
    let oid = git2::Oid::from_str("f41a052ad0a6b8a17ddae486cf2322cc48215222").unwrap();
    oids.insert(oid.into());

    let mut output: Vec<u8> = Vec::new();
    minicbor::encode(&oids, &mut output).unwrap();
    let decoded: BTreeSet<radicle_git_ext::Oid> = minicbor::decode(&output).unwrap();
    assert_eq!(oids, decoded);
}
