// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    net::connection::{Duplex, RemoteAddr, RemotePeer},
    peer::PeerId,
};

use std::{io, pin::Pin};

use futures::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadHalf, WriteHalf},
    task::{Context, Poll},
};
use futures_ringbuf::Endpoint;

pub struct MockStream {
    id: PeerId,
    inner: Endpoint,
}

impl MockStream {
    pub fn pair(id1: PeerId, id2: PeerId, bufsize: usize) -> (MockStream, MockStream) {
        let (a, b) = Endpoint::pair(bufsize, bufsize);
        (
            MockStream { id: id1, inner: a },
            MockStream { id: id2, inner: b },
        )
    }
}

impl RemotePeer for MockStream {
    fn remote_peer_id(&self) -> PeerId {
        self.id
    }
}

impl RemoteAddr for MockStream {
    type Addr = PeerId;

    fn remote_addr(&self) -> Self::Addr {
        self.id
    }
}

impl Duplex for MockStream {
    type Read = ReadHalf<Endpoint>;
    type Write = WriteHalf<Endpoint>;

    fn split(self) -> (Self::Read, Self::Write) {
        AsyncReadExt::split(self.inner)
    }
}

impl AsyncRead for MockStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().inner), cx, buf)
    }
}

impl AsyncWrite for MockStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.get_mut().inner), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().inner), cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_close(Pin::new(&mut self.get_mut().inner), cx)
    }
}
