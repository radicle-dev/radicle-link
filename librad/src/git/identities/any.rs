// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use git_ext::is_not_found_err;

use super::{
    super::{
        storage::{self, glob, Storage},
        types::Reference,
    },
    error::Error,
};
use crate::{
    bloom::BloomFilter,
    identities::{
        self,
        git::{Identities, SomeIdentity},
        SomeUrn,
    },
};

pub use identities::git::Urn;

/// Read an identity for which the type is not known statically.
///
/// Note that the [`Urn::path`] is honoured, and the identity is read from the
/// tip of the branch it resolves to. If that branch is not found, `None` is
/// returned.
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn get(storage: &Storage, urn: &Urn) -> Result<Option<SomeIdentity>, Error> {
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
#[tracing::instrument(level = "debug", skip(storage), err)]
pub fn list<'a>(
    storage: &'a Storage,
) -> Result<impl Iterator<Item = Result<SomeIdentity, Error>> + 'a, Error> {
    let iter = self::list_urns(storage)?.filter_map(move |urn| match urn {
        Ok(urn) => self::get(storage, &urn).transpose(),
        Err(e) => Some(Err(e)),
    });

    Ok(iter)
}

/// Construct a [`BloomFilter`] containing the [`Urn`]s of all identities
/// found in `storage`.
///
/// If the operation would result in an empty bloom filter, `None` is returned.
pub fn bloom(storage: &Storage, fp_rate: f64) -> Result<Option<BloomFilter<SomeUrn>>, Error> {
    let urns = self::list_urns(storage)?.collect::<Result<Vec<_>, _>>()?;
    let sz = urns.len();
    Ok(urns
        .into_iter()
        .map(SomeUrn::Git)
        .fold(BloomFilter::new(sz, fp_rate), |mut bloom, urn| {
            if let Some(b) = bloom.as_mut() {
                b.insert(&urn);
            }
            bloom
        }))
}

/// List only the [`Urn`]s of all identities found in `storage.
///
/// Note that this means that only the namespace must successfully parse as a
/// [`Urn`], but neither the existence nor the validity of the identity
/// histories is guaranteed.
pub fn list_urns(
    storage: &Storage,
) -> Result<impl Iterator<Item = Result<Urn, Error>> + '_, Error> {
    lazy_static! {
        static ref GLOB: glob::RefspecMatcher =
            refspec_pattern!("refs/namespaces/*/refs/rad/id").into();
    }

    let iter = storage
        .reference_names_glob(GLOB.clone())?
        .map(|name| Ok(Urn::try_from(name?)?));

    Ok(iter)
}

fn identities(storage: &Storage) -> Identities<!> {
    storage.identities()
}
