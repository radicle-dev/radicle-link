// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, fmt};

use thiserror::Error;

use librad::{
    git::{
        identities::{self, Person},
        refs::{self, Refs},
        storage::{self, ReadOnly, ReadOnlyStorage},
        types::{Namespace, One, Reference},
        Urn,
    },
    git_ext,
    identities::urn,
    PeerId,
};

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Refs(#[from] refs::stored::Error),

    #[error(transparent)]
    Storage(#[from] storage::Error),

    #[error(transparent)]
    Urn(#[from] urn::error::FromRefLike<git_ext::oid::FromMultihashError>),
}

pub fn rad_self<S, P>(storage: &S, urn: &Urn, peer: P) -> Result<Option<Person>, Error>
where
    P: Into<Option<PeerId>>,
    S: AsRef<ReadOnly>,
{
    let rad_self = Reference::rad_self(Namespace::from(urn), peer);
    let urn = convert_or_die(rad_self);
    Ok(identities::person::get(storage, &urn)?)
}

pub fn rad_signed<S, P>(storage: &S, urn: &Urn, peer: P) -> Result<Option<Refs>, Error>
where
    P: Into<Option<PeerId>> + fmt::Debug,
    S: AsRef<ReadOnly>,
{
    Ok(Refs::load(storage, urn, peer)?)
}

pub fn rad_delegate<S, P>(
    storage: &S,
    urn: &Urn,
    delegate: &Urn,
    peer: P,
) -> Result<Option<Person>, Error>
where
    P: Into<Option<PeerId>>,
    S: AsRef<ReadOnly>,
{
    let delegate = Reference::rad_delegate(Namespace::from(urn), delegate).with_remote(peer);
    let urn = convert_or_die(delegate);
    Ok(identities::person::get(storage, &urn)?)
}

pub fn rad_delegates<'a, S, P>(
    storage: &'a S,
    urn: &Urn,
    peer: P,
) -> Result<impl Iterator<Item = Result<Person, Error>> + 'a, Error>
where
    P: Into<Option<PeerId>>,
    S: AsRef<ReadOnly>,
{
    let delegates = Reference::rad_ids_glob(Namespace::from(urn)).with_remote(peer);

    Ok(storage
        .as_ref()
        .reference_names(&delegates)?
        .into_iter()
        .map(|name| {
            name.map_err(Error::from)
                .and_then(|name| Urn::try_from(name).map_err(Error::from))
        })
        .filter_map(move |urn| {
            urn.and_then(|urn| identities::person::get(storage, &urn).map_err(Error::from))
                .transpose()
        }))
}

fn convert_or_die(r: Reference<One>) -> Urn {
    match Urn::try_from(r.clone()) {
        Err(err) => panic!("failed to convert `{}` to Urn: {}", r, err),
        Ok(urn) => urn,
    }
}
