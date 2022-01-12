// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, convert::TryFrom, marker::PhantomData};

use tracing::warn;

use link_crypto::PeerId;
use link_identities::urn::Urn;
use radicle_git_ext::{Oid, RefLike, RefspecPattern};

use crate::tracking;

use super::{
    config::{self, Config},
    odb,
    refdb,
};

pub mod batch;
pub use batch::{batch, Action, Applied};
pub mod error;
pub mod policy;
pub mod reference;
pub use reference::{RefName, Remote};

pub type Ref = refdb::Ref<'static, Oid>;
pub type PreviousError = refdb::PreviousError<Oid>;
pub type Tracked = tracking::Tracked<Oid, Config>;

/// Track the `urn` for the given `peer`, storing the provided `config` at
/// `refs/rad/remotes/<urn>/(<peer> | default)`.
///
/// If `peer` is `None`, the `default` entry is created.
///
/// Use the `Default` instance of `Config` to allow all references to be fetched
/// for the given peer. Otherwise see [`Config`] for details on restricting
/// references.
///
/// The [`Ref`] that was created/updated is returned if the tracking entry was
/// created/updated.
///
/// # Concurrency
///
/// Depending on the `policy` provided, and the existing state, the inner
/// `Result` of `track` will succeed or fail.
///
/// * [`policy::Track::Any`] will always succeed. This can be seen as forceful
///   write.
/// * [`policy::Track::MustNotExist`] will only succeed iff the tracking entry
///   did not already exist. This can be seen as a safe way to create a new
///   tracking entry.
/// * [`policy::Track::MustExist`] will always succeed iff the tracking entry
///   did already exist. This can be seen as a safe way to set the configuration
///   of an existing tracking entry.
pub fn track<'a, Db>(
    db: &'a Db,
    urn: &Urn<Oid>,
    peer: Option<PeerId>,
    config: Config,
    policy: policy::Track,
) -> Result<Result<Ref, PreviousError>, error::Track>
where
    Db: odb::Read<Oid = Oid>
        + odb::Write<Oid = Oid>
        + refdb::Read<'a, Oid = Oid>
        + refdb::Write<Oid = Oid>,
{
    let reference = RefName::borrowed(urn, peer);
    let target = db
        .write_config(&config)
        .map_err(|err| error::Track::WriteObj {
            name: reference.clone().into_owned(),
            source: err.into(),
        })?;
    db.update(Some(refdb::Update::Write {
        name: reference.clone(),
        target,
        previous: policy.into(),
    }))
    .map_err(|err| error::Track::Create {
        name: reference.into_owned(),
        source: err.into(),
    })
    .map(
        |refdb::Applied {
             updates,
             rejections,
         }| {
            match updates.first() {
                Some(updated) => {
                    debug_assert!(rejections.is_empty());
                    match updated {
                        refdb::Updated::Written { name, target } => Ok(Ref {
                            name: name.clone().into_owned(),
                            target: *target,
                        }),
                        refdb::Updated::Deleted { .. } => {
                            panic!("BUG: Updated::Written was expected, found Updated::Deleted")
                        },
                    }
                },
                None => {
                    debug_assert!(!rejections.is_empty());
                    Err(*rejections.first().unwrap())
                },
            }
        },
    )
}

