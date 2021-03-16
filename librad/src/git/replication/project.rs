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

pub struct ReplicateResult {
    delegates: ProjectDelegates<VerifiedProject>,
    tracked: Tracked,
    signed_refs: SignedRefs,
    identity: IdStatus,
    mode: Mode,
}

impl From<ReplicateResult> for super::ReplicateResult {
    fn from(result: ReplicateResult) -> Self {
        let mut tips = result.delegates.0.result.updated_tips;
        tips.extend(result.signed_refs.result.updated_tips);
        Self {
            updated_tips: tips,
            identity: result.identity,
            mode: result.mode,
        }
    }
}

pub fn clone(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    provider: Provider<Project>,
) -> Result<ReplicateResult, Error> {
    let urn = provider.identity.urn();
    let delegates = ProjectDelegates::from_provider(storage, fetcher, config, provider)?;
    let delegates = delegates.verify(storage, &urn)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let signed_refs = SignedRefs::new(storage, fetcher, config, &urn, &delegates, &tracked)?;
    let identity = delegates.adopt(storage, &urn)?;

    Ok(ReplicateResult {
        delegates,
        tracked,
        signed_refs,
        identity,
        mode: Mode::Clone,
    })
}

pub fn fetch(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    urn: &Urn,
) -> Result<ReplicateResult, Error> {
    let tracked = Tracked::load(storage, urn)?;
    let delegates = ProjectDelegates::from_local(storage, fetcher, config, urn, tracked)?
        .verify(storage, urn)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let signed_refs = SignedRefs::new(storage, fetcher, config, &urn, &delegates, &tracked)?;
    let identity = delegates.adopt(storage, urn)?;

    Ok(ReplicateResult {
        delegates,
        tracked,
        signed_refs,
        identity,
        mode: Mode::Fetch,
    })
}

/* FIXME: turn this into docs
 * What do I want to do here?
 *
 * clone: We have looked at the rad/ids from the provider and so we can
 * determine who the delegates are and we fetch those remotes.
 * But we _should_ have at least one project view for each delegate. If it's
 * anonymous then we expect there to one and only one remote entry for them.
 * If it's a Person delegation we can have one or more remote entries.
 */
#[derive(Clone)]
pub enum DelegateView<P> {
    Direct {
        remote: PeerId,
        project: P,
    },
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
    pub fn direct(storage: &Storage, urn: &Urn, remote: PeerId) -> Result<Self, Error> {
        let urn = unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(remote));
        let project = identities::project::get(storage, &urn)?.ok_or(Error::MissingIdentity)?;
        Ok(DelegateView::Direct { remote, project })
    }

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
            remotes.insert(remote);
            let project = if &remote == local {
                identities::project::get(storage, urn)?.ok_or(Error::MissingIdentity)?
            } else {
                let urn =
                    unsafe_into_urn(Reference::rad_id(Namespace::from(urn)).with_remote(remote));
                identities::project::get(storage, &urn)?.ok_or(Error::MissingIdentity)?
            };
            projects.insert(remote, project);
        }

        Ok(Self::Indirect {
            person,
            projects,
            remotes,
        })
    }

    pub fn verify(
        self,
        storage: &Storage,
        urn: &Urn,
    ) -> Result<DelegateView<VerifiedProject>, Error> {
        self.adopt_direct(storage, urn)?;
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
                        identities::project::verify(storage, &urn)
                            .map_err(Error::from)
                            .and_then(|project| {
                                project
                                    .ok_or(Error::MissingIdentity)
                                    .map(|project| (remote, project))
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

    pub fn adopt_direct(&self, storage: &Storage, project_urn: &Urn) -> Result<Tracked, Error> {
        match self {
            Self::Direct { remote, project } => {
                Tracked::new(storage, &project.urn(), Some(*remote).into_iter())
            },
            Self::Indirect {
                person, remotes, ..
            } => {
                let urn = person.urn();
                let tracked = Tracked::new(storage, &urn, remotes.iter().cloned())?;

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
            },
        }
    }
}

pub struct SignedRefs {
    result: fetch::FetchResult,
    tracked: Tracked,
}

impl<P> ProjectDelegates<P> {
    pub fn remotes(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.0.views.iter().flat_map(|view| match view {
            DelegateView::Direct { remote, .. } => Either::Left(iter::once(*remote)),
            DelegateView::Indirect { remotes, .. } => Either::Right(remotes.iter().copied()),
        })
    }
}

impl ProjectDelegates<VerifiedProject> {
    pub fn rad_ids(&'_ self) -> impl Iterator<Item = Urn> + '_ {
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

    pub fn verify(
        self,
        storage: &Storage,
        urn: &Urn,
    ) -> Result<ProjectDelegates<VerifiedProject>, Error> {
        let ProjectDelegates(Delegates {
            result,
            fetched,
            views,
        }) = self;
        let views = views
            .into_iter()
            .map(|view| view.verify(storage, urn))
            .collect::<Result<_, _>>()?;
        Ok(ProjectDelegates(Delegates {
            result,
            fetched,
            views,
        }))
    }

    pub fn rad_ids(&'_ self) -> impl Iterator<Item = Urn> + '_ {
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
        // FIXME: actually why do I want this?
        let mut delegate_remotes = BTreeSet::new();
        let urn = project.urn();

        tracing::debug!(remotes = ?remotes, "peeking remotes");
        let peeked = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes,
                limit: config.fetch_limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;
        tracing::debug!(tips = ?peeked.updated_tips);

        for delegate in project.delegations().into_iter() {
            match delegate {
                Either::Left(key) => {
                    let remote = PeerId::from(*key);
                    delegate_remotes.insert(remote);
                    delegates.push(DelegateView::direct(storage, &urn, remote)?);
                },
                Either::Right(person) => {
                    let indirect =
                        DelegateView::indirect(storage, &urn, &person.urn(), who.clone())?;
                    match &indirect {
                        DelegateView::Indirect {
                            remotes: indirect_remotes,
                            ..
                        } => delegate_remotes.extend(indirect_remotes),
                        _ => unreachable!(),
                    }
                    delegates.push(indirect);
                },
            }
        }

        Ok(Delegates {
            result: peeked,
            fetched: delegate_remotes,
            views: delegates,
        }
        .into())
    }
}

impl SignedRefs {
    pub fn new(
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
