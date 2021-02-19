// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Calculate and distribute announcements to your network.
//!
//! The main types module are the [`Store`] trait, the [`Announcement`] struct,
//! and the [`Updates`] type alias.
//!
//! The main functions are [`snapshot`] and [`announce`].

use std::collections::HashSet;

use crate::{
    git::{
        identities,
        refs::{stored, Refs},
        storage::Storage,
        Urn,
    },
    git_ext::Oid,
    net::{peer::Peer, protocol::gossip},
    signer::Signer,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] identities::error::Error),

    #[error(transparent)]
    Stored(#[from] stored::Error),
}

/// The `Store` trait allows us to track [`Updates`] during announcements.
///
/// `load` provides a method for loading past updates so we can compare it with
/// a new set, effectively calculating the announcements we wish to make.
///
/// `save` allows us to update the `Store` with latest view of our world.
pub trait Store {
    type Error;

    fn load(&self) -> Result<Updates, Self::Error>;
    fn save(&mut self, updates: Updates) -> Result<(), Self::Error>;
}

#[cfg(test)]
impl Store for HashSet<Announcement> {
    type Error = !;

    fn load(&self) -> Result<Updates, Self::Error> {
        Ok(self.clone())
    }

    fn save(&mut self, updates: Updates) -> Result<(), Self::Error> {
        *self = self.union(&updates).cloned().collect();
        Ok(())
    }
}

/// Announcements pair a [`Urn`], the full reference path we are announcing,
/// with the current tip of the reference.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Announcement {
    urn: Urn,
    commit: Oid,
}

/// Unique set of [`Announcement`]s.
pub type Updates = HashSet<Announcement>;

/// Builds a snapshot of [`Updates`] by collecting all projects in the
/// [`Storage`], and loading their [`Refs`], which in turn are turned into
/// [`Announcement`]s.
#[tracing::instrument(skip(storage))]
pub fn snapshot(storage: &Storage) -> Result<Updates, Error> {
    let mut updates = HashSet::new();
    for identity in identities::any::list(storage)? {
        let identity = identity?;
        let project = match identity.project() {
            None => continue,
            Some(project) => project,
        };
        let urn = project.urn();
        match Refs::load(storage, &urn, None)? {
            None => continue,
            Some(refs) => updates.extend(into_updates(urn, refs)),
        }
    }
    Ok(updates)
}

fn into_updates(urn: Urn, refs: Refs) -> Updates {
    refs.iter_categorised()
        .map({
            move |((head, commit), category)| Announcement {
                urn: Urn {
                    path: Some(head.clone().into_qualified(category.into()).into()),
                    ..urn.clone()
                },
                commit: *commit,
            }
        })
        .collect()
}

fn diff<'a>(old_state: &'a Updates, new_state: &'a Updates) -> Updates {
    new_state.difference(old_state).cloned().collect()
}

/// Providing a `snapshot` of [`Updates`], we calculate the difference between
/// them and the [`Store`]. If there are new [`Announcement`]s to be made, they
/// will emitted to the network and the [`Store`] will be updated. The resulting
/// [`Updates`] will be returned.
///
/// Note that we can build a snapshot using the, aptly named, [`snapshot`]
/// function.
#[tracing::instrument(skip(peer, store))]
pub fn announce<Sign, S>(
    peer: &Peer<Sign>,
    store: &mut S,
    snapshot: Updates,
) -> Result<Updates, S::Error>
where
    S: Store,
    Sign: Signer + Clone,
{
    let previous = store.load()?;
    let updates = diff(&previous, &snapshot);

    if !updates.is_empty() {
        emit(peer, updates.clone().into_iter());
        store.save(snapshot.clone())?;
    }

    Ok(updates)
}

