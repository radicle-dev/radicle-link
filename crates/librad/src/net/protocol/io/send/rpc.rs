// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, ops::DerefMut as _};

use futures::{SinkExt as _, TryFutureExt as _};
use futures_codec::FramedWrite;

use crate::net::{
    connection::{RemoteAddr as _, RemotePeer},
    protocol::{broadcast, error, io::codec, membership},
    quic,
    upgrade,
};

#[derive(Debug)]
pub enum Rpc<A, P> {
    Membership(membership::Message<A>),
    Gossip(broadcast::Message<A, P>),
}

impl<A, P> From<membership::Message<A>> for Rpc<A, P> {
    fn from(m: membership::Message<A>) -> Self {
        Self::Membership(m)
    }
}

impl<A, P> From<broadcast::Message<A, P>> for Rpc<A, P> {
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

    fn into_protocol_error(
        e: quic::BorrowUniError<upgrade::Error<quic::SendStream>>,
    ) -> error::Rpc<quic::SendStream> {
        match e {
            quic::BorrowUniError::Quic(f) => error::Rpc::Quic(f),
            quic::BorrowUniError::Upgrade(f) => error::Rpc::Upgrade(f),
        }
    }

    enum StreamIndex {
        Member = 0,
        Gossip = 1,
    }

    impl From<StreamIndex> for usize {
        fn from(idx: StreamIndex) -> usize {
            idx as usize
        }
    }

    match rpc.into() {
        Membership(msg) => {
            let mut stream = conn
                .borrow_uni(StreamIndex::Member, |s| {
                    upgrade::upgrade(s, upgrade::Membership)
                        .map_ok(|upgraded| upgraded.into_stream())
                })
                .await
                .map_err(into_protocol_error)?;
            FramedWrite::new(stream.deref_mut(), codec::Membership::new())
                .send(msg)
                .await?;
        },

        Gossip(msg) => {
            let mut stream = conn
                .borrow_uni(StreamIndex::Gossip, |s| {
                    upgrade::upgrade(s, upgrade::Gossip).map_ok(|upgraded| upgraded.into_stream())
                })
                .await
                .map_err(into_protocol_error)?;
            FramedWrite::new(stream.deref_mut(), codec::Gossip::new())
                .send(msg)
                .await?;
        },
    }

    Ok(())
}
