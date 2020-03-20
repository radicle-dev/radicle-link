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

use std::{io, net::SocketAddr};

use crate::peer::PeerId;
use futures::io::{AsyncRead, AsyncWrite};

pub trait LocalInfo {
    fn local_peer_id(&self) -> &PeerId;
    fn local_addr(&self) -> io::Result<SocketAddr>;
}

pub trait RemoteInfo {
    fn remote_peer_id(&self) -> &PeerId;
    fn remote_addr(&self) -> SocketAddr;
}

pub trait Stream: RemoteInfo + AsyncRead + AsyncWrite + Unpin + Send + Sync + Sized {
    type Read;
    type Write;

    fn split(self) -> (Self::Read, Self::Write);
}

#[derive(Clone)]
pub enum CloseReason {
    DuplicateConnection = 1,
    ProtocolDisconnect = 2,
    ConnectionError = 3,
    InternalError = 4,
}

impl CloseReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::DuplicateConnection => "duplicate connection",
            Self::ProtocolDisconnect => "bye!",
            Self::ConnectionError => "connection error",
            Self::InternalError => "internal server error",
        }
    }
}
