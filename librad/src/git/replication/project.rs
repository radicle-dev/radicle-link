// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

use either::Either;

use crate::git::{
    fetch,
    refs::Refs,
    storage::Storage,
    types::{Namespace, Reference},
};

use super::*;

use crate::{
    identities::git::{Project, VerifiedPerson, VerifiedProject},
    peer::PeerId,
};

pub struct ProjectDelegates<P>(Delegates<Vec<DelegateView<P>>>);

impl<P> From<Delegates<Vec<DelegateView<P>>>> for ProjectDelegates<P> {
    fn from(delegates: Delegates<Vec<DelegateView<P>>>) -> Self {
        ProjectDelegates(delegates)
    }
}

/// Clone the [`Project`] from the `provider` by fetching the delegates in the
/// document.
///
/// We track all the delegates in the document and adopt the `rad/id` for this
/// identity.
pub fn clone(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    provider: Provider<Project>,
) -> Result<ReplicateResult, Error> {
    let provider_id = provider.provider;
    let urn = provider.identity.urn();
    let delegates =
        ProjectDelegates::from_provider(storage, fetcher, config, provider)?.verify(storage)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let signed_refs = SignedRefs::fetch(storage, fetcher, config, &urn, &delegates, &tracked)?;
    let identity = delegates.adopt(storage, &urn)?;

    let pruned = if delegates.remotes().any(|remote| remote == provider_id) {
        Pruned::new(storage, &urn, Some(provider_id).into_iter())?
    } else {
        Pruned::empty()
    };

    Ok(mk_replicate_result(
        delegates,
        tracked,
        pruned,
        signed_refs,
        identity,
        Mode::Clone,
    ))
}

/// Fetch the latest changes for the remotes that we are tracking for `urn`.
///
/// If there are any new delegates we track them. Following that, we
/// [`adopt`][`ProjectDelegates::adopt`] the latest tip if necessary.
///
/// **Note**: new delegates could be removed or added, these are not fetched
/// immediately, but instead added to the tracking graph. This means
/// that we wait for another pass of replication to fetch those, and so
/// on.
pub fn fetch(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    urn: &Urn,
) -> Result<ReplicateResult, Error> {
    let previous = Tracked::load(storage, urn)?;
    let delegates =
        ProjectDelegates::from_local(storage, fetcher, config, urn, previous.clone())?.verify(storage)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let signed_refs = SignedRefs::fetch(storage, fetcher, config, &urn, &delegates, &tracked)?;
    let identity = delegates.adopt(storage, urn)?;
    let pruned = Pruned::new(storage, urn, previous.removed(&tracked).into_iter())?;

    Ok(mk_replicate_result(
        delegates,
        tracked,
        pruned,
        signed_refs,
        identity,
        Mode::Fetch,
    ))
}

#[tracing::instrument(skip(delegates, tracked, pruned, signed_refs))]
fn mk_replicate_result(
    delegates: ProjectDelegates<VerifiedProject>,
    tracked: Tracked,
    pruned: Pruned,
    signed_refs: SignedRefs,
    identity: IdStatus,
    mode: Mode,
) -> ReplicateResult {
    let mut updated_tips = delegates.0.result.updated_tips;
    tracing::debug!(tips = ?updated_tips, "tips for delegates fetch");

    let sigref_tips = signed_refs.result.updated_tips;
    tracing::debug!(tips = ?sigref_tips, "tips for rad/signed_refs");
    tracing::debug!(tracked = ?signed_refs.tracked.trace(), "tracked peers");
    updated_tips.extend(sigref_tips);

    tracing::debug!(tracked = ?tracked.trace(), "tracked peers");
    tracing::debug!(pruned = ?pruned.trace(), "pruned peers");

    ReplicateResult {
        updated_tips,
        identity,
        mode,
    }
}

