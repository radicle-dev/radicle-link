// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;

use gitd_lib::git_subprocess::command;
use it_helpers::tmp;
use librad::{
    git::{storage::Storage, Urn},
    reflike,
    SecretKey,
};

#[test]
fn visible_remotes() {
    let should_match = vec![
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/heads/main"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/heads/deep/nested/main"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/tags/repl-3"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/notes/hello"
        ),
    ];
    let should_not_match = vec![
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/rad/id"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/rad/ids/mmkayyyy"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/rad/signed_refs"
        ),
        reflike!(
            "refs/namespaces/hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/xyz/cobs/issues/1"
        ),
    ];

    let storage = tmp::storage(SecretKey::new());
    {
        let raw = git2::Repository::open(storage.path()).unwrap();
        let oid = raw.blob(b"just data").unwrap();

        for name in &should_match {
            raw.reference(name.as_str(), oid, false, "").unwrap();
        }
        for name in &should_not_match {
            raw.reference(name.as_str(), oid, false, "").unwrap();
        }
    }

    let storage: &Storage = storage.as_ref();
    let remotes = command::visible_remotes(
        storage,
        &Urn::try_from_id("hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy").unwrap(),
    )
    .unwrap()
    .collect::<BTreeSet<_>>();

    for r in &should_match {
        assert!(remotes.contains(r), "should have matched {}", r)
    }

    for r in &should_not_match {
        assert!(!remotes.contains(r), "should not have matched {}", r)
    }
}
