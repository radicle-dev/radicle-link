// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    net::SocketAddr,
};

use either::Either;
use git_ext::{self as ext, is_exists_err};
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

    #[error("no reference tip found for: {0}")]
    MissingTip(Urn),

    #[error("missing required ref: {0}")]
    Missing(ext::RefLike),

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

pub enum Setup {
    Project(Project),
    Person(Person),
}

pub enum Replication {
    Clone {
        urn: Urn,
        setup: Setup,
        fetched_peers: BTreeSet<PeerId>,
    },
    Fetch {
        urn: Urn,
        setup: Setup,
    },
}

fn replication(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    urn: Urn,
    remote_peer: PeerId,
) -> Result<Replication, Error> {
    let remote_ident: Urn = Reference::rad_id(Namespace::from(&urn))
        .with_remote(remote_peer)
        .try_into()
        .expect("namespace is set");

    if !storage.has_urn(&urn)? {
        // TODO(finto): If we do a shotgun fetch we might fetch our own remote?
        // TODO(finto): We can shotgun fetch but we can't fetch rad/ids/\* because of
        // one-level
        let fetched_peers = fetcher
            .fetch(fetch::Fetchspecs::All)
            .map_err(|e| Error::Fetch(e.into()))
            .and_then(project::fetched_peers)?;

        let setup = match identities::any::get(storage, &remote_ident)? {
            None => return Err(Error::MissingIdentity),
            Some(some_id) => match some_id {
                SomeIdentity::Person(person) => Setup::Person(person),
                SomeIdentity::Project(project) => Setup::Project(project),
            },
        };

        Ok(Replication::Clone {
            urn,
            fetched_peers,
            setup,
        })
    } else {
        let setup = match identities::any::get(storage, &remote_ident)? {
            None => return Err(Error::MissingIdentity),
            Some(some_id) => match some_id {
                SomeIdentity::Person(person) => Setup::Person(person),
                SomeIdentity::Project(project) => Setup::Project(project),
            },
        };
        Ok(Replication::Fetch { urn, setup })
    }
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
) -> Result<(), Error>
where
    Addrs: IntoIterator<Item = SocketAddr>,
{
    if storage.peer_id() == &remote_peer {
        return Err(Error::SelfReplication);
    }

    let urn = Urn::new(urn.id);
    let mut fetcher = storage.fetcher(urn.clone(), remote_peer, addr_hints)?;
    match replication(storage, &mut fetcher, urn, remote_peer)? {
        Replication::Clone {
            urn,
            setup,
            fetched_peers,
        } => {
            let allowed = match setup {
                Setup::Project(proj) => {
                    let delegates = project::delegate_views(storage, proj, remote_peer)?;
                    let allowed = delegates.keys().copied().collect();
                    project::ensure_setup(storage, &mut fetcher, delegates, &urn, remote_peer)?;
                    allowed
                },
                Setup::Person(person) => {
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

            // Remove any remote tracking branches we don't need
            let prune_list = fetched_peers.difference(&allowed);
            prune(storage, &urn, prune_list);

            Ok(())
        },
        Replication::Fetch { urn, setup } => {
            // Fetch what we know
            // Check delegations of old identity
            // 1. Track new delegations + fetch them
            // 2. Prune old delegations
            match setup {
                Setup::Project(proj) => {
                    let delegations = project::all_delegates(&proj);
                    let _ = fetcher
                        .fetch(fetch::Fetchspecs::Peek {
                            remotes: delegations.clone(),
                        })
                        .map_err(|e| Error::Fetch(e.into()))?;
                    let proj = identities::project::verify(storage, &urn)?
                        .ok_or(Error::MissingIdentity)?;
                    let updated_delegations = project::all_delegates(&proj);
                    let delegate_views =
                        project::delegate_views(storage, proj.into_inner(), remote_peer)?;
                    project::replicate_signed_refs(
                        storage,
                        &mut fetcher,
                        &urn,
                        delegate_views
                            .values()
                            .map(|view| view.urn.clone())
                            .collect(),
                    )?;

                    let (removed, _) = disjoint_difference(&delegations, &updated_delegations);
                    prune(storage, &urn, removed.iter());
                },
                Setup::Person(person) => {
                    // FIXME: Probably need to do more than this
                    person::ensure_setup(storage, person, remote_peer)?;
                },
            }

            Ok(())
        },
    }

    // TODO: At this point, the tracking graph may have changed, and/or we
    // created top-level person namespaces. We will eventually converge, but
    // perhaps we'd want to return some kind of continuation here, so the caller
    // could schedule a deferred task directly?
}

#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage), err)]
fn ensure_rad_id(storage: &Storage, urn: &Urn, tip: ext::Oid) -> Result<(), Error> {
    identities::common::IdRef::from(urn)
        .create(storage, tip)
        .map_err(|e| Error::Store(e.into()))
}

// TODO(finto): Should this raise an error?
#[allow(clippy::unit_arg)]
#[tracing::instrument(level = "trace", skip(storage, prune_list))]
pub fn prune<'a>(storage: &Storage, urn: &Urn, prune_list: impl Iterator<Item = &'a PeerId>) {
    for peer in prune_list {
        match tracking::untrack(storage, urn, *peer) {
            Ok(removed) => {
                if removed {
                    tracing::trace!("pruned `{}`", peer);
                } else {
                    tracing::trace!("attempted to prune `{}` but it did not exist", peer);
                }
            },
            Err(err) => {
                tracing::warn!("failed to prune `{}`\nreason: {}", peer, err);
            },
        }
    }
}

// Return two sets where the first consists of elements in `ys` but not in `xs`
// and the second vice-versa.
//
// If `ys` represents an "updated" set of `xs` then the first set will be all
// elements that were removed and the second set will be all the elements added.
fn disjoint_difference<'a, A: Clone + Ord>(
    xs: &'a BTreeSet<A>,
    ys: &'a BTreeSet<A>,
) -> (BTreeSet<A>, BTreeSet<A>) {
    let mut left = BTreeSet::new();
    let mut right = BTreeSet::new();

    for e in xs.symmetric_difference(ys) {
        if xs.contains(e) {
            right.insert(e.clone());
        } else {
            left.insert(e.clone());
        }
    }

    (left, right)
}

