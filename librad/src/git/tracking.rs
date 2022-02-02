// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub use crate::identities::git::Urn;

mod odb;
mod refdb;
pub mod v1;

pub use link_tracking::{
    config,
    git::{
        self,
        config::Config,
        tracking::{
            batch::{self, batch, Action, Applied, Updated},
            default_only,
            error,
            get,
            is_tracked,
            modify,
            policy,
            reference,
            track,
            tracked,
            tracked_peers,
            untrack,
            PreviousError,
            Ref,
            Tracked,
            TrackedEntries,
            TrackedPeers,
        },
    },
};

/// Migration from tracking-v1 to tracking-v2.
///
/// NOTE: This is used in `Storage::open` and will be deprecated once enough
/// time has passed for upstream dependencies to migrate to the latest version.
pub mod migration {
    use std::borrow::Cow;

    use super::*;
    use crate::PeerId;

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error(transparent)]
        Batch(#[from] error::Batch),
        #[error(transparent)]
        Tracking(#[from] v1::Error),
    }

    pub struct Migration {
        pub successes: Vec<(Urn, PeerId)>,
        pub failures: Vec<(v1::Error, Urn, PeerId)>,
    }

    pub fn migrate(
        storage: &super::super::Storage,
        urns: impl IntoIterator<Item = Urn>,
    ) -> Result<Migration, Error> {
        let mut migration = Migration {
            successes: Default::default(),
            failures: Default::default(),
        };
        for urn in urns {
            let peers = v1::tracked(storage, &urn)?;
            let config = Config::default();
            let (peers, actions): (Vec<_>, Vec<_>) = peers
                .map(|peer| {
                    (
                        peer,
                        Action::Track {
                            peer: Some(peer),
                            urn: Cow::from(&urn),
                            config: &config,
                            policy: policy::Track::MustNotExist,
                        },
                    )
                })
                .unzip();
            let applied = batch(storage, actions)?;
            for update in applied.updates {
                match update {
                    Updated::Tracked { reference } => {
                        tracing::info!(reference = %reference.name, oid = %reference.target, "tracked in v2");
                    },
                    Updated::Untracked { .. } => {
                        unreachable!("should not untrack during migration")
                    },
                }
            }

            for peer in peers {
                if let Ok(it_is) = is_tracked(storage, &urn, Some(peer)) {
                    if it_is {
                        match v1::untrack(storage, &urn, peer) {
                            Ok(_) => {
                                migration.successes.push((urn.clone(), peer));
                            },
                            Err(err) => {
                                migration.failures.push((err, urn.clone(), peer));
                            },
                        }
                    }
                }
            }
        }
        Ok(migration)
    }
}
