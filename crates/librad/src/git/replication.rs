// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    iter,
};

use either::Either;
use git_ext::{self as ext, is_exists_err};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    fetch,
    identities::{self, local::LocalIdentity},
    refs::{self, Refs},
    storage::{self, ReadOnlyStorage, Storage},
    tracking,
    types::{reference, Force, Namespace, One, Reference},
};
use crate::{
    identities::git::{Person, Project, Revision, SomeIdentity, VerifiedPerson, VerifiedProject},
    PeerId,
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

    #[error("fork detected between `{mine}` and `{theirs}`")]
    Fork { mine: Urn, theirs: Urn },

    #[error("unknown identity kind")]
    UnknownIdentityKind(SomeIdentity),

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Track(#[from] tracking::Error),

    #[error("signer error: {0}")]
    Sign(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("fetcher error: {0}")]
    Fetch(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Identities(#[from] Box<identities::error::Error>),

    #[error(transparent)]
    Store(#[from] storage::Error),
}

impl From<identities::error::Error> for Error {
    fn from(e: identities::error::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub fetch_limit: fetch::Limit,
}

/// The success outcome of [`self::replicate`].
#[derive(Debug)]
pub struct ReplicateResult {
    /// The set of refs which were updated during the sync, along with the
    /// [`ext::Oid`] they are now pointing to.
    ///
    /// If this is empty, no data was fetched from the other side.
    pub updated_tips: BTreeMap<ext::RefLike, ext::Oid>,

    /// An indicator of whether the local view of the identity document might
    /// need approval of updates.
    pub identity: IdStatus,

    /// Whether the replicated [`Urn`] was previously present in local storage
    /// or not.
    pub mode: Mode,
}

/// The "freshness" of the local view of a repo identity wrt the delegates.
#[derive(Debug)]
pub enum IdStatus {
    /// Up-to-date, no further action is required.
    Even,
    /// Delegate tips are either behind or ahead. Interactive review is
    /// recommended.
    Uneven,
}

/// The "mode" `replicate` was operating in.
#[derive(Debug)]
pub enum Mode {
    /// The git tree corresponding to [`Urn`] was previously **not** present
    /// locally, so the operation was equivalent to `git clone`.
    Clone,
    /// The git tree corresponding to [`Urn`] was already present locally, so
    /// the operation was equivalent to `git fetch`.
    Fetch,
}

enum ModeInternal {
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
#[tracing::instrument(skip(storage, fetcher, whoami))]
pub fn replicate<'a, F>(
    storage: &'a Storage,
    mut fetcher: F,
    config: Config,
    whoami: Option<LocalIdentity>,
) -> Result<ReplicateResult, Error>
where
    F: fetch::Fetcher<PeerId = PeerId, UrnId = Revision>,
    F::Error: std::error::Error + Send + Sync + 'static,
{
    let remote_peer = *fetcher.remote_peer();
    let local_peer_id = storage.peer_id();
    if local_peer_id == &remote_peer {
        return Err(Error::SelfReplication);
    }
    let urn = Urn::new(fetcher.urn().id);
    let (mut updated_tips, next) = determine_mode(
        storage,
        &mut fetcher,
        config.fetch_limit,
        urn.clone(),
        remote_peer,
    )?;
    let (result, mut remove) = match next {
        ModeInternal::Clone {
            urn,
            identity,
            fetched_peers,
        } => {
            let (allowed, id_status) = match identity {
                SomeIdentity::Project(proj) => {
                    let delegates = project::delegate_views(storage, proj, Some(remote_peer))?;
                    let mut allowed = delegates.keys().copied().collect::<BTreeSet<_>>();
                    let rad_id = unsafe_into_urn(
                        Reference::rad_id(Namespace::from(&urn)).with_remote(remote_peer),
                    );
                    let proj = project::verify_with_delegate(storage, &rad_id, Some(remote_peer))?;
                    let project::SetupResult {
                        updated_tips: mut project_tips,
                        identity: id_status,
                    } = project::ensure_setup(
                        storage,
                        &mut fetcher,
                        config.fetch_limit,
                        delegates,
                        &rad_id,
                        proj,
                    )?;
                    updated_tips.append(&mut project_tips);
                    let tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
                    allowed.extend(tracked);

                    (allowed, id_status)
                },
                SomeIdentity::Person(person) => {
                    let rad_id = unsafe_into_urn(
                        Reference::rad_id(Namespace::from(&person.urn())).with_remote(remote_peer),
                    );
                    let id_status = person::ensure_setup(storage, &rad_id, person.clone())?;
                    let allowed = person
                        .delegations()
                        .iter()
                        .copied()
                        .map(PeerId::from)
                        .collect();
                    (allowed, id_status)
                },

                unknown => return Err(Error::UnknownIdentityKind(unknown)),
            };

            // Symref `rad/self` if a `LocalIdentity` was given
            if let Some(local_id) = whoami {
                local_id.link(storage, &urn)?;
            }

            Ok::<_, Error>((
                ReplicateResult {
                    updated_tips,
                    identity: id_status,
                    mode: Mode::Clone,
                },
                fetched_peers.difference(&allowed).copied().collect(),
            ))
        },

        ModeInternal::Fetch {
            urn,
            identity,
            existing,
        } => {
            let (result, updated) = match identity {
                SomeIdentity::Project(proj) => {
                    let delegate_views = project::delegate_views(storage, proj, None)?;
                    let proj = project::verify_with_delegate(storage, &urn, None)?;
                    let mut updated_delegations = project::all_delegates(&proj);
                    let rad_id = unsafe_into_urn(Reference::rad_id(Namespace::from(&urn)));
                    let project::SetupResult {
                        updated_tips: mut project_tips,
                        identity: id_status,
                    } = project::ensure_setup(
                        storage,
                        &mut fetcher,
                        config.fetch_limit,
                        delegate_views,
                        &rad_id,
                        proj,
                    )?;
                    updated_tips.append(&mut project_tips);

                    let mut updated_tracked =
                        tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
                    updated_tracked.append(&mut updated_delegations);
                    (
                        ReplicateResult {
                            updated_tips,
                            identity: id_status,
                            mode: Mode::Fetch,
                        },
                        updated_tracked,
                    )
                },
                SomeIdentity::Person(person) => {
                    let rad_id = unsafe_into_urn(Reference::rad_id(Namespace::from(&person.urn())));
                    let id_status = person::ensure_setup(storage, &rad_id, person)?;
                    (
                        ReplicateResult {
                            updated_tips,
                            identity: id_status,
                            mode: Mode::Fetch,
                        },
                        tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>(),
                    )
                },

                unknown => return Err(Error::UnknownIdentityKind(unknown)),
            };

            let Partition { removed, .. } = partition(&existing, &updated);
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
#[tracing::instrument(skip(storage, fetcher, urn), fields(urn = %urn))]
fn determine_mode<F>(
    storage: &Storage,
    fetcher: &mut F,
    limit: fetch::Limit,
    urn: Urn,
    remote_peer: PeerId,
) -> Result<(BTreeMap<ext::RefLike, ext::Oid>, ModeInternal), Error>
where
    F: fetch::Fetcher<PeerId = PeerId>,
    F::Error: std::error::Error + Send + Sync + 'static,
{
    if !storage.has_urn(&urn)? {
        let updated = fetcher
            .fetch(fetch::Fetchspecs::PeekAll { limit })
            .map_err(|e| Error::Fetch(e.into()))?;
        let fetched_peers = project::fetched_peers(&updated)?;

        let mut tips = updated.updated_tips;
        // We can't fetch `refs/remotes/*/rad/ids/*` since we can't have two globs, so
        // we fetch `refs/remotes/{fetched_peer}/rad/ids/*`.
        let peeked = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: fetched_peers.clone(),
                limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;
        tips.extend(peeked.updated_tips);

        let remote_ident =
            unsafe_into_urn(Reference::rad_id(Namespace::from(&urn)).with_remote(remote_peer));
        Ok((
            tips,
            ModeInternal::Clone {
                urn,
                fetched_peers,
                identity: identities::any::get(storage, &remote_ident)?
                    .ok_or(Error::MissingIdentity)?,
            },
        ))
    } else {
        let identity = identities::any::get(storage, &urn)?.ok_or(Error::MissingIdentity)?;
        let existing = match identity {
            SomeIdentity::Project(ref proj) => {
                let mut remotes = project::all_delegates(proj);
                let mut tracked = tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>();
                remotes.append(&mut tracked);

                remotes
            },
            SomeIdentity::Person(_) => tracking::tracked(storage, &urn)?.collect::<BTreeSet<_>>(),

            unknown => return Err(Error::UnknownIdentityKind(unknown)),
        };

        let fetch::FetchResult { updated_tips } = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: existing.clone(),
                limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Ok((
            updated_tips,
            ModeInternal::Fetch {
                urn,
                identity,
                existing,
            },
        ))
    }
}

fn unsafe_into_urn(reference: Reference<git_ext::RefLike>) -> Urn {
    reference.try_into().expect("namespace is set")
}

/// Set the `rad/id` ref of `urn` to the given [`ext::Oid`].
///
/// No-op if the ref already exists. Returns the [`ext::Oid`] the ref points to
/// after the operation.
#[tracing::instrument(level = "trace", skip(storage, urn), fields(urn = %urn))]
fn ensure_rad_id(storage: &Storage, urn: &Urn, tip: ext::Oid) -> Result<ext::Oid, Error> {
    let id_ref = identities::common::IdRef::from(urn);
    id_ref
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))?;

    id_ref.oid(storage).map(Into::into).map_err(Error::Store)
}

fn adopt_rad_self(storage: &Storage, urn: &Urn, peer: PeerId) -> Result<(), Error> {
    let rad_self = Reference::rad_self(Namespace::from(urn), peer);

    // We only need to create the rad/id there's a rad/self
    if storage.has_ref(&rad_self)? {
        if let Some(person) =
            identities::person::verify(storage, &unsafe_into_urn(rad_self.clone()))?
        {
            let rad_id = unsafe_into_urn(Reference::rad_id(Namespace::from(person.urn())));
            if !storage.has_urn(&person.urn())? {
                ensure_rad_id(storage, &rad_id, person.content_id)?;
                symref(storage, &rad_id, rad_self)?;
                tracking::track(storage, &rad_id, peer)?;
            }
        }
    }

    Ok(())
}

fn symref(storage: &Storage, top_level: &Urn, symbolic: Reference<One>) -> Result<(), Error> {
    // Now point our view to the top-level
    Reference::try_from(top_level)
        .map_err(|e| Error::RefFromUrn {
            urn: top_level.clone(),
            source: e,
        })?
        .symbolic_ref::<_, PeerId>(symbolic, Force::False)
        .create(storage.as_raw())
        .and(Ok(()))
        .or_matches(is_exists_err, || Ok(()))
        .map_err(|e: git2::Error| Error::Store(e.into()))
}

/// Untrack the list of `PeerId`s, which also has the side-effect of removing
/// that peer's remote references in the storage.
///
/// **Note**: this function will return early on failure. This could mean that
/// remotes which were meant for pruning might not have been removed, resulting
/// in unnecessary remote references.
#[allow(clippy::unit_arg)]
#[tracing::instrument(
    level = "trace",
    skip(storage, urn, prune_list),
    fields(urn = %urn),
    err
)]
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

// Allowing dead code to keep the other fields
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Partition<A> {
    removed: BTreeSet<A>,
    added: BTreeSet<A>,
    kept: BTreeSet<A>,
}

// Return three sets where the first consists of elements in `ys` but not in
// `xs` and the second vice-versa, and the final set contains the elements they
// both share.
//
// If `ys` represents an "updated" set of `xs` then the first set will be all
// elements that were removed, the second set will be all the elements added,
// and the third set all the elements that stayed the same.
fn partition<'a, A: Clone + Ord>(xs: &'a BTreeSet<A>, ys: &'a BTreeSet<A>) -> Partition<A> {
    let mut removed = BTreeSet::new();
    let mut added = BTreeSet::new();
    let kept = xs.intersection(ys).cloned().collect();

    for e in xs.symmetric_difference(ys) {
        if xs.contains(e) {
            removed.insert(e.clone());
        } else {
            added.insert(e.clone());
        }
    }

    Partition {
        removed,
        added,
        kept,
    }
}

mod person {
    use super::*;

    /// Process the `Person` that was replicated by:
    ///   * Verifying the identity
    ///   * Tracking the delegations
    ///   * Ensuring we have a top-level `rad/id` that points to the latest
    ///     version
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage))]
    pub fn ensure_setup(
        storage: &Storage,
        rad_id: &Urn,
        person: Person,
    ) -> Result<IdStatus, Error> {
        let local_peer = storage.peer_id();
        let urn = person.urn();

        let delegations = match identities::person::verify(storage, rad_id)? {
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
                        adopt_rad_self(storage, &urn, *peer_id)?;
                    }
                }

                Ok(delegations)
            },
        }?;
        // Create `rad/id` here, if not exists
        adopt_latest(storage, &person.urn(), delegations)
    }

    /// Adopt the `rad/id` that has the most up-to-date commit from the set of
    /// `Person` delegates.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage))]
    pub fn adopt_latest(
        storage: &Storage,
        urn: &Urn,
        delegates: BTreeSet<PeerId>,
    ) -> Result<IdStatus, Error> {
        use IdStatus::*;

        let local_peer = storage.peer_id();
        let delegates: BTreeMap<PeerId, VerifiedPerson> = delegates
            .into_iter()
            .map(|peer| {
                let remote_urn =
                    unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(peer));
                let verified = identities::person::verify(storage, &remote_urn)?
                    .ok_or_else(|| Error::MissingIdentities(remote_urn.clone()))?;

                Ok((peer, verified))
            })
            .collect::<Result<_, Error>>()?;
        let latest = {
            let mut prev = None;
            for pers in delegates.values().cloned() {
                match prev {
                    None => prev = Some(pers),
                    Some(p) => {
                        let newer = identities::person::newer(storage, p, pers)?;
                        prev = Some(newer);
                    },
                }
            }
            prev.expect("empty delegations")
        };

        let expected = match delegates.get(local_peer) {
            Some(ours) => ours.content_id,
            None => latest.content_id,
        };
        let actual = ensure_rad_id(storage, urn, expected)?;
        if actual == expected {
            Ok(Even)
        } else {
            Ok(Uneven)
        }
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

    pub struct SetupResult {
        pub updated_tips: BTreeMap<ext::RefLike, ext::Oid>,
        pub identity: IdStatus,
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
    #[tracing::instrument(level = "trace", skip(storage, fetcher))]
    pub fn ensure_setup<F>(
        storage: &Storage,
        fetcher: &mut F,
        limit: fetch::Limit,
        delegates: BTreeMap<PeerId, project::DelegateView>,
        rad_id: &Urn,
        proj: VerifiedProject,
    ) -> Result<SetupResult, Error>
    where
        F: fetch::Fetcher<PeerId = PeerId, UrnId = Revision>,
        F::Error: std::error::Error + Send + Sync + 'static,
    {
        let local_peer = storage.peer_id();
        let urn = proj.urn();
        let id_status = self::adopt_latest(storage, &urn, &delegates)?;

        self::track_direct(storage, &proj)?;
        let (fetch_result, tracked) = replicate_signed_refs(
            storage,
            fetcher,
            limit,
            &urn,
            delegates
                .values()
                .map(|delegate| delegate.urn.clone())
                .collect(),
        )?;
        for peer in tracked {
            if peer != *local_peer {
                tracking::track(storage, &urn, peer)?;
                adopt_rad_self(storage, &urn, peer)?;
            }
        }

        Ok(SetupResult {
            updated_tips: fetch_result.updated_tips,
            identity: id_status,
        })
    }

    /// Fetch `rad/signed_refs` and `refs/heads` of the delegates and our
    /// tracked graph, returning the set of tracked peers.
    #[tracing::instrument(
        level = "trace",
        skip(storage, fetcher, urn),
        fields(urn = %urn),
        err
    )]
    pub fn replicate_signed_refs<F>(
        storage: &Storage,
        fetcher: &mut F,
        limit: fetch::Limit,
        urn: &Urn,
        delegates: BTreeSet<Urn>,
    ) -> Result<(fetch::FetchResult, BTreeSet<PeerId>), Error>
    where
        F: fetch::Fetcher<PeerId = PeerId, UrnId = Revision>,
        F::Error: std::error::Error + Send + Sync + 'static,
    {
        // Read `signed_refs` for all tracked
        let tracked = tracking::tracked(storage, urn)?.collect::<BTreeSet<_>>();
        let tracked_sigrefs = tracked
            .into_iter()
            .filter_map(|peer| match Refs::load(storage, urn, peer) {
                Ok(Some(refs)) => Some(Ok((peer, refs))),

                Ok(None) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        // Fetch all the rest
        tracing::debug!("fetching heads: {:?}, {:?}", tracked_sigrefs, delegates);
        let res = fetcher
            .fetch(fetch::Fetchspecs::Replicate {
                tracked_sigrefs: tracked_sigrefs.clone(),
                delegates,
                limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Refs::update(storage, urn)?;
        Ok((
            res,
            tracked_sigrefs
                .iter()
                .flat_map(|(peer, refs)| iter::once(*peer).chain(refs.remotes.flatten().copied()))
                .collect(),
        ))
    }

    /// For each delegate in `remotes/<remote_peer>/rad/ids/*` get the view for
    /// that delegate that _should_ be local the `storage` after a fetch.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage))]
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
                            let verified =
                                project::verify_with_delegate(storage, &urn, remote_peer)?;
                            (urn, verified)
                        } else {
                            let remote_urn = unsafe_into_urn(
                                Reference::rad_id(Namespace::from(&proj.urn()))
                                    .with_remote(peer_id),
                            );
                            adopt_delegate_person(storage, peer_id, &person, &proj.urn())?;
                            let verified =
                                project::verify_with_delegate(storage, &remote_urn, remote_peer)?;
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
    #[tracing::instrument(level = "trace", skip(storage))]
    pub fn adopt_delegate_person(
        storage: &Storage,
        peer: PeerId,
        person: &VerifiedPerson,
        project_urn: &Urn,
    ) -> Result<(), Error> {
        let delegate_urn = person.urn();

        // if the identity is known we see if we can fast-forward it
        if storage.has_urn(&delegate_urn)? {
            identities::person::fast_forward(storage, person)?;
        } else {
            ensure_rad_id(storage, &delegate_urn, person.content_id)?;
            tracking::track(storage, &delegate_urn, peer)?;
            tracking::track(storage, project_urn, peer)?;
        }

        // Now point our view to the top-level
        symref(
            storage,
            &delegate_urn,
            Reference::rad_delegate(Namespace::from(project_urn), &delegate_urn),
        )
    }

    /// Track all direct delegations of a `Project`.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage))]
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
    #[tracing::instrument(level = "trace", skip(storage))]
    pub fn adopt_latest(
        storage: &Storage,
        urn: &Urn,
        delegates: &BTreeMap<PeerId, DelegateView>,
    ) -> Result<IdStatus, Error> {
        use IdStatus::*;

        let local_peer = storage.peer_id();
        let latest = {
            let mut prev = None;
            for proj in delegates.values().map(|view| view.project.clone()) {
                match prev {
                    None => prev = Some(proj),
                    Some(p) => {
                        let newer = identities::project::newer(storage, p, proj)?;
                        prev = Some(newer);
                    },
                }
            }
            prev.expect("empty delegations")
        };

        let expected = match delegates.get(local_peer) {
            Some(ours) => ours.project.content_id,
            None => latest.content_id,
        };
        let actual = ensure_rad_id(storage, urn, expected)?;
        if actual == expected {
            Ok(Even)
        } else {
            Ok(Uneven)
        }
    }

    /// Using the fetched references we parse out the set of `PeerId`s that were
    /// fetched.
    pub fn fetched_peers(result: &fetch::FetchResult) -> Result<BTreeSet<PeerId>, Error> {
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

    pub fn verify_with_delegate<S>(
        storage: &S,
        urn: &Urn,
        peer: Option<PeerId>,
    ) -> Result<VerifiedProject, Error>
    where
        S: AsRef<storage::ReadOnly>,
    {
        let storage = storage.as_ref();
        identities::project::verify_with(storage, urn, |delegate| {
            let refname =
                Reference::rad_delegate(Namespace::from(urn.clone()), &delegate).with_remote(peer);
            storage.reference_oid(&refname).map(|oid| oid.into())
        })?
        .ok_or(Error::MissingIdentity)
    }
}
