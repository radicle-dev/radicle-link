// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use thiserror::Error;

use crate::{git::storage::pool::PoolError, net::quic};

mod internal;
pub(super) use internal::*;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Bootstrap {
    #[error(transparent)]
    Pool(#[from] PoolError),

    #[error(transparent)]
    Quic(#[from] quic::Error),
}
