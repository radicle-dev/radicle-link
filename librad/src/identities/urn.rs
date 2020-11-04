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

use git_ext as ext;
use multihash::{Multihash, MultihashRef};
use percent_encoding::percent_decode_str;
use thiserror::Error;

use super::sealed;

lazy_static! {
    pub static ref DEFAULT_PATH: ext::RefLike = reflike!("refs/rad/id");
}

pub trait HasProtocol: sealed::Sealed {
    const PROTOCOL: &'static str;
}

impl HasProtocol for super::git::Revision {
    const PROTOCOL: &'static str = "git";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SomeProtocol {
    Git,
}

impl minicbor::Encode for SomeProtocol {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            Self::Git => e.u8(0),
        }?;

        Ok(())
    }
}

impl<'de> minicbor::Decode<'de> for SomeProtocol {
    fn decode(d: &mut minicbor::Decoder) -> Result<Self, minicbor::decode::Error> {
        match d.u8()? {
            0 => Ok(Self::Git),
            _ => Err(minicbor::decode::Error::Message("unknown protocol")),
        }
    }
}

impl TryFrom<&str> for SomeProtocol {
    type Error = &'static str;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "git" => Ok(SomeProtocol::Git),
            _ => Err("unknown protocol"),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Urn<R> {
    pub id: R,
    pub path: Option<ext::RefLike>,
}

impl<R> Urn<R> {
    pub const fn new(id: R) -> Self {
        Self { id, path: None }
    }

    pub fn map<F, S>(self, f: F) -> Urn<S>
    where
        F: FnOnce(R) -> S,
    {
        Urn {
            id: f(self.id),
            path: self.path,
        }
    }

    pub fn map_path<F>(self, f: F) -> Self
    where
        F: FnOnce(Option<ext::RefLike>) -> Option<ext::RefLike>,
    {
        Self {
            id: self.id,
            path: f(self.path),
        }
    }

