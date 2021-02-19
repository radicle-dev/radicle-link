// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Perform full state syncs with remote peers.

use std::collections::HashSet;

use crate::{
    git::{
        refs::{stored, Refs},
        replication,
        storage::Storage,
        Urn,
    },
    peer::PeerId,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Remote {
    pub urn: Urn,
    pub peer: PeerId,
}

pub fn remotes(storage: &Storage, urn: Urn) -> Result<Vec<Remote>, stored::Error> {
    Ok(match Refs::load(storage, &urn, None)? {
        None => vec![],
        Some(refs) => refs
            .remotes
            .flatten()
            .map({
                let fetch_urn = urn.clone();
                move |remote| Remote {
                    urn: fetch_urn.clone(),
                    peer: *remote,
                }
            })
            .collect(),
    })
}

/// Attempts to perfom a fetch for all [`Remote`]s provided.
///
/// If the fetch fails we continue the process and drop the `Remote` from the
/// resulting `HashSet`.
#[tracing::instrument(skip(storage, remotes))]
pub fn fetch(
    storage: &Storage,
    config: replication::Config,
    remotes: impl Iterator<Item = Remote>,
) -> HashSet<Remote> {
    tracing::trace!("starting synchronisation of peer");
    remotes
        .filter(move |remote| {
            tracing::trace!(urn = %remote.urn, "starting fetch");
            match replication::replicate(
                storage,
                config,
                None,
                remote.urn.clone(),
                remote.peer,
                None,
            ) {
                Ok(_result) => {
                    tracing::trace!(urn = %remote.urn, "finished fetch");
                    true
                },
                Err(err) => {
                    tracing::warn!(err = %err, "failed fetch");
                    false
                },
            }
        })
        .collect()
}
