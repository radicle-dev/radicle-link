// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};

// Copied from async_std::os::unix::net::Incoming
pub struct Incoming<'a, P: PollSocket>(&'a P);

impl<'a, P: PollSocket> futures::stream::Stream for Incoming<'a, P> {
    type Item = std::io::Result<P::Socket>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.0.poll_accept(cx) {
            Poll::Ready(Ok((socket, _))) => Poll::Ready(Some(Ok(socket))),
            Poll::Ready(Err(e)) => {
                tracing::error!(err=?e, "error accepting socket");
                Poll::Ready(None)
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

pub trait PollSocket {
    type Socket;
    type SockAddr;

    fn poll_accept(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<(Self::Socket, Self::SockAddr)>>;
}

impl PollSocket for UnixListener {
    type Socket = UnixStream;
    type SockAddr = tokio::net::unix::SocketAddr;

    fn poll_accept(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<(Self::Socket, Self::SockAddr)>> {
        self.poll_accept(cx)
    }
}

pub trait UnixListenerExt {
    fn incoming(&self) -> Incoming<'_, UnixListener>;
}

impl UnixListenerExt for UnixListener {
    fn incoming(&self) -> Incoming<'_, UnixListener> {
        Incoming(self)
    }
}

impl PollSocket for TcpListener {
    type Socket = TcpStream;
    type SockAddr = std::net::SocketAddr;

    fn poll_accept(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<(Self::Socket, Self::SockAddr)>> {
        self.poll_accept(cx)
    }
}

pub trait TcpListenerExt {
    fn incoming(&self) -> Incoming<'_, TcpListener>;
}

impl TcpListenerExt for TcpListener {
    fn incoming(&self) -> Incoming<'_, TcpListener> {
        Incoming(self)
    }
}
