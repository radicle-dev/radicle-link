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
    fmt::{self, Display},
    str::FromStr,
};

use thiserror::Error;
use url::Url;

use crate::{
    hash::{self, Hash},
    peer::{self, PeerId},
    uri::{RadUrl, RadUrlRef, RadUrn},
};

pub struct GitUrl {
    pub local_peer: PeerId,
    pub remote_peer: PeerId,
    pub repo: Hash,
}

impl GitUrl {
    pub fn from_rad_url(url: RadUrl, local_peer: PeerId) -> Self {
        Self::from_rad_urn(url.urn, local_peer, url.authority)
    }

    pub fn from_rad_urn(urn: RadUrn, local_peer: PeerId, remote_peer: PeerId) -> Self {
        Self {
            local_peer,
            remote_peer,
            repo: urn.id,
        }
    }

    pub fn as_ref(&self) -> GitUrlRef {
        GitUrlRef {
            local_peer: &self.local_peer,
            remote_peer: &self.remote_peer,
            repo: &self.repo,
        }
    }
}

impl Display for GitUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_ref().fmt(f)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    #[error("Invalid scheme: {0}, expected rad+git://")]
    InvalidScheme(String),

    #[error("Missing repo path")]
    MissingRepo,

    #[error("Cannot-be-a-base URL")]
    CannotBeABase,

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error("Malformed URL")]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Hash(#[from] hash::ParseError),
}

impl FromStr for GitUrl {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if url.scheme() != "rad+git" {
            return Err(Self::Err::InvalidScheme(url.scheme().to_owned()));
        }
        if url.cannot_be_a_base() {
            return Err(Self::Err::CannotBeABase);
        }

        let local_peer = url.username().parse()?;
        let remote_peer = url
            .host_str()
            .expect("we checked for cannot-be-a-base. qed")
            .parse()?;
        let repo = url
            .path_segments()
            .expect("we checked for cannot-be-a-base. qed")
            .next()
            .ok_or_else(|| Self::Err::MissingRepo)
            .and_then(|path| {
                path.trim_end_matches(".git")
                    .parse()
                    .map_err(Self::Err::Hash)
            })?;

        Ok(Self {
            local_peer,
            remote_peer,
            repo,
        })
    }
}

pub struct GitUrlRef<'a> {
    pub local_peer: &'a PeerId,
    pub remote_peer: &'a PeerId,
    pub repo: &'a Hash,
}

impl<'a> GitUrlRef<'a> {
    pub fn from_rad_url(url: &'a RadUrl, local_peer: &'a PeerId) -> Self {
        Self::from_rad_urn(&url.urn, local_peer, &url.authority)
    }

    pub fn from_rad_url_ref(url: RadUrlRef<'a>, local_peer: &'a PeerId) -> Self {
        Self::from_rad_urn(url.urn, local_peer, url.authority)
    }

    pub fn from_rad_urn(urn: &'a RadUrn, local_peer: &'a PeerId, remote_peer: &'a PeerId) -> Self {
        Self {
            local_peer,
            remote_peer,
            repo: &urn.id,
        }
    }

    pub fn to_owned(&self) -> GitUrl {
        GitUrl {
            local_peer: self.local_peer.clone(),
            remote_peer: self.remote_peer.clone(),
            repo: self.repo.clone(),
        }
    }
}

impl<'a> Display for GitUrlRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad+git://{}@{}/{}.git",
            self.local_peer, self.remote_peer, self.repo
        )
    }
}
