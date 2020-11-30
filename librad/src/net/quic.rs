// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::{BoxStream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::{self, NewConnection, TransportConfig, VarInt};
use thiserror::Error;

use crate::{
    net::{
        connection::{
            Closable,
            CloseReason,
            Duplex,
            HasStableId,
            LocalInfo,
            LocalPeer,
            RemoteInfo,
            RemotePeer,
        },
        tls,
        x509,
    },
    peer::{self, PeerId},
    signer::Signer,
};

// XXX: we _may_ want to allow runtime configuration of below consts at some
// point

/// The ALPN protocol(s) for the radicle-link protocol stack.
///
/// Not currently of significance, but established in order to allow future
/// major protocol upgrades.
const ALPN: &[&[u8]] = &[b"rad/1"];

/// Connection keep alive interval.
///
/// Only set for initiators (clients). The value of 30s is recommended for
/// keeping middlebox UDP flows alive.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Connection idle timeout.
///
/// Only has an effect for responders (servers), which we configure to not send
/// keep alive probes. Should tolerate the loss of 1-2 keep-alive probes.
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(65);

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("remote PeerId could not be determined")]
    RemoteIdUnavailable,

    #[error("connect to self")]
    SelfConnect,

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error("signer error")]
    Signer(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

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
    pub async fn bind<'a, S>(signer: S, listen_addr: SocketAddr) -> Result<BoundEndpoint<'a>>
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let peer_id = PeerId::from_signer(&signer);
        let (endpoint, incoming) = make_endpoint(signer, listen_addr).await?;
        let endpoint = Endpoint { peer_id, endpoint };
        let incoming = incoming
            .filter_map(move |connecting| async move {
                handle_incoming(peer_id, connecting).await.map_or_else(
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
    ) -> Result<(Connection, IncomingStreams<'a>)> {
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

impl LocalPeer for Endpoint {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id
    }
}

impl LocalInfo for Endpoint {
    type Addr = SocketAddr;

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }
}

async fn handle_incoming<'a>(
    local_peer: PeerId,
    connecting: quinn::Connecting,
    connecting: quinn::Connecting,
) -> Result<(Connection, IncomingStreams<'a>)> {
    let conn = connecting.await?;
    let remote_peer = remote_peer(&conn)?.ok_or(Error::RemoteIdUnavailable)?;
    // This should be prevented by the TLS handshake, but let's double check
    if remote_peer == local_peer {
        Err(Error::SelfConnect)
    } else {
        Ok(mk_connection(remote_peer, conn))
    }
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
            Ok(Some(
                x509::Certificate::from_der(first)
                    .map(|cert| cert.peer_id())
                    .unwrap(),
            ))
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
        uni_streams,
        ..
    }: NewConnection,
) -> (Connection, IncomingStreams<'a>) {
    let conn = Connection::new(remote_peer, connection);
    let bidi = {
        let conn = conn.clone();
        bi_streams
            .map_ok(move |(send, recv)| BidiStream {
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
            .map_err(Error::from)
    };
    let uni = {
        let conn = conn.clone();
        uni_streams
            .map_ok(move |recv| RecvStream {
                conn: conn.clone(),
                recv,
            })
            .map_err(Error::from)
    };

    let incoming = IncomingStreams {
        bidi: Box::pin(bidi),
        uni: Box::pin(uni),
    };

    (conn, incoming)
}

pub struct IncomingStreams<'a> {
    pub bidi: BoxStream<'a, Result<BidiStream>>,
    pub uni: BoxStream<'a, Result<RecvStream>>,
}

pub struct BoundEndpoint<'a> {
    pub endpoint: Endpoint,
    pub incoming: BoxStream<'a, (Connection, IncomingStreams<'a>)>,
}

impl<'a> LocalPeer for BoundEndpoint<'a> {
    fn local_peer_id(&self) -> PeerId {
        self.endpoint.local_peer_id()
    }
}

impl<'a> LocalInfo for BoundEndpoint<'a> {
    type Addr = SocketAddr;

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

