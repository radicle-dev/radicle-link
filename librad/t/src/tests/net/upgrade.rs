// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use futures::try_join;
use librad::{
    net::upgrade::{
        upgrade,
        with_upgraded,
        Error,
        Git,
        Gossip,
        Interrogation,
        Membership,
        RequestPull,
        SomeUpgraded,
        UpgradeRequest,
    },
    PeerId,
    SecretKey,
};
use test_helpers::roundtrip;

use crate::helpers::connection::MockStream;

lazy_static! {
    static ref INITIATOR: PeerId = PeerId::from(SecretKey::from_seed([
        164, 74, 212, 59, 165, 115, 21, 231, 172, 182, 132, 97, 153, 209, 157, 239, 159, 129, 46,
        66, 173, 231, 36, 196, 164, 59, 203, 197, 153, 232, 150, 24
    ]));
    static ref RECEIVER: PeerId = PeerId::from(SecretKey::from_seed([
        187, 77, 103, 158, 241, 220, 26, 209, 116, 9, 70, 140, 27, 149, 254, 144, 80, 207, 112,
        171, 189, 222, 235, 233, 211, 249, 4, 159, 219, 39, 166, 112
    ]));
}

async fn test_upgrade(
    req: impl Into<UpgradeRequest>,
) -> Result<SomeUpgraded<()>, Error<MockStream>> {
    let (initiator, receiver) = MockStream::pair(*INITIATOR, *RECEIVER, 512);
    try_join!(
        async { upgrade(initiator, req).await.map_err(Error::from) },
        async {
            with_upgraded(receiver)
                .await
                .map(|upgrade| upgrade.map(|_| ()))
        }
    )
    .map(|(_, upgrade)| upgrade)
}

#[tokio::test]
async fn upgrade_gossip() {
    assert_matches!(test_upgrade(Git).await, Ok(SomeUpgraded::Git(_)))
}

#[tokio::test]
async fn upgrade_git() {
    assert_matches!(test_upgrade(Gossip).await, Ok(SomeUpgraded::Gossip(_)))
}

#[tokio::test]
async fn upgrade_membership() {
    assert_matches!(
        test_upgrade(Membership).await,
        Ok(SomeUpgraded::Membership(_))
    )
}

#[tokio::test]
async fn upgrade_interrogation() {
    assert_matches!(
        test_upgrade(Interrogation).await,
        Ok(SomeUpgraded::Interrogation(_))
    )
}

#[tokio::test]
async fn upgrade_request_pull() {
    assert_matches!(
        test_upgrade(RequestPull).await,
        Ok(SomeUpgraded::RequestPull(_))
    )
}

#[test]
fn roundtrip_upgrade_request() {
    roundtrip::cbor(UpgradeRequest::Gossip);
    roundtrip::cbor(UpgradeRequest::Git);
    roundtrip::cbor(UpgradeRequest::Membership);
    roundtrip::cbor(UpgradeRequest::Interrogation);
    roundtrip::cbor(UpgradeRequest::RequestPull);
}
