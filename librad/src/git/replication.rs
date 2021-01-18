// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    iter,
    net::SocketAddr,
};

use either::Either;
use git_ext::{self as ext, is_exists_err};
use nonempty::NonEmpty;
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    fetch::{self, CanFetch as _},
    identities::{self, local::LocalIdentity},
    refs::{self, Refs},
    storage::{self, Storage},
    tracking,
    types::{reference, Force, Namespace, Reference},
};
use crate::{
    identities::git::{Person, Project, SomeIdentity, VerifiedPerson, VerifiedProject},
    peer::PeerId,
};

pub use crate::identities::git::Urn;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot replicate from self")]
    SelfReplication,

    #[error("identity not found")]
    MissingIdentity,

    #[error("no identity was found for `{0}`, leading to not being able to adopt a `rad/id`")]
    MissingIdentities(Urn),

    #[error("missing required ref: {0}")]
    Missing(ext::RefLike),

    #[error("the identity did not have any delegates or tracked peers")]
    NoTrustee,

    #[error("failed to convert {urn} to reference")]
    RefFromUrn {
        urn: Urn,
        source: reference::FromUrnError,
    },

    #[error("fork detected")]
    Fork,

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Track(#[from] tracking::Error),

    #[error("signer error")]
    Sign(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("fetcher error")]
    Fetch(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Identities(#[from] identities::error::Error),

    #[error(transparent)]
    Store(#[from] storage::Error),
}

pub enum Replication {
    Clone {
        urn: Urn,
        identity: SomeIdentity,
        fetched_peers: BTreeSet<PeerId>,
    },
    Fetch {
        urn: Urn,
        identity: SomeIdentity,
        existing: BTreeSet<PeerId>,
    },
}

/// The result of replicating an identity will tell us if we are at the latest
/// tip or, in the case of our peer being a delegate, if we are behind and
/// require updating the document.
#[derive(Debug)]
pub enum ReplicateResult {
    Latest,
    Behind,
}

/// Attempt to fetch `urn` from `remote_peer`, optionally supplying
/// `addr_hints`. `urn` may or may not already exist locally.
///
/// The [`Urn::path`] is ignored (defaulting to `rad/id`).
///
/// The fetch proceeds in three stages:
///
/// 1. Update the remote branches needed for identity verification
///
///    The identity according to `remote_peer` is verified, and if that passes
///    the local branch layout created (if it does not already exist). It may
///    also update local tracking relationships based on the identity
///    information.
///
/// 2. Fetch the `rad/signed_refs` of all tracked peers, and compute the
///    eligible heads (i.e. where the `remote_peer` advertises the same tip
///    oid as found in the signed refs)
///
/// 3. Fetch the rest (i.e. eligible heads)
///
/// Optionally, a [`LocalIdentity`] can be specified to identify as in the
/// context of this namespace (ie. to be used as the `rad/self` branch). If not
/// specified, the existing identity is left untouched. If there is no existing
/// `rad/self` identity (eg. because this is the first time `urn` is fetched),
/// not specifying `whoami` is also referred to as "anonymous replication".
///
/// Note, however, that pushing local modifications requires a `rad/self` to be
/// set, which is enforced by the
/// [`crate::git::local::transport::LocalTransport`].
#[allow(clippy::unit_arg)]
#[tracing::instrument(skip(storage, whoami, addr_hints), err)]
pub fn replicate<Addrs>(
    storage: &Storage,
    whoami: Option<LocalIdentity>,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<ReplicateResult, Error>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    let urn = Urn::new(urn.id);
    let local_peer_id = storage.peer_id();

    if local_peer_id == &remote_peer {
        return Err(Error::SelfReplication);
    }

    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;
    let (result, mut remove) = match replication(storage, &mut fetcher, urn.clone(), remote_peer)? {
        Replication::Clone {
            urn,
            identity,
            fetched_peers,
        } => {
            let allowed = match identity {
                SomeIdentity::Project(proj) => {
                    let delegates = project::delegate_views(storage, proj, Some(remote_peer))?;
                    let allowed = delegates.keys().copied().collect();
                    let rad_id = unsafe_into_urn(
                        Reference::rad_id(Namespace::from(&urn)).with_remote(remote_peer),
                    );
                    let proj = identities::project::verify(storage, &rad_id)?
                        .ok_or(Error::MissingIdentity)?;
                    project::ensure_setup(storage, &mut fetcher, delegates, &rad_id, proj)?;
                    allowed
                },
                SomeIdentity::Person(person) => {
                    person::ensure_setup(storage, person.clone(), remote_peer)?;
                    person
                        .delegations()
                        .iter()
                        .copied()
                        .map(PeerId::from)
                        .collect()
                },
            };

            // Symref `rad/self` if a `LocalIdentity` was given
            if let Some(local_id) = whoami {
                local_id.link(storage, &urn)?;
            }

            Ok::<_, Error>((
                ReplicateResult::Latest,
                fetched_peers.difference(&allowed).copied().collect(),
            ))
        },
        Replication::Fetch {
            urn,
            identity,
            existing,
        } => {
            let (result, updated) = match identity {
                SomeIdentity::Project(proj) => {
                    let delegate_views = project::delegate_views(storage, proj, None)?;
                    let proj = identities::project::verify(storage, &urn)?
                        .ok_or(Error::MissingIdentity)?;
                    let mut updated_delegations = project::all_delegates(&proj);
                    let rad_id = unsafe_into_urn(Reference::rad_id(Namespace::from(&urn)));
                    let result = project::ensure_setup(
                        &storage,
                        &mut fetcher,
                        delegate_views,
                        &rad_id,
                        proj,
                    )?;

                    let mut updated_tracked =
                        tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
                    updated_tracked.append(&mut updated_delegations);
                    (result, updated_tracked)
                },
                SomeIdentity::Person(person) => {
                    person::ensure_setup(storage, person, remote_peer)?;
                    (
                        ReplicateResult::Latest,
                        tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>(),
                    )
                },
            };

            let (removed, _, _) = partition(&existing, &updated);
            Ok((result, removed))
        },
    }?;

    // Ensure we're not tracking ourselves
    remove.insert(*local_peer_id);

    // Remove any remote tracking branches we don't need
    prune(storage, &urn, remove.iter())?;

    // TODO: At this point, the tracking graph may have changed, and/or we
    // created top-level person namespaces. We will eventually converge, but
    // perhaps we'd want to return some kind of continuation here, so the caller
    // could schedule a deferred task directly?
    Ok(result)
}

/// Identify the type of replication case we're in -- whether it's a new
/// identity which we're cloning onto our machine or an existing identity that
/// we are updating.
///
/// # Clone
///
/// If we are cloning then we pre-fetch all the references to kick-off the
/// adoption of the progress.
///
/// # Fetch
///
/// If we are fetching updates then we only fetch the relevant remotes that we
/// already know about.
#[allow(clippy::unit_arg)]
#[tracing::instrument(skip(storage, fetcher), err)]
fn replication(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    urn: Urn,
    remote_peer: PeerId,
) -> Result<Replication, Error> {
    if !storage.has_urn(&urn)? {
        let fetched_peers = fetcher
            .fetch(fetch::Fetchspecs::PeekAll {
                max_fetch: fetch::ONE_GB,
            })
            .map_err(|e| Error::Fetch(e.into()))
            .and_then(project::fetched_peers)?;

        let remote_ident =
            unsafe_into_urn(Reference::rad_id(Namespace::from(&urn)).with_remote(remote_peer));
        Ok(Replication::Clone {
            urn,
            fetched_peers,
            identity: identities::any::get(storage, &remote_ident)?
                .ok_or(Error::MissingIdentity)?,
        })
    } else {
        let identity = identities::any::get(storage, &urn)?.ok_or(Error::MissingIdentity)?;
        let existing = match identity {
            SomeIdentity::Project(ref proj) => {
                let mut remotes = project::all_delegates(&proj);
                let mut tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
                remotes.append(&mut tracked);

                remotes
            },
            SomeIdentity::Person(_) => tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>(),
        };

        let _ = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: existing.clone(),
                max_fetch: fetch::ONE_MB * existing.len(),
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Ok(Replication::Fetch {
            urn,
            identity,
            existing,
        })
    }
}

fn unsafe_into_urn(reference: Reference<git_ext::RefLike>) -> Urn {
    reference.try_into().expect("namespace is set")
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage), err)]
fn ensure_rad_id(storage: &Storage, urn: &Urn, tip: ext::Oid) -> Result<(), Error> {
    identities::common::IdRef::from(urn)
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))
}

