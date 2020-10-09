// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::peer::PeerId;

#[derive(Clone, Copy, Debug)]
pub enum PutResult {
    Applied,
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
    async fn put(&self, provider: PeerId, has: Self::Update) -> PutResult;

    /// Ask the local storage if value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    async fn ask(&self, want: Self::Update) -> bool;
}
