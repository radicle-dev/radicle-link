// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use serde_json::json;

use librad::{
    git::{fetch::RemoteHeads, refs::Remotes},
    net::protocol::io::graft::*,
    reflike,
};

#[test]
fn interesting() {
    let remote_peer = "lolek".to_owned();
    let remotes: Remotes<String> = serde_json::from_value(json!({
        "bolek": {
            "tola": {}
        }
    }))
    .unwrap();
    let remote_heads: RemoteHeads = vec![
        (reflike!("refs/heads/memester"), git2::Oid::zero().into()),
        (
            reflike!("refs/remotes/tola/heads/memestress"),
            git2::Oid::zero().into(),
        ),
    ]
    .into_iter()
    .collect();

    assert!(is_interesting(remote_peer, &remote_heads, &remotes))
}

#[test]
fn not_interesting() {
    let remote_peer = "lolek".to_owned();
    let remotes: Remotes<String> = serde_json::from_value(json!({
        "tola": {}
    }))
    .unwrap();
    let remote_heads: RemoteHeads = vec![
        (reflike!("refs/heads/memester"), git2::Oid::zero().into()),
        (
            reflike!("refs/remotes/bolek/heads/main"),
            git2::Oid::zero().into(),
        ),
    ]
    .into_iter()
    .collect();

    assert!(!is_interesting(remote_peer, &remote_heads, &remotes))
}