/// Untrack the list of `PeerId`s, which also has the side-effect of removing
/// that peer's remote references in the storage.
///
/// In the case of an error occurring during the untrack call we emit a warning
/// and append it to a `Vec` of unpruned peers. The unpruned peers are returned.
#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage, prune_list))]
fn prune<'a>(
    storage: &Storage,
    urn: &Urn,
    prune_list: impl Iterator<Item = &'a PeerId>,
) -> Result<(), Error> {
    for peer in prune_list {
        match tracking::untrack(storage, urn, *peer) {
            Ok(removed) => {
                if removed {
                    tracing::info!(peer = %peer, "pruned");
                } else {
                    tracing::trace!(peer = %peer, "peer did not exist for pruning");
                }
            },
            Err(err) => {
                tracing::warn!(peer = %peer, err = %err, "failed to prune");
                return Err(err.into());
            },
        }
    }
    Ok(())
}

// Return three sets where the first consists of elements in `ys` but not in
// `xs` and the second vice-versa, and the final set contains the elements they
// both share.
//
// If `ys` represents an "updated" set of `xs` then the first set will be all
// elements that were removed, the second set will be all the elements added,
// and the third set all the elements that stayed the same.
fn partition<'a, A: Clone + Ord>(
    xs: &'a BTreeSet<A>,
    ys: &'a BTreeSet<A>,
) -> (BTreeSet<A>, BTreeSet<A>, BTreeSet<A>) {
    let mut removed = BTreeSet::new();
    let mut added = BTreeSet::new();
    let kept = xs.intersection(ys).cloned().collect();

    for e in xs.symmetric_difference(ys) {
        if xs.contains(e) {
            added.insert(e.clone());
        } else {
            removed.insert(e.clone());
        }
    }

    (removed, added, kept)
}

