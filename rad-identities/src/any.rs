// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{
        identities::{any, Error, SomeIdentity},
        storage::ReadOnly,
        Urn,
    },
    identities::payload::SomePayload,
};

use crate::display;

pub type Display = display::Display<SomePayload>;

pub fn get<S>(storage: &S, urn: &Urn) -> Result<Option<SomeIdentity>, Error>
where
    S: AsRef<ReadOnly>,
{
    any::get(storage, urn)
}

pub fn list<'a, S, A>(
    storage: &'a S,
    filter: impl Fn(SomeIdentity) -> Option<A> + 'a,
) -> Result<impl Iterator<Item = Result<A, Error>> + 'a, Error>
where
    S: AsRef<ReadOnly>,
{
    Ok(any::list(storage)?.filter_map(move |i| match i {
        Ok(i) => filter(i).map(Ok),
        Err(e) => Some(Err(e)),
    }))
}
