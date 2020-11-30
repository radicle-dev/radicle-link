// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use thiserror::Error;

use crate::net::codec::CborCodecError;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("connection to self")]
    SelfConnection,

    #[error("too many storage errors")]
    StorageErrorRateLimitExceeded,

    #[error(transparent)]
    Cbor(#[from] CborCodecError),

    #[error(transparent)]
    Io(#[from] io::Error),
}
