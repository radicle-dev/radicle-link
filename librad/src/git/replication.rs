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
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{
    fetch::{self, CanFetch as _},
    identities::{self, local::LocalIdentity},
    refs,
    storage::{self, Storage},
    tracking,
    types::{reference, Force, Namespace, Reference},
};
use crate::{
    identities::git::{Project, SomeIdentity, VerifiedPerson, VerifiedProject},
    peer::PeerId,
};

pub use crate::identities::git::Urn;

pub mod person;
pub mod project;

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

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Track(#[from] tracking::Error),

    #[error("signer error")]
    Sign(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("fetcher error")]
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
pub enum IdStatus {
    /// Up-to-date, no further action is required.
    Even,
    /// Delegate tips are either behind or ahead. Interactive review is
    /// recommended.
    Uneven,
}

/// The "mode" `replicate` was operating in.
pub enum Mode {
    /// The git tree corresponding to [`Urn`] was previously **not** present
    /// locally, so the operation was equivalent to `git clone`.
    Clone,
    /// The git tree corresponding to [`Urn`] was already present locally, so
    /// the operation was equivalent to `git fetch`.
    Fetch,
}

pub struct Provider<A> {
    result: fetch::FetchResult,
    provider: PeerId,
    identity: A,
}

impl<A> Provider<A> {
    pub fn map<F, B>(self, f: F) -> Provider<B>
    where
        F: FnOnce(A) -> B,
    {
        Provider {
            result: self.result,
            provider: self.provider,
            identity: f(self.identity),
        }
    }
}

// TODO(finto): Maybe we don't need these
impl<A> Provider<Option<A>> {
    pub fn sequence(self) -> Option<Provider<A>> {
        Some(Provider {
            result: self.result,
            provider: self.provider,
            identity: self.identity?,
        })
    }
}

impl<A, E> Provider<Result<A, E>> {
    pub fn sequence(self) -> Result<Provider<A>, E> {
        Ok(Provider {
            result: self.result,
            provider: self.provider,
            identity: self.identity?,
        })
    }
}

impl Provider<SomeIdentity> {
    pub fn fetch(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        urn: &Urn,
        provider: PeerId,
    ) -> Result<Self, Error> {
        let peeked = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: Some(provider).into_iter().collect(),
                limit: config.fetch_limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        let rad_id = unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(provider));
        let identity = identities::any::get(storage, &rad_id)?.ok_or(Error::MissingIdentity)?;

        Ok(Self {
            result: peeked,
            provider,
            identity,
        })
    }

    pub fn try_verify(
        self,
        storage: &Storage,
    ) -> Result<Either<Provider<VerifiedPerson>, Provider<Project>>, Error> {
        Ok(match self.identity {
            SomeIdentity::Person(person) => {
                let rad_id = unsafe_into_urn(
                    Reference::rad_id(Namespace::from(person.urn())).with_remote(self.provider),
                );
                tracing::debug!(urn = %rad_id, "verifying provider");
                let person =
                    identities::person::verify(storage, &rad_id)?.ok_or(Error::MissingIdentity)?;
                Either::Left(Provider {
                    result: self.result,
                    provider: self.provider,
                    identity: person,
                })
            },
            SomeIdentity::Project(project) => Either::Right(Provider {
                result: self.result,
                provider: self.provider,
                identity: project,
            }),
        })
    }
}

impl Provider<VerifiedProject> {
    pub fn delegates(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.identity
            .delegations()
            .into_iter()
            .flat_map(|delegate| match delegate {
                Either::Left(key) => Either::Left(iter::once(PeerId::from(*key))),
                Either::Right(person) => Either::Right(
                    person
                        .delegations()
                        .into_iter()
                        .map(|key| PeerId::from(*key)),
                ),
            })
    }
}

impl Provider<Project> {
    pub fn delegates(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.identity
            .delegations()
            .into_iter()
            .flat_map(|delegate| match delegate {
                Either::Left(key) => Either::Left(iter::once(PeerId::from(*key))),
                Either::Right(person) => Either::Right(
                    person
                        .delegations()
                        .into_iter()
                        .map(|key| PeerId::from(*key)),
                ),
            })
    }
}
impl Provider<VerifiedPerson> {
    pub fn delegates(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.identity
            .delegations()
            .iter()
            .copied()
            .map(PeerId::from)
    }
}

pub struct Delegates<A> {
    result: fetch::FetchResult,
    fetched: BTreeSet<PeerId>,
    views: A,
}

#[derive(Clone, Debug)]
pub struct Tracked {
    remotes: BTreeSet<PeerId>,
}

impl Tracked {
    pub fn new(
        storage: &Storage,
        urn: &Urn,
        remotes: impl Iterator<Item = PeerId>,
    ) -> Result<Self, Error> {
        let local = storage.peer_id();
        remotes
            .filter(|remote| remote != local)
            .map(|remote| tracking::track(storage, urn, remote).map(|_| remote))
            .collect::<Result<_, _>>()
            .map(|remotes| Self { remotes })
            .map_err(Error::Track)
    }

