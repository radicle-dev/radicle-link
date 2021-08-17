// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use librad::{
    git::{
        storage::Storage,
        tracking::{is_tracked, track, tracked, untrack},
        Urn,
    },
    paths::Paths,
    reflike,
    PeerId,
    SecretKey,
};

#[test]
fn track_is_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let remote_peer = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        track(&storage, &urn, remote_peer).unwrap();
        assert!(is_tracked(&storage, &urn, remote_peer).unwrap())
    }
}

#[test]
fn track_untrack_is_not_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let remote_peer = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        track(&storage, &urn, remote_peer).unwrap();
        assert!(is_tracked(&storage, &urn, remote_peer).unwrap());
        untrack(&storage, &urn, remote_peer).unwrap();
        assert!(!is_tracked(&storage, &urn, remote_peer).unwrap())
    }
}

#[test]
fn track_track_is_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let remote_peer = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        track(&storage, &urn, remote_peer).unwrap();
        assert!(is_tracked(&storage, &urn, remote_peer).unwrap());
        track(&storage, &urn, remote_peer).unwrap();
        assert!(is_tracked(&storage, &urn, remote_peer).unwrap())
    }
}

#[test]
fn untrack_nonexistent_is_not_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let remote_peer = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        untrack(&storage, &urn, remote_peer).unwrap();
        assert!(!is_tracked(&storage, &urn, remote_peer).unwrap());
    }
}

#[test]
fn track_yields_tracked() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let peer1 = PeerId::from(SecretKey::new());
        let peer2 = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        track(&storage, &urn, peer1).unwrap();
        track(&storage, &urn, peer2).unwrap();
        assert_eq!(
            [peer1, peer2].iter().copied().collect::<BTreeSet<_>>(),
            tracked(&storage, &urn).unwrap().collect::<BTreeSet<_>>()
        )
    }
}

#[test]
fn tracked_ignores_urn_path() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let paths = Paths::from_root(&tmp).unwrap();
        let storage = Storage::open(&paths, SecretKey::new()).unwrap();
        let remote_peer = PeerId::from(SecretKey::new());
        let urn = Urn::new(git2::Oid::zero().into());

        track(&storage, &urn, remote_peer).unwrap();

        let urn = urn.with_path(reflike!("ri/ra/rutsch"));
        assert_eq!(Some(remote_peer), tracked(&storage, &urn).unwrap().next())
    }
}
