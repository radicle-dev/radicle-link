// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::BTreeSet;

use futures::{pin_mut, StreamExt as _};
use tracing::{error, info, instrument};

use librad::{
    git::{tracking, Urn},
    net::{
        peer::{event::upstream::Gossip, Peer, PeerInfo, ProtocolEvent},
        protocol::{broadcast::PutResult::Uninteresting, gossip::Payload},
    },
    PeerId,
    Signer,
};

pub enum Tracker {
    Everything,
    Selected {
        peer_ids: BTreeSet<PeerId>,
        urns: BTreeSet<Urn>,
    },
}

impl Tracker {
    fn is_tracked(&self, peer_id: &PeerId, urn: &Urn) -> bool {
        match self {
            Tracker::Everything => true,
            Tracker::Selected {
                ref peer_ids,
                ref urns,
            } if peer_ids.contains(peer_id) || urns.contains(urn) => true,
            _ => false,
        }
    }
}

#[instrument(name = "tracking subroutine", skip(peer, tracker))]
pub async fn routine<S>(peer: Peer<S>, tracker: Tracker) -> anyhow::Result<()>
where
    S: Signer + Clone,
{
    let events = peer.subscribe();
    pin_mut!(events);

    while let Some(res) = events.next().await {
        if let Err(err) = res {
            error!(?err, "event error");
            continue;
        }

        if let ProtocolEvent::Gossip(box Gossip::Put {
            payload: Payload { urn, .. },
            provider:
                PeerInfo {
                    peer_id,
                    seen_addrs,
                    ..
                },
            result: Uninteresting,
        }) = res.unwrap()
        {
            if !tracker.is_tracked(&peer_id, &urn) {
                continue;
            }

            let go = async {
                let updated = peer
                    .using_storage({
                        let urn = urn.clone();
                        move |storage| tracking::track(storage, &urn, peer_id)
                    })
                    .await??;

                // Skip explicit replication if the peer is already tracked.
                if updated {
                    let addr_hints = seen_addrs.iter().copied().collect::<Vec<_>>();
                    peer.replicate((peer_id, addr_hints), urn.clone(), None)
                        .await?;
                }

                Ok::<_, anyhow::Error>(updated)
            };

            match go.await {
                Ok(true) => info!("tracked project {} from {}", urn, peer_id),
                Ok(false) => info!("already tracked {} from {}", urn, peer_id),
                Err(err) => error!(?err, "tracking failed for {} from {}", urn, peer_id),
            }
        }
    }

    Ok(())
}