mod person {
    use super::*;

    /// Process the `Person` that was replicated by:
    ///   * Verifying the identity
    ///   * Tracking the delegations
    ///   * Ensuring we have a top-level `rad/id` that points to the latest
    ///     version
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn ensure_setup(
        storage: &Storage,
        person: Person,
        remote_peer: PeerId,
    ) -> Result<(), Error> {
        let local_peer = storage.peer_id();
        let urn = unsafe_into_urn(
            Reference::rad_id(Namespace::from(person.urn())).with_remote(remote_peer),
        );

        let delegations = match identities::person::verify(storage, &urn)? {
            None => Err(Error::MissingIdentity),
            Some(person) => {
                let delegations = person
                    .into_inner()
                    .doc
                    .delegations
                    .into_iter()
                    .map(PeerId::from)
                    .collect::<BTreeSet<_>>();

                // Track all delegations
                for peer_id in delegations.iter() {
                    if peer_id != local_peer {
                        tracking::track(storage, &urn, *peer_id)?;
                    }
                }

                Ok(delegations)
            },
        }?;

        // Create `rad/id` here, if not exists
        adopt_latest(storage, &urn, delegations)?;
        Ok(())
    }

    /// Adopt the `rad/id` that has the most up-to-date commit from the set of
    /// `Person` delegates.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn adopt_latest(
        storage: &Storage,
        urn: &Urn,
        delegates: BTreeSet<PeerId>,
    ) -> Result<(), Error> {
        let persons = NonEmpty::from_vec(
            delegates
                .into_iter()
                .flat_map(|peer| {
                    let urn =
                        unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(peer));
                    identities::person::get(storage, &urn).ok().flatten()
                })
                .collect(),
        );
        let tip = match persons {
            Some(persons) => identities::person::latest_tip(storage, persons).map_err(Error::from),
            None => Err(Error::MissingIdentities(urn.clone())),
        }?;
        ensure_rad_id(storage, urn, tip.into())
    }
}

mod project {
    use super::*;

    #[derive(Clone, Debug)]
    pub struct DelegateView {
        pub urn: Urn,
        pub delegate: VerifiedPerson,
        pub project: VerifiedProject,
    }

