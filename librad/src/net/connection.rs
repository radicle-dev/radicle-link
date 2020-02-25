use std::{io, net::SocketAddr, pin::Pin};

use failure::Error;
use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::StreamExt,
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::{NewConnection, VarInt};

use crate::{
    keys::device,
    net::{quic, tls},
    peer::PeerId,
};

pub type IncomingStreams = quinn::IncomingBiStreams;

#[derive(Clone)]
pub struct Endpoint {
    endpoint: quinn::Endpoint,
}

impl Endpoint {
    pub fn new(
        local_key: &device::Key,
        listen_addr: &SocketAddr,
    ) -> Result<
        (
            Self,
            impl futures::Stream<Item = (Connection, IncomingStreams)>,
        ),
        Error,
    > {
        let (endpoint, incoming) = quic::make_endpoint(local_key, listen_addr)?;

        let incoming = incoming.filter_map(|connecting| async move {
            connecting.await.ok().and_then(
                |NewConnection {
                     connection,
                     bi_streams,
                     ..
                 }| {
                    let cert = &connection
                        .presented_certs()
                        .expect("Certificates must be presented. qed")[0];
                    let peer_id = tls::extract_peer_id(cert.as_der())
                        .expect("TLS layer ensures the cert contains a PeerId. qed");

                    Some((Connection::new(&peer_id, connection), bi_streams))
                },
            )
        });

        Ok((Self { endpoint }, incoming))
    }

    pub async fn connect(
        &mut self,
        peer: &PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, IncomingStreams), Error> {
        let NewConnection {
            connection,
            bi_streams,
            ..
        } = self
            .endpoint
            .connect(addr, &format!("{}.radicle", peer))?
            .await?;

        Ok((Connection::new(peer, connection), bi_streams))
    }
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
            peer: self.peer.clone(),
            recv,
            send,
        })
    }

    pub fn close(&self, reason: CloseReason) {
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
    peer: PeerId,
    recv: quinn::RecvStream,
    send: quinn::SendStream,
}

impl Stream {
    pub fn new(peer: PeerId, recv: quinn::RecvStream, send: quinn::SendStream) -> Self {
        Self { peer, recv, send }
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer
    }

    pub fn recv(self) -> impl AsyncRead {
        self.recv
    }

    pub fn send(self) -> impl AsyncWrite {
        self.send
    }

    pub fn framed<C>(self, codec: C) -> Framed<Self, C>
    where
        C: Decoder + Encoder,
    {
        Framed::new(self, codec)
    }
}

impl Into<(quinn::RecvStream, quinn::SendStream)> for Stream {
    fn into(self) -> (quinn::RecvStream, quinn::SendStream) {
        (self.recv, self.send)
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let recv = Pin::new(&mut self.get_mut().recv);
        quinn::RecvStream::poll_read(recv, cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let send = Pin::new(&mut self.get_mut().send);
        quinn::SendStream::poll_write(send, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let send = Pin::new(&mut self.get_mut().send);
        quinn::SendStream::poll_flush(send, cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let send = Pin::new(&mut self.get_mut().send);
        quinn::SendStream::poll_close(send, cx)
    }
}
