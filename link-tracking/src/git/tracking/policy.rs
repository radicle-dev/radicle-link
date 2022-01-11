// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use radicle_git_ext::Oid;

use super::refdb::PreviousValue;

#[derive(Clone, Copy, Debug)]
pub enum Track {
    /// Will attempt to create the tracking entry even if it already exists.
    Any,
    /// Will only attempt to create the tracking entry iff it did not already
    /// exist.
    MustNotExist,
    /// Will only attempt to create the tracking entry iff if already existed.
    /// This can be used to safely set the existing tracking entry to a new
    /// configuration.
    MustExist,
}

#[derive(Clone, Copy, Debug)]
pub enum Untrack {
    /// Will attempt to remove the tracking entry even if it did not already
    /// exist.
    Any,
    /// Will only attempt to remove the tracking entry iff it already existed.
    MustExist,
}

#[derive(Clone, Copy, Debug)]
pub enum UntrackAll {
    /// Will forcefully attempt to remove the tracking entries.
    Any,
    /// Will only attempt to remove the tracking entry iff they already existed
    /// *and* the targets read matched.
    MustExistAndMatch,
}

impl From<Track> for PreviousValue<Oid> {
    fn from(policy: Track) -> Self {
        match policy {
            Track::Any => Self::Any,
            Track::MustNotExist => Self::MustNotExist,
            Track::MustExist => Self::MustExist,
        }
    }
}

impl From<Untrack> for PreviousValue<Oid> {
    fn from(policy: Untrack) -> Self {
        match policy {
            Untrack::Any => Self::Any,
            Untrack::MustExist => Self::MustExist,
        }
    }
}

impl UntrackAll {
    pub(super) fn into_previous_value(self, oid: Oid) -> PreviousValue<Oid> {
        match self {
            Self::Any => PreviousValue::Any,
            Self::MustExistAndMatch => PreviousValue::MustExistAndMatch(oid),
        }
    }
}
