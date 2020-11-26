// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use futures::io::{AsyncRead, AsyncWrite};
use quinn::VarInt;

use super::Connection;
use crate::{
    net::connection::{CloseReason, Duplex, RemoteAddr, RemotePeer},
    PeerId,
};

pub struct BidiStream {
    pub(super) conn: Connection,
    pub(super) recv: RecvStream,
    pub(super) send: SendStream,
}

impl BidiStream {
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

impl RemoteAddr for BidiStream {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl Duplex for BidiStream {
    type Read = RecvStream;
    type Write = SendStream;

    fn split(self) -> (Self::Read, Self::Write) {
        (self.recv, self.send)
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
    pub(super) conn: Connection,
    pub(super) recv: quinn::RecvStream,
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

impl RemoteAddr for RecvStream {
    type Addr = SocketAddr;

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
        let this = self.get_mut();
        let res = AsyncRead::poll_read(Pin::new(&mut this.recv), cx, buf);

        if let Poll::Ready(ready) = &res {
            match ready {
                Err(e) => this.conn.on_stream_error(e),
                Ok(_) => this.conn.tickle(),
            }
        }

        res
    }
}

pub struct SendStream {
    pub(super) conn: Connection,
    pub(super) send: quinn::SendStream,
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

impl RemoteAddr for SendStream {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_addr()
    }
}

impl AsyncWrite for SendStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let res = AsyncWrite::poll_write(Pin::new(&mut this.send), cx, buf);

        if let Poll::Ready(ready) = &res {
            match ready {
                Err(e) => this.conn.on_stream_error(e),
                Ok(_) => this.conn.tickle(),
            }
        }

        res
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let res = AsyncWrite::poll_flush(Pin::new(&mut this.send), cx);

        if let Poll::Ready(ready) = &res {
            match ready {
                Err(e) => this.conn.on_stream_error(e),
                Ok(()) => this.conn.tickle(),
            }
        }

        res
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let res = AsyncWrite::poll_close(Pin::new(&mut this.send), cx);

        if let Poll::Ready(Err(e)) = &res {
            this.conn.on_stream_error(e)
        }

        res
    }
}
