// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::Deref;

use futures::io::{AsyncRead, AsyncWrite};
use futures_codec::FramedWrite;

use crate::PeerId;

pub trait LocalPeer {
    fn local_peer_id(&self) -> PeerId;
}

pub trait LocalAddr {
    type Addr;

    fn listen_addrs(&self) -> Vec<Self::Addr>;
}

pub trait LocalInfo: LocalPeer + LocalAddr {}
impl<T> LocalInfo for T where T: LocalPeer + LocalAddr {}

pub trait RemotePeer {
    fn remote_peer_id(&self) -> PeerId;
}

impl<T, E> RemotePeer for FramedWrite<T, E>
where
    T: RemotePeer,
{
    fn remote_peer_id(&self) -> PeerId {
        self.deref().remote_peer_id()
    }
}

pub trait RemoteAddr {
    type Addr;

    fn remote_addr(&self) -> Self::Addr;
}

pub trait RemoteInfo: RemotePeer + RemoteAddr {}
impl<T> RemoteInfo for T where T: RemotePeer + RemoteAddr {}

pub trait Duplex: RemoteInfo + AsyncRead + AsyncWrite + Unpin + Send + Sync + Sized {
    type Read;
    type Write;

    fn split(self) -> (Self::Read, Self::Write);
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum CloseReason {
    ConnectionError = 3,
    ServerShutdown = 5,
    InvalidUpgrade = 6,
    TooManyConnections = 7,
    Timeout = 8,
}

impl CloseReason {
    pub fn reason_phrase(&self) -> &[u8] {
        match self {
            Self::ConnectionError => b"connection error",
            Self::ServerShutdown => b"server shutdown",
            Self::InvalidUpgrade => b"invalid or unsupported protocol upgrade",
            Self::TooManyConnections => b"too many connections",
            Self::Timeout => b"timeout",
        }
    }
}
