// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use thiserror::Error;

use crate::net::codec::{CborCodecError, CborError};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("connection to self")]
    SelfConnection,

    #[error("too many storage errors")]
    StorageErrorRateLimitExceeded,

    #[error("protocol violation: {0}")]
    ProtocolViolation(&'static str),

    #[error("CBOR encoding/decoding error")]
    Cbor(#[source] CborError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<CborCodecError> for Error {
    fn from(e: CborCodecError) -> Self {
        match e {
            CborCodecError::Cbor(e) => Self::Cbor(e),
            CborCodecError::Io(e) => Self::Io(e),
        }
    }
}
