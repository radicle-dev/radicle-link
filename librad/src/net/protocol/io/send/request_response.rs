// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net::SocketAddr;

use futures::{SinkExt as _, StreamExt as _, TryStreamExt as _};
use futures_codec::Framed;

use crate::net::{
    codec::CborCodec,
    connection::{RemoteAddr as _, RemotePeer as _},
    protocol::{error, interrogation, quic, request_pull, upgrade},
};

pub trait Request {
    type Response;
    type Upgrade: Into<upgrade::UpgradeRequest>;
    const UPGRADE: Self::Upgrade;
}

impl Request for interrogation::Request {
    type Response = interrogation::Response<'static, SocketAddr>;
    type Upgrade = upgrade::Interrogation;
    const UPGRADE: Self::Upgrade = upgrade::Interrogation;
}

impl Request for request_pull::Request {
    type Response = request_pull::Response;
    type Upgrade = upgrade::RequestPull;
    const UPGRADE: Self::Upgrade = upgrade::RequestPull;
}

#[tracing::instrument(
    skip(conn, req),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub async fn single_response<R>(
    conn: &quic::Connection,
    req: R,
) -> Result<Option<R::Response>, error::Rpc<quic::BidiStream>>
where
    R: Request + minicbor::Encode,
    for<'a> R::Response: minicbor::Decode<'a>,
{
    request(conn, req).await?.try_next().await
}

#[tracing::instrument(
    skip(conn, req),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub async fn multi_response<R>(
    conn: &quic::Connection,
    req: R,
) -> Result<
    impl futures::Stream<Item = Result<R::Response, error::Rpc<quic::BidiStream>>>,
    error::Rpc<quic::BidiStream>,
>
where
    R: Request + minicbor::Encode,
    for<'a> R::Response: minicbor::Decode<'a>,
{
    request(conn, req).await
}

#[tracing::instrument(
    skip(conn, req),
    fields(
        remote_id = %conn.remote_peer_id(),
        remote_addr = %conn.remote_addr()
    ),
    err
)]
pub async fn request<R>(
    conn: &quic::Connection,
    req: R,
) -> Result<
    impl futures::Stream<Item = Result<R::Response, error::Rpc<quic::BidiStream>>>,
    error::Rpc<quic::BidiStream>,
>
where
    R: Request + minicbor::Encode,
    for<'a> R::Response: minicbor::Decode<'a>,
{
    let stream = conn.open_bidi().await?;
    let upgraded = upgrade::upgrade(stream, R::UPGRADE).await?;
    let mut framing = Framed::new(upgraded.into_stream(), CborCodec::<R, R::Response>::new());
    framing.send(req).await?;
    Ok(framing.map(|item| item.map_err(error::Rpc::from)))
}
