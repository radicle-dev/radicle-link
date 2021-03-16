// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeMap, BTreeSet};

use crate::git::{
    fetch,
    storage::Storage,
    types::{Namespace, Reference},
};

use super::*;

use crate::{identities::git::VerifiedPerson, peer::PeerId};

pub struct PersonDelegates(Delegates<BTreeMap<PeerId, VerifiedPerson>>);

impl From<Delegates<BTreeMap<PeerId, VerifiedPerson>>> for PersonDelegates {
    fn from(delegates: Delegates<BTreeMap<PeerId, VerifiedPerson>>) -> Self {
        PersonDelegates(delegates)
    }
}

pub struct ReplicateResult {
    delegates: PersonDelegates,
    tracked: Tracked,
    identity: IdStatus,
    mode: Mode,
}

impl From<ReplicateResult> for super::ReplicateResult {
    fn from(result: ReplicateResult) -> Self {
        Self {
            updated_tips: result.delegates.0.result.updated_tips,
            identity: result.identity,
            mode: result.mode,
        }
    }
}

pub fn clone(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    provider: Provider<VerifiedPerson>,
) -> Result<ReplicateResult, Error> {
    let urn = provider.identity.urn();
    let delegates = PersonDelegates::from_provider(storage, fetcher, config, provider)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let identity = delegates.adopt(storage, &urn)?;

    Ok(ReplicateResult {
        delegates,
        tracked,
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
    let delegates = PersonDelegates::from_local(storage, fetcher, config, urn, tracked)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let identity = delegates.adopt(storage, urn)?;

    Ok(ReplicateResult {
        delegates,
        tracked,
        identity,
        mode: Mode::Fetch,
    })
}

impl PersonDelegates {
    pub fn from_provider(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        proivder: Provider<VerifiedPerson>,
    ) -> Result<Self, Error> {
        Self::from_identity(
            storage,
            fetcher,
            config,
            proivder.identity.clone(),
            proivder.delegates().collect(),
        )
    }

    pub fn from_local(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        urn: &Urn,
        tracked: Tracked,
    ) -> Result<Self, Error> {
        let project = identities::person::verify(storage, urn)?.ok_or(Error::MissingIdentity)?;
        Self::from_identity(storage, fetcher, config, project, tracked.remotes)
    }

    pub fn remotes(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.0.views.keys().copied()
    }

    pub fn rad_ids(&'_ self) -> impl Iterator<Item = Urn> + '_ {
        self.0.views.iter().map(|(remote, person)| {
            unsafe_into_urn(Reference::rad_id(Namespace::from(person.urn())).with_remote(*remote))
        })
    }

    pub fn adopt(&self, storage: &Storage, urn: &Urn) -> Result<IdStatus, Error> {
        use IdStatus::*;

        let local = storage.peer_id();
        let latest = {
            let mut prev = None;
            for delegate in self.0.views.values().cloned() {
                match prev {
                    None => prev = Some(delegate),
                    Some(p) => {
                        let newer = identities::person::newer(storage, p, delegate)?;
                        prev = Some(newer);
                    },
                }
            }
            prev.expect("empty delegations")
        };

        let expected = match self.0.views.get(local) {
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

    fn from_identity(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        person: VerifiedPerson,
        remotes: BTreeSet<PeerId>,
    ) -> Result<Self, Error> {
        let mut delegates = BTreeMap::new();
        let urn = person.urn();

        let peeked = fetcher
            .fetch(fetch::Fetchspecs::Peek {
                remotes: remotes.clone(),
                limit: config.fetch_limit,
            })
            .map_err(|e| Error::Fetch(e.into()))?;

        for key in person.delegations().into_iter() {
            let remote = PeerId::from(*key);
            let rad_id =
                unsafe_into_urn(Reference::rad_id(Namespace::from(&urn)).with_remote(remote));
            let delegate =
                identities::person::verify(storage, &rad_id)?.ok_or(Error::MissingIdentity)?;
            delegates.insert(remote, delegate);
        }

        Ok(Delegates {
            result: peeked,
            fetched: remotes,
            views: delegates,
        }
        .into())
    }
}
