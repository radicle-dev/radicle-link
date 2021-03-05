// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, io};

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
    SendGossip(#[from] SendGossip),
}

#[derive(Debug, Error)]
pub enum BestEffortSend<A: Clone + Ord + Debug + 'static> {
    #[error("could not connect to {}", to.peer_id)]
    CouldNotConnect { to: PeerInfo<A> },

    #[error(transparent)]
    SendGossip(#[from] SendGossip),
}

#[derive(Debug, Error)]
pub enum SendGossip {
    #[error(transparent)]
    Upgrade(#[from] upgrade::Error<quic::SendStream>),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for SendGossip {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}
