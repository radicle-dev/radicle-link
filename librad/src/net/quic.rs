// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{io, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::{BoxStream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::{self, NewConnection, TransportConfig, VarInt};
use thiserror::Error;

use keystore::sign;

use crate::{
    keys::AsPKCS8,
    net::{
        connection::{self, CloseReason, LocalInfo, RemoteInfo},
        tls,
    },
    peer::{self, PeerId},
};

/// The ALPN protocol(s) for the radicle-link protocol stack.
///
/// Not currently of significance, but established in order to allow future
/// major protocol upgrades.
const ALPN: &[&[u8]] = &[b"rad/1"];

/// Timeout duration before sending a keep alive message to a connected peer.
const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(1);

/// Timeout duration before a peer is considered disconnected.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("remote PeerId could not be determined")]
    RemoteIdUnavailable,

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error(transparent)]
    Cert(#[from] yasna::ASN1Error),

    #[error(transparent)]
    Endpoint(#[from] quinn::EndpointError),

    #[error(transparent)]
    Connect(#[from] quinn::ConnectError),

    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Endpoint {
    peer_id: PeerId,
    endpoint: quinn::Endpoint,
}

impl Endpoint {
    pub async fn bind<'a, S>(signer: &S, listen_addr: SocketAddr) -> Result<BoundEndpoint<'a>>
    where
        S: sign::Signer + AsPKCS8,
    {
        let peer_id = PeerId::from_signer(signer);
        let (endpoint, incoming) = make_endpoint(signer, listen_addr).await?;
        let endpoint = Endpoint { peer_id, endpoint };
        let incoming = incoming
            .filter_map(|connecting| async move {
                handle_incoming(connecting).await.map_or_else(
                    |e| {
                        tracing::warn!("Error handling incoming connection: {}", e);
                        None
                    },
                    Some,
                )
            })
            .boxed();

        Ok(BoundEndpoint { endpoint, incoming })
    }

    pub async fn connect<'a>(
        &mut self,
        peer: PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, BoxStream<'a, Result<Stream>>)> {
        let conn = self
            .endpoint
            .connect(addr, peer.as_dns_name().as_ref().into())?
            .await?;
        match remote_peer(&conn)? {
            Some(peer) => Ok(mk_connection(peer, conn)),
            None => Ok(mk_connection(peer, conn)),
        }
    }

    /// Close all of this endpoint's connections immediately and cease accepting
    /// new connections.
    ///
    /// Should only ever by called when the protocol stack is shut down.
    ///
    /// Morally, this method should consume `self`, but we shut down from a
    /// `Drop` impl, so all we got is `&mut self`.
    pub(crate) fn shutdown(&mut self) {
        const CODE: VarInt = VarInt::from_u32(CloseReason::ServerShutdown as u32);
        self.endpoint
            .close(CODE, CloseReason::ServerShutdown.reason_phrase())
    }
}

impl LocalInfo for Endpoint {
    type Addr = SocketAddr;

    fn local_peer_id(&self) -> PeerId {
        self.peer_id
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }
}

async fn handle_incoming<'a>(connecting: quinn::Connecting) -> Result<(Connection, Incoming<'a>)> {
    let conn = connecting.await?;
    remote_peer(&conn)?
        .ok_or(Error::RemoteIdUnavailable)
        .map(|peer| mk_connection(peer, conn))
}

/// Try to extract the remote identity from a newly established connection
///
/// Our source of truth is the peer certificate presented during the TLS
/// handshake. However, if TLS session resumption is used, this may not be
/// available -- in this case, we try to fall back on the SNI for incoming
/// connections. If both are not available, outgoing connections can still be
/// made, as the remote [`PeerId`] is known and guaranteed to match the remote
/// end.
///
/// If this returns [`Err`], the connection attempt should be aborted. If it
/// returns `Ok(None)`, the remote identity could not be determined.
fn remote_peer(conn: &NewConnection) -> Result<Option<PeerId>> {
    match conn.connection.peer_identity() {
        Some(certs) => {
            let first = certs
                .iter()
                .next()
                .expect("One certificate must have been presented")
                .as_ref();
            tls::extract_peer_id(first).map(Some).map_err(Error::from)
        },

        None => conn
            .connection
            .handshake_data()
            .and_then(|hsd| hsd.server_name)
            .map(|sni| sni.parse().map_err(Error::from))
            .transpose(),
    }
}

fn mk_connection<'a>(
    remote_peer: PeerId,
    NewConnection {
        connection,
        bi_streams,
        ..
    }: NewConnection,
) -> (Connection, Incoming<'a>) {
    let conn = Connection::new(remote_peer, connection);
    (
        conn.clone(),
        Box::pin(
            bi_streams
                .map_ok(move |(send, recv)| Stream {
                    conn: conn.clone(),
                    send: SendStream {
                        conn: conn.clone(),
                        send,
                    },
                    recv: RecvStream {
                        conn: conn.clone(),
                        recv,
                    },
                })
                .map_err(|e| e.into()),
        ),
    )
}