/// Modify the configuration found for the given `urn` and `peer`, storing the
/// `config` at `refs/rad/remotes/<urn>/(<peer> | default)`.
///
/// If `peer` is `None`, the `default` entry is modified.
///
/// It is expected that there exists a `Config` for the given tracking entry.
///
/// The resulting [`Ref`], after the modification, is returned.
///
/// # Concurrency
///
/// If the previous `Config` was updated before the write of the modified
/// `Config` occurred, then `modify` will fail.
pub fn modify<'a, Db, F>(
    db: &'a Db,
    urn: &Urn<Oid>,
    peer: Option<PeerId>,
    f: F,
) -> Result<Result<Ref, PreviousError>, error::Modify>
where
    F: FnOnce(Config) -> Config,
    Db: odb::Read<Oid = Oid>
        + odb::Write<Oid = Oid>
        + refdb::Read<'a, Oid = Oid>
        + refdb::Write<Oid = Oid>,
{
    let name = RefName::borrowed(urn, peer);
    let (existing, new) = match db
        .find_reference(&name)
        .map_err(|err| error::Modify::FindRef {
            name: name.clone().into_owned(),
            source: err.into(),
        })? {
        None => Err(error::Modify::DidNotExist {
            name: name.clone().into_owned(),
        }),
        Some(reference) => {
            let new =
                db.modify_config(&reference.target, f)
                    .map_err(|err| error::Modify::ModifyObj {
                        name: name.clone().into_owned(),
                        target: reference.target,
                        source: err.into(),
                    })?;
            Ok((reference.target, new))
        },
    }?;

    db.update(Some(refdb::Update::Write {
        name: name.clone(),
        target: new,
        previous: refdb::PreviousValue::MustExistAndMatch(existing),
    }))
    .map_err(|err| error::Modify::WriteRef {
        object: new,
        name: name.into_owned(),
        source: err.into(),
    })
    .map(
        |refdb::Applied {
             updates,
             rejections,
         }| {
            match updates.first() {
                Some(updated) => {
                    debug_assert!(rejections.is_empty());
                    match updated {
                        refdb::Updated::Written { name, target } => Ok(Ref {
                            name: name.clone().into_owned(),
                            target: *target,
                        }),
                        refdb::Updated::Deleted { .. } => {
                            panic!("BUG: Updated::Written was expected, found Updated::Deleted")
                        },
                    }
                },
                None => {
                    debug_assert!(!rejections.is_empty());
                    Err(*rejections.first().unwrap())
                },
            }
        },
    )
}

/// Untrack the `urn` for the given `peer`, removing the reference
/// `refs/rad/remotes/<urn>/<peer>`.
///
/// If the tracking entry existed for removal, the [`Oid`] of the previous
/// [`Config`] is returned in the inner result, otherwise `None` is returned.
///
/// # Concurrency
///
/// Depending on the `policy` provided, and the existing state, the inner
/// `Result` of `untrack` will succeed or fail.
///
/// * [`policy::Untrack::Any`] will always succeed. If the tracking entry did
///   not exist then this is a no-op.
/// * [`policy::Untrack::MustExist`] will only succeed iff the tracking entry
///   did already exist. This can be seen as a safe way to delete an existing
///   tracking entry.
pub fn untrack<'a, Db>(
    db: &Db,
    urn: &Urn<Oid>,
    peer: PeerId,
    policy: policy::Untrack,
) -> Result<Result<Option<Oid>, PreviousError>, error::Untrack>
where
    Db: odb::Read<Oid = Oid> + refdb::Read<'a, Oid = Oid> + refdb::Write<Oid = Oid>,
{
    let reference = RefName::borrowed(urn, peer);
    db.update(Some(refdb::Update::Delete {
        name: reference.clone(),
        previous: policy.into(),
    }))
    .map_err(|err| error::Untrack::Delete {
        name: reference.clone().into_owned(),
        source: err.into(),
    })
    .map(
        |refdb::Applied {
             updates,
             rejections,
         }| {
            match updates.first() {
                Some(updated) => match updated {
                    refdb::Updated::Deleted { previous, .. } => Ok(*previous),
                    refdb::Updated::Written { .. } => {
                        panic!("BUG: expected Updated::Deleted, found Updated::Written")
                    },
                },
                None => {
                    debug_assert!(!rejections.is_empty());
                    Err(*rejections.first().unwrap())
                },
            }
        },
    )
}

