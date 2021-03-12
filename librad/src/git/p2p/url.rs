// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::{self, Display},
    net::{AddrParseError, SocketAddr},
    str::FromStr,
};

use multihash::Multihash;
use thiserror::Error;
use url::Url;

use crate::{
    identities::urn::Urn,
    peer::{self, PeerId},
};

#[derive(Clone, Debug, PartialEq)]
pub struct GitUrl<R> {
    pub local_peer: PeerId,
    pub remote_peer: PeerId,
    pub addr_hints: Vec<SocketAddr>,
    pub repo: R,
}

impl<R> GitUrl<R> {
    pub fn as_ref(&self) -> GitUrlRef<R> {
        GitUrlRef {
            local_peer: &self.local_peer,
            remote_peer: &self.remote_peer,
            addr_hints: &self.addr_hints,
            repo: &self.repo,
        }
    }
}

impl<R> Display for GitUrl<R>
where
    for<'a> &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    #[error("invalid scheme: {0}")]
    InvalidScheme(String),

    #[error("missing repo path")]
    MissingRepo,

    #[error("cannot-be-a-base URL")]
    CannotBeABase,

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error("malformed URL")]
    Url(#[from] url::ParseError),

    #[error("invalid repository identifier")]
    Repo(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error(transparent)]
    Multibase(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),

    #[error(transparent)]
    Addr(#[from] AddrParseError),
}

impl<R> FromStr for GitUrl<R>
where
    R: TryFrom<Multihash>,
    R::Error: std::error::Error + Send + Sync + 'static,
{
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if url.scheme() != super::URL_SCHEME {
            return Err(Self::Err::InvalidScheme(url.scheme().to_owned()));
        }
        if url.cannot_be_a_base() {
            return Err(Self::Err::CannotBeABase);
        }

        let local_peer = url.username().parse()?;
        let host = url
            .host_str()
            .expect("we checked for cannot-be-a-base. qed");

        let remote_peer = host.parse()?;
        let repo = url
            .path_segments()
            .expect("we checked for cannot-be-a-base. qed")
            .next()
            .ok_or(Self::Err::MissingRepo)
            .and_then(|path| {
                let path = path.trim_end_matches(".git");
                let bytes = multibase::decode(path).map(|(_base, bytes)| bytes)?;
                let mhash = Multihash::from_bytes(bytes)?;
                R::try_from(mhash).map_err(|e| Self::Err::Repo(Box::new(e)))
            })?;
        let addr_hints = url
            .query_pairs()
            .filter_map(|(k, v)| if k == "addr" { v.parse().ok() } else { None })
            .collect();

        Ok(Self {
            local_peer,
            remote_peer,
            addr_hints,
            repo,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct GitUrlRef<'a, R> {
    pub local_peer: &'a PeerId,
    pub remote_peer: &'a PeerId,
    pub addr_hints: &'a [SocketAddr],
    pub repo: &'a R,
}

impl<'a, R> GitUrlRef<'a, R>
where
    &'a R: Into<Multihash>,
{
    pub fn from_urn<Addrs>(
        urn: &'a Urn<R>,
        local_peer: &'a PeerId,
        remote_peer: &'a PeerId,
        addr_hints: &'a Addrs,
    ) -> Self
    where
        Addrs: AsRef<[SocketAddr]>,
    {
        Self {
            local_peer,
            remote_peer,
            addr_hints: addr_hints.as_ref(),
            repo: &urn.id,
        }
    }
}

impl<R> GitUrlRef<'_, R> {
    pub fn to_owned(&self) -> GitUrl<R>
    where
        R: Clone,
    {
        GitUrl {
            local_peer: *self.local_peer,
            remote_peer: *self.remote_peer,
            addr_hints: self.addr_hints.to_vec(),
            repo: self.repo.clone(),
        }
    }
}

impl<'a, R> Clone for GitUrlRef<'a, R> {
    #[inline]
    fn clone(&self) -> GitUrlRef<'a, R> {
        Self {
            local_peer: &self.local_peer,
            remote_peer: &self.remote_peer,
            addr_hints: &self.addr_hints,
            repo: &self.repo,
        }
    }
}

impl<'a, R> Copy for GitUrlRef<'a, R> {}

impl<'a, R> Display for GitUrlRef<'a, R>
where
    &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Url::from(*self).as_str())
    }
}

impl<'a, R> From<GitUrlRef<'a, R>> for Url
where
    &'a R: Into<Multihash>,
{
    fn from(git: GitUrlRef<'a, R>) -> Self {
        let mut url = Url::parse(&format!(
            "{}://{}@{}",
            super::URL_SCHEME,
            git.local_peer,
            git.remote_peer
        ))
        .unwrap();

        url.query_pairs_mut()
            .extend_pairs(git.addr_hints.iter().map(|addr| ("addr", addr.to_string())));
        let repo: Multihash = git.repo.into();
        url.set_path(&format!(
            "/{}.git",
            multibase::encode(multibase::Base::Base32Z, repo)
        ));

        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    use crate::{identities::git, keys::SecretKey};
    use librad_test::roundtrip::str_roundtrip;

    #[test]
    fn test_str_roundtrip() {
        let url = GitUrl {
            local_peer: PeerId::from(SecretKey::new()),
            remote_peer: PeerId::from(SecretKey::new()),
            addr_hints: vec![
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 42)),
                SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
                    69,
                    0,
                    0,
                )),
            ],
            repo: git::Revision::from(git2::Oid::zero()),
        };

        str_roundtrip(url)
    }
}
