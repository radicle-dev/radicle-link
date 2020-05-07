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

use std::{io, net::SocketAddr, pin::Pin, sync::Arc};

use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::{BoxStream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::{self, NewConnection, VarInt};
use thiserror::Error;

use crate::{
    keys::device,
    net::{
        connection::{self, CloseReason, LocalInfo, RemoteInfo},
        tls,
    },
    peer::PeerId,
};

#[derive(Debug, Error)]
pub enum Error {
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
    pub async fn bind<'a>(
        local_key: &device::Key,
        listen_addr: SocketAddr,
    ) -> Result<BoundEndpoint<'a>> {
        let (endpoint, incoming) = make_endpoint(local_key, listen_addr).await?;
        let endpoint = Endpoint {
            peer_id: PeerId::from(local_key),
            endpoint,
        };
        let incoming = incoming
            .filter_map(|connecting| async move { connecting.await.ok().map(new_connection) })
            .boxed();

        Ok(BoundEndpoint { endpoint, incoming })
    }

    pub async fn connect<'a>(
        &mut self,
        peer: &PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, BoxStream<'a, Result<Stream>>)> {
        let conn = self
            .endpoint
            .connect(addr, peer.as_dns_name().as_ref().into())?
            .await?;

        Ok(new_connection(conn))
    }
}

impl LocalInfo for Endpoint {
    type Addr = SocketAddr;

    fn local_peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }
}

pub type Incoming<'a> = BoxStream<'a, Result<Stream>>;

pub struct BoundEndpoint<'a> {
    pub endpoint: Endpoint,
    pub incoming: BoxStream<'a, (Connection, Incoming<'a>)>,
}

impl<'a> LocalInfo for BoundEndpoint<'a> {
    type Addr = SocketAddr;

    fn local_peer_id(&self) -> &PeerId {
        self.endpoint.local_peer_id()
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }
}

fn new_connection<'a>(
    NewConnection {
        connection,
        bi_streams,
        ..
    }: NewConnection,
) -> (Connection, Incoming<'a>) {
    let peer_id = tls::extract_peer_id(
        connection
            .authentication_data()
            .peer_certificates
            .expect("Certificates must be presented. qed")
            .iter()
            .next()
            .expect("One certificate must have been presented. qed")
            .as_ref(),
    )
    .expect("TLS layer ensures the cert contains a PeerId. qed");

    let conn = Connection::new(&peer_id, connection);

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

#[derive(Debug, Error)]
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
    pub fn new(peer: &PeerId, conn: quinn::Connection) -> Self {
        Self {
            peer: peer.clone(),
            conn,
        }
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
        let code = VarInt::from_u32(reason.clone() as u32);
        self.conn.close(code, reason.as_str().as_bytes())
    }
}

impl RemoteInfo for Connection {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> &PeerId {
        &self.peer
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
}

impl RemoteInfo for Stream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> &PeerId {
        &self.conn.remote_peer_id()
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

impl RemoteInfo for RecvStream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> &PeerId {
        &self.conn.remote_peer_id()
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

impl RemoteInfo for SendStream {
    type Addr = SocketAddr;

    fn remote_peer_id(&self) -> &PeerId {
        &self.conn.remote_peer_id()
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
async fn make_endpoint(
    key: &device::Key,
    listen_addr: SocketAddr,
) -> std::result::Result<(quinn::Endpoint, quinn::Incoming), quinn::EndpointError> {
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(key));
    builder.listen(make_server_config(key));

    builder.bind(&listen_addr)
}

fn make_client_config(key: &device::Key) -> quinn::ClientConfig {
    let mut quic_config = quinn::ClientConfigBuilder::default().build();
    let tls_config = Arc::new(tls::make_client_config(key));
    quic_config.crypto = tls_config;

    quic_config
}

fn make_server_config(key: &device::Key) -> quinn::ServerConfig {
    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    let tls_config = Arc::new(tls::make_server_config(key));
    quic_config.crypto = tls_config;

    quic_config
}
