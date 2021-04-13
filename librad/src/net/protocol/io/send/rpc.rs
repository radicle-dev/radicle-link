// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::sink::SinkExt as _;
use futures_codec::FramedWrite;
use serde::Serialize;

use crate::net::{
    connection::{RemoteAddr as _, RemotePeer},
    protocol::{broadcast, error, io::codec, membership},
    quic,
    upgrade,
};

#[derive(Debug, Clone, Serialize)]
pub enum Rpc<A, P>
where
    A: Clone + Ord,
{
    Membership(membership::Message<A>),
    Gossip(broadcast::Message<A, P>),
}

impl<A, P> From<membership::Message<A>> for Rpc<A, P>
where
    A: Clone + Ord,
{
    fn from(m: membership::Message<A>) -> Self {
        Self::Membership(m)
    }
}

impl<A, P> From<broadcast::Message<A, P>> for Rpc<A, P>
where
    A: Clone + Ord,
{
    fn from(m: broadcast::Message<A, P>) -> Self {
        Self::Gossip(m)
    }
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(
    skip(conn, rpc),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub async fn send_rpc<R, P>(
    conn: &quic::Connection,
    rpc: R,
) -> Result<(), error::Rpc<quic::SendStream>>
where
    R: Into<Rpc<SocketAddr, P>>,
    P: minicbor::Encode,
{
    use Rpc::*;

    let stream = conn.open_uni().await?;

    match rpc.into() {
        Membership(msg) => {
            let upgraded = upgrade::upgrade(stream, upgrade::Membership).await?;
            FramedWrite::new(upgraded, codec::Membership::new())
                .send(msg)
                .await?;
        },

        Gossip(msg) => {
            let upgraded = upgrade::upgrade(stream, upgrade::Gossip).await?;
            FramedWrite::new(upgraded, codec::Gossip::new())
                .send(msg)
                .await?;
        },
    }

    Ok(())
}