fn emit<S>(peer: &Peer<S>, updates: impl Iterator<Item = Announcement>)
where
    S: Signer + Clone,
{
    for Announcement { urn, commit } in updates {
        match peer.announce(gossip::Payload {
            urn: urn.clone(),
            rev: Some(gossip::Rev::Git(commit.into())),
            origin: None,
        }) {
            Ok(()) => {},
            Err(payload) => {
                tracing::warn!(urn = %payload.urn, rev = ?payload.rev, origin = ?payload.origin, "failed to announce update");
            },
        }
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod test {
    use std::{collections::HashSet, convert::TryFrom as _};

    use pretty_assertions::assert_eq;

    use crate::{
        git::Urn,
        git_ext::{Oid, RefLike},
    };

    use super::*;

    lazy_static! {
        static ref COMMIT1: Oid = "68986574".parse::<Oid>().unwrap();
        static ref COMMIT2: Oid = "c8d2ad44".parse::<Oid>().unwrap();
        static ref COMMIT3: Oid = "2d2e1408".parse::<Oid>().unwrap();
        static ref COMMIT4: Oid = "f90353ba".parse::<Oid>().unwrap();
        static ref COMMIT5: Oid = "a3403e2d".parse::<Oid>().unwrap();
    }

    #[test]
    fn diff_worksd() -> Result<(), Box<dyn std::error::Error>> {
        let shared = vec![
            Announcement {
                urn: project0("dev"),
                commit: *COMMIT1,
            },
            Announcement {
                urn: project0("master"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project0("stable"),
                commit: *COMMIT3,
            },
            Announcement {
                urn: project0("cloudhead/cool-feature"),
                commit: *COMMIT1,
            },
            Announcement {
                urn: project0("fintohaps/doc-tests"),
                commit: *COMMIT4,
            },
            Announcement {
                urn: project1("dev"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project0("master"),
                commit: *COMMIT3,
            },
            Announcement {
                urn: project1("stable"),
                commit: *COMMIT5,
            },
        ];
        let old = vec![
            Announcement {
                urn: project0("igor/zero-assertions"),
                commit: *COMMIT1,
            },
            Announcement {
                urn: project0("thoshol/remove"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project1("rudolfs/release"),
                commit: *COMMIT3,
            },
        ];
        let new = vec![
            Announcement {
                urn: project0("igor/zero-assertions"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project0("fintohaps/notations"),
                commit: *COMMIT1,
            },
            Announcement {
                urn: project0("kalt/eat-my-impls"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project1("backport"),
                commit: *COMMIT3,
            },
        ];

        let left: HashSet<_> = [&shared[..], &old[..]].concat().iter().cloned().collect();
        let right: HashSet<_> = [&shared[..], &new[..]].concat().iter().cloned().collect();
        let announcements = diff(&left, &right);

        assert_eq!(announcements, new.iter().cloned().collect::<HashSet<_>>());

        Ok(())
    }

    // FIXME: This could easily be a roundtrip test
    #[test]
    fn save_and_load() {
        let updates: HashSet<_> = vec![
            Announcement {
                urn: project0("cloudead/new-language"),
                commit: *COMMIT1,
            },
            Announcement {
                urn: project0("fintohaps/notations"),
                commit: *COMMIT2,
            },
            Announcement {
                urn: project0("kalt/loops"),
                commit: *COMMIT3,
            },
            Announcement {
                urn: project1("backport"),
                commit: *COMMIT4,
            },
        ]
        .into_iter()
        .collect();
        let mut store = HashSet::new();

        store.save(updates.clone()).unwrap();

        assert_eq!(store.load().unwrap(), updates);
    }

    fn project0(head: &str) -> Urn {
        Urn {
            id: "7ab8629dd6da14dcacde7f65b3d58cd291d7e235"
                .parse::<radicle_git_ext::Oid>()
                .expect("oid parse failed"),
            path: Some(RefLike::try_from(head).expect("head was not reflike")),
        }
    }

    fn project1(head: &str) -> Urn {
        Urn {
            id: "7ab8629dd6da14dcacde7f65b3d58cd291d7e234"
                .parse::<radicle_git_ext::Oid>()
                .expect("oid parse failed"),
            path: Some(RefLike::try_from(head).expect("head was not reflike")),
        }
    }
}