    /// Process the setup of a `Project` by:
    ///   * Ensuring there are no forks between the given `rad/id` and the
    ///     delegates. In the case of a clone, the `rad/id` will point to the
    ///     remote. In the case of a fetch, the `rad/id` will point to the
    ///     already replicated identity.
    ///   * Tracking the delegates
    ///   * Replicating the `rad/signed_refs`
    ///   * Tracking the remotes of the delegates
    ///   * Ensuring we have a top-level `rad/id` that points to the latest
    ///     version
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage, fetcher), err)]
    pub fn ensure_setup(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        delegates: BTreeMap<PeerId, project::DelegateView>,
        rad_id: &Urn,
        proj: VerifiedProject,
    ) -> Result<ReplicateResult, Error> {
        let local_peer = storage.peer_id();
        project::ensure_no_forking(
            storage,
            rad_id,
            delegates.values().map(|view| view.urn.clone()).collect(),
        )?;

        let urn = proj.urn();
        project::track_direct(storage, &proj)?;
        let tracked = replicate_signed_refs(
            storage,
            fetcher,
            &urn,
            delegates
                .values()
                .map(|delegate| delegate.urn.clone())
                .collect(),
        )?;
        for peer in tracked {
            if peer != *local_peer {
                tracking::track(&storage, &urn, peer)?;
            }
        }

        project::adopt_latest(storage, &urn, delegates)
    }

    /// Fetch `rad/signed_refs` and `refs/heads` of the delegates and our
    /// tracked graph, returning the set of tracked peers.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage, fetcher), err)]
    pub fn replicate_signed_refs(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        urn: &Urn,
        delegates: BTreeSet<Urn>,
    ) -> Result<BTreeSet<PeerId>, Error> {
        // Read `signed_refs` for all tracked
        let tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
        let tracked_sigrefs = tracked
            .into_iter()
            .filter_map(|peer| match Refs::load(storage, &urn, peer) {
                Ok(Some(refs)) => Some(Ok((peer, refs))),

                Ok(None) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        // Fetch all the rest
        tracing::debug!("fetching heads: {:?}, {:?}", tracked_sigrefs, delegates);
        fetcher
            .fetch(fetch::Fetchspecs::Replicate {
                tracked_sigrefs: tracked_sigrefs.clone(),
                delegates,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Refs::update(storage, &urn)?;
        Ok(tracked_sigrefs
            .iter()
            .flat_map(|(peer, refs)| iter::once(*peer).chain(refs.remotes.flatten().copied()))
            .collect())
    }

    /// Compare the remotes view of the project against each delegate and ensure
    /// there are no forks.
    ///
    /// # Errors
    ///   * If there is a fork
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn ensure_no_forking(
        storage: &Storage,
        rad_id: &Urn,
        delegates: BTreeSet<Urn>,
    ) -> Result<(), Error> {
        for delegate in delegates.iter() {
            match identities::project::is_fork(&storage, &rad_id, &delegate) {
                Ok(false) => { /* all good */ },
                Ok(true) => return Err(Error::Fork),
                Err(identities::error::Error::NotFound(urn)) => {
                    tracing::debug!("`{}` not found when checking for fork", urn);
                },
                Err(err) => return Err(err.into()),
            }
        }

        Ok(())
    }

    /// For each delegate in `remotes/<remote_peer>/rad/ids/*` get the view for
    /// that delegate that _should_ be local the `storage` after a fetch.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn delegate_views(
        storage: &Storage,
        proj: Project,
        remote_peer: Option<PeerId>,
    ) -> Result<BTreeMap<PeerId, DelegateView>, Error> {
        let mut delegate_views = BTreeMap::new();
        let local_peer_id = storage.peer_id();
        for delegate in proj.delegations().iter().indirect() {
            let in_rad_ids = unsafe_into_urn(
                Reference::rad_delegate(Namespace::from(&proj.urn()), &delegate.urn())
                    .with_remote(remote_peer),
            );
            match identities::person::verify(storage, &in_rad_ids)? {
                None => return Err(Error::Missing(in_rad_ids.into())),
                Some(delegate_person) => {
                    let person = delegate_person.clone();
                    for key in delegate_person.delegations().iter() {
                        let peer_id = PeerId::from(*key);
                        let (urn, project) = if &peer_id == local_peer_id {
                            let urn = proj.urn();
                            let verified = identities::project::verify(storage, &urn)?
                                .ok_or(Error::MissingIdentity)?;
                            (urn, verified)
                        } else {
                            let remote_urn = unsafe_into_urn(
                                Reference::rad_id(Namespace::from(&proj.urn()))
                                    .with_remote(peer_id),
                            );
                            adopt_delegate_person(storage, peer_id, &person, &proj.urn())?;
                            let verified = identities::project::verify(storage, &remote_urn)?
                                .ok_or(Error::MissingIdentity)?;
                            (remote_urn, verified)
                        };
                        delegate_views.insert(
                            peer_id,
                            DelegateView {
                                urn,
                                delegate: person.clone(),
                                project,
                            },
                        );
                    }
                },
            }
        }

        Ok(delegate_views)
    }

    /// Persist a delegate identity in our storage.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn adopt_delegate_person(
        storage: &Storage,
        peer: PeerId,
        person: &VerifiedPerson,
        project_urn: &Urn,
    ) -> Result<(), Error> {
        let delegate_urn = person.urn();
        ensure_rad_id(storage, &delegate_urn, person.content_id)?;
        tracking::track(storage, &delegate_urn, peer)?;
        tracking::track(storage, &project_urn, peer)?;

        // Now point our view to the top-level
        Reference::try_from(&delegate_urn)
            .map_err(|e| Error::RefFromUrn {
                urn: delegate_urn.clone(),
                source: e,
            })?
            .symbolic_ref::<_, PeerId>(
                Reference::rad_delegate(Namespace::from(project_urn), &delegate_urn),
                Force::False,
            )
            .create(storage.as_raw())
            .and(Ok(()))
            .or_matches(is_exists_err, || Ok(()))
            .map_err(|e: git2::Error| Error::Store(e.into()))
    }

    /// Track all direct delegations of a `Project`.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    fn track_direct(storage: &Storage, proj: &VerifiedProject) -> Result<(), Error> {
        let local_peer_id = storage.peer_id();

        for key in proj
            .delegations()
            .iter()
            .direct()
            .filter(|&key| key != local_peer_id.as_public_key())
        {
            tracking::track(storage, &proj.urn(), PeerId::from(*key))?;
        }

        Ok(())
    }

    /// Adopt the `rad/id` that has the most up-to-date commit from the set of
    /// `Project` delegates.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn adopt_latest(
        storage: &Storage,
        urn: &Urn,
        delegates: BTreeMap<PeerId, DelegateView>,
    ) -> Result<ReplicateResult, Error> {
        let local_peer = storage.peer_id();
        let projects = NonEmpty::from_vec(
            delegates
                .values()
                .map(|view| view.project.clone().into_inner())
                .collect::<Vec<_>>(),
        );

        let tip = match projects {
            Some(projects) => {
                identities::project::latest_tip(storage, projects).map_err(Error::from)
            },
            None => Err(Error::MissingIdentities(urn.clone())),
        }?;

        // Are we a delegate?
        match delegates.get(local_peer) {
            None => {
                ensure_rad_id(storage, urn, tip.into())?;
                Ok(ReplicateResult::Latest)
            },
            Some(view) => {
                Ok(if view.project.content_id == tip.into() {
                    ReplicateResult::Latest
                } else {
                    // FIXME: We could be ahead
                    ReplicateResult::Behind
                })
            },
        }
    }

    /// Using the fetched references we parse out the set of `PeerId`s that were
    /// fetched.
    pub fn fetched_peers(result: fetch::FetchResult) -> Result<BTreeSet<PeerId>, Error> {
        use std::str::FromStr;

        let mut peers = BTreeSet::new();
        for reference in result.updated_tips.keys() {
            let path: ext::RefLike = match Urn::try_from(reference.clone()).map(|urn| urn.path) {
                Ok(Some(path)) => path,
                Ok(None) | Err(_) => {
                    /* FIXME: prune reference */
                    continue;
                },
            };
            let suffix = match path.strip_prefix(reflike!("refs/remotes")) {
                Ok(suffix) => suffix,
                Err(_) => continue,
            };
            let peer = match suffix.as_str().split('/').next().map(PeerId::from_str) {
                None | Some(Err(_)) => {
                    /* FIXME: prune reference */
                    continue;
                },
                Some(Ok(remote)) => remote,
            };
            peers.insert(peer);
        }

        Ok(peers)
    }

    pub fn all_delegates(proj: &Project) -> BTreeSet<PeerId> {
        proj.delegations()
            .iter()
            .flat_map(|delegate| match delegate {
                Either::Left(pk) => vec![PeerId::from(*pk)],
                Either::Right(person) => person
                    .delegations()
                    .iter()
                    .map(|pk| PeerId::from(*pk))
                    .collect(),
            })
            .collect()
    }
}
