// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
};

use either::Either::{self, Left, Right};
use link_crypto::PeerId;
use link_identities::urn::Urn;
use radicle_git_ext::Oid;

use super::{config::Config, error, odb, policy, refdb, PreviousError, Ref, RefName};

use crate::git::tracking::policy::compose::{Compose as _, Reduction, WithConfig};

/// A tracking action that performs a write during a [`batch`] operation.
#[derive(Clone, Debug)]
pub enum Action<'a, Oid: Clone> {
    Track {
        urn: Cow<'a, Urn<Oid>>,
        peer: Option<PeerId>,
        config: &'a Config,
        policy: policy::Track,
    },
    Untrack {
        urn: Cow<'a, Urn<Oid>>,
        peer: PeerId,
        policy: policy::Untrack,
    },
}

impl<'a, Oid: Clone + PartialEq> Action<'a, Oid> {
    pub fn urn(&self) -> &Cow<'a, Urn<Oid>> {
        match self {
            Self::Track { urn, .. } => urn,
            Self::Untrack { urn, .. } => urn,
        }
    }

    pub fn peer(&self) -> Option<PeerId> {
        match self {
            Self::Track { peer, .. } => *peer,
            Self::Untrack { peer, .. } => Some(*peer),
        }
    }
}

/// The applied updates for the given set of [`Action`]s in a [`batch`]
/// operation.
pub struct Applied {
    pub updates: Vec<Updated>,
    pub rejections: Vec<PreviousError>,
}

impl From<refdb::Applied<'_, Oid>> for Applied {
    fn from(
        refdb::Applied {
            updates,
            rejections,
        }: refdb::Applied<'_, Oid>,
    ) -> Self {
        Self {
            updates: updates.into_iter().map(Updated::from).collect(),
            rejections,
        }
    }
}

pub enum Updated {
    /// The `Ref` was either created/updated during an [Action::Track].
    Tracked { reference: Ref },
    /// The `Ref` was removed during an [Action::Untrack].
    Untracked { reference: Ref },
}

impl<'a> From<refdb::Updated<'a, Oid>> for Updated {
    fn from(updated: refdb::Updated<Oid>) -> Updated {
        match updated {
            refdb::Updated::Written { name, target } => Self::Tracked {
                reference: Ref {
                    name: name.clone().into_owned(),
                    target,
                },
            },
            refdb::Updated::Deleted { name, previous } => Self::Untracked {
                reference: Ref {
                    name: name.clone().into_owned(),
                    target: previous,
                },
            },
        }
    }
}

/// Perform a transactional update of the provided `actions`.
///
/// # Note
///
/// The transactional nature of the operation depends on the implementation of
/// [`refdb::Write::update`].
///
/// Any [`Config`]s that require writing to the `Odb` are not part of the
/// transaction and happen before the references are updated.
///
/// # Fusion
///
/// For any actions that have the same `Urn` and `PeerId`, they are subject to
/// fusion. This may simplify a series of these actions to a single action.
///
/// For an explanation and list of fusion rules see:
/// `./docs/rfc/0699-tracking-storage.adoc#_batch_tracking`
pub fn batch<'a, Db, I>(db: &'a Db, actions: I) -> Result<Applied, error::Batch>
where
    Db: odb::Read<Oid = Oid>
        + odb::Write<Oid = Oid>
        + refdb::Read<'a, Oid = Oid>
        + refdb::Write<Oid = Oid>,
    I: IntoIterator<Item = Action<'a, Oid>> + 'a,
{
    let updates = into_updates(db, fuse(actions)).collect::<Result<Vec<_>, _>>()?;
    let applied = db
        .update(updates)
        .map_err(|err| error::Batch::Txn { source: err.into() })?;
    Ok(applied.into())
}

