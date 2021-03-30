// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::{self, Debug},
    io,
};

use thiserror::Error;

use crate::{
    net::{
        codec::{CborCodecError, CborError},
        protocol::{membership, PeerInfo},
        quic,
        upgrade,
    },
    PeerId,
};

#[derive(Debug, Error)]
pub enum Gossip {
    #[error(transparent)]
    Membership(#[from] membership::Error),

    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for Gossip {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}

#[derive(Debug, Error)]
pub enum Tock<A: Clone + Ord + Debug + 'static> {
    #[error(transparent)]
    Reliable(#[from] ReliableSend<A>),

    #[error(transparent)]
    Unreliable(#[from] BestEffortSend<A>),
}

#[derive(Debug, Error)]
#[error("reliable send failed")]
pub struct ReliableSend<A: Clone + Ord + Debug + 'static> {
    pub cont: Vec<membership::Tick<A>>,
    pub source: ReliableSendSource,
}

#[derive(Debug, Error)]
pub enum ReliableSendSource {
    #[error("no connection to {to}")]
    NotConnected { to: PeerId },

    #[error(transparent)]
    SendGossip(#[from] Rpc<quic::SendStream>),
}

#[derive(Debug, Error)]
pub enum BestEffortSend<A: Clone + Ord + Debug + 'static> {
    #[error("could not connect to {}", to.peer_id)]
    CouldNotConnect { to: PeerInfo<A> },

    #[error(transparent)]
    SendGossip(#[from] Rpc<quic::SendStream>),
}

#[derive(Error)]
pub enum Rpc<S: 'static> {
    #[error(transparent)]
    Upgrade(#[from] upgrade::Error<S>),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

// As per usual, the derive macro generates too strict bounds on the type
// parameter
impl<S> Debug for Rpc<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Upgrade(x) => f.debug_tuple("Upgrade").field(x).finish(),
            Self::Quic(x) => f.debug_tuple("Quic").field(x).finish(),
            Self::Cbor(x) => f.debug_tuple("Cbor").field(x).finish(),
            Self::Io(x) => f.debug_tuple("Io").field(x).finish(),
        }
    }
}

impl<S> From<CborCodecError> for Rpc<S> {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}
