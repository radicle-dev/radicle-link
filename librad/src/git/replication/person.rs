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

/// Clone the [`Person`] from the `provider` by fetching the delegates in the document.
///
/// We track all the delegates in the document and adopt the `rad/id` for this identity.
pub fn clone(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    provider: Provider<Person>,
) -> Result<ReplicateResult, Error> {
    let urn = provider.identity.urn();
    let delegates = PersonDelegates::from_provider(storage, fetcher, config, provider)?;
    let tracked = Tracked::new(storage, &urn, delegates.remotes())?;
    let identity = delegates.adopt(storage, &urn)?;
    let updated_tips = delegates.0.result.updated_tips;

    tracing::debug!(tips = ?updated_tips, "tips for delegates fetch");
    tracing::debug!(tips = ?tracked.trace(), "tracked peers");

    Ok(ReplicateResult {
        updated_tips,
        identity,
        mode: Mode::Clone,
    })
}

/// Fetch the latest changes for the remotes that we are tracking for `urn`.
///
/// If there are any new delegates we track them. Following that, we
/// [`adopt`][`PersonDelegates::adopt`] the latest tip if necessary.
pub fn fetch(
    storage: &Storage,
    fetcher: &mut fetch::DefaultFetcher,
    config: Config,
    urn: &Urn,
) -> Result<ReplicateResult, Error> {
    let tracked = Tracked::load(storage, urn)?;
    let delegates = PersonDelegates::from_local(storage, fetcher, config, urn, tracked)?;
    let tracked = Tracked::new(storage, &urn, delegates.updates().into_iter())?;
    let identity = delegates.adopt(storage, urn)?;
    let updated_tips = delegates.0.result.updated_tips;

    tracing::debug!(tips = ?updated_tips, "tips for delegates fetch");
    tracing::debug!(tips = ?tracked.trace(), "tracked peers");

    Ok(ReplicateResult {
        updated_tips,
        identity,
        mode: Mode::Fetch,
    })
}

impl PersonDelegates {
    /// Verifies the `provider` and resolves the delegate [`PeerId`]s for this
    /// [`Person`] identity.
    ///
    /// We look at what [`PeerId`]s are advertised in the document and fetch the
    /// `rad/*` references, giving us a [`VerifiedPerson`] for each delegate
    /// in the set.
    pub fn from_provider(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        provider: Provider<Person>,
    ) -> Result<Self, Error> {
        let provider = provider.verify(storage)?;
        Self::from_identity(
            storage,
            fetcher,
            config,
            provider.identity.clone(),
            provider.delegates().collect(),
        )
    }

    /// We use the existing [`VerifiedPerson`] from our own [`Storage`], along
    /// with the existing tracked remote [`PeerId`]s. The remotes are
    /// fetched and the delegates are resolved from the
    /// existing [`VerifiedPerson`].
    ///
    /// **Note**: new delegates could be removed or added, these are not fetched
    /// immediately, but instead added to the tracking graph. This means
    /// that we wait for another pass of replication to fetch those, and so
    /// on.
    pub fn from_local(
        storage: &Storage,
        fetcher: &mut fetch::DefaultFetcher,
        config: Config,
        urn: &Urn,
        tracked: Tracked,
    ) -> Result<Self, Error> {
        let person = identities::person::verify(storage, urn)?.ok_or(Error::MissingIdentity)?;
        Self::from_identity(storage, fetcher, config, person, tracked.remotes)
    }

    /// Using the delegates we determine the latest tip for `rad/id`.
    ///
    /// If we are one of the delegates then we keep our own tip and determine
    /// the [`IdStatus`] by comparing our tip to the latest.
    ///
    /// Otherwise, we adopt the latest tip for our version of `rad/id`.
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

    pub fn remotes(&'_ self) -> impl Iterator<Item = PeerId> + '_ {
        self.0.views.keys().copied()
    }

    pub fn rad_ids(&'_ self) -> impl Iterator<Item = Urn> + '_ {
        self.0.views.iter().map(|(remote, person)| {
            unsafe_into_urn(Reference::rad_id(Namespace::from(person.urn())).with_remote(*remote))
        })
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

    fn updates(&self) -> BTreeSet<PeerId> {
        self.0
            .views
            .values()
            .flat_map(|person| person.delegations().iter().map(|key| PeerId::from(*key)))
            .collect()
    }
}