pub type Incoming<'a> = BoxStream<'a, Result<Stream>>;

pub struct BoundEndpoint<'a> {
    pub endpoint: Endpoint,
    pub incoming: BoxStream<'a, (Connection, Incoming<'a>)>,
}

impl<'a> LocalInfo for BoundEndpoint<'a> {
    type Addr = SocketAddr;

    fn local_peer_id(&self) -> PeerId {
        self.endpoint.local_peer_id()
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConnectionError {
    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
}

#[derive(Clone)]
pub struct Connection {
    peer: PeerId,
    conn: quinn::Connection,
}

impl Connection {
    pub fn new(peer: PeerId, conn: quinn::Connection) -> Self {
        Self { peer, conn }
    }

    pub async fn open_stream(&self) -> Result<Stream> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok(Stream {
            conn: self.clone(),
            recv: RecvStream {
                conn: self.clone(),
                recv,
            },
            send: SendStream {
                conn: self.clone(),
                send,
            },
        })
    }

    pub fn close(self, reason: CloseReason) {
        let code = VarInt::from_u32(reason as u32);
        self.conn.close(code, reason.reason_phrase())
    }
}

impl RemoteInfo for Connection {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> PeerId {
        self.peer
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }
}

pub struct Stream {
    conn: Connection,
    recv: RecvStream,
    send: SendStream,
}

impl Stream {
    pub fn framed<C>(self, codec: C) -> Framed<Self, C>
    where
        C: Decoder + Encoder,
    {
        Framed::new(self, codec)
    }

    pub fn close(self, reason: CloseReason) {
        self.send.close(reason);
        self.recv.close(reason);
    }
}

impl RemoteInfo for Stream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl connection::Stream for Stream {
    type Read = RecvStream;
    type Write = SendStream;

    fn split(self) -> (Self::Read, Self::Write) {
        (self.recv, self.send)
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<std::result::Result<usize, io::Error>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}

pub struct RecvStream {
    conn: Connection,
    recv: quinn::RecvStream,
}

impl RecvStream {
    pub fn close(mut self, reason: CloseReason) {
        let _ = self.recv.stop(VarInt::from_u32(reason as u32));
    }
}

impl RemoteInfo for RecvStream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl AsyncRead for RecvStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

pub struct SendStream {
    conn: Connection,
    send: quinn::SendStream,
}

impl SendStream {
    pub fn close(mut self, reason: CloseReason) {
        let _ = self.send.reset(VarInt::from_u32(reason as u32));
    }
}

impl RemoteInfo for SendStream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl AsyncWrite for SendStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}
async fn make_endpoint<S>(
    signer: &S,
    listen_addr: SocketAddr,
) -> std::result::Result<(quinn::Endpoint, quinn::Incoming), quinn::EndpointError>
where
    S: sign::Signer + AsPKCS8,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer));
    builder.listen(make_server_config(signer));

    builder.bind(&listen_addr)
}

fn make_client_config<S>(signer: &S) -> quinn::ClientConfig
where
    S: sign::Signer + AsPKCS8,
{
    let mut tls_config = tls::make_client_config(signer);
    tls_config.alpn_protocols = ALPN.iter().map(|x| x.to_vec()).collect();

    let mut quic_config = quinn::ClientConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(make_transport_config());

    quic_config
}

fn make_server_config<S>(signer: &S) -> quinn::ServerConfig
where
    S: sign::Signer + AsPKCS8,
{
    let mut tls_config = tls::make_server_config(signer);
    tls_config.alpn_protocols = ALPN.iter().map(|x| x.to_vec()).collect();

    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(make_transport_config());

    quic_config
}

fn make_transport_config() -> quinn::TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.keep_alive_interval(Some(DEFAULT_PING_TIMEOUT));
    transport_config
        .max_idle_timeout(Some(DEFAULT_IDLE_TIMEOUT))
        .unwrap();

    transport_config
}
