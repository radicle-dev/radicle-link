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
        connection::{self, CloseReason, LocalInfo, RemoteInfo},
        tls,
        x509,
        Network,
        PROTOCOL_VERSION,
    },
    peer::{self, PeerId},
    signer::Signer,
};

const ALPN_PREFIX: &[u8] = b"rad";

// XXX: we _may_ want to allow runtime configuration of below consts at some
// point

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
    pub async fn bind<'a, S>(
        signer: S,
        listen_addr: SocketAddr,
        network: Network,
    ) -> Result<BoundEndpoint<'a>>
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let peer_id = PeerId::from_signer(&signer);
        let (endpoint, incoming) = make_endpoint(signer, listen_addr, alpn(network)).await?;
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
    ) -> Result<(Connection, BoxStream<'a, Result<Stream>>)> {
        // Short-circuit: the other end will reject anyway
        if self.peer_id == peer {
            return Err(Error::SelfConnect);
        }

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

async fn handle_incoming<'a>(
    local_peer: PeerId,
    connecting: quinn::Connecting,
) -> Result<(Connection, Incoming<'a>)> {
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

type Alpn = Vec<u8>;

fn alpn(network: Network) -> Alpn {
    let mut alpn = ALPN_PREFIX.to_vec();
    alpn.push(b'/');
    alpn.push(PROTOCOL_VERSION);
    match network {
        Network::Main => alpn,
        Network::Custom(id) => {
            alpn.push(b'/');
            alpn.extend(id);
            alpn
        },
    }
}

async fn make_endpoint<S>(
    signer: S,
    listen_addr: SocketAddr,
    alpn: Alpn,
) -> Result<(quinn::Endpoint, quinn::Incoming)>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer.clone(), alpn.clone())?);
    builder.listen(make_server_config(signer, alpn)?);

    Ok(builder.bind(&listen_addr)?)
}

fn make_client_config<S>(signer: S, alpn: Alpn) -> Result<quinn::ClientConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_client_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = vec![alpn];

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

fn make_server_config<S>(signer: S, alpn: Alpn) -> Result<quinn::ServerConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_server_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = vec![alpn];

    let mut transport_config = TransportConfig::default();
    transport_config
        .max_idle_timeout(Some(MAX_IDLE_TIMEOUT))
        .expect("idle timeout is in vetted range");

    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(transport_config);

    Ok(quic_config)
}
