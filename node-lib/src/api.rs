// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{sync::Arc, time::Duration};

use tracing::instrument;

use librad::{
    crypto::Signer,
    net::{peer::Peer, protocol::RequestPullGuard},
};
use link_async::Spawner;

pub use sockets::Sockets;

pub mod announce;
pub mod client;
pub mod io;
pub mod messages;
pub mod request_pull;
mod rpc;
pub mod sockets;
pub mod wire_types;

#[instrument(name = "api subroutine", skip(spawner, peer, sockets))]
pub async fn routine<'a, S, G>(
    spawner: Arc<Spawner>,
    peer: Peer<S, G>,
    sockets: &'a Sockets,
    linger_timeout: Option<Duration>,
    announce_wait_time: Duration,
) -> ()
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    let tasks = Box::pin(rpc::tasks(spawner, peer, sockets.rpc(), announce_wait_time));
    if let Some(timeout) = linger_timeout {
        link_async::tasks::run_until_idle(tasks, timeout).await
    } else {
        link_async::tasks::run_forever(tasks).await
    }
}
