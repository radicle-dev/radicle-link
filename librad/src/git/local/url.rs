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
use crate::peer;

#[derive(Clone, Debug, PartialEq)]
pub struct LocalUrl {
    pub urn: Urn,
    pub(super) active_index: Option<usize>,
}

impl From<Urn> for LocalUrl {
    fn from(urn: Urn) -> Self {
        Self {
            urn,
            active_index: None,
        }
    }
}

impl Display for LocalUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}://{}.git", super::URL_SCHEME, self.urn.encode_id(),)?;

        if let Some(idx) = self.active_index {
            write!(f, "#{}", idx)?;
        }

        Ok(())
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

    #[error("active index is not a number")]
    Idx(#[from] std::num::ParseIntError),

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

        let active_index = url.fragment().map(|s| s.parse()).transpose()?;

        Ok(Self { urn, active_index })
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

    use crate::git::Urn;
    use librad_test::roundtrip::str_roundtrip;

    #[test]
    fn trip() {
        let url = LocalUrl::from(Urn::new(git2::Oid::zero().into()));
        str_roundtrip(url)
    }
}
