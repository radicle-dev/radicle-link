// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug};

use either::Either;
use git_ext::{is_not_found_err, OneLevel};

use super::{
    super::{
        refs::Refs as Sigrefs,
        storage::{self, ReadOnlyStorage as _, Storage},
        types::{namespace, reference, Force, Reference, Single, SymbolicRef},
    },
    common,
    error::Error,
    local::LocalIdentity,
};
use crate::{
    identities::{
        self,
        git::{Identities, IndirectDelegation, Project, Revision, VerifiedProject, Verifying},
        urn,
    },
    PeerId,
};

pub use identities::{git::Urn, payload::ProjectPayload};

type Namespace = namespace::Namespace<Revision>;

/// Read a [`Project`] from the tip of the ref [`Urn::path`] points to.
///
/// If the ref is not found, `None` is returned.
#[tracing::instrument(level = "trace", skip(storage))]
pub fn get<S>(storage: &S, urn: &Urn) -> Result<Option<Project>, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(storage).get(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Read and verify the [`Project`] pointed to by `urn`.
///
/// If the ref pointed to by [`Urn::path`] is not found, `None` is returned.
///
/// # Caveats
///
/// Keep in mind that the `content_id` of a successfully verified project may
/// not be the same as the tip of the ref [`Urn::path`] points to. That is, this
/// function cannot be used to assert that the state after an [`update`] is
/// valid.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn verify<S>(storage: &S, urn: &Urn) -> Result<Option<VerifiedProject>, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    let lookup = |urn| {
        let refname = Reference::rad_id(Namespace::from(urn));
        storage.reference_oid(&refname).map(|oid| oid.into())
    };
    verify_with(storage, urn, lookup)
}

/// Read and verify the [`Project`] pointed to by `urn`.
///
/// The `lookup` callback is used to specify how the delegate's latest tip
/// should retrieved. For example, the `Urn` could be used to point it to
/// `rad/ids/<urn.id>`.
///
/// If the ref pointed to by [`Urn::path`] is not found, `None` is returned.
///
/// # Caveats
///
/// Keep in mind that the `content_id` of a successfully verified project may
/// not be the same as the tip of the ref [`Urn::path`] points to. That is, this
/// function cannot be used to assert that the state after an [`update`] is
/// valid.
#[tracing::instrument(level = "debug", skip(storage, lookup))]
pub fn verify_with<S, E, F>(
    storage: &S,
    urn: &Urn,
    lookup: F,
) -> Result<Option<VerifiedProject>, Error>
where
    S: AsRef<storage::ReadOnly>,
    E: std::error::Error + Send + Sync + 'static,
    F: Fn(Urn) -> Result<git2::Oid, E>,
{
    let storage = storage.as_ref();
    match storage.reference(&Reference::try_from(urn)?) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            identities(storage)
                .verify(tip, lookup)
                .map(Some)
                .map_err(|e| Error::Verify(e.into()))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get the root [`Urn`] for the given `payload` and set of `delegations`.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn urn<S, P>(storage: &S, payload: P, delegations: IndirectDelegation) -> Result<Urn, Error>
where
    S: AsRef<storage::ReadOnly>,
    P: Into<ProjectPayload> + Debug,
{
    let storage = storage.as_ref();
    let (_, revision) = identities(storage).base(payload.into(), delegations)?;
    Ok(Urn::new(revision))
}

/// Create a new [`Project`].
#[tracing::instrument(level = "debug", skip(storage, whoami))]
pub fn create<P>(
    storage: &Storage,
    whoami: LocalIdentity,
    payload: P,
    delegations: IndirectDelegation,
) -> Result<Project, Error>
where
    P: Into<ProjectPayload> + Debug,
{
    let project = identities(storage).create(payload.into(), delegations, storage.signer())?;
    let urn = project.urn();
    ProjectRefs::Create(&project).apply(storage)?;
    whoami.link(storage, &urn)?;
    Sigrefs::update(storage, &urn)?;

    Ok(project)
}

/// Update the [`Project`] at `urn`.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn update<L, P, D>(
    storage: &Storage,
    urn: &Urn,
    whoami: L,
    payload: P,
    delegations: D,
) -> Result<Project, Error>
where
    L: Into<Option<LocalIdentity>> + Debug,
    P: Into<Option<ProjectPayload>> + Debug,
    D: Into<Option<IndirectDelegation>> + Debug,
{
    let prev = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let prev = Verifying::from(prev).signed()?;
    let next = identities(storage).update(prev, payload, delegations, storage.signer())?;

    ProjectRefs::Update(&next, "update").apply(storage)?;
    if let Some(local_id) = whoami.into() {
        local_id.link(storage, urn)?;
    }
    Sigrefs::update(storage, urn)?;

    Ok(next)
}

