// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt::Debug, ops::Deref, str::FromStr};

use git2::transport::Service as GitService;
use lazy_static::lazy_static;

lazy_static! {
    static ref SERVICE_REGEX: regex::Regex = regex::Regex::new(r"(\S+) '/?(.+)'").unwrap();
}

#[derive(Clone, Copy, PartialEq)]
pub struct Service(pub GitService);

/// A service and URN as passed to the exec_request of an SSH server by git when
/// talking to an SSH remote. The `FromStr` implementation for this type expects
/// a string of the form:
///
/// <request type> /<path>
///
/// Where the request type is either `upload-pack` or `receive-pack`, the
/// leading slash before the urn is optional, and the `path` is whatever the
/// `FromStr` of `Path` provides.
#[derive(Debug, Clone)]
pub struct SshService<Path> {
    pub service: Service,
    pub path: Path,
}

impl From<GitService> for Service {
    fn from(g: GitService) -> Self {
        Service(g)
    }
}

impl From<Service> for GitService {
    fn from(s: Service) -> Self {
        s.0
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseService {
    #[error("the exec str must be in the form <service> <urn>")]
    Format,
    #[error(transparent)]
    Namespace(Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("unknown service {0}")]
    UnknownService(String),
}

impl Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            GitService::UploadPack => write!(f, "upload-pack"),
            GitService::UploadPackLs => write!(f, "upload-pack-ls"),
            GitService::ReceivePack => write!(f, "receive-pack"),
            GitService::ReceivePackLs => write!(f, "receive-pack-ls"),
        }
    }
}

impl Deref for Service {
    type Target = GitService;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Path> FromStr for SshService<Path>
where
    Path: FromStr,
    Path::Err: std::error::Error + Send + Sync + 'static,
{
    type Err = ParseService;

    fn from_str(exec_str: &str) -> Result<Self, Self::Err> {
        let cap = SERVICE_REGEX
            .captures_iter(exec_str)
            .next()
            .ok_or(ParseService::Format)?;
        debug_assert!(cap.len() == 3);
        let service_str: &str = &cap[1];
        let urn_str = &cap[2];

        let path = urn_str
            .parse()
            .map_err(|err| ParseService::Namespace(Box::new(err)))?;
        let service = match service_str {
            "git-upload-pack" => Ok(Service(GitService::UploadPack)),
            "git-receive-pack" => Ok(Service(GitService::ReceivePack)),
            other => Err(ParseService::UnknownService(other.to_string())),
        }?;
        Ok(Self { service, path })
    }
}
