use std::{io, net::SocketAddr, pin::Pin};

use failure::Error;
use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::{BoxStream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::{NewConnection, VarInt};

use crate::{
    keys::device,
    net::{quic, tls},
    peer::PeerId,
};

pub trait LocalInfo {
    fn local_addr(&self) -> Result<SocketAddr, Error>;
}

pub trait RemoteInfo {
    fn peer_id(&self) -> &PeerId;
    fn remote_addr(&self) -> SocketAddr;
}

#[derive(Clone)]
pub struct Endpoint {
    endpoint: quinn::Endpoint,
}

impl Endpoint {
    pub async fn bind<'a>(
        local_key: &device::Key,
        listen_addr: SocketAddr,
    ) -> Result<BoundEndpoint<'a>, Error> {
        let (endpoint, incoming) = quic::make_endpoint(local_key, listen_addr).await?;
        let endpoint = Endpoint { endpoint };
        let incoming = incoming
            .filter_map(|connecting| async move { connecting.await.ok().map(new_connection) })
            .boxed();

        Ok(BoundEndpoint { endpoint, incoming })
    }

    pub async fn connect<'a>(
        &mut self,
        peer: &PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, BoxStream<'a, Result<Stream, Error>>), Error> {
        let conn = self
            .endpoint
            .connect(addr, peer.as_dns_name().as_ref().into())?
            .await?;

        Ok(new_connection(conn))
    }
}

impl LocalInfo for Endpoint {
    fn local_addr(&self) -> Result<SocketAddr, Error> {
        self.endpoint.local_addr().map_err(|e| e.into())
    }
}

pub struct BoundEndpoint<'a> {
    pub endpoint: Endpoint,
    pub incoming: BoxStream<'a, (Connection, BoxStream<'a, Result<Stream, Error>>)>,
}

impl<'a> LocalInfo for BoundEndpoint<'a> {
    fn local_addr(&self) -> Result<SocketAddr, Error> {
        self.endpoint.local_addr()
    }
}

fn new_connection<'a>(
    NewConnection {
        connection,
        bi_streams,
        ..
    }: NewConnection,
) -> (Connection, BoxStream<'a, Result<Stream, Error>>) {
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
                    send,
                    recv,
                })
                .map_err(|e| e.into()),
        ),
    )
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

    pub async fn open_stream(&self) -> Result<Stream, Error> {
        let (send, recv) = self.conn.open_bi().await?;
        Ok(Stream {
            conn: self.clone(),
            recv,
            send,
        })
    }

    pub fn close(self, reason: CloseReason) {
        let code = VarInt::from_u32(reason.clone() as u32);
        self.conn.close(code, reason.reason().as_bytes())
    }
}

impl RemoteInfo for Connection {
    fn peer_id(&self) -> &PeerId {
        &self.peer
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }
}

#[derive(Clone)]
pub enum CloseReason {
    DuplicateConnection = 1,
    ProtocolDisconnect = 2,
    ConnectionError = 3,
    InternalError = 4,
}

impl CloseReason {
    fn reason(&self) -> &str {
        match self {
            Self::DuplicateConnection => "duplicate connection",
            Self::ProtocolDisconnect => "bye!",
            Self::ConnectionError => "connection error",
            Self::InternalError => "internal server error",
        }
    }
}

pub struct Stream {
    conn: Connection,
    recv: quinn::RecvStream,
    send: quinn::SendStream,
}

impl Stream {
    pub fn framed<C>(self, codec: C) -> Framed<Self, C>
    where
        C: Decoder + Encoder,
    {
        Framed::new(self, codec)
    }

    pub fn split(self) -> (RecvStream, SendStream) {
        (
            RecvStream {
                conn: self.conn.clone(),
                recv: self.recv,
            },
            SendStream {
                conn: self.conn,
                send: self.send,
            },
        )
    }
}

impl RemoteInfo for Stream {
    fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        quinn::RecvStream::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        quinn::SendStream::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        quinn::SendStream::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        quinn::SendStream::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}

pub struct RecvStream {
    conn: Connection,
    recv: quinn::RecvStream,
}

impl RemoteInfo for RecvStream {
    fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
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
    ) -> Poll<Result<usize, io::Error>> {
        quinn::RecvStream::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

pub struct SendStream {
    conn: Connection,
    send: quinn::SendStream,
}

impl RemoteInfo for SendStream {
    fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
    }

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl AsyncWrite for SendStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        quinn::SendStream::poll_write(Pin::new(&mut self.get_mut().send), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        quinn::SendStream::poll_flush(Pin::new(&mut self.get_mut().send), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        quinn::SendStream::poll_close(Pin::new(&mut self.get_mut().send), cx)
    }
}
