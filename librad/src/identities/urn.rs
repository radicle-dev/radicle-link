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
    convert::TryFrom,
    fmt::{self, Debug, Display},
    str::FromStr,
};

use multihash::Multihash;
use thiserror::Error;

use crate::git::ext;

pub trait HasProtocol: protocol::Sealed {
    const PROTOCOL: &'static str;
}

impl HasProtocol for super::git::Revision {
    const PROTOCOL: &'static str = "git";
}

mod protocol {
    use super::ext;

    pub trait Sealed {}

    impl Sealed for ext::Oid {}
}

// TODO(kim): shall replace RadUrn, need to add path component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Urn<R> {
    pub id: R,
}

impl<R> Urn<R> {
    pub const fn new(id: R) -> Self {
        Self { id }
    }

    pub fn map<F, T>(self, f: F) -> Urn<T>
    where
        F: FnOnce(R) -> T,
    {
        Urn { id: f(self.id) }
    }
}

impl<R> Display for Urn<R>
where
    R: HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad:{}:{}",
            R::PROTOCOL,
            multibase::encode(multibase::Base::Base32Z, (&self.id).into())
        )
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError<E: std::error::Error + 'static> {
    #[error("missing {0}")]
    Missing(&'static str),

    #[error("invalid namespace identifier: {0}")]
    InvalidNID(String),

    #[error("invalid protocol: {0}")]
    InvalidProto(String),

    #[error("invalid Id")]
    InvalidId(#[source] E),

    #[error(transparent)]
    Encoding(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),
}

impl<R, E> FromStr for Urn<R>
where
    R: HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    type Err = ParseError<E>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut components = s.split(':');

        components
            .next()
            .ok_or(Self::Err::Missing("namespace"))
            .and_then(|nid| {
                (nid == "rad")
                    .then_some(())
                    .ok_or_else(|| Self::Err::InvalidNID(nid.to_string()))
            })?;

        components
            .next()
            .ok_or(Self::Err::Missing("protocol"))
            .and_then(|proto| {
                (R::PROTOCOL == proto)
                    .then_some(())
                    .ok_or_else(|| Self::Err::InvalidProto(proto.to_string()))
            })?;

        components
            .next()
            .ok_or(Self::Err::Missing("id"))
            .and_then(|s| {
                multibase::decode(s)
                    .map(|(_base, bytes)| bytes)
                    .map_err(Self::Err::from)
            })
            .and_then(|bytes| Multihash::from_bytes(bytes).map_err(Self::Err::from))
            .and_then(|mhash| R::try_from(mhash).map_err(Self::Err::InvalidId))
            .map(Self::new)
    }
}

impl<R> serde::Serialize for Urn<R>
where
    R: HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de, R, E> serde::Deserialize<'de> for Urn<R>
where
    R: HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: &str = serde::Deserialize::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use librad_test::roundtrip::{json_roundtrip, str_roundtrip};
    use proptest::prelude::*;

    use crate::git::ext::oid::{tests::gen_oid, Oid};

    fn gen_urn() -> impl Strategy<Value = Urn<Oid>> {
        gen_oid(git2::ObjectType::Tree).prop_map(Urn::new)
    }

    /// All serialisation roundtrips [`Urn`] must pass
    fn trippin<R, E>(urn: Urn<R>)
    where
        R: Clone + Debug + PartialEq + TryFrom<Multihash, Error = E> + HasProtocol,
        for<'a> &'a R: Into<Multihash>,
        E: std::error::Error + 'static,
    {
        str_roundtrip(urn.clone());
        json_roundtrip(urn);
    }

    proptest! {
        #[test]
        fn roundtrip(urn in gen_urn()) {
            trippin(urn)
        }
    }
}
