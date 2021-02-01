// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, io};

use thiserror::Error;

use super::graft;
use crate::{
    git::storage::pool::PoolError,
    net::{
        codec::{CborCodecError, CborError},
        quic,
        upgrade,
    },
};

mod internal;
pub(super) use internal::*;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Bootstrap {
    #[error(transparent)]
    Graft(#[from] graft::error::State),

    #[error(transparent)]
    Pool(#[from] PoolError),

    #[error(transparent)]
    Quic(#[from] quic::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GraftInitiate {
    #[error("lost contact to deep space 9")]
    Unavailable,

    #[error(transparent)]
    Graft(#[from] graft::error::Offer),

    #[error("invalid bloom filter: {0}")]
    Bloom(&'static str),

    #[error(transparent)]
    Upgrade(#[from] upgrade::Error<quic::BidiStream>),

    #[error(transparent)]
    Quic(#[from] quic::Error),

    #[error(transparent)]
    Cbor(#[from] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for GraftInitiate {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GraftReset {
    #[error("lost contact to deep space 9")]
    Unavailable,

    #[error(transparent)]
    Graft(#[from] graft::error::State),

    #[error(transparent)]
    Pool(#[from] PoolError),
}
