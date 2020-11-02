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
    str::FromStr,
};

use git_ext as ext;
use multihash::Multihash;
use thiserror::Error;

use super::Urn;
use crate::peer::{self, PeerId};

#[derive(Clone, Debug, PartialEq)]
pub struct LocalUrl {
    pub urn: Urn,
    pub local_peer_id: PeerId,
}

impl LocalUrl {
    pub fn from_urn(urn: Urn, local_peer_id: PeerId) -> Self {
        Self { urn, local_peer_id }
    }
}

impl Display for LocalUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}://{}@{}.git",
            super::URL_SCHEME,
            self.local_peer_id,
            multibase::encode(multibase::Base::Base32Z, Multihash::from(&self.urn.id))
        )
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    #[error("invalid scheme: {0}")]
    InvalidScheme(String),

    #[error("cannot-be-a-base URL")]
    CannotBeABase,

    #[error("malformed URL")]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Oid(#[from] ext::oid::FromMultihashError),

    #[error(transparent)]
    Multibase(#[from] multibase::Error),

    #[error(transparent)]
    Multihash(#[from] multihash::DecodeOwnedError),

    #[error(transparent)]
    Peer(#[from] peer::conversion::Error),
}

impl FromStr for LocalUrl {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = url::Url::parse(s)?;
        if url.scheme() != super::URL_SCHEME {
            return Err(Self::Err::InvalidScheme(url.scheme().to_owned()));
        }
        if url.cannot_be_a_base() {
            return Err(Self::Err::CannotBeABase);
        }

        let host = url
            .host_str()
            .expect("we checked for cannot-be-a-base. qed")
            .trim_end_matches(".git");
        let bytes = multibase::decode(host).map(|(_base, bytes)| bytes)?;
        let mhash = Multihash::from_bytes(bytes)?;
        let oid = ext::Oid::try_from(mhash)?;
        let urn = Urn::new(oid);

        let local_peer_id = url.username().parse()?;

        Ok(Self { urn, local_peer_id })
    }
}

impl Into<Urn> for LocalUrl {
    fn into(self) -> Urn {
        self.urn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{git::Urn, keys::SecretKey, peer::PeerId};
    use librad_test::roundtrip::str_roundtrip;

    #[test]
    fn trip() {
        let url = LocalUrl {
            urn: Urn::new(git2::Oid::zero().into()),
            local_peer_id: PeerId::from(SecretKey::new()),
        };

        str_roundtrip(url)
    }
}