    pub async fn open_bidi(&self) -> Result<BidiStream> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok(BidiStream {
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

    pub async fn open_uni(&self) -> Result<SendStream> {
        let send = self.conn.open_uni().await?;
        Ok(SendStream {
            conn: self.clone(),
            send,
        })
    }

    pub fn close(self, reason: CloseReason) {
        let code = VarInt::from_u32(reason as u32);
        self.conn.close(code, reason.reason_phrase())
    }

    pub fn stable_id(&self) -> usize {
        self.conn.stable_id()
    }
}

impl RemotePeer for Connection {
    fn remote_peer_id(&self) -> PeerId {
        self.peer
    }
}

impl RemoteInfo for Connection {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }
}

impl HasStableId for Connection {
    type Id = usize;

    fn stable_id(&self) -> Self::Id {
        self.stable_id()
    }
}

impl Closable for Connection {
    fn close(self, reason: CloseReason) {
        self.close(reason)
    }
}

pub struct BidiStream {
    conn: Connection,
    recv: RecvStream,
    send: SendStream,
}

impl BidiStream {
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

    pub fn id(&self) -> quinn::StreamId {
        let (x, y) = (self.recv.id(), self.send.id());
        debug_assert!(x == y);
        x
    }
}

impl RemotePeer for BidiStream {
    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }
}

impl RemoteInfo for BidiStream {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl HasStableId for BidiStream {
    type Id = quinn::StreamId;

    fn stable_id(&self) -> Self::Id {
        self.id()
    }
}

impl Duplex for BidiStream {
    type Read = RecvStream;
    type Write = SendStream;

    fn split(self) -> (Self::Read, Self::Write) {
        (self.recv, self.send)
    }
}

impl Closable for BidiStream {
    fn close(self, reason: CloseReason) {
        self.close(reason)
    }
}

impl AsyncRead for BidiStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

impl AsyncWrite for BidiStream {
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

    pub fn id(&self) -> quinn::StreamId {
        self.recv.id()
    }
}

impl RemotePeer for RecvStream {
    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }
}

impl RemoteInfo for RecvStream {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl HasStableId for RecvStream {
    type Id = quinn::StreamId;

    fn stable_id(&self) -> Self::Id {
        self.id()
    }
}

impl Closable for RecvStream {
    fn close(self, reason: CloseReason) {
        self.close(reason)
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

    pub fn id(&self) -> quinn::StreamId {
        self.send.id()
    }
}

impl RemotePeer for SendStream {
    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }
}

impl RemoteInfo for SendStream {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl HasStableId for SendStream {
    type Id = quinn::StreamId;

    fn stable_id(&self) -> Self::Id {
        self.id()
    }
}

impl Closable for SendStream {
    fn close(self, reason: CloseReason) {
        self.close(reason)
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
    signer: S,
    listen_addr: SocketAddr,
) -> Result<(quinn::Endpoint, quinn::Incoming)>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer.clone())?);
    builder.listen(make_server_config(signer)?);

    Ok(builder.bind(&listen_addr)?)
}

fn make_client_config<S>(signer: S) -> Result<quinn::ClientConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_client_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = ALPN.iter().map(|x| x.to_vec()).collect();

    let mut transport_config = TransportConfig::default();
    transport_config
        .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
        // Set idle timeout anyway, as the default is smaller than our
        // keep-alive
        .max_idle_timeout(Some(MAX_IDLE_TIMEOUT))
        .expect("idle timeout is in vetted range");

    let mut quic_config = quinn::ClientConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(transport_config);

    Ok(quic_config)
}

fn make_server_config<S>(signer: S) -> Result<quinn::ServerConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_server_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = ALPN.iter().map(|x| x.to_vec()).collect();

    let mut transport_config = TransportConfig::default();
    transport_config
        .max_idle_timeout(Some(MAX_IDLE_TIMEOUT))
        .expect("idle timeout is in vetted range");

    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(transport_config);

    Ok(quic_config)
}
