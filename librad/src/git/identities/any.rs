// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use git_ext::is_not_found_err;
use itertools::Itertools as _;
use std_ext::Void;

use super::{
    super::{
        storage::{self, glob, ReadOnlyStorage as _},
        types::Reference,
    },
    error::Error,
};
use crate::identities::{
    self,
    git::{Identities, SomeIdentity},
    xor::{self, Xor},
    SomeUrn,
};

pub use identities::git::Urn;

/// Read an identity for which the type is not known statically.
///
/// Note that the [`Urn::path`] is honoured, and the identity is read from the
/// tip of the branch it resolves to. If that branch is not found, `None` is
/// returned.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn get<S>(storage: &S, urn: &Urn) -> Result<Option<SomeIdentity>, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();
    let branch = Reference::try_from(urn)?;
    tracing::trace!(
        "trying to resolve unknown identity at {} from {}",
        urn,
        branch
    );
    match storage.reference(&branch) {
        Ok(Some(reference)) => {
            let tip = reference.peel_to_commit()?.id();
            Ok(Some(identities(&storage).some_identity(tip)?))
        },

        Ok(None) => Ok(None),
        Err(storage::Error::Git(e)) if is_not_found_err(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all identities found in `storage`.
#[tracing::instrument(level = "debug", skip(storage))]
pub fn list<'a, S>(
    storage: &'a S,
) -> Result<impl Iterator<Item = Result<SomeIdentity, Error>> + 'a, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let iter = self::list_urns(storage)?.filter_map(move |urn| match urn {
        Ok(urn) => self::get(storage, &urn).transpose(),
        Err(e) => Some(Err(e)),
    });

    Ok(iter)
}

/// List only the [`Urn`]s of all identities found in `storage.
///
/// Note that this means that only the namespace must successfully parse as a
/// [`Urn`], but neither the existence nor the validity of the identity
/// histories is guaranteed.
pub fn list_urns<S>(storage: &S) -> Result<impl Iterator<Item = Result<Urn, Error>> + '_, Error>
where
    S: AsRef<storage::ReadOnly>,
{
    let storage = storage.as_ref();

    lazy_static! {
        static ref GLOB: glob::RefspecMatcher =
            refspec_pattern!("refs/namespaces/*/refs/rad/id").into();
    }

    let iter = storage
        .reference_names_glob(GLOB.clone())?
        .map(|name| Ok(Urn::try_from(name?)?.with_path(None)));

    Ok(iter)
}

/// Build an [`Xor`] filter from all available [`Urn`]s.
///
/// The returned `usize` is the number of URNs added to the filter.
pub fn xor_filter<S>(storage: &S) -> Result<(Xor, usize), xor::BuildError<Error>>
where
    S: AsRef<storage::ReadOnly>,
{
    Xor::try_from_iter(list_urns(storage)?.map_ok(SomeUrn::from))
}

fn identities<S>(storage: &S) -> Identities<Void>
where
    S: AsRef<storage::ReadOnly>,
{
    storage.as_ref().identities()
}