/// Delegates for [`Project`]s can either be direct, using only a [`PeerId`], or
/// indirect, using a [`Person`] identity.
///
/// `DelegateView` is parametrised over `P` since we cannot directly construct
/// one for a [`VerifiedProject`]. This is because we must adopt any
/// [`VerifiedPerson`] in the `Indirect` case for verification. We can however
/// get a [`Project`].
#[derive(Clone)]
pub enum DelegateView<P> {
    /// The delegate remains anonymous and only goes by their `PeerId`.
    Direct { remote: PeerId, project: P },
    /// The delegate is using a [`Person`] identity to delegate for the project.
    /// The [`Person`] in turn has one or more `PeerId`s associated with it,
    /// and so we can have one or more remote entries for this particular
    /// project.
    ///
    /// Note: the entries in `remotes` SHOULD be the keys of `projects`. They're
    /// copied for convenience.
    Indirect {
        person: VerifiedPerson,
        projects: BTreeMap<PeerId, P>,
        remotes: BTreeSet<PeerId>,
    },
}

impl<P> DelegateView<P> {
    pub fn views(&'_ self) -> impl Iterator<Item = (PeerId, P)> + '_
    where
        P: Clone,
    {
        match self {
            Self::Direct { remote, project } => {
                Either::Left(iter::once((*remote, project.clone())))
            },
            Self::Indirect { projects, .. } => Either::Right(
                projects
                    .iter()
                    .map(|(remote, project)| (*remote, project.clone())),
            ),
        }
    }
}

impl DelegateView<Project> {
    /// Construct the `Direct` variant by calling
    /// [`get`][`identities::project::get`] for the [`Project`].
    pub fn direct(storage: &Storage, urn: &Urn, remote: PeerId) -> Result<Self, Error> {
        let urn = unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(remote));
        let project = identities::project::get(storage, &urn)?.ok_or(Error::MissingIdentity)?;
        Ok(DelegateView::Direct { remote, project })
    }

    /// Construct the `Indirect` variant. We must
    /// [`verify`][`identities::person::verify`] the person identity. For
    /// each key in the [`VerifiedPerson`] we attempt to
    /// [`get`][`identities::project::get`] the [`Project`].
    pub fn indirect<P>(storage: &Storage, urn: &Urn, delegate: &Urn, who: P) -> Result<Self, Error>
    where
        P: Into<Option<PeerId>>,
    {
        let who = who.into();
        let local = storage.peer_id();
        let in_rad_ids = unsafe_into_urn(
            Reference::rad_delegate(Namespace::from(urn), delegate).with_remote(who),
        );
        let mut projects = BTreeMap::new();
        let mut remotes = BTreeSet::new();
        let person =
            identities::person::verify(storage, &in_rad_ids)?.ok_or(Error::MissingIdentity)?;
        for key in person.delegations().iter() {
            let remote = PeerId::from(*key);
            if &remote == local {
                let project =
                    identities::project::get(storage, urn)?.ok_or(Error::MissingIdentity)?;
                projects.insert(remote, project);
                remotes.insert(remote);
            } else {
                let urn =
                    unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(remote));
                if let Some(project) = identities::project::get(storage, &urn)? {
                    projects.insert(remote, project);
                    remotes.insert(remote);
                }
            };
        }

