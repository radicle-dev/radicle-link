// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    net::{IpAddr, SocketAddr},
};

use crate::peer::PeerId;
use futures::io::{AsyncRead, AsyncWrite};

pub trait LocalInfo {
    type Addr;

    fn local_peer_id(&self) -> PeerId;
    fn local_addr(&self) -> io::Result<Self::Addr>;
}

pub trait RemoteInfo {
    type Addr;

    fn remote_peer_id(&self) -> PeerId;
    fn remote_addr(&self) -> Self::Addr;
}

pub trait AsAddr<A> {
    fn as_addr(&self) -> A;
}

impl AsAddr<IpAddr> for SocketAddr {
    fn as_addr(&self) -> IpAddr {
        self.ip()
    }
}

pub trait Stream: RemoteInfo + AsyncRead + AsyncWrite + Unpin + Send + Sync + Sized {
    type Read;
    type Write;

    fn split(self) -> (Self::Read, Self::Write);
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum CloseReason {
    DuplicateConnection = 1,
    ProtocolDisconnect = 2,
    ConnectionError = 3,
    InternalError = 4,
    ServerShutdown = 5,
    InvalidUpgrade = 6,
}

impl CloseReason {
    pub fn reason_phrase(&self) -> &[u8] {
        match self {
            Self::DuplicateConnection => b"duplicate connection",
            Self::ProtocolDisconnect => b"bye!",
            Self::ConnectionError => b"connection error",
            Self::InternalError => b"internal server error",
            Self::ServerShutdown => b"server shutdown",
            Self::InvalidUpgrade => b"invalid or unsupported protocol upgrade",
        }
    }
}

pub trait Closable {
    fn close(self, reason: CloseReason);
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;

    use std::pin::Pin;

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

    impl RemoteInfo for MockStream {
        type Addr = PeerId;

        fn remote_peer_id(&self) -> PeerId {
            self.id
        }

        fn remote_addr(&self) -> Self::Addr {
            self.id
        }
    }

    impl Stream for MockStream {
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
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            AsyncWrite::poll_write(Pin::new(&mut self.get_mut().inner), cx, buf)
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
            AsyncWrite::poll_flush(Pin::new(&mut self.get_mut().inner), cx)
        }

        fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
            AsyncWrite::poll_close(Pin::new(&mut self.get_mut().inner), cx)
        }
    }
}
