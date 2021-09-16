// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("remote PeerId could not be determined")]
    RemoteIdUnavailable,

    #[error("connect to self")]
    SelfConnect,

    #[error("endpoint is shutting down")]
    Shutdown,

    #[error(transparent)]
    PeerId(#[from] crypto::peer::conversion::Error),

    #[error("signer error")]
    Signer(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Endpoint(#[from] quinn::EndpointError),

    #[error(transparent)]
    Connect(#[from] quinn::ConnectError),

    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Connection {
    #[error(transparent)]
    Connection(#[from] quinn::ConnectionError),
}
