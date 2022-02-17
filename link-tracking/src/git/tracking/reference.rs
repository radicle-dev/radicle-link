// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, convert::TryFrom, fmt, str::FromStr};

use git_ref_format::{refname, Component, RefStr, RefString};
use link_crypto::{peer, PeerId};
use link_identities::urn::{HasProtocol, Urn};
use multihash::Multihash;

pub fn base() -> RefString {
    refname!("refs/rad/remotes")
}

/// The remote component of a tracking reference.
///
/// Its rendered value is of the form:
/// ```ignore
/// (default | <peer>)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Remote {
    Default,
    Peer(PeerId),
}

impl From<Remote> for Option<PeerId> {
    fn from(remote: Remote) -> Self {
        match remote {
            Remote::Default => None,
            Remote::Peer(peer) => Some(peer),
        }
    }
}

impl From<Option<PeerId>> for Remote {
    fn from(peer: Option<PeerId>) -> Self {
        peer.map_or(Self::Default, Self::Peer)
    }
}

impl fmt::Display for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remote::Default => write!(f, "default"),
            Remote::Peer(peer) => write!(f, "{}", peer),
        }
    }
}

impl FromStr for Remote {
    type Err = peer::conversion::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default" => Ok(Self::Default),
            _ => s.parse().map(Self::Peer),
        }
    }
}

impl Default for Remote {
    fn default() -> Self {
        Self::Default
    }
}

impl From<&Remote> for RefString {
    fn from(remote: &Remote) -> Self {
        match remote {
            Remote::Default => refname!("default"),
            Remote::Peer(peer) => Component::from(peer).into_inner().into_owned(),
        }
    }
}

/// The reference name of a tracking reference.
///
/// Its rendered value is of the form:
/// ```ignore
/// refs/rad/remotes/<urn>/(default | <peer>)
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefName<'a, R: ToOwned + Clone> {
    pub remote: Remote,
    pub urn: Cow<'a, Urn<R>>,
}

impl<'a, R: ToOwned + Clone> RefName<'a, R> {
    pub fn new<U, P>(urn: U, peer: P) -> Self
    where
        U: Into<Cow<'a, Urn<R>>>,
        P: Into<Option<PeerId>>,
    {
        Self {
            remote: peer.into().map(Remote::Peer).unwrap_or_default(),
            urn: {
                let urn = urn.into();
                if urn.path.is_some() {
                    urn.into_owned().with_path(None).into()
                } else {
                    urn
                }
            },
        }
    }

    pub fn into_owned<'b>(self) -> RefName<'b, R> {
        let urn = self.urn.into_owned();
        RefName {
            remote: self.remote,
            urn: Cow::Owned(urn),
        }
    }
}

impl<'a, R> From<&'a RefName<'a, R>> for RefString
where
    R: HasProtocol + ToOwned + Clone,
    &'a R: Into<Multihash>,
{
    fn from(r: &'a RefName<'a, R>) -> Self {
        let namespace = Component::from(r.urn.as_ref());
        let remote = RefString::from(&r.remote);
        base().and(namespace).and(remote)
    }
}

pub mod error {
    use link_crypto::peer;

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Parse {
        #[error("expected prefix `refs/rad/remotes`")]
        WrongPrefix,
        #[error("unexpected suffix")]
        Extra,
        #[error("missing path component `{0}`")]
        MissingComponent(&'static str),
        #[error(transparent)]
        Peer(#[from] peer::conversion::Error),
        #[error(transparent)]
        Urn(Box<dyn std::error::Error + Send + Sync + 'static>),
        #[error("invalid ref string")]
        NotARef(#[from] git_ref_format::Error),
    }
}

impl<R> FromStr for RefName<'_, R>
where
    R: TryFrom<Multihash> + ToOwned + Clone,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    type Err = error::Parse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use git_ref_format::name;

        let rs = RefStr::try_from_str(s)?;
        let q = rs.qualified().ok_or(error::Parse::WrongPrefix)?;

        let (_refs, rad, remotes, mut tail) = q.non_empty_components();
        if name::RAD != rad.as_ref() || name::REMOTES != remotes.as_ref() {
            return Err(error::Parse::WrongPrefix);
        }

        let urn: Urn<R> = tail
            .next()
            .ok_or(error::Parse::MissingComponent("<urn>"))
            .and_then(|id| {
                Urn::try_from_id(id.as_str()).map_err(|e| error::Parse::Urn(e.into()))
            })?;
        let remote: Remote = tail
            .next()
            .ok_or(error::Parse::MissingComponent("(default | <peer>)"))
            .and_then(|x| x.as_str().parse().map_err(error::Parse::from))?;

        if tail.next().is_some() {
            return Err(error::Parse::Extra);
        }

        Ok(Self {
            remote,
            urn: Cow::from(urn),
        })
    }
}

impl<'a, R> fmt::Display for RefName<'a, R>
where
    R: HasProtocol + ToOwned + Clone,
    for<'b> &'b R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(RefString::from(self).as_str())
    }
}