fn into_updates<'a, Db, I>(
    db: &'a Db,
    actions: I,
) -> impl Iterator<Item = Result<refdb::Update<'a, Oid>, error::Batch>> + 'a
where
    Db: odb::Read<Oid = Oid> + odb::Write<Oid = Oid> + refdb::Read<'a, Oid = Oid>,
    I: IntoIterator<Item = Action<'a, Oid>> + 'a,
{
    let mut seen: HashMap<Config, Oid> = HashMap::new();
    actions.into_iter().map(move |action| match action {
        Action::Track {
            urn,
            peer,
            config,
            policy,
        } => {
            let name = RefName::new(urn, peer);
            match policy {
                policy::Track::Any => {
                    target(db, &mut seen, &name, config).map(|target| refdb::Update::Write {
                        name,
                        target,
                        previous: refdb::PreviousValue::Any,
                    })
                },
                policy::Track::MustExist => {
                    let r = db
                        .find_reference(&name)
                        .map_err(|err| error::Batch::FindRef {
                            name: name.clone().into_owned(),
                            source: err.into(),
                        })?
                        .ok_or(error::Batch::MissingRef {
                            name: name.clone().into_owned(),
                        })?;
                    target(db, &mut seen, &name, config).map(|target| refdb::Update::Write {
                        name,
                        target,
                        previous: refdb::PreviousValue::MustExistAndMatch(r.target),
                    })
                },
                policy::Track::MustNotExist => {
                    target(db, &mut seen, &name, config).map(|target| refdb::Update::Write {
                        name,
                        target,
                        previous: refdb::PreviousValue::MustNotExist,
                    })
                },
            }
        },
        Action::Untrack { urn, peer, policy } => {
            let name = RefName::new(urn, peer);
            match policy {
                policy::Untrack::Any => Ok(refdb::Update::Delete {
                    name,
                    previous: policy.into(),
                }),
                policy::Untrack::MustExist => {
                    let r = db
                        .find_reference(&name)
                        .map_err(|err| error::Batch::FindRef {
                            name: name.clone().into_owned(),
                            source: err.into(),
                        })?
                        .ok_or(error::Batch::MissingRef {
                            name: name.clone().into_owned(),
                        })?;
                    Ok(refdb::Update::Delete {
                        name,
                        previous: refdb::PreviousValue::MustExistAndMatch(r.target),
                    })
                },
            }
        },
    })
}

fn target<'a, Db>(
    db: &Db,
    cache: &mut HashMap<Config, Oid>,
    name: &RefName<'a, Oid>,
    config: &Config,
) -> Result<Oid, error::Batch>
where
    Db: odb::Write<Oid = Oid>,
{
    match cache.get(config) {
        None => {
            let target = db
                .write_config(config)
                .map_err(|err| error::Batch::WriteObj {
                    name: name.clone().into_owned(),
                    source: err.into(),
                })?;
            cache.insert(config.clone(), target);
            Ok(target)
        },
        Some(target) => Ok(*target),
    }
}

fn fuse<'a, I>(actions: I) -> impl Iterator<Item = Action<'a, Oid>>
where
    I: IntoIterator<Item = Action<'a, Oid>> + 'a,
{
    type Pair<'a> = (Cow<'a, Urn<Oid>>, PeerId);
    let mut pairs: BTreeMap<Pair<'a>, Reduction<Either<WithConfig<'a, Config>, policy::Untrack>>> =
        BTreeMap::new();
    let mut defaults: BTreeMap<Cow<'a, Urn<Oid>>, Reduction<WithConfig<'a, Config>>> =
        BTreeMap::new();
    for action in actions {
        match action {
            Action::Track {
                urn,
                peer: None,
                config,
                policy,
            } => {
                defaults
                    .entry(urn)
                    .and_modify(|r| *r = r.compose(&WithConfig { policy, config }))
                    .or_insert_with(|| WithConfig { policy, config }.into());
            },
            Action::Track {
                urn,
                peer: Some(peer),
                config,
                policy,
            } => {
                let key = (urn, peer);
                let track = Left(WithConfig { policy, config });
                pairs
                    .entry(key)
                    .and_modify(|r| {
                        *r = r.compose(&track);
                    })
                    .or_insert_with(|| track.into());
            },
            Action::Untrack { urn, peer, policy } => {
                let key = (urn, peer);
                let untrack = Right(policy);
                pairs
                    .entry(key)
                    .and_modify(|r| {
                        *r = r.compose(&untrack);
                    })
                    .or_insert_with(|| untrack.into());
            },
        }
    }

    pairs
        .into_iter()
        .flat_map(|((urn, peer), simple)| {
            simple.into_iter().map(move |e| match e {
                Left(WithConfig { policy, config }) => Action::Track {
                    urn: urn.clone(),
                    peer: Some(peer),
                    config,
                    policy,
                },
                Right(policy) => Action::Untrack {
                    urn: urn.clone(),
                    peer,
                    policy,
                },
            })
        })
        .chain(defaults.into_iter().flat_map(|(urn, simple)| {
            simple
                .into_iter()
                .map(move |WithConfig { policy, config }| Action::Track {
                    urn: urn.clone(),
                    peer: None,
                    config,
                    policy,
                })
        }))
}
