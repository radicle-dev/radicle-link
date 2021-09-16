// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{partial_view::Transition, rpc::Message};
use crate::{net::protocol::info::PeerInfo, PeerId};

#[derive(Debug)]
pub enum Tick<Addr> {
    /// Deliver `message` to all `recipients`.
    ///
    /// Failed recipients must be evicted from the active view.
    All {
        recipients: Vec<PeerId>,
        message: Message<Addr>,
    },

    /// Deliver `message`.
    ///
    /// If delivery fails, the peer must be evicted from the active view.
    Reply { to: PeerId, message: Message<Addr> },

    /// Attempt to deliver `message` to `recipient`.
    ///
    /// Delivery may fail, in which case no further action is required.
    Try {
        recipient: PeerInfo<Addr>,
        message: Message<Addr>,
    },

    /// Attempt to connect.
    ///
    /// If successful, a `Join` or `Neighbour` message must be sent, depending
    /// on the connectedness.
    Connect { to: PeerInfo<Addr> },

    /// `peer` was completely evicted from the partial view. It is safe to
    /// close any connections.
    Forget { peer: PeerId },
}

impl<A> From<Transition<A>> for Option<Tick<A>> {
    fn from(t: Transition<A>) -> Self {
        use Tick::*;
        use Transition::*;

        match t {
            Demoted(info) => Some(Try {
                recipient: info,
                message: Message::Disconnect,
            }),
            Evicted(info) => Some(Forget { peer: info.peer_id }),
            _ => None,
        }
    }
}
