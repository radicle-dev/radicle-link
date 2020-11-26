// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::peer::PeerId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PutResult<Update> {
    Applied(Update),
    Stale,
    Uninteresting,
    Error,
}

#[async_trait]
pub trait LocalStorage: Clone + Send + Sync {
    type Update;

    /// Notify the local storage that a new value is available.
    ///
    /// If the value was stored locally already, [`PutResult::Stale`] must be
    /// returned. Otherwise, [`PutResult::Applied`] indicates that we _now_
    /// have the value locally, and other peers may fetch it from us.
    ///
    /// [`PutResult::Error`] indicates that a storage error occurred -- either
    /// the implementer wasn't able to determine if the local storage is
    /// up-to-date, or it was not possible to fetch the actual state from
    /// the `provider`. In this case, the network is asked to retransmit
    /// [`Self::Update`], so we can eventually try again.
    async fn put(&self, provider: PeerId, has: Self::Update) -> PutResult<Self::Update>;

    /// Ask the local storage if value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    async fn ask(&self, want: Self::Update) -> bool;
}
