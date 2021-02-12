// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::identities;

use crate::{
    garden::{graft, plant, repot},
    include,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Plant(#[from] plant::Error),

    #[error(transparent)]
    Repot(#[from] repot::Error),

    #[error(transparent)]
    Graft(#[from] graft::Error),

    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Include(#[from] include::Error),
}