        if projects.is_empty() {
            Err(Error::NoTrustee)
        } else {
            Ok(Self::Indirect {
                person,
                projects,
                remotes,
            })
        }
    }

    /// Verify the [`Project`] in the `DelegateView`, turning them into
    /// [`VerifiedProject`]s.
    ///
    /// For `Direct` it's straight-forward since there is no indirection on the
    /// delegation.
    ///
    /// For `Indirect` we must first adopt the [`VerifiedProject`] associated
    /// and then verify the project.
    pub fn verify(self, storage: &Storage) -> Result<DelegateView<VerifiedProject>, Error> {
        match self {
            DelegateView::Direct { remote, project } => {
                let urn = unsafe_into_urn(
                    Reference::rad_id(Namespace::from(&project.urn())).with_remote(remote),
                );
                let project =
                    identities::project::verify(storage, &urn)?.ok_or(Error::MissingIdentity)?;
                Ok(DelegateView::Direct { remote, project })
            },
            DelegateView::Indirect {
                person,
                projects,
                remotes,
            } => {
                let projects = projects
                    .into_iter()
                    .map(|(remote, project)| {
                        let urn = unsafe_into_urn(
                            Reference::rad_id(Namespace::from(&project.urn())).with_remote(remote),
                        );
                        adopt_direct(storage, &person, remotes.iter().copied(), &urn).and_then(|tracked| {
                            tracing::debug!(tracked = ?tracked.trace(), urn = %person.urn(), "tracked peers for delegate");
                            identities::project::verify(storage, &urn)
                                .map_err(Error::from)
                                .and_then(|project| {
                                    project
                                        .ok_or(Error::MissingIdentity)
                                        .map(|project| (remote, project))
                                })
                        })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(DelegateView::Indirect {
                    person,
                    projects,
                    remotes,
                })
            },
        }
    }
}

fn adopt_direct(
    storage: &Storage,
    person: &VerifiedPerson,
    remotes: impl Iterator<Item = PeerId>,
    project_urn: &Urn,
) -> Result<Tracked, Error> {
    let urn = person.urn();
    let tracked = Tracked::new(storage, &urn, remotes)?;

    ensure_rad_id(storage, &urn, person.content_id)?;
    // Now point our view to the top-level
    Reference::try_from(&urn)
        .map_err(|e| Error::RefFromUrn {
            urn: urn.clone(),
            source: e,
        })?
        .symbolic_ref::<_, PeerId>(
            Reference::rad_delegate(Namespace::from(project_urn), &urn),
            Force::False,
        )
        .create(storage.as_raw())
        .and(Ok(()))
        .or_matches(is_exists_err, || Ok(()))
        .map_err(|e: git2::Error| Error::Store(e.into()))?;

    Ok(tracked)
}

impl<P> ProjectDelegates<P> {
    pub fn remotes(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.0.views.iter().flat_map(|view| match view {
            DelegateView::Direct { remote, .. } => Either::Left(iter::once(*remote)),
            DelegateView::Indirect { remotes, .. } => Either::Right(remotes.iter().copied()),
        })
    }

    pub fn rad_ids(&'_ self) -> impl Iterator<Item = Urn> + '_
    where
        P: std::ops::Deref<Target = Project>,
    {
        self.0.views.iter().flat_map(|view| match view {
            DelegateView::Direct { remote, project } => Either::Left(iter::once(unsafe_into_urn(
                Reference::rad_id(Namespace::from(project.urn())).with_remote(*remote),
            ))),
            DelegateView::Indirect { projects, .. } => {
                Either::Right(projects.iter().map(|(remote, project)| {
                    unsafe_into_urn(
                        Reference::rad_id(Namespace::from(project.urn())).with_remote(*remote),
                    )
                }))
            },
        })
    }
}

impl ProjectDelegates<VerifiedProject> {
    /// Using the delegates we determine the latest tip for `rad/id`.
    ///
    /// If we are one of the delegates then we keep our own tip and determine
    /// the [`IdStatus`] by comparing our tip to the latest.
    ///
    /// Otherwise, we adopt the latest tip for our version of `rad/id`.
    pub fn adopt(&self, storage: &Storage, urn: &Urn) -> Result<IdStatus, Error> {
        use IdStatus::*;

        let local = storage.peer_id();
        let mut ours = None;
        let latest = {
            let mut prev = None;
            for (remote, proj) in self
                .0
                .views
                .iter()
                .cloned()
                .flat_map(|view| view.views().collect::<Vec<_>>().into_iter())
            {
                if remote == *local {
                    ours = Some(proj.clone());
                }
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

        let expected = match ours {
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

impl ProjectDelegates<Project> {
    /// Looking at the delegates of using the `provider`'s view we build up a
    /// set of [`DelegateView<Project>`]. At this point we have only fetched the
    /// delegates and we haven't verified them.
    pub fn from_provider(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        proivder: Provider<Project>,
    ) -> Result<Self, Error> {
        Self::from_identity(
            storage,
            fetcher,
            config,
            proivder.identity.clone(),
            proivder.delegates().collect(),
            proivder.provider,
        )
    }

    /// We use the `tracked` set of peers to fetch any updates for them, and
    /// build up a set of [`DelegateView<Project>`] from our local view of
    /// the [`Project`].
    pub fn from_local(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        urn: &Urn,
        tracked: Tracked,
    ) -> Result<Self, Error> {
        let project = identities::project::get(storage, urn)?.ok_or(Error::MissingIdentity)?;
        Self::from_identity(storage, fetcher, config, project, tracked.remotes, None)
    }

    /// Verify the [`Project`]s in the delegate set, see
    /// [`DelegateView::verify`].
    pub fn verify(self, storage: &Storage) -> Result<ProjectDelegates<VerifiedProject>, Error> {
        let ProjectDelegates(Delegates {
            result,
            fetched,
            views,
        }) = self;
        let views = views
            .into_iter()
            .map(|view| view.verify(storage))
            .collect::<Result<_, _>>()?;
        Ok(ProjectDelegates(Delegates {
            result,
            fetched,
            views,
        }))
    }

    #[allow(clippy::unit_arg)]
    #[tracing::instrument(skip(storage, fetcher, project, remotes, who), fields(project.urn = %project.urn()), err)]
    fn from_identity<P>(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        project: Project,
        remotes: BTreeSet<PeerId>,
        who: P,
    ) -> Result<Self, Error>
    where
        P: Into<Option<PeerId>> + Clone,
    {
        let mut delegates = vec![];
        let urn = project.urn();

        let peeked = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: remotes.clone(),
                limit: config.fetch_limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        for delegate in project.delegations().into_iter() {
            match delegate {
                Either::Left(key) => {
                    let remote = PeerId::from(*key);
                    delegates.push(DelegateView::direct(storage, &urn, remote)?);
                },
                Either::Right(person) => {
                    let indirect =
                        DelegateView::indirect(storage, &urn, &person.urn(), who.clone())?;
                    delegates.push(indirect);
                },
            }
        }

        Ok(Delegates {
            result: peeked,
            fetched: remotes,
            views: delegates,
        }
        .into())
    }
}

pub struct SignedRefs {
    result: fetch::FetchResult,
    tracked: Tracked,
}

impl SignedRefs {
    pub fn fetch(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        urn: &Urn,
        delegates: &ProjectDelegates<VerifiedProject>,
        tracked: &Tracked,
    ) -> Result<Self, Error> {
        // Read `signed_refs` for all tracked
        let tracked_sigrefs = tracked
            .remotes
            .iter()
            .copied()
            .filter_map(|peer| match Refs::load(storage, &urn, peer) {
                Ok(Some(refs)) => Some(Ok((peer, refs))),

                Ok(None) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        // Fetch all the rest
        let delegates = delegates.rad_ids().collect();
        tracing::debug!("fetching heads: {:?}, {:?}", tracked_sigrefs, delegates);
        let result = fetcher
            .fetch(fetch::Fetchspecs::Replicate {
                tracked_sigrefs: tracked_sigrefs.clone(),
                delegates,
                limit: config.fetch_limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        Refs::update(storage, &urn)?;
        // TODO(finto): Verify we got what we asked for
        Ok(SignedRefs {
            result,
            tracked: Tracked {
                remotes: tracked_sigrefs
                    .iter()
                    .flat_map(|(peer, refs)| {
                        iter::once(*peer).chain(refs.remotes.flatten().copied())
                    })
                    .collect(),
            },
        })
    }
}
