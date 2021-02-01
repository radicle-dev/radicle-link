// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::identities;

use crate::{existing, fork, include, new};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    New(#[from] new::Error),

    #[error(transparent)]
    Existing(#[from] existing::Error),

    #[error(transparent)]
    Fork(#[from] fork::Error),

    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Include(#[from] include::Error),
}
