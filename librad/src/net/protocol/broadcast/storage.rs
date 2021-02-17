// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::peer::PeerId;

/// Result of applying a broadcast update to local storage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PutResult<Update> {
    /// The `Update` was not previously seen, and applied successfully.
    ///
    /// The `Update` may be subject to transformations, e.g. to adjust origin
    /// information. The payload of `Applied` will be used for further
    /// broadcasting.
    ///
    /// Since the data is now available from the local peer, the `origin` value
    /// of the gossip [`super::Message`] will be modified to point to the
    /// local peer.
    Applied(Update),

    /// The `Update` has already been applied previously.
    ///
    /// Broadcast will terminate here.
    Stale,

    /// The `Update` is not interesting, typically because there is no tracking
    /// relationship.
    ///
    /// The `Update` will be relayed, while the `origin` of the
    /// [`super::Message`] stays unmodified.
    Uninteresting,

    /// An (intermittent) error occurred while trying to apply the `Update`.
    ///
    /// The `Update` will be relayed, while the `origin` of the
    /// [`super::Message`] stays unmodified. Additionally, the local peer
    /// may ask for a retransmission of the `Update` at a later point.
    Error,
}

#[async_trait]
pub trait LocalStorage<Addr>: Clone + Send + Sync {
    /// The payload value of the broadcast message.
    ///
    /// Corresponds to `val` of [`super::Message`].
    type Update;

    /// Notify the local storage that a new value is available.
    ///
    /// The `provider` corresponds to the `origin` of [`super::Message::Have`].
    async fn put<P>(&self, provider: P, has: Self::Update) -> PutResult<Self::Update>
    where
        P: Into<(PeerId, Vec<Addr>)> + Send;

    /// Ask the local storage if value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    async fn ask(&self, want: Self::Update) -> bool;
}
