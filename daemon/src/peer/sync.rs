// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Perform full state syncs with remote peers.

use librad::{identities::generic::Identity, net::peer::Peer, peer::PeerId, signer::Signer};

use crate::state;

use super::{include, Error};

/// Initiaites a fetch for all locally tracked projects from the given
/// [`PeerId`].
pub async fn sync<S>(peer: &Peer<S>, remote_peer: PeerId) -> Result<(), Error>
where
    S: Clone + Signer,
{
    log::debug!("Starting sync from {}", remote_peer);

    let urns = state::list_projects(peer)
        .await?
        .iter()
        .map(Identity::urn)
        .collect::<Vec<_>>();

    for urn in urns {
        log::debug!("Starting fetch of {} from {}", urn, remote_peer);
        match state::fetch(peer, urn.clone(), remote_peer, vec![], None).await {
            Ok(result) => {
                log::debug!(
                    "Finished fetch of {} from {} with the result {:?}",
                    urn,
                    remote_peer,
                    result.updated_tips
                );
                include::update(peer.clone(), urn).await;
            },
            Err(e) => log::debug!("Fetch of {} from {} errored: {}", urn, remote_peer, e),
        }
    }

    Ok(())
}
