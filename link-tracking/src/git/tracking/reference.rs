// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, convert::TryFrom, fmt, str::FromStr};

use multihash::Multihash;

use link_crypto::{peer, PeerId};
use link_identities::urn::{HasProtocol, Urn};
use radicle_git_ext::RefLike;

pub fn base() -> RefLike {
    reflike!("refs/rad/remotes")
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

impl From<&Remote> for RefLike {
    fn from(remote: &Remote) -> Self {
        match remote {
            Remote::Default => reflike!("default"),
            Remote::Peer(peer) => RefLike::from(peer),
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

    pub fn into_owned(self) -> RefName<'static, R> {
        let urn = self.urn.into_owned();
        RefName {
            remote: self.remote,
            urn: Cow::Owned(urn),
        }
    }
}

impl<'a, R> From<&'a RefName<'a, R>> for RefLike
where
    R: HasProtocol + ToOwned + Clone,
    &'a R: Into<Multihash>,
{
    fn from(r: &'a RefName<'a, R>) -> Self {
        let namespace: String = r.urn.encode_id();
        let namespace =
            RefLike::try_from(namespace).expect("namespace should be valid ref component");
        base().join(namespace).join(&r.remote)
    }
}

pub mod error {
    use link_crypto::peer;

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Parse {
        #[error("expected prefix `refs/rad/remotes`")]
        WrongPrefix,
        #[error("missing path component `{0}`")]
        MissingComponent(&'static str),
        #[error(transparent)]
        Peer(#[from] peer::conversion::Error),
        #[error(transparent)]
        Urn(Box<dyn std::error::Error + Send + Sync + 'static>),
    }
}

impl<'a, R> FromStr for RefName<'a, R>
where
    R: TryFrom<Multihash> + ToOwned + Clone,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    type Err = error::Parse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let suffix = s
            .strip_prefix("refs/rad/remotes/")
            .ok_or(error::Parse::WrongPrefix)?;

        let mut components = suffix.split('/');

        let urn = if let Some(urn) = components.next() {
            Urn::try_from_id(urn).map_err(|e| error::Parse::Urn(e.into()))?
        } else {
            return Err(error::Parse::MissingComponent("<urn>"));
        };

        let remote = if let Some(remote) = components.next() {
            remote.parse()?
        } else {
            return Err(error::Parse::MissingComponent("(default | <peer>)"));
        };

        Ok(Self {
            remote,
            urn: Cow::Owned(urn),
        })
    }
}

impl<'a, R> fmt::Display for RefName<'a, R>
where
    R: HasProtocol + ToOwned + Clone,
    for<'b> &'b R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "refs/rad/remotes/{}/{}",
            self.urn.encode_id(),
            self.remote
        )
    }
}