/// Untrack all peers under `urn`, removing all references
/// `refs/rad/remotes/<urn>/*`.
///
/// The [`RefName`] of each deleted reference is returned.
///
/// # Concurrency
///
/// Depending on the `policy` provided, and the existing state, the inner
/// `Result` of each `untrack_all` will succeed or fail.
///
/// * [`policy::UntrackAll::Any`] will always succeed. This can be seen as a
///   forceful delete of all tracking entries, since it does not take care to
///   ensure concurrent updates are respected.
/// * [`policy::UntrackAll::MustExistAndMatch`] will only succeed iff the
///   tracking entry did already exist and the previous value matched the
///   existing one. This can be seen as a safe way to delete the existing
///   tracking entries, respecting concurrent updates.
pub fn untrack_all<'a, Db>(
    db: &'a Db,
    urn: &Urn<Oid>,
    policy: policy::UntrackAll,
) -> Result<impl Iterator<Item = Result<RefName<'a, Oid>, PreviousError>>, error::UntrackAll>
where
    Db: refdb::Read<'a, Oid = Oid> + refdb::Write<Oid = Oid>,
{
    let prefix = reflike!("refs/rad/remotes");
    let namespace =
        RefLike::try_from(urn.encode_id()).expect("namespace should be valid ref component");
    let spec = prefix
        .join(namespace)
        .with_pattern_suffix(refspec_pattern!("*"));
    let updates = {
        let refs = db
            .references(&spec)
            .map_err(|err| error::UntrackAll::References {
                spec: spec.clone(),
                source: err.into(),
            })?;
        refs.map(|r| {
            r.map(|r| refdb::Update::Delete {
                name: r.name,
                previous: policy.into_previous_value(r.target),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| error::UntrackAll::Iter {
            spec: spec.clone(),
            source: err.into(),
        })?
    };
    db.update(updates)
        .map(
            |refdb::Applied {
                 updates,
                 rejections,
             }| {
                updates
                    .into_iter()
                    .map(|updated| match updated {
                        refdb::Updated::Written { .. } => {
                            panic!("BUG: Updated::Deleted was expected, found Updated::Written")
                        },
                        refdb::Updated::Deleted { name, previous: _ } => Ok(name),
                    })
                    .chain(rejections.into_iter().map(Err))
            },
        )
        .map_err(|err| error::UntrackAll::Delete {
            spec,
            source: err.into(),
        })
}

/// Iterator of [`Tracked`] entries.
pub struct TrackedEntries<'a, Db, R> {
    db: &'a Db,
    // for error reporting
    spec: RefspecPattern,
    seen: BTreeMap<Oid, Config>,
    iter: R,
}

impl<'a, Db, R> TrackedEntries<'a, Db, R> {
    fn resolve(&mut self, reference: refdb::Ref<Oid>) -> Result<Option<Tracked>, error::Tracked>
    where
        Db: odb::Read<Oid = Oid>,
    {
        // We may have seen this config already
        if let Some(config) = self.seen.get(&reference.target) {
            return Ok(Some(from_reference(&reference.name, config.clone())));
        }

        // Otherwise we attempt to fetch it from the backend
        match self
            .db
            .find_config(&reference.target)
            .map_err(|err| error::Tracked::FindObj {
                name: reference.name.clone().into_owned(),
                target: reference.target,
                source: err.into(),
            })? {
            None => {
                warn!(name=?reference.name, oid=?reference.target, "missing blob");
                Ok(None)
            },
            Some(config) => {
                self.seen.insert(reference.target, config.clone());
                Ok(Some(from_reference(&reference.name, config)))
            },
        }
    }
}

impl<'a, Db> Iterator for TrackedEntries<'a, Db, Db::References>
where
    Db: odb::Read<Oid = Oid> + refdb::Read<'a, Oid = Oid>,
{
    type Item = Result<Tracked, error::Tracked>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().and_then(|r| {
            r.map_err(|err| error::Tracked::Iter {
                spec: self.spec.clone(),
                source: err.into(),
            })
            .and_then(|ok| self.resolve(ok))
            .transpose()
        })
    }
}

/// Return all tracked entries, optionally filtering by an [`Urn`].
pub fn tracked<'a, Db>(
    db: &'a Db,
    filter_by: Option<&Urn<Oid>>,
) -> Result<TrackedEntries<'a, Db, Db::References>, error::Tracked>
where
    Db: odb::Read<Oid = Oid> + refdb::Read<'a, Oid = Oid>,
{
    let spec = remotes_refspec(filter_by);
    let seen: BTreeMap<Oid, Config> = BTreeMap::new();
    let iter = db
        .references(&spec)
        .map_err(|err| error::Tracked::References {
            spec: spec.clone(),
            source: err.into(),
        })?;
    Ok(TrackedEntries {
        db,
        spec,
        seen,
        iter,
    })
}

/// Iterator of tracked [`PeerId`]s.
pub struct TrackedPeers<'a, R, E> {
    // for error reporting
    spec: RefspecPattern,
    iter: R,
    _marker: PhantomData<&'a E>,
}

impl<'a, R, E> Iterator for TrackedPeers<'a, R, E>
where
    R: Iterator<Item = Result<refdb::Ref<'a, Oid>, E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<PeerId, error::TrackedPeers>;

    fn next(&mut self) -> Option<Self::Item> {
        let spec = &self.spec;
        self.iter.find_map(|r| {
            r.map_err(|err| error::TrackedPeers::Iter {
                spec: spec.clone(),
                source: err.into(),
            })
            .map(|reference| reference.name.remote.into())
            .transpose()
        })
    }
}

