// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
    pub static ref DEFAULT_PATH: ext::Qualified = ext::Qualified::from(reflike!("refs/rad/id"));
}

pub mod error {
    use super::*;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum DecodeId<E: std::error::Error + 'static> {
        #[error("invalid id")]
        InvalidId(#[source] E),

        #[error(transparent)]
        Encoding(#[from] multibase::Error),

        #[error(transparent)]
        Multihash(#[from] multihash::DecodeOwnedError),
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum FromRefLike<E: std::error::Error + 'static> {
        #[error("missing {0}")]
        Missing(&'static str),

        #[error("must be a fully-qualified ref, ie. start with `refs/namespaces`")]
        Namespaced(#[from] ext::reference::name::StripPrefixError),

        #[error("invalid id")]
        InvalidId(#[source] DecodeId<E>),

        #[error(transparent)]
        Path(#[from] ext::reference::name::Error),
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum FromStr<E: std::error::Error + 'static> {
        #[error("missing {0}")]
        Missing(&'static str),

        #[error("invalid namespace identifier: {0}")]
        InvalidNID(String),

        #[error("invalid protocol: {0}")]
        InvalidProto(String),

        #[error("invalid id")]
        InvalidId(#[source] DecodeId<E>),

        #[error(transparent)]
        Path(#[from] ext::reference::name::Error),

        #[error(transparent)]
        Utf8(#[from] std::str::Utf8Error),
    }
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

    /// Render [`Self::id`] into the canonical string encoding.
    pub fn encode_id<'a>(&'a self) -> String
    where
        &'a R: Into<Multihash>,
    {
        multibase::encode(multibase::Base::Base32Z, (&self.id).into())
    }

    pub fn try_from_id(s: impl AsRef<str>) -> Result<Self, error::DecodeId<R::Error>>
    where
        R: TryFrom<Multihash>,
        R::Error: std::error::Error + 'static,
    {
        let bytes = multibase::decode(s.as_ref()).map(|x| x.1)?;
        let mhash = Multihash::from_bytes(bytes)?;
        let id = R::try_from(mhash).map_err(error::DecodeId::InvalidId)?;
        Ok(Self::new(id))
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

// FIXME: For some inexplicable reason, rustc rejects an impl for Urn<R>,
// claiming that the blanket impl `impl<T, U> TryFrom<U> for T where U: Into<T>`
// overlaps. We absolutely do not have `Into<Urn<R>> for ext::RefLike`.
impl TryFrom<ext::RefLike> for Urn<ext::Oid> {
    type Error = error::FromRefLike<ext::oid::FromMultihashError>;

    fn try_from(refl: ext::RefLike) -> Result<Self, Self::Error> {
        let refl = refl.strip_prefix("refs/namespaces/")?;
        let mut suf = refl.split('/');
        let ns = suf.next().ok_or(Self::Error::Missing("namespace"))?;
        let urn = Self::try_from_id(ns).map_err(Self::Error::InvalidId)?;
        let path = {
            let path = suf.collect::<Vec<_>>().join("/");
            if path.is_empty() {
                Ok(None)
            } else {
                ext::RefLike::try_from(path).map(Some)
            }
        }?;

        Ok(urn.with_path(path))
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
        let refl = Self::try_from(urn.encode_id()).unwrap();
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
        write!(f, "rad:{}:{}", R::PROTOCOL, self.encode_id())?;

        if let Some(path) = &self.path {
            write!(f, "/{}", path.percent_encode())?;
        }

        Ok(())
    }
}

impl<R, E> FromStr for Urn<R>
where
    R: HasProtocol + TryFrom<Multihash, Error = E>,
    E: std::error::Error + 'static,
{
    type Err = error::FromStr<E>;

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

                let id = iter.next().ok_or(Self::Err::Missing("id"))?;
                let urn = Self::try_from_id(id).map_err(Self::Err::InvalidId)?;
                let path = iter.next().map(ext::RefLike::try_from).transpose()?;
                Ok(urn.with_path(path))
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
    #[cbor(with = "minicbor::bytes")]
    id: &'a [u8],

    #[n(1)]
    proto: SomeProtocol,

    #[b(2)]
    path: Option<&'a str>,
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

pub mod test {
    use super::*;

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
}
