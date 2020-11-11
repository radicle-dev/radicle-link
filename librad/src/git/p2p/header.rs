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
    fmt::{self, Debug, Display},
    ops::Deref,
    str::FromStr,
};

use git2::transport::Service as GitService;
use thiserror::Error;

use crate::peer::{self, PeerId};

#[derive(Debug, PartialEq)]
pub struct Header<Urn> {
    pub service: Service,
    pub repo: Urn,
    pub peer: PeerId,
}

impl<Urn> Header<Urn> {
    pub fn new(service: GitService, repo: Urn, peer: PeerId) -> Self {
        Self {
            service: Service(service),
            repo,
            peer,
        }
    }
}

impl<Urn: Display> Display for Header<Urn> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.service.0 {
            GitService::UploadPackLs => {
                writeln!(f, "git-upload-pack {}\0host={}\0ls\0", self.repo, self.peer)
            },
            GitService::UploadPack => {
                writeln!(f, "git-upload-pack {}\0host={}\0", self.repo, self.peer)
            },
            GitService::ReceivePackLs => writeln!(
                f,
                "git-receive-pack {}\0host={}\0ls\0",
                self.repo, self.peer
            ),
            GitService::ReceivePack => {
                writeln!(f, "git-receive-pack {}\0host={}\0", self.repo, self.peer)
            },
        }
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    #[error("missing service")]
    MissingService,

    #[error("invalid service: {0}. Must be one of `git-upload-pack` or `git-receive-pack`")]
    InvalidService(String),

    #[error("missing repo")]
    MissingRepo,

    #[error("invalid repo")]
    InvalidRepo(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("missing host")]
    MissingHost,

    #[error("malformed host. Must be a PeerId")]
    MalformedHost(#[from] peer::conversion::Error),

    #[error("invalid mode: `{0}`. Must be `ls`, or absent")]
    InvalidMode(String),
}

impl<Urn> FromStr for Header<Urn>
where
    Urn: FromStr,
    <Urn as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(|c| c == ' ' || c == '\0');

        let service = parts.next().ok_or(ParseError::MissingService)?;
        let repo = parts
            .next()
            .ok_or(ParseError::MissingRepo)
            .and_then(|repo| {
                repo.parse::<Urn>()
                    .map_err(|e| ParseError::InvalidRepo(Box::new(e)))
            })?;
        let peer = parts
            .next()
            .and_then(|peer| peer.strip_prefix("host="))
            .ok_or(ParseError::MissingHost)
            .and_then(|peer| peer.parse::<PeerId>().map_err(|e| e.into()))?;
        let mode = parts.next().unwrap_or("");

        let service = match service {
            "git-upload-pack" => match mode {
                "ls" => Ok(GitService::UploadPackLs),
                "" | "\n" => Ok(GitService::UploadPack),
                _ => Err(ParseError::InvalidMode(mode.to_owned())),
            },

            "git-receive-pack" => match mode {
                "ls" => Ok(GitService::ReceivePackLs),
                "" | "\n" => Ok(GitService::ReceivePack),
                _ => Err(ParseError::InvalidMode(mode.to_owned())),
            },

            unknown => Err(ParseError::InvalidService(unknown.to_owned())),
        }?;

        Ok(Self::new(service, repo, peer))
    }
}

pub struct Service(pub GitService);

impl Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Service")
            .field(match self.0 {
                GitService::UploadPackLs => &"UploadPackLs",
                GitService::UploadPack => &"UploadPack",
                GitService::ReceivePackLs => &"ReceivePackLs",
                GitService::ReceivePack => &"ReceivePack",
            })
            .finish()
    }
}

impl PartialEq for Service {
    #[allow(clippy::match_like_matches_macro)]
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            (GitService::UploadPackLs, GitService::UploadPackLs) => true,
            (GitService::UploadPack, GitService::UploadPack) => true,
            (GitService::ReceivePackLs, GitService::ReceivePackLs) => true,
            (GitService::ReceivePack, GitService::ReceivePack) => true,
            _ => false,
        }
    }
}

impl Deref for Service {
    type Target = GitService;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{git::Urn, keys::SecretKey};

    #[test]
    fn test_str_roundtrip() {
        let hdr = Header::new(
            GitService::UploadPackLs,
            Urn::new(git_ext::Oid::from(git2::Oid::zero())),
            PeerId::from(SecretKey::new()),
        );

        assert_eq!(hdr, hdr.to_string().parse::<Header<Urn>>().unwrap())
    }
}
