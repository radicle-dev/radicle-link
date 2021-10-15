// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use cob::{
    internals::{forward_compatible_decode, ThinChangeGraph},
    History,
    Schema,
};
use std::{collections::BTreeSet, convert::TryFrom};

#[test]
fn test_thin_change_graph_encode_decode() {
    let commit = git2::Oid::from_str("f41a052ad0a6b8a17ddae486cf2322cc48215222").unwrap();
    let some_urn = radicle_git_ext::Oid::from(commit).into();
    let schema = Schema::try_from(&serde_json::json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    }))
    .unwrap();
    let g = ThinChangeGraph {
        validated_history: None,
        history: History::Automerge(vec![1, 2, 3, 4, 5]),
        refs: BTreeSet::new(),
        schema_commit: commit,
        schema,
        state: serde_json::json!({"some": "state"}),
        object_id: commit.into(),
        typename: "some.type.name".parse().unwrap(),
        authorizing_identity_urn: some_urn,
    };
    let mut output: Vec<u8> = Vec::new();
    minicbor::encode(&g, &mut output).unwrap();
    let decoded: ThinChangeGraph = forward_compatible_decode(&mut minicbor::Decoder::new(&output))
        .unwrap()
        .unwrap();
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
