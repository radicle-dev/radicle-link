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
    fmt::{self, Display},
    net::{AddrParseError, SocketAddr},
    str::FromStr,
};

use multihash::Multihash;
use thiserror::Error;
use url::Url;

use crate::{
    hash::Hash,
    identities::urn::Urn,
    peer::{self, PeerId},
    uri::{self, RadUrl, RadUrlRef, RadUrn},
};

#[derive(Clone, Debug, PartialEq)]
pub struct GitUrl<R> {
    pub local_peer: PeerId,
    pub remote_peer: PeerId,
    pub addr_hints: Vec<SocketAddr>,
    pub repo: R,
}

impl GitUrl<Hash> {
    pub fn from_rad_url<Addrs>(url: RadUrl, local_peer: PeerId, addrs: Addrs) -> Self
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        Self::from_rad_urn(url.urn, local_peer, url.authority, addrs)
    }

    pub fn from_rad_urn<Addrs>(
        urn: RadUrn,
        local_peer: PeerId,
        remote_peer: PeerId,
        addrs: Addrs,
    ) -> Self
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        Self {
            local_peer,
            remote_peer,
            addr_hints: addrs.into_iter().collect(),
            repo: urn.id,
        }
    }

    pub fn into_rad_url(self) -> RadUrl {
        self.into()
    }
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
pub enum ParseError<Repo: std::error::Error + Send + Sync + 'static> {
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
    Repo(#[source] Repo),

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
    type Err = ParseError<R::Error>;

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
            .ok_or_else(|| Self::Err::MissingRepo)
            .and_then(|path| {
                let path = path.trim_end_matches(".git");
                let bytes = multibase::decode(path).map(|(_base, bytes)| bytes)?;
                let mhash = Multihash::from_bytes(bytes)?;
                R::try_from(mhash).map_err(Self::Err::Repo)
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

impl Into<RadUrl> for GitUrl<Hash> {
    fn into(self) -> RadUrl {
        RadUrl {
            authority: self.remote_peer,
            urn: RadUrn {
                id: self.repo,
                proto: uri::Protocol::Git,
                path: uri::Path::empty(),
            },
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct GitUrlRef<'a, R> {
    pub local_peer: &'a PeerId,
    pub remote_peer: &'a PeerId,
    pub addr_hints: &'a [SocketAddr],
    pub repo: &'a R,
}

impl<'a> GitUrlRef<'a, Hash> {
    pub fn from_rad_url<Addrs>(
        url: &'a RadUrl,
        local_peer: &'a PeerId,
        addr_hints: &'a Addrs,
    ) -> Self
    where
        Addrs: AsRef<[SocketAddr]>,
    {
        Self::from_rad_urn(&url.urn, local_peer, &url.authority, addr_hints)
    }

    pub fn from_rad_url_ref<Addrs>(
        url: RadUrlRef<'a>,
        local_peer: &'a PeerId,
        addr_hints: &'a Addrs,
    ) -> Self
    where
        Addrs: AsRef<[SocketAddr]>,
    {
        Self::from_rad_urn(url.urn, local_peer, &url.authority, addr_hints)
    }

    pub fn from_rad_urn<Addrs>(
        urn: &'a RadUrn,
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

impl<'a, R> Display for GitUrlRef<'a, R>
where
    &'a R: Into<Multihash>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut url = Url::parse(&format!(
            "{}://{}@{}",
            super::URL_SCHEME,
            self.local_peer,
            self.remote_peer
        ))
        .unwrap();

        url.query_pairs_mut().extend_pairs(
            self.addr_hints
                .iter()
                .map(|addr| ("addr", addr.to_string())),
        );
        url.set_path(&format!(
            "/{}.git",
            multibase::encode(multibase::Base::Base32Z, self.repo.into())
        ));

        f.write_str(url.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};

    use crate::keys::SecretKey;
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
            repo: Hash::hash(b"leboeuf"),
        };

        str_roundtrip(url)
    }
}
