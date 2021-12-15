// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use bstr::{BString, ByteSlice as _};
use link_replication::refs;

use super::PEER;

#[test]
fn namespaced() {
    assert_eq!(
        BString::from("refs/namespaces/xyz/refs/rad/id"),
        refs::Namespaced {
            namespace: Some(BString::from("xyz").into()),
            refname: refs::RadId.into()
        }
        .qualified()
    );
}

#[test]
fn namespaced_without_namespace() {
    assert_eq!(
        BString::from("refs/rad/id"),
        refs::Namespaced {
            namespace: None,
            refname: refs::RadId.into()
        }
        .qualified()
    );
}

#[test]
fn remote_tracking_identity() {
    assert_eq!(
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar",
        refs::remote_tracking(
            &PEER,
            BString::from(
                "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar"
            )
        )
        .to_str_lossy()
    );
}

#[test]
fn remote_tracking_conversion() {
    assert_eq!(
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar",
        refs::remote_tracking(&PEER, BString::from("foo/bar")).to_str_lossy()
    )
}

#[test]
fn remote_tracking_conversion_with_refs() {
    assert_eq!(
        "refs/remotes/hyn3aar1qghrnjrdi161oks1w3z9s173mxti88ci6qthps8brmp6yo/foo/bar",
        refs::remote_tracking(&PEER, BString::from("refs/foo/bar")).to_str_lossy()
    )
}

#[test]
fn owned_identity() {
    assert_eq!(
        "refs/heads/main",
        refs::owned(BString::from("refs/heads/main"))
            .unwrap()
            .to_str_lossy()
    )
}

#[test]
fn owned_conversion() {
    assert_eq!(
        "refs/heads/main",
        refs::owned(BString::from("refs/remotes/origin/heads/main"))
            .unwrap()
            .to_str_lossy()
    )
}

#[test]
#[should_panic]
fn owned_empty() {
    refs::owned(BString::from("refs/remotes/foo/")).unwrap();
}

#[test]
#[should_panic]
fn owned_invalid() {
    refs::owned(BString::from("refs/remotes")).unwrap();
}

#[test]
#[should_panic]
fn owned_no_leaf() {
    refs::owned(BString::from("refs/remotes/foo")).unwrap();
}

#[test]
fn scoped_wanted_is_remote() {
    assert_eq!(
        "refs/heads/main",
        refs::scoped(&PEER, &PEER, BString::from("refs/heads/main"))
            .as_ref()
            .to_str_lossy()
    )
}

#[test]
fn scoped_wanted_is_not_remote() {
    let wanted = "hydkgqxj4q3zgp34n8mwf9ycghi8mguz8qd3h8gyyn6ktz4hb7pync"
        .parse()
        .unwrap();
    assert_eq!(
        "refs/remotes/hydkgqxj4q3zgp34n8mwf9ycghi8mguz8qd3h8gyyn6ktz4hb7pync/heads/main",
        refs::scoped(&wanted, &PEER, BString::from("refs/heads/main"))
            .as_ref()
            .to_str_lossy()
    )
}

#[test]
#[should_panic]
fn scoped_invalid_remote_tracking_branch() {
    refs::scoped(&PEER, &PEER, BString::from("refs/remotes"));
}
