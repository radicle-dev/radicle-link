use std::{io, net::SocketAddr, pin::Pin};

use failure::Error;
use futures::{
    io::{AsyncRead, AsyncWrite},
    task::{Context, Poll},
};
use futures_codec::{Decoder, Encoder, Framed};
use quinn::VarInt;

use crate::peer::PeerId;

pub type IncomingStreams = quinn::IncomingBiStreams;

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
        Ok(Stream { recv, send })
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
    recv: quinn::RecvStream,
    send: quinn::SendStream,
}

impl Stream {
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

impl From<(quinn::SendStream, quinn::RecvStream)> for Stream {
    fn from((send, recv): (quinn::SendStream, quinn::RecvStream)) -> Self {
        Self { recv, send }
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
