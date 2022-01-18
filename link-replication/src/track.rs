// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either;

use crate::{PeerId, Urn};

/// Tracking relationship.
///
/// The variants help [`Tracking`] impls to determine the appropriate rfc699
/// configuration.
#[derive(Clone, Debug)]
pub enum Rel<Urn> {
    /// [`Urn`] or [`PeerId`] of a delegation.
    Delegation(Either<PeerId, Urn>),
    /// `rad/self` identity.
    SelfRef(Urn),
}

pub trait Tracking {
    type Urn: Urn;

    type Updated: Iterator<Item = Either<PeerId, Self::Urn>>;
    type Tracked: Iterator<Item = Result<PeerId, Self::TrackedError>>;

    type TrackError: std::error::Error + Send + Sync + 'static;
    type TrackedError: std::error::Error + Send + Sync + 'static;

    /// Atomically create tracking relationships.
    fn track<I>(&mut self, iter: I) -> Result<Self::Updated, Self::TrackError>
    where
        I: IntoIterator<Item = Rel<Self::Urn>>;

    /// All tracked [`PeerId`]s in the context of the current [`Urn`].
    fn tracked(&self) -> Result<Self::Tracked, Self::TrackedError>;
}