    pub fn load(storage: &Storage, urn: &Urn) -> Result<Self, Error> {
        Ok(Tracked {
            remotes: tracking::tracked(storage, urn)?.into_iter().collect(),
        })
    }
}

/// The peers that are ripe for pruning since they were removed from the
/// tracking graph.
pub struct Pruned {
    remotes: BTreeSet<PeerId>,
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
#[tracing::instrument(skip(storage, whoami, urn, addr_hints), fields(urn = %urn), err)]
pub fn replicate<Addrs>(
    storage: &Storage,
    config: Config,
    whoami: Option<LocalIdentity>,
    urn: Urn,
    remote_peer: PeerId,
    addr_hints: Addrs,
) -> Result<ReplicateResult, Error>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    println!("here?");
    let urn = Urn::new(urn.id);
    let local_peer_id = storage.peer_id();

    if local_peer_id == &remote_peer {
        return Err(Error::SelfReplication);
    }

    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;
    let result = if storage.has_urn(&urn)? {
        match identities::any::get(storage, &urn)?.ok_or(Error::MissingIdentity)? {
            SomeIdentity::Person(_) => person::fetch(storage, &mut fetcher, config, &urn)?.into(),
            SomeIdentity::Project(_) => project::fetch(storage, &mut fetcher, config, &urn)?.into(),
        }
    } else {
        let provider = Provider::fetch(storage, &mut fetcher, config, &urn, remote_peer)?;
        let provider_tips = provider.result.updated_tips.clone();
        tracing::debug!(tips = ?provider_tips, "provider tips");
        let mut result: ReplicateResult = match provider.try_verify(storage)? {
            Either::Left(person) => person::clone(storage, &mut fetcher, config, person)?.into(),
            Either::Right(project) => {
                project::clone(storage, &mut fetcher, config, project)?.into()
            },
        };

        // Symref `rad/self` if a `LocalIdentity` was given
        if let Some(local_id) = whoami {
            local_id.link(storage, &urn)?;
        }
        result.updated_tips.extend(provider_tips);
        result
    };

    // TODO: At this point, the tracking graph may have changed, and/or we
    // created top-level person namespaces. We will eventually converge, but
    // perhaps we'd want to return some kind of continuation here, so the caller
    // could schedule a deferred task directly?
    Ok(result)
}

fn unsafe_into_urn(reference: Reference<git_ext::RefLike>) -> Urn {
    reference.try_into().expect("namespace is set")
}

/// Set the `rad/id` ref of `urn` to the given [`ext::Oid`].
///
/// No-op if the ref already exists. Returns the [`ext::Oid`] the ref points to
/// after the operation.
#[tracing::instrument(level = "trace", skip(storage, urn), fields(urn = %urn), err)]
fn ensure_rad_id(storage: &Storage, urn: &Urn, tip: ext::Oid) -> Result<ext::Oid, Error> {
    let id_ref = identities::common::IdRef::from(urn);
    id_ref
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))?;

    id_ref
        .oid(storage)
        .map(Into::into)
        .map_err(|e| Error::Store(e.into()))
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

#[cfg(test)]
mod test {
    use proptest::{collection, prelude::*};

    use super::*;

    proptest! {
        #[test]
        fn annihilated(xs in collection::btree_set(0u32..1000, 0..100)) {
            annihilated_prop(xs)
        }

        #[test]
        fn full(ys in collection::btree_set(0u32..1000, 0..100)) {
            full_prop(ys)
        }

        #[test]
        fn kept(xs in collection::btree_set(0u32..1000, 0..100), ys in collection::btree_set(0u32..1000, 0..100)) {
            kept_prop(xs, ys)
        }
    }

    #[test]
    fn partitioning() {
        let xs = vec![1, 2, 3, 4, 5].into_iter().collect();
        let ys = vec![3, 4, 5, 6, 7].into_iter().collect();
        let expected = Partition {
            removed: vec![1, 2].into_iter().collect(),
            added: vec![6, 7].into_iter().collect(),
            kept: vec![3, 4, 5].into_iter().collect(),
        };
        assert_eq!(partition(&xs, &ys), expected);
    }

    /// If the `ys` parameter to partition is empty then `xs` is considered
    /// removed.
    fn annihilated_prop(xs: BTreeSet<u32>) {
        assert_eq!(
            partition(&xs, &BTreeSet::new()),
            Partition {
                removed: xs.clone(),
                added: BTreeSet::new(),
                kept: BTreeSet::new()
            }
        )
    }

    /// If the `xs` parameter to partition is empty then `ys` is considered
    /// added.
    fn full_prop(ys: BTreeSet<u32>) {
        assert_eq!(
            partition(&BTreeSet::new(), &ys),
            Partition {
                removed: BTreeSet::new(),
                added: ys.clone(),
                kept: BTreeSet::new()
            }
        )
    }

    /// The intersection of `xs` and `ys` is always kept
    fn kept_prop(xs: BTreeSet<u32>, ys: BTreeSet<u32>) {
        assert_eq!(
            partition(&xs, &ys).kept,
            xs.intersection(&ys).copied().collect()
        )
    }
}
