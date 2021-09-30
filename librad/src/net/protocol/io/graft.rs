// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, iter, net::SocketAddr, str::FromStr, time::Duration};

use crate::{
    executor,
    git::{
        fetch::{Fetcher as _, RemoteHeads},
        refs::{self, Refs, Remotes},
        replication,
        storage::{self, fetcher},
        Urn,
    },
    PeerId,
};

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum Rere {
        #[error("precomputed signed refs for {0} not found")]
        MissingSignedRefs(Urn),

        #[error(transparent)]
        Replicate(#[from] Box<replication::Error>),

        #[error(transparent)]
        Refs(#[from] refs::stored::Error),

        #[error("unable to obtain fetcher")]
        Fetcher(#[from] fetcher::error::Retrying<git2::Error>),

        #[error(transparent)]
        Storage(#[from] storage::Error),

        #[error(transparent)]
        Pool(#[from] storage::PoolError),
    }

    impl From<replication::Error> for Rere {
        fn from(e: replication::Error) -> Self {
            Self::from(Box::new(e))
        }
    }
}

pub mod config {
    use super::*;

    pub struct Rere {
        pub replication: replication::Config,
        pub fetch_slot_wait_timeout: Duration,
    }
}

/// Initiate [`replication::replicate`] if the `remote_peer` appears to serve
/// "interesting" refs.
///
/// Remote refs are deemed interesting if there is an intersection between the
/// set of [`PeerId`]s acc. to the local [`Remotes`] for the given [`Urn`], and
/// the advertised remote tracking branches of the remote peer (plus the remote
/// peer itself).
///
/// This function is invoked when the `remote_peer` initiates a fetch (hence
/// "rere" -- replicate replicate), as per [`super::recv::git`].
///
/// A fetch initiated by this function will **not** generate a
/// [`crate::git::p2p::header::Header::nonce`], so as to not trigger reres
/// recursively. Using this function thus requires to inspect the git header for
/// the presence of a nonce (or else skip the rere), and to keep track of recent
/// nonces in case of nonce re-use.
#[tracing::instrument(level = "debug", skip(spawner, storage, config, addr_hints))]
pub async fn rere<S, Addrs>(
    spawner: &executor::Spawner,
    storage: &S,
    config: config::Rere,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<Option<replication::ReplicateResult>, error::Rere>
where
    S: storage::Pooled<storage::Storage> + Send + Sync + 'static,
    Addrs: IntoIterator<Item = SocketAddr>,
{
    fetcher::retrying(
        spawner,
        storage,
        fetcher::PeerToPeer::new(urn.clone(), remote_peer, addr_hints),
        config.fetch_slot_wait_timeout,
        move |storage, fetcher| {
            let remote_heads = fetcher.remote_heads();
            let refs = Refs::load(&storage, &urn, None)
                .map_err(error::Rere::from)?
                .ok_or_else(|| error::Rere::MissingSignedRefs(urn.clone()))?;

            tracing::debug!("remotes: {:?}", refs.remotes);

            if is_interesting(remote_peer, remote_heads, &refs.remotes) {
                tracing::debug!("interesting");
                Ok(Some(
                    replication::replicate(storage, fetcher, config.replication, None)
                        .map_err(error::Rere::from)?,
                ))
            } else {
                tracing::debug!("uninteresting");
                Ok(None)
            }
        },
    )
    .await?
}

pub fn is_interesting<P>(remote_peer: P, remote_heads: &RemoteHeads, remotes: &Remotes<P>) -> bool
where
    P: Ord + FromStr,
{
    let tracked_local = remotes.flatten().collect::<BTreeSet<_>>();
    let mut tracked_remote =
        iter::once(remote_peer).chain(remote_heads.keys().filter_map(|ref_ish| {
            ref_ish
                .split('/')
                .skip_while(|&x| x != "remotes")
                .skip(1)
                .take(1)
                .next()
                .and_then(|remote| remote.parse().ok())
        }));
    tracked_remote.any(|peer_id: P| tracked_local.contains(&peer_id))
}