/// Merge and sign the [`Project`] state as seen by `from`.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn merge(storage: &Storage, urn: &Urn, from: PeerId) -> Result<Project, Error> {
    let ours = get(storage, urn)?.ok_or_else(|| Error::NotFound(urn.clone()))?;
    let theirs = {
        let (path, rad) = OneLevel::from_qualified(urn::DEFAULT_PATH.clone());
        let rad = rad.expect("default path should be refs/rad/id");
        let their_urn = Urn {
            id: urn.id,
            path: Some(reflike!("refs/remotes").join(from).join(rad).join(path)),
        };
        get(storage, &their_urn)?.ok_or(Error::NotFound(their_urn))?
    };

    let ours = Verifying::from(ours).signed()?;
    let theirs = Verifying::from(theirs).signed()?;
    let next = identities(storage).update_from(ours, theirs, storage.signer())?;

    ProjectRefs::Update(&next, &format!("merge from {}", from)).apply(storage)?;
    Sigrefs::update(storage, urn)?;

    Ok(next)
}

/// Return the newer of `a` and `b`, or an error if their histories are
/// unrelated.
pub fn newer<S>(
    storage: &S,
    a: VerifiedProject,
    b: VerifiedProject,
) -> Result<VerifiedProject, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    Ok(verified(storage).newer(a, b)?)
}

enum ProjectRefs<'a> {
    Create(&'a Project),
    Update(&'a Project, &'a str),
}

impl<'a> ProjectRefs<'a> {
    pub fn apply(&self, storage: &Storage) -> Result<(), Error> {
        for symref in self.delegates() {
            symref.create(storage.as_raw())?;
        }
        match self {
            Self::Create(project) => {
                common::IdRef::from(&project.urn()).create(storage, project.content_id)
            },
            Self::Update(project, msg) => {
                common::IdRef::from(&project.urn()).update(storage, project.content_id, msg)
            },
        }?;

        Ok(())
    }

    fn project(&self) -> &Project {
        match self {
            Self::Create(project) => project,
            Self::Update(project, _) => project,
        }
    }

    fn delegates(
        &'a self,
    ) -> impl Iterator<
        Item = SymbolicRef<
            reference::Reference<Namespace, PeerId, Single>,
            reference::Reference<Namespace, PeerId, Single>,
        >,
    > + 'a {
        let source = self.project().urn();
        self.project()
            .delegations()
            .iter()
            .filter_map(Either::right)
            .map(move |id| {
                let urn = id.urn();
                SymbolicRef {
                    source: Reference::rad_delegate(Namespace::from(&source), &urn),
                    target: Reference::rad_id(Namespace::from(&urn)),
                    force: Force::True,
                }
            })
    }
}

fn identities<S>(storage: &S) -> Identities<Project>
where
    S: AsRef<storage::ReadOnly>,
{
    storage.as_ref().identities()
}

fn verified<S>(storage: &S) -> Identities<VerifiedProject>
where
    S: AsRef<storage::ReadOnly>,
{
    storage.as_ref().identities()
}
