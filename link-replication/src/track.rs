// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::{PeerId, Urn};

pub trait Tracking {
    type Urn: Urn;
    type Tracked: Iterator<Item = Result<PeerId, Self::TrackedError>>;

    type TrackError: std::error::Error + Send + Sync + 'static;
    type TrackedError: std::error::Error + Send + Sync + 'static;

    /// Track `id` in the context of `urn`, or the current [`Urn`].
    ///
    /// Return `true` if the tracking relationship did not exist and was
    /// created, `false` otherwise.
    fn track(&mut self, id: &PeerId, urn: Option<&Self::Urn>) -> Result<bool, Self::TrackError>;

    /// All tracked [`PeerId`]s in the context of the current [`Urn`].
    fn tracked(&self) -> Result<Self::Tracked, Self::TrackedError>;
}
