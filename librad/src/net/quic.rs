// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::Duration;

mod connection;
pub use connection::{BoxedIncomingStreams, Connection, ConnectionId, Conntrack, IncomingStreams};

mod endpoint;
pub use endpoint::{BoundEndpoint, Endpoint, IncomingConnections};

pub mod error;
pub use error::{Error, Result};

mod stream;
pub use stream::{BidiStream, RecvStream, SendStream};

const ALPN_PREFIX: &[u8] = b"rad";

// XXX: we _may_ want to allow runtime configuration of below consts at some
// point

/// Connection keep alive interval.
///
/// Only set for initiators (clients). The value of 30s is recommended for
/// keeping middlebox UDP flows alive.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Connection idle timeout.
///
/// Only has an effect for responders (servers), which we configure to not send
/// keep alive probes. Should tolerate the loss of 1-2 keep-alive probes.
pub(in crate::net) const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(65);

/// Maximum number of connections to a single peer.
const MAX_PEER_CONNECTIONS: usize = 5;
