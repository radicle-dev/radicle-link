use std::{io, net::SocketAddr, pin::Pin};

use failure::Error;
use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::{BoxStream, StreamExt},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use log::warn;
use quinn::{NewConnection, VarInt};

use crate::{
    keys::device,
    net::{quic, tls},
    peer::PeerId,
};

#[derive(Clone)]
pub struct Endpoint {
    endpoint: quinn::Endpoint,
}

impl Endpoint {
    pub fn new<'a>(
        local_key: &device::Key,
        listen_addr: &SocketAddr,
    ) -> Result<
        (
            Self,
            impl futures::Stream<Item = (Connection, BoxStream<'a, Stream>)>,
        ),
        Error,
    > {
        let (endpoint, incoming) = quic::make_endpoint(local_key, listen_addr)?;
        let incoming = incoming
            .filter_map(|connecting| async move { connecting.await.ok().map(new_connection) });

        Ok((Self { endpoint }, incoming))
    }

    pub async fn connect<'a>(
        &mut self,
        peer: &PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, BoxStream<'a, Stream>), Error> {
        let conn = self
            .endpoint
            .connect(addr, peer.as_dns_name().as_ref().into())?
            .await?;

        Ok(new_connection(conn))
    }
}

fn new_connection<'a>(
    NewConnection {
        connection,
        bi_streams,
        ..
    }: NewConnection,
) -> (Connection, BoxStream<'a, Stream>) {
    // Boy, that's how I like to move it move it
    let incoming_streams = move |peer_id: PeerId, conn: Connection| {
        bi_streams
            .filter_map(move |stream| {
                let peer_id1 = peer_id.clone();
                let conn1 = conn.clone();
                async move {
                    match stream {
                        Err(e) => {
                            warn!(
                                "Connection error in incoming stream from {}: {}",
                                peer_id1, e
                            );
                            None
                        },
                        Ok((send, recv)) => Some(Stream {
                            conn: conn1.clone(),
                            send,
                            recv,
                        }),
                    }
                }
            })
            .fuse()
    };

    let peer_id = {
        let cert = &connection
            .presented_certs()
            .expect("Certificates must be presented. qed")[0];
        tls::extract_peer_id(cert.as_der())
            .expect("TLS layer ensures the cert contains a PeerId. qed")
    };

    let conn = Connection::new(&peer_id, connection);

    (conn.clone(), Box::pin(incoming_streams(peer_id, conn)))
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

    pub fn peer_id(&self) -> &PeerId {
        &self.peer
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
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

#[derive(Clone)]
pub enum CloseReason {
    DuplicateConnection = 1,
    ProtocolDisconnect = 2,
    ConnectionError = 3,
}

impl CloseReason {
    fn reason(&self) -> &str {
        match self {
            Self::DuplicateConnection => "duplicate connection",
            Self::ProtocolDisconnect => "bye!",
            Self::ConnectionError => "connection error",
        }
    }
}

pub struct Stream {
    conn: Connection,
    recv: quinn::RecvStream,
    send: quinn::SendStream,
}

impl Stream {
    pub fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
    }

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

impl RecvStream {
    pub fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
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

impl SendStream {
    pub fn peer_id(&self) -> &PeerId {
        &self.conn.peer_id()
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.conn.remote_address()
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
