use crate::{net::gossip::rpc::Update, peer::PeerId};

#[derive(Debug)]
pub enum PutResult {
    Applied,
    Stale,
    Uninteresting,
    Error,
}

pub trait LocalStorage: Clone + Send + Sync {
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
    /// [`Gossip::Have`], so we can eventually try again.
    ///
    /// [`Gossip::Have`]: ../rpc/enum.Gossip.html
    fn put(&self, provider: &PeerId, has: Update) -> PutResult;

    /// Ask the local storage if value `A` is available.
    ///
    /// This is used to notify the asking peer that they may fetch value `A`
    /// from us.
    fn ask(&self, want: &Update) -> bool;
}
