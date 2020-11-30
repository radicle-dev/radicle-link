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
}

impl<R> From<R> for Urn<R> {
    fn from(r: R) -> Self {
        Self::new(r)
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
mod tests {
    use super::*;

    use git_ext::Oid;
    use librad_test::roundtrip::*;
    use proptest::prelude::*;

    use crate::identities::gen::gen_oid;

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
}
