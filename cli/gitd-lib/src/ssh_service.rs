use std::str::FromStr;

use librad::{git::Urn, git_ext};

/// A wrapper around Urn which parses strings of the form "rad:git:<id>.git",
/// this is used as the path parameter of `link_git::SshService`.
#[derive(Debug, Clone)]
pub(crate) struct UrnPath(Urn);

pub(crate) type SshService = link_git::service::SshService<UrnPath>;

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("path component of remote should end with '.git'")]
    MissingSuffix,
    #[error(transparent)]
    Urn(#[from] librad::identities::urn::error::FromStr<git_ext::oid::FromMultihashError>),
}

impl std::fmt::Display for UrnPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.git", self.0)
    }
}

impl AsRef<Urn> for UrnPath {
    fn as_ref(&self) -> &Urn {
        &self.0
    }
}

impl FromStr for UrnPath {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.strip_suffix(".git") {
            Some(prefix) => {
                let urn = Urn::from_str(prefix)?;
                Ok(Self(urn))
            },
            None => Err(Error::MissingSuffix),
        }
    }
}

impl From<UrnPath> for Urn {
    fn from(u: UrnPath) -> Self {
        u.0
    }
}
