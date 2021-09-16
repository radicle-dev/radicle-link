// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::net::quic;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Accept {
    #[error("endpoint shut down")]
    Done,

    #[error(transparent)]
    Quic(#[from] quic::Error),
}
