// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use thiserror::Error;

use super::{gossip, quic, upgrade};
use crate::{
    net::codec::{CborCodecError, CborError},
    peer::PeerId,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("no connection to {0}")]
    NoConnection(PeerId),

    #[error("duplicate connection")]
    DuplicateConnection,

    #[error("unsupported upgrade requested")]
    UnsupportedUpgrade,

    #[error(transparent)]
    Upgrade(#[from] upgrade::ErrorSource),

    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error("error handling gossip upgrade")]
    Gossip(#[from] gossip::error::Error),

    #[error("error handling git upgrade")]
    Git(#[source] io::Error),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(e: CborCodecError) -> Self {
        use CborCodecError::*;

        match e {
            Cbor(e) => Self::Cbor(e),
            Io(e) => Self::Io(e),
        }
    }
}
