// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::{Infallible, TryFrom},
    iter,
};

use bstr::{BStr, ByteSlice as _};
use either::Either;
use git_ref_format::{lit, Component, Qualified, RefString};
use link_crypto::PeerId;
use thiserror::Error;

use super::{Owned, RemoteTracking};
use crate::ids;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("unqualified ref name")]
    Unqualified,

    #[error("unexpected namespace: '{0}'")]
    Namespaced(String),

    #[error("invalid remote peer id")]
    PeerId(#[from] link_crypto::peer::conversion::Error),

    #[error("malformed ref name")]
    Check(#[from] git_ref_format::Error),

    #[error(transparent)]
    Utf8(#[from] bstr::Utf8Error),

    #[error("failed to parse")]
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Parsed<'a, Urn> {
    pub remote: Option<PeerId>,
    pub inner: Either<Rad<Urn>, Owned<'a>>,
}

impl<'a, Urn> Parsed<'a, Urn>
where
    Urn: ids::Urn + Clone,
{
    pub fn to_owned<'b>(&self) -> Owned<'b> {
        use Either::*;

        match &self.inner {
            Left(rad) => rad.clone().into(),
            Right(o) => o.clone().into_owned(),
        }
    }

    pub fn to_remote_tracking<'b>(&self) -> Option<RemoteTracking<'b>> {
        let id = self.remote.as_ref()?;
        self.inner.as_ref().either(
            |x| super::remote_tracking(id, x.clone()),
            |y| super::remote_tracking(id, y.clone().into_owned()),
        )
    }
}

impl<'a, Urn> AsRef<Either<Rad<Urn>, Owned<'a>>> for Parsed<'a, Urn> {
    fn as_ref(&self) -> &Either<Rad<Urn>, Owned<'a>> {
        &self.inner
    }
}

impl<Urn> TryFrom<RefString> for Parsed<'_, Urn>
where
    Urn: ids::Urn,
{
    type Error = Error;

    fn try_from(r: RefString) -> Result<Self, Self::Error> {
        r.into_qualified()
            .ok_or(Error::Unqualified)
            .and_then(Self::try_from)
    }
}

impl<'a, Urn> TryFrom<Qualified<'a>> for Parsed<'_, Urn>
where
    Urn: ids::Urn,
{
    type Error = Error;

    fn try_from(qualified: Qualified<'a>) -> Result<Self, Self::Error> {
        use git_ref_format::name::str::*;

        fn parse_inner<'a, 'b, Urn>(
            mut iter: impl Iterator<Item = Component<'a>>,
        ) -> Option<Either<Rad<Urn>, Owned<'b>>>
        where
            Urn: ids::Urn,
        {
            use Either::*;

            match iter.next()? {
                x if RAD == x.as_str() => match (iter.next()?.as_str(), iter.next()) {
                    (ID, None) => Some(Left(Rad::Id)),
                    (SELF, None) => Some(Left(Rad::Selv)),
                    (SIGNED_REFS, None) => Some(Left(Rad::SignedRefs)),
                    (IDS, Some(id)) => {
                        let urn = Urn::try_from_id(id.as_str()).ok()?;
                        iter.next().is_none().then_some(Left(Rad::Ids { urn }))
                    },

                    _ => None,
                },

                x => {
                    let owned = iter.next().and_then(|y| {
                        let q = Qualified::from((
                            lit::Refs,
                            x,
                            iter::once(y).chain(iter).collect::<RefString>(),
                        ));
                        super::owned(q)
                    })?;
                    Some(Right(owned))
                },
            }
        }

        match qualified.non_empty_components() {
            (_refs, namespaces, _, _) if NAMESPACES == namespaces.as_str() => {
                Err(Error::Namespaced(namespaces.as_str().to_owned()))
            },

            (_refs, remotes, peer_id, name) if REMOTES == remotes.as_str() => {
                let remote = peer_id.as_str().parse::<PeerId>().map_err(Error::from)?;
                parse_inner(name).ok_or(Error::Other).map(|inner| Parsed {
                    remote: Some(remote),
                    inner,
                })
            },

            (_refs, cat, head, tail) => {
                parse_inner(iter::once(cat).chain(iter::once(head)).chain(tail))
                    .ok_or(Error::Other)
                    .map(|inner| Parsed {
                        remote: None,
                        inner,
                    })
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Rad<Urn> {
    Id,
    Selv, // self
    SignedRefs,
    Ids { urn: Urn },
}

impl<Urn> From<Rad<Urn>> for Qualified<'_>
where
    Urn: ids::Urn,
{
    fn from(r: Rad<Urn>) -> Self {
        match r {
            Rad::Id => lit::REFS_RAD_ID.into(),
            Rad::Selv => lit::REFS_RAD_SELF.into(),
            Rad::SignedRefs => lit::REFS_RAD_SIGNED_REFS.into(),
            Rad::Ids { urn } => (
                lit::Refs,
                lit::Rad,
                lit::Ids,
                Component::from_refstring(super::from_urn(&urn)).expect("urn is a valid component"),
            )
                .into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Identity(String);

impl Identity {
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl ids::Urn for Identity {
    type Error = Infallible;

    fn try_from_id(s: impl AsRef<str>) -> Result<Self, Self::Error> {
        Ok(Self(s.as_ref().to_owned()))
    }

    fn encode_id(&self) -> String {
        self.0.clone()
    }
}

impl AsRef<str> for Identity {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

pub fn parse<Urn>(input: &BStr) -> Result<Parsed<Urn>, Error>
where
    Urn: ids::Urn,
{
    let rs = RefString::try_from(input.to_str()?)?;
    Parsed::try_from(rs)
}
