// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Perform full state syncs with remote peers.

use librad::{identities::generic::Identity, net::peer::Peer, PeerId, Signer};

use crate::state;

use super::{include, Error};

/// Initiaites a fetch for all locally tracked projects from the given
/// [`PeerId`].
pub async fn sync<S>(peer: &Peer<S>, remote_peer: PeerId) -> Result<(), Error>
where
    S: Clone + Signer,
{
    tracing::debug!(%remote_peer, "Starting sync");

    let urns = state::list_projects(peer)
        .await?
        .iter()
        .map(Identity::urn)
        .collect::<Vec<_>>();

    for urn in urns {
        tracing::debug!(%urn, %remote_peer, "starting fetch");
        match state::fetch(peer, urn.clone(), remote_peer, vec![], None).await {
            Ok(result) => {
                tracing::debug!(
                    %urn,
                    %remote_peer,
                    updated_tips = ?result.updated_tips,
                    "finished fetch",
                );
                include::update(peer.clone(), urn).await;
            },
            Err(error) => tracing::error!(%urn, %remote_peer, ?error, "fetch error"),
        }
    }

    Ok(())
}
