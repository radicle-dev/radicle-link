// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

use git2::transport::Service as GitService;
use thiserror::Error;

use link_git::service;

use crate::PeerId;

#[derive(Debug, PartialEq)]
pub struct Header<Urn> {
    pub service: service::Service,
    pub repo: Urn,
    pub peer: PeerId,
}

impl<Urn> Header<Urn> {
    pub fn new(service: service::Service, repo: Urn, peer: PeerId) -> Self {
        Self {
            service,
            repo,
            peer,
        }
    }
}

impl<Urn: Display> Display for Header<Urn> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.service.0 {
            GitService::UploadPackLs => {
                write!(f, "git-upload-pack {}\0host={}\0ls", self.repo, self.peer)
            },
            GitService::UploadPack => {
                write!(f, "git-upload-pack {}\0host={}", self.repo, self.peer)
            },
            GitService::ReceivePackLs => {
                writeln!(f, "git-receive-pack {}\0host={}\0ls", self.repo, self.peer)
            },
            GitService::ReceivePack => {
                write!(f, "git-receive-pack {}\0host={}", self.repo, self.peer)
            },
        }?;

        writeln!(f, "\0")
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
    MalformedHost(#[from] crypto::peer::conversion::Error),

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

        let mut peer = None;
        let mut ls = false;

        for part in parts {
            if part == "ls" {
                ls = true
            } else if let Some(("host", v)) = part.split_once('=') {
                peer = Some(v.parse::<PeerId>()?);
            }
        }

        let peer = peer.ok_or(ParseError::MissingHost)?;
        let service = match service {
            "git-upload-pack" => {
                if ls {
                    Ok(GitService::UploadPackLs)
                } else {
                    Ok(GitService::UploadPack)
                }
            },

            "git-receive-pack" => {
                if ls {
                    Ok(GitService::ReceivePackLs)
                } else {
                    Ok(GitService::ReceivePack)
                }
            },

            unknown => Err(ParseError::InvalidService(unknown.to_owned())),
        }?;

        Ok(Self::new(service.into(), repo, peer))
    }
}
