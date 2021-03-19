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
    identities::git::{Person, Project, SomeIdentity, VerifiedPerson, VerifiedProject},
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
        let mut result: ReplicateResult = match provider.determine_identity() {
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

    pub fn determine_identity(self) -> Either<Provider<Person>, Provider<Project>> {
        match self.identity {
            SomeIdentity::Person(person) => Either::Left(Provider {
                result: self.result,
                provider: self.provider,
                identity: person,
            }),
            SomeIdentity::Project(project) => Either::Right(Provider {
                result: self.result,
                provider: self.provider,
                identity: project,
            }),
        }
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

impl Provider<Person> {
    pub fn verify(self, storage: &Storage) -> Result<Provider<VerifiedPerson>, Error> {
        let remote = self.provider;
        self.map(|identity| {
            let urn = unsafe_into_urn(
                Reference::rad_id(Namespace::from(&identity.urn())).with_remote(remote),
            );
            identities::person::verify(storage, &urn)
                .map_err(Error::from)
                .and_then(|person| person.ok_or(Error::MissingIdentity))
        })
        .sequence()
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

/// The peers that are part of the identity's tracking graph.
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

    pub fn removed(&self, latest: &Self) -> BTreeSet<PeerId> {
        self.remotes.iter().filter(|remote| !latest.remotes.contains(remote)).copied().collect()
    }

    /// Readable format for the tracked peers
    fn trace(&self) -> Vec<String> {
        self.remotes.iter().map(|peer| peer.to_string()).collect()
    }
}

/// The peers that are ripe for pruning since they were removed from the
/// tracking graph.
pub struct Pruned {
    remotes: BTreeSet<PeerId>,
}

impl Pruned {
    pub fn new(
        storage: &Storage,
        urn: &Urn,
        remotes: impl Iterator<Item = PeerId>,
    ) -> Result<Self, Error> {
        remotes
            .map(|remote| tracking::track(storage, urn, remote).map(|_| remote))
            .collect::<Result<_, _>>()
            .map(|remotes| Self { remotes })
            .map_err(Error::Track)
    }

    pub fn empty() -> Self {
        Pruned { remotes: BTreeSet::new() }
    }

    /// Readable format for the pruned peers
    fn trace(&self) -> Vec<String> {
        self.remotes.iter().map(|peer| peer.to_string()).collect()
    }
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