/// Return all tracked peers, optionally filtering by an [`Urn`].
pub fn tracked_peers<'a, Db>(
    db: &'a Db,
    filter_by: Option<&Urn<Oid>>,
) -> Result<TrackedPeers<'a, Db::References, Db::IterError>, error::TrackedPeers>
where
    Db: refdb::Read<'a, Oid = Oid>,
{
    let spec = remotes_refspec(filter_by);
    let iter = db
        .references(&spec)
        .map_err(|err| error::TrackedPeers::References {
            spec: spec.clone(),
            source: err.into(),
        })?;
    Ok(TrackedPeers {
        spec,
        iter,
        _marker: PhantomData,
    })
}

/// Return a tracking entry for a given `urn` and `peer`.
///
/// If `refs/rad/remotes/<urn>/(<peer> | default)` does not exist, then `None`
/// is returned.
pub fn get<'a, Db>(
    db: &Db,
    urn: &'_ Urn<Oid>,
    peer: Option<PeerId>,
) -> Result<Option<Tracked>, error::Get>
where
    Db: odb::Read<Oid = Oid> + refdb::Read<'a, Oid = Oid>,
{
    let name = RefName::borrowed(urn, peer);
    match db
        .find_reference(&name)
        .map_err(|err| error::Get::FindRef {
            name: name.clone().into_owned(),
            source: err.into(),
        })? {
        None => Ok(None),
        Some(reference) => {
            match db
                .find_config(&reference.target)
                .map_err(|err| error::Get::FindObj {
                    name: reference.name.into_owned(),
                    target: reference.target,
                    source: err.into(),
                })? {
                None => Ok(None),
                Some(config) => Ok(Some(from_reference(&name, config))),
            }
        },
    }
}

/// Check if a tracking entry for a given `urn` and `peer` exists.
pub fn is_tracked<'a, Db>(
    db: &Db,
    urn: &Urn<Oid>,
    peer: Option<PeerId>,
) -> Result<bool, error::IsTracked>
where
    Db: refdb::Read<'a, Oid = Oid>,
{
    let name = RefName::borrowed(urn, peer);
    match db
        .find_reference(&name)
        .map_err(|err| error::IsTracked::FindRef {
            name: name.into_owned(),
            source: err.into(),
        })? {
        None => Ok(false),
        Some(_) => Ok(true),
    }
}

/// Check that the only tracking entry for the given `urn` is the default entry.
/// This will return false if there are either:
///   * No tracking entries for the `urn`
///   * There is at least one tracked peer for the `urn`
pub fn default_only<'a, Db>(db: &'a Db, urn: &Urn<Oid>) -> Result<bool, error::DefaultOnly>
where
    Db: refdb::Read<'a, Oid = Oid>,
{
    let spec = remotes_refspec(Some(urn));
    let mut seen_default = false;
    for reference in db
        .references(&spec)
        .map_err(|err| error::DefaultOnly::References {
            spec: spec.clone(),
            source: err.into(),
        })?
    {
        match reference
            .map_err(|err| error::DefaultOnly::Iter {
                spec: spec.clone(),
                source: err.into(),
            })?
            .name
            .remote
        {
            Remote::Default => {
                seen_default = true;
            },
            Remote::Peer(_) => return Ok(false),
        }
    }

    Ok(seen_default)
}

fn from_reference(name: &RefName<'_, Oid>, config: Config) -> Tracked {
    match name.remote {
        Remote::Default => Tracked::Default {
            urn: name.urn.clone().into_owned(),
            config,
        },
        Remote::Peer(peer) => Tracked::Peer {
            urn: name.urn.clone().into_owned(),
            peer,
            config,
        },
    }
}

fn remotes_refspec(filter_by: Option<&Urn<Oid>>) -> RefspecPattern {
    let base = reference::base();
    match filter_by {
        Some(urn) => {
            let namespace = RefLike::try_from(urn.encode_id())
                .expect("namespace should be valid ref component");
            base.join(namespace)
                .with_pattern_suffix(refspec_pattern!("*"))
        },
        None => base.with_pattern_suffix(refspec_pattern!("*")),
    }
}
