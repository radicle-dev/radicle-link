// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use super::{info::PeerAdvertisement, tick::Tock};
use crate::PeerId;

pub mod error;
pub use error::Error;

mod hpv;
pub(super) use hpv::TnT;
pub use hpv::{Hpv, Shuffle};

mod params;
pub use params::Params;

mod partial_view;
pub use partial_view::{PartialView, Transition};

mod periodic;
pub use periodic::Periodic;

mod rpc;
pub use rpc::Message;

mod tick;
pub use tick::Tick;

#[allow(clippy::type_complexity)] // get off my lawn, glibbi!
pub(super) fn apply<R, A, F, P>(
    hpv: &Hpv<R, A>,
    info: F,
    remote_id: PeerId,
    remote_addr: A,
    message: Message<A>,
) -> Result<(Vec<Transition<A>>, Vec<Tock<A, P>>), Error>
where
    R: rand::Rng + Clone,
    A: Clone + Debug + PartialEq,
    F: Fn() -> PeerAdvertisement<A>,
{
    hpv.apply(remote_id, remote_addr, message)
        .map(|TnT { trans, ticks }| {
            (
                trans,
                ticks
                    .into_iter()
                    .flat_map(|tick| collect_tocks(hpv, &info, tick))
                    .collect(),
            )
        })
}

pub(super) fn collect_tocks<R, A, F, P>(hpv: &Hpv<R, A>, info: F, tick: Tick<A>) -> Vec<Tock<A, P>>
where
    R: rand::Rng + Clone,
    A: Clone + Debug + PartialEq,
    F: Fn() -> PeerAdvertisement<A>,
{
    use Tick::*;
    use Tock::*;

    let mut tocks = Vec::new();
    match tick {
        Forget { peer } => {
            tocks.push(Disconnect { peer });
        },

        Connect { to } => {
            let message = hpv.hello(info()).into();
            tocks.push(AttemptSend { to, message });
        },

        Reply { to, message } => tocks.push(SendConnected {
            to,
            message: message.into(),
        }),

        Try { recipient, message } => tocks.push(AttemptSend {
            to: recipient,
            message: message.into(),
        }),

        All {
            recipients,
            message,
        } => {
            for recipient in recipients {
                tocks.push(SendConnected {
                    to: recipient,
                    message: message.clone().into(),
                })
            }
        },
    }

    tocks
}
