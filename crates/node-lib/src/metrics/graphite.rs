// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    net::SocketAddr,
    time::{Duration, SystemTime},
};

use tokio::{net::UdpSocket, time};
use tracing::{debug, info, instrument};

use librad::{net::peer::Peer, Signer};

const CONNECTIONS_TOTAL: &str = "connections_total";
const CONNECTED_PEERS: &str = "connected_peers";
const MEMBERSHIP_ACTIVE: &str = "membership_active";
const MEMBERSHIP_PASSIVE: &str = "membership_passive";

#[instrument(name = "graphite subroutine", skip(peer))]
pub async fn routine<S>(peer: Peer<S>, graphite_addr: SocketAddr) -> anyhow::Result<()>
where
    S: Signer + Clone,
{
    info!("starting graphite stats routine");

    debug!("connecting to graphite at {}", graphite_addr);
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(graphite_addr).await?;
    debug!("connected to graphite at {}", graphite_addr);

    let peer_id = peer.peer_id().to_string();
    loop {
        time::sleep(Duration::from_secs(10)).await;

        let stats = time::timeout(Duration::from_secs(5), peer.stats()).await?;
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;

        for (metric, value) in &[
            (CONNECTED_PEERS, stats.connected_peers.len()),
            (CONNECTIONS_TOTAL, stats.connections_total),
            (MEMBERSHIP_ACTIVE, stats.membership_active),
            (MEMBERSHIP_PASSIVE, stats.membership_passive),
        ] {
            sock.send(line(peer_id.clone(), metric, *value as f32, now).as_bytes())
                .await?;
        }
    }
}

fn line(peer_id: String, metric: &str, value: f32, time: Duration) -> String {
    format!(
        "linkd_{};peer={} {:?} {}",
        metric,
        peer_id,
        value,
        time.as_secs()
    )
}