mod person {
    use super::*;

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn ensure_setup(
        storage: &Storage,
        person: Person,
        remote_peer: PeerId,
    ) -> Result<(), Error> {
        let urn: Urn = Reference::rad_id(Namespace::from(person.urn()))
            .with_remote(remote_peer)
            .try_into()
            .expect("namespace is set");

        match identities::person::verify(storage, &urn)? {
            None => Err(Error::MissingIdentity),
            Some(person) => {
                // Create `rad/id` here, if not exists
                ensure_rad_id(storage, &urn, person.content_id)?;

                // Track all delegations
                for key in person.into_inner().doc.delegations {
                    tracking::track(storage, &urn, PeerId::from(key))?;
                }

                Ok(())
            },
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

    pub fn ensure_setup(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        delegates: BTreeMap<PeerId, project::DelegateView>,
        urn: &Urn,
        remote_peer: PeerId,
    ) -> Result<(), Error> {
        let proj = project::verify(storage, urn.clone(), remote_peer)?;
        project::ensure_no_forking(
            storage,
            &urn,
            remote_peer,
            delegates.values().map(|view| view.urn.clone()).collect(),
        )?;
        project::track_direct(storage, &proj)?;
        replicate_signed_refs(
            storage,
            fetcher,
            urn,
            delegates
                .values()
                .map(|delegate| delegate.urn.clone())
                .collect(),
        )?;
        project::adopt_latest(storage, &urn, delegates)?;
        Ok(())
    }

    pub fn replicate_signed_refs(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        urn: &Urn,
        delegates: BTreeSet<Urn>,
    ) -> Result<(), Error> {
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
                tracked_sigrefs,
                delegates,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Refs::update(storage, &urn)?;
        Ok(())
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn verify(storage: &Storage, urn: Urn, remote: PeerId) -> Result<VerifiedProject, Error> {
        let urn: Urn = Reference::rad_id(Namespace::from(urn))
            .with_remote(remote)
            .try_into()
            .expect("namespace is set");

        Ok(identities::project::verify(storage, &urn)?.ok_or(Error::MissingIdentity)?)
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "trace", skip(storage), err)]
    pub fn ensure_no_forking(
        storage: &Storage,
        urn: &Urn,
        remote_peer: PeerId,
        delegates: BTreeSet<Urn>,
    ) -> Result<(), Error> {
        // Get the remote's view
        // Get the delegates' views
        // Validate their histories
        let remote: Urn = Reference::rad_id(Namespace::from(urn.clone()))
            .with_remote(remote_peer)
            .try_into()
            .expect("namespace is set");

        for delegate in delegates.iter() {
            match identities::project::is_fork(&storage, &remote, &delegate) {
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
    pub fn delegate_views(
        storage: &Storage,
        proj: Project,
        remote_peer: PeerId,
    ) -> Result<BTreeMap<PeerId, DelegateView>, Error> {
        let mut delegate_views = BTreeMap::new();
        let local_peer_id = storage.peer_id();
        for delegate in proj.delegations().iter().indirect() {
            let in_rad_ids: Urn =
                Reference::rad_delegate(Namespace::from(&proj.urn()), &delegate.urn())
                    .with_remote(remote_peer)
                    .try_into()
                    .expect("namespace is set");
            match identities::person::verify(storage, &in_rad_ids)? {
                None => return Err(Error::Missing(in_rad_ids.into())),
                Some(delegate_person) => {
                    let person = delegate_person.clone();
                    for key in delegate_person.delegations().iter() {
                        let peer_id = PeerId::from(*key);
                        if &peer_id == local_peer_id {
                            continue;
                        } else {
                            let remote_urn: Urn = Reference::rad_id(Namespace::from(&proj.urn()))
                                .with_remote(peer_id)
                                .try_into()
                                .expect("namespace is set");
                            adopt_delegate_person(storage, peer_id, &person, &proj.urn())?;
                            let verified = identities::project::verify(storage, &remote_urn)?;
                            match verified {
                                None => continue,
                                Some(verified) => {
                                    delegate_views.insert(
                                        peer_id,
                                        DelegateView {
                                            urn: remote_urn,
                                            delegate: person.clone(),
                                            project: verified,
                                        },
                                    );
                                },
                            }
                        }
                    }
                },
            }
        }

        Ok(delegate_views)
    }

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

    pub fn adopt_latest(
        storage: &Storage,
        urn: &Urn,
        delegates: BTreeMap<PeerId, DelegateView>,
    ) -> Result<(), Error> {
        let projects = delegates
            .values()
            .map(|view| view.project.clone().into_inner());
        let commit = identities::project::latest_tip(storage, projects)?;
        match commit {
            None => Err(Error::MissingTip(urn.clone())),
            Some(tip) => ensure_rad_id(storage, urn, tip.into()),
        }
    }

    pub fn fetched_peers(result: fetch::FetchResult) -> Result<BTreeSet<PeerId>, Error> {
        use std::str::FromStr;

        let mut peers = BTreeSet::new();
        for reference in result.updated_tips.keys() {
            let path: ext::RefLike = match Urn::try_from(reference.clone()).map(|urn| urn.path) {
                Ok(Some(path)) => path,
                Ok(None) | Err(_) => {
                    /* prune reference */
                    continue;
                },
            };
            let suffix = match path.strip_prefix(reflike!("refs/remotes")) {
                Ok(suffix) => suffix,
                Err(_) => continue,
            };
            let peer = match suffix.as_str().split('/').next().map(PeerId::from_str) {
                None | Some(Err(_)) => {
                    /* prune reference */
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
