// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either::{Left, Right};

use librad::{
    git::Urn,
    git_ext as ext,
    identities::urn,
    keys::SecretKey,
    net::peer::storage::urn_context,
    peer::{Originates, PeerId},
    reflike,
};

lazy_static! {
    static ref LOCAL_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
        188, 124, 109, 100, 178, 93, 115, 53, 15, 22, 114, 181, 15, 211, 233, 104, 32, 189, 9, 162,
        235, 148, 204, 172, 21, 117, 34, 9, 236, 247, 238, 113
    ]));
    static ref OTHER_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed([
        236, 225, 197, 234, 16, 153, 83, 54, 15, 203, 86, 253, 157, 81, 144, 96, 106, 99, 65, 129,
        8, 181, 125, 141, 120, 122, 58, 48, 22, 97, 32, 9
    ]));
    static ref ZERO_OID: ext::Oid = git2::Oid::zero().into();
}

#[test]
fn direct_empty() {
    let urn = Urn::new(*ZERO_OID);
    let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
    assert_eq!(
        urn.with_path(ext::RefLike::from(urn::DEFAULT_PATH.clone())),
        ctx
    )
}

#[test]
fn direct_onelevel() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
    let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
    assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
}

#[test]
fn direct_qualified() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
    let ctx = urn_context(*LOCAL_PEER_ID, Left(urn.clone()));
    assert_eq!(urn, ctx)
}

#[test]
fn remote_empty() {
    let urn = Urn::new(*ZERO_OID);
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *OTHER_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(
        urn.with_path(
            reflike!("refs/remotes").join(*OTHER_PEER_ID).join(
                ext::RefLike::from(urn::DEFAULT_PATH.clone())
                    .strip_prefix("refs")
                    .unwrap()
            )
        ),
        ctx
    )
}

#[test]
fn remote_onelevel() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *OTHER_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(
        urn.with_path(
            reflike!("refs/remotes")
                .join(*OTHER_PEER_ID)
                .join(reflike!("heads/ban/ana"))
        ),
        ctx
    )
}

#[test]
fn remote_qualified() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *OTHER_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(
        urn.with_path(
            reflike!("refs/remotes")
                .join(*OTHER_PEER_ID)
                .join(reflike!("heads/next"))
        ),
        ctx
    )
}

#[test]
fn self_origin_empty() {
    let urn = Urn::new(*ZERO_OID);
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *LOCAL_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(
        urn.with_path(ext::RefLike::from(urn::DEFAULT_PATH.clone())),
        ctx
    )
}

#[test]
fn self_origin_onelevel() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("ban/ana"));
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *LOCAL_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(urn.with_path(reflike!("refs/heads/ban/ana")), ctx)
}

#[test]
fn self_origin_qualified() {
    let urn = Urn::new(*ZERO_OID).with_path(reflike!("refs/heads/next"));
    let ctx = urn_context(
        *LOCAL_PEER_ID,
        Right(Originates {
            from: *LOCAL_PEER_ID,
            value: urn.clone(),
        }),
    );
    assert_eq!(urn, ctx)
}
