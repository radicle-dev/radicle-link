// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{
    collections::BTreeMap,
    convert::TryFrom,
    iter::FromIterator,
    ops::{Deref, DerefMut},
};

use serde::{
    de::{value::StrDeserializer, IntoDeserializer},
    Deserialize,
};

use crate::{
    git::trailer::{self, Token, Trailer},
    keys::{self, PublicKey},
};

pub mod error;

const TRAILER_TOKEN: &str = "X-Rad-Signature";

#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    key: PublicKey,
    sig: keys::Signature,
}

impl From<(PublicKey, keys::Signature)> for Signature {
    fn from((key, sig): (PublicKey, keys::Signature)) -> Self {
        Self { key, sig }
    }
}

impl TryFrom<Trailer<'_>> for Signature {
    type Error = error::Signature;

    fn try_from(Trailer { values, .. }: Trailer) -> Result<Self, Self::Error> {
        let mut iter = values.iter().flat_map(|val| val.split_whitespace());

        let key = iter
            .next()
            .ok_or(error::Signature::Missing("public key"))
            .and_then(|key| {
                PublicKey::deserialize(
                    key.deref().into_deserializer() as StrDeserializer<serde::de::value::Error>
                )
                .map_err(|e| e.into())
            })?;
        let sig = iter
            .next()
            .ok_or(error::Signature::Missing("signature"))
            .and_then(|sig| {
                keys::Signature::deserialize(
                    sig.deref().into_deserializer() as StrDeserializer<serde::de::value::Error>
                )
                .map_err(|e| e.into())
            })?;

        Ok(Self { key, sig })
    }
}

/// Lets us avoid writing `impl From<(&PublicKey, &keys::Signature)> for
/// Trailer`. While that isn't an orphan because `Trailer` is defined in this
/// crate, it is quite confusing nevertheless, and breaks modularity.
struct SignatureRef<'a> {
    key: &'a PublicKey,
    sig: &'a keys::Signature,
}

impl<'a> From<&'a Signature> for SignatureRef<'a> {
    fn from(Signature { key, sig }: &'a Signature) -> Self {
        Self { key, sig }
    }
}

impl<'a> From<(&'a PublicKey, &'a keys::Signature)> for SignatureRef<'a> {
    fn from((key, sig): (&'a PublicKey, &'a keys::Signature)) -> Self {
        Self { key, sig }
    }
}

impl<'a> From<&'a Signature> for Trailer<'_> {
    fn from(sig: &'a Signature) -> Self {
        Self::from(SignatureRef::from(sig))
    }
}

impl From<Signature> for Trailer<'_> {
    fn from(sig: Signature) -> Self {
        Self::from(SignatureRef::from(&sig))
    }
}

impl<'a> From<SignatureRef<'a>> for Trailer<'_> {
    fn from(SignatureRef { key, sig }: SignatureRef<'a>) -> Self {
        Self {
            token: Token::try_from(TRAILER_TOKEN).unwrap(),
            values: vec![key.to_string().into(), sig.to_string().into()],
        }
    }
}

// FIXME(kim): This should really be a HashMap with a no-op Hasher -- PublicKey
// collisions are catastrophic
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Signatures(BTreeMap<PublicKey, keys::Signature>);

impl Signatures {
    pub fn from_trailers(message: &str) -> Result<Self, error::Signatures> {
        Self::try_from(trailer::parse(message, ":")?)
    }
}

impl Deref for Signatures {
    type Target = BTreeMap<PublicKey, keys::Signature>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Signatures {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Signature> for Signatures {
    fn from(Signature { key, sig }: Signature) -> Self {
        let mut map = BTreeMap::new();
        map.insert(key, sig);
        map.into()
    }
}

impl From<BTreeMap<PublicKey, keys::Signature>> for Signatures {
    fn from(map: BTreeMap<PublicKey, keys::Signature>) -> Self {
        Self(map)
    }
}

impl From<Signatures> for BTreeMap<PublicKey, keys::Signature> {
    fn from(s: Signatures) -> Self {
        s.0
    }
}

impl TryFrom<Vec<Trailer<'_>>> for Signatures {
    type Error = error::Signatures;

    fn try_from(trailers: Vec<Trailer>) -> Result<Self, Self::Error> {
        trailers
            .into_iter()
            .filter(|t| t.token.deref() == TRAILER_TOKEN)
            .map(|trailer| {
                Signature::try_from(trailer)
                    .map(|Signature { key, sig }| (key, sig))
                    .map_err(error::Signatures::from)
            })
            .collect()
    }
}

impl FromIterator<(PublicKey, keys::Signature)> for Signatures {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (PublicKey, keys::Signature)>,
    {
        Self(BTreeMap::from_iter(iter))
    }
}

impl IntoIterator for Signatures {
    type Item = (PublicKey, keys::Signature);
    type IntoIter = <BTreeMap<PublicKey, keys::Signature> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Extend<Signature> for Signatures {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = Signature>,
    {
        for Signature { key, sig } in iter {
            self.insert(key, sig);
        }
    }
}

impl Extend<(PublicKey, keys::Signature)> for Signatures {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = (PublicKey, keys::Signature)>,
    {
        for (key, sig) in iter {
            self.insert(key, sig);
        }
    }
}

impl<'a> From<&'a Signatures> for Vec<Trailer<'a>> {
    fn from(sigs: &'a Signatures) -> Self {
        sigs.deref()
            .iter()
            .map(SignatureRef::from)
            .map(Trailer::from)
            .collect()
    }
}

impl From<Signatures> for Vec<Trailer<'_>> {
    fn from(sigs: Signatures) -> Self {
        sigs.0
            .into_iter()
            .map(Signature::from)
            .map(Trailer::from)
            .collect()
    }
}
