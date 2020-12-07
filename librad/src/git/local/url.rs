// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::{self, Display},
    str::FromStr,
};

use thiserror::Error;

use crate::{
    hash::{self, Hash},
    peer::{self, PeerId},
    uri::{self, RadUrn},
};

#[derive(Clone)]
pub struct LocalUrl {
    pub repo: Hash,
    pub local_peer_id: PeerId,
}

impl LocalUrl {
    pub fn from_urn(urn: RadUrn, local_peer_id: PeerId) -> Self {
        Self {
            repo: urn.id,
            local_peer_id,
        }
    }
}

impl Display for LocalUrl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}://{}@{}.git",
            super::URL_SCHEME,
            self.local_peer_id,
            self.repo
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
    Hash(#[from] hash::ParseError),

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

        let repo = url
            .host_str()
            .expect("we checked for cannot-be-a-base. qed")
            .trim_end_matches(".git")
            .parse()?;

        let local_peer_id = url.username().parse()?;

        Ok(Self {
            repo,
            local_peer_id,
        })
    }
}

impl Into<RadUrn> for LocalUrl {
    fn into(self) -> RadUrn {
        RadUrn {
            id: self.repo,
            proto: uri::Protocol::Git,
            path: uri::Path::empty(),
        }
    }
}
