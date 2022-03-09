// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_ref_format::{name, refname, Qualified};
use link_crypto::PeerId;
use link_replication::refs;
use once_cell::sync::Lazy;

use super::PEER;

static REFS_HEADS_MAIN: Lazy<Qualified<'static>> =
    Lazy::new(|| name::REFS_HEADS_MAIN.qualified().unwrap());

#[test]
fn remote_tracking_identity() {
    assert_eq!(
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar",
        refs::remote_tracking(
            &PEER,
            refname!("refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar")
                .into_qualified()
                .unwrap()
        )
        .unwrap()
        .as_str()
    );
}

#[test]
fn owned_identity() {
    assert_eq!(
        REFS_HEADS_MAIN.as_str(),
        refs::owned(REFS_HEADS_MAIN.clone()).unwrap().as_str()
    )
}

#[test]
fn owned_nested() {
    assert_eq!(
        "refs/heads/foo/bar",
        refs::owned(refname!("refs/heads/foo/bar").into_qualified().unwrap())
            .unwrap()
            .as_str()
    )
}

#[test]
fn owned_conversion() {
    assert_eq!(
        "refs/heads/main",
        refs::owned(
            refname!("refs/remotes/origin/heads/main")
                .into_qualified()
                .unwrap()
        )
        .unwrap()
        .as_str()
    )
}

#[test]
fn owned_no_leaf() {
    assert!(refs::owned(refname!("refs/remotes/foo").into_qualified().unwrap()).is_none())
}

#[test]
fn scoped_wanted_is_remote() {
    assert_eq!(
        REFS_HEADS_MAIN.as_str(),
        Qualified::from(refs::scoped(
            &PEER,
            &PEER,
            refs::owned(REFS_HEADS_MAIN.clone()).unwrap()
        ))
        .as_str()
    )
}

#[test]
fn scoped_wanted_is_not_remote() {
    let wanted: PeerId = "hydkgqxj4q3zgp34n8mwf9ycghi8mguz8qd3h8gyyn6ktz4hb7pync"
        .parse()
        .unwrap();
    assert_eq!(
        "refs/remotes/hydkgqxj4q3zgp34n8mwf9ycghi8mguz8qd3h8gyyn6ktz4hb7pync/heads/main",
        Qualified::from(refs::scoped(
            &wanted,
            &PEER,
            refs::owned(REFS_HEADS_MAIN.clone()).unwrap()
        ))
        .as_str()
    )
}