    pub fn with_path<P>(self, path: P) -> Self
    where
        P: Into<Option<ext::RefLike>>,
    {
        self.map_path(|_| path.into())
    }
}

impl<R> From<R> for Urn<R> {
    fn from(r: R) -> Self {
        Self::new(r)
    }
}

#[derive(Debug, Error)]
pub enum FromRefLikeError {
    #[error("missing {0}")]
    Missing(&'static str),

    #[error("must be an absolute ref, ie. start with `refs/namespaces`")]
    Absolute(#[from] std::path::StripPrefixError),

    #[error(transparent)]
    OidFromMultihash(#[from] ext::oid::FromMultihashError),

    #[error(transparent)]
    Path(#[from] ext::reference::name::Error),

    #[error(transparent)]
    Encoding(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),

    #[error("invalid utf8")]
    Utf8,
}

// FIXME: For some inexplicable reason, rustc rejects an impl for Urn<R>,
// claiming that the blanket impl `impl<T, U> TryFrom<U> for T where U: Into<T>`
// overlaps. We absolutely do not have `Into<Urn<R>> for ext::RefLike`.
impl TryFrom<ext::RefLike> for Urn<ext::Oid> {
    type Error = FromRefLikeError;

    fn try_from(refl: ext::RefLike) -> Result<Self, Self::Error> {
        let mut suf = refl.strip_prefix("refs/namespaces/")?.iter();
        let id = suf
            .next()
            .ok_or(Self::Error::Missing("namespace"))
            .and_then(|ns| {
                let ns = ns.to_str().ok_or(Self::Error::Utf8)?;
                let bytes = multibase::decode(ns).map(|(_base, bytes)| bytes)?;
                let mhash = Multihash::from_bytes(bytes)?;
                Ok(ext::Oid::try_from(mhash)?)
            })?;
        let path = {
            let path = suf.as_path();
            if path.as_os_str().is_empty() {
                Ok(None)
            } else {
                ext::RefLike::try_from(path).map(Some)
            }
        }?;

        Ok(Self { id, path })
    }
}

impl<R> From<Urn<R>> for ext::RefLike
where
    R: HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn from(urn: Urn<R>) -> Self {
        Self::from(&urn)
    }
}

// FIXME: this is not kosher -- doesn't include `refs/namespaces`, but
// everything after that. Should have a better type for that.
impl<'a, R> From<&'a Urn<R>> for ext::RefLike
where
    R: HasProtocol,
    &'a R: Into<Multihash>,
{
    fn from(urn: &'a Urn<R>) -> Self {
        let refl = Self::try_from(multibase::encode(
            multibase::Base::Base32Z,
            (&urn.id).into(),
        ))
        .unwrap();
        match &urn.path {
            None => refl,
            Some(path) => refl.join(ext::Qualified::from(path.clone())),
        }
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
        )?;

        if let Some(path) = &self.path {
            write!(f, "/{}", path.percent_encode())?;
        }

        Ok(())
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
    InvalidPath(#[from] ext::reference::name::Error),

    #[error(transparent)]
    Encoding(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),

    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
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
            .ok_or(Self::Err::Missing("id[/path]"))
            .and_then(|s| {
                let decoded = percent_decode_str(s).decode_utf8()?;
                let mut iter = decoded.splitn(2, '/');

                let id = iter.next().ok_or(Self::Err::Missing("id")).and_then(|id| {
                    let bytes = multibase::decode(id).map(|(_base, bytes)| bytes)?;
                    let mhash = Multihash::from_bytes(bytes)?;
                    R::try_from(mhash).map_err(Self::Err::InvalidId)
                })?;

                let path = iter.next().map(ext::RefLike::try_from).transpose()?;

                Ok(Self { id, path })
            })
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

#[derive(Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
#[cbor(array)]
struct AsCbor<'a> {
    #[b(0)]
    #[cbor(with = "bytes")]
    id: &'a [u8],

    #[n(1)]
    proto: SomeProtocol,

    #[b(2)]
    path: Option<&'a str>,
}

// Need to force minicbor to treat our slice as bytes, not array (the `Encode` /
// `Decode` impls disagree).
mod bytes {
    use minicbor::*;

    pub(super) fn encode<W: encode::Write>(
        x: &[u8],
        e: &mut Encoder<W>,
    ) -> Result<(), encode::Error<W::Error>> {
        e.bytes(x)?;
        Ok(())
    }

    pub(super) fn decode<'a>(d: &mut Decoder<'a>) -> Result<&'a [u8], decode::Error> {
        d.bytes()
    }
}

impl<R> minicbor::Encode for Urn<R>
where
    R: HasProtocol,
    for<'a> &'a R: Into<Multihash>,
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let id: Multihash = (&self.id).into();
        e.encode(AsCbor {
            id: id.as_bytes(),
            proto: SomeProtocol::try_from(R::PROTOCOL).unwrap(),
            path: self.path.as_ref().map(|path| path.as_str()),
        })?;

        Ok(())
    }
}

impl<'de, R> minicbor::Decode<'de> for Urn<R>
where
    for<'a> R: HasProtocol + TryFrom<MultihashRef<'a>>,
{
    fn decode(d: &mut minicbor::Decoder) -> Result<Self, minicbor::decode::Error> {
        use minicbor::decode::Error::Message as Error;

        let AsCbor { id, path, .. } = d.decode()?;

        let id = {
            let mhash = MultihashRef::from_slice(id).or(Err(Error("invalid multihash")))?;
            R::try_from(mhash).or(Err(Error("invalid id")))
        }?;
        let path = path
            .map(ext::RefLike::try_from)
            .transpose()
            .or(Err(Error("invalid path")))?;

        Ok(Self { id, path })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use git_ext::Oid;
    use librad_test::roundtrip::*;
    use proptest::prelude::*;

    use crate::identities::gen::gen_oid;

    /// Fake `id` of a `Urn<FakeId>`.
    ///
    /// Not cryptographically secure, but cheap to create for tests.
    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
    pub struct FakeId(pub usize);

    impl sealed::Sealed for FakeId {}

    impl HasProtocol for FakeId {
        const PROTOCOL: &'static str = "test";
    }

    impl From<usize> for FakeId {
        fn from(sz: usize) -> FakeId {
            Self(sz)
        }
    }

    impl From<FakeId> for Multihash {
        fn from(id: FakeId) -> Self {
            Self::from(&id)
        }
    }

    impl From<&FakeId> for Multihash {
        fn from(id: &FakeId) -> Self {
            multihash::wrap(multihash::Code::Identity, &id.0.to_be_bytes())
        }
    }

    fn gen_urn() -> impl Strategy<Value = Urn<Oid>> {
        (
            gen_oid(git2::ObjectType::Tree),
            prop::option::of(prop::collection::vec("[a-z0-9]+", 1..3)),
        )
            .prop_map(|(id, path)| {
                let path = path.map(|elems| {
                    ext::RefLike::try_from(elems.join("/")).unwrap_or_else(|e| {
                        panic!(
                            "Unexpected error generating a RefLike from `{}`: {}",
                            elems.join("/"),
                            e
                        )
                    })
                });
                Urn { id, path }
            })
    }

    /// All serialisation roundtrips [`Urn`] must pass
    fn trippin<R, E>(urn: Urn<R>)
    where
        R: Clone + Debug + PartialEq + TryFrom<Multihash, Error = E> + HasProtocol,
        for<'a> R: TryFrom<MultihashRef<'a>>,
        for<'a> &'a R: Into<Multihash>,
        E: std::error::Error + 'static,
    {
        str_roundtrip(urn.clone());
        json_roundtrip(urn.clone());
        cbor_roundtrip(urn);
    }

    proptest! {
        #[test]
        fn roundtrip(urn in gen_urn()) {
            trippin(urn)
        }
    }

    #[test]
    fn is_reflike() {
        assert_eq!(
            "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
            ext::RefLike::from(Urn::new(ext::Oid::from(git2::Oid::zero()))).as_str()
        )
    }

    #[test]
    fn is_reflike_with_path() {
        assert_eq!(
            "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/heads/lolek/bolek",
            ext::RefLike::from(Urn {
                id: ext::Oid::from(git2::Oid::zero()),
                path: Some(ext::RefLike::try_from("lolek/bolek").unwrap())
            })
            .as_str()
        )
    }

    #[test]
    fn is_reflike_with_qualified_path() {
        assert_eq!(
            "hnrkyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/refs/remotes/lolek/bolek",
            ext::RefLike::from(Urn {
                id: ext::Oid::from(git2::Oid::zero()),
                path: Some(ext::RefLike::try_from("refs/remotes/lolek/bolek").unwrap())
            })
            .as_str()
        )
    }
}
