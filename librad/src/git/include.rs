// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    convert::TryFrom,
    fmt::Debug,
    io::{self, Write},
    path::{self, PathBuf},
};

use git_ext as ext;
use tempfile::NamedTempFile;

use super::{
    local::url::LocalUrl,
    types::{Flat, Force, GenericRef, Reference, Refspec, Remote},
};
use crate::PeerId;

/// Config key to reference generated include files in working copies.
pub const GIT_CONFIG_PATH_KEY: &str = "include.path";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error("a remote must be set with a fetch refspec when creating an include entry")]
    MissingRefspec,

    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),

    #[error(transparent)]
    Refname(#[from] ext::reference::name::Error),
}

/// An `Include` is a representation of an include file which we want to
/// generate for working copies.
///
/// When generated it will include a list of remotes of the form:
/// ```text
/// [remote "<handle>@<peer_id>"]
///     url = rad://<local_peer_id>@<repo>.git
///     fetch = refs/remotes/<peer_id>/heads/*:refs/remotes/<handle>@<peer_id>/*
/// ```
///
/// This file can then be added to the working copy's `config` file as:
/// ```text
/// [include]
///     path = <path>/<repo>.inc
/// ```
pub struct Include<Path> {
    /// The list of remotes that will be generated for this include file.
    remotes: Vec<Remote<LocalUrl>>,
    /// The directory path where the include file will be stored.
    pub path: Path,
    /// The namespace and `PeerId` this include file is interested in. In other
    /// words, the generated include file should be for some project that
    /// lives under this namespace, of this URL, in the monorepo.
    ///
    /// Note that the final file name will be named after
    /// the namespace.
    pub local_url: LocalUrl,
}

impl<Path> Include<Path> {
    /// Create a new `Include` with an empty set of remotes.
    pub fn new(path: Path, local_url: LocalUrl) -> Self {
        Include {
            remotes: vec![],
            path,
            local_url,
        }
    }

    pub fn add_remote(&mut self, url: LocalUrl, peer: PeerId, handle: impl Into<ext::RefLike>) {
        let remote = Self::build_remote(url, peer, handle);
        self.remotes.push(remote);
    }

    /// Writes the contents of the [`git2::Config`] of the include file to disk.
    #[allow(clippy::unit_arg)]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn save(self) -> Result<(), Error>
    where
        Path: AsRef<path::Path>,
    {
        let mut tmp = NamedTempFile::new_in(&self.path)?;
        {
            // **NB**: We can't use `git2::Config::set_multivar`, because
            // `libgit2` does not realise that we have only one file (it thinks
            // the file is included). This would limit us to a single fetchspec /
            // pushspec respectively.
            for remote in &self.remotes {
                if remote.fetchspecs.is_empty() {
                    return Err(Error::MissingRefspec);
                }

                tracing::debug!("writing remote {}", remote.name);
                writeln!(tmp, "[remote \"{}\"]", remote.name)?;
                tracing::debug!("remote.{}.url = {}", remote.name, remote.url);
                writeln!(tmp, "\turl = {}", remote.url)?;

                for spec in remote.fetchspecs.iter() {
                    tracing::debug!("remote.{}.fetch = {}", remote.name, spec);
                    writeln!(tmp, "\tfetch = {}", spec)?;
                }

                for spec in remote.pushspecs.iter() {
                    tracing::debug!("remote.{}.push = {}", remote.name, spec);
                    writeln!(tmp, "\tpush = {}", spec)?;
                }
            }
        }
        tmp.as_file().sync_data()?;
        tmp.persist(self.file_path())?;
        tracing::trace!("persisted include file to {}", self.file_path().display());

        Ok(())
    }

    /// The full file path where this include file will be created.
    pub fn file_path(&self) -> PathBuf
    where
        Path: AsRef<path::Path>,
    {
        self.path
            .as_ref()
            .to_path_buf()
            .join(self.local_url.urn.encode_id())
            .with_extension("inc")
    }

    /// Generate an include file by giving it a `RadUrn` for a project and the
    /// tracked person's handle/`PeerId` pairs for that project.
    ///
    /// The tracked personal identities are expected to be retrieved by talking
    /// to the [`crate::git::storage::Storage`].
    #[tracing::instrument(level = "debug", skip(tracked))]
    pub fn from_tracked_persons<R, I>(path: Path, local_url: LocalUrl, tracked: I) -> Self
    where
        Path: Debug,
        R: Into<ext::RefLike>,
        I: IntoIterator<Item = (R, PeerId)>,
    {
        let remotes = tracked
            .into_iter()
            .map(|(handle, peer)| Self::build_remote(local_url.clone(), peer, handle.into()))
            .collect();
        tracing::trace!("computed remotes: {:?}", remotes);

        Self {
            remotes,
            path,
            local_url,
        }
    }

    fn build_remote(
        url: LocalUrl,
        peer: PeerId,
        handle: impl Into<ext::RefLike>,
    ) -> Remote<LocalUrl> {
        let handle = handle.into();
        let name = ext::RefLike::try_from(format!("{}@{}", handle, peer))
            .expect("handle and peer are reflike");
        Remote::new(url, name.clone()).with_fetchspecs(vec![Refspec {
            src: Reference::heads(Flat, peer),
            dst: GenericRef::heads(Flat, name),
            force: Force::True,
        }])
    }
}

/// Adds an include directive to the `repo`.
pub fn set_include_path(repo: &git2::Repository, include_path: PathBuf) -> Result<(), Error> {
    let mut config = repo.config().unwrap();
    config
        .set_str(GIT_CONFIG_PATH_KEY, &format!("{}", include_path.display()))
        .map_err(Error::from)
}
