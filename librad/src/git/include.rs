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
use crate::peer::PeerId;

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
    #[tracing::instrument(level = "debug", skip(self), err)]
    pub fn save(self) -> Result<(), Error>
    where
        Path: AsRef<path::Path>,
    {
        let mut tmp = NamedTempFile::new_in(&self.path)?;
        {
            // **NB**: We can't use `git2::Config::set_multivar`, because
            // `libgit2` does not realise that we have only one file (it thinks
            // the file is included). This would limit is to a single fetchspec /
            // pushspec respectively.
            for remote in &self.remotes {
                if remote.fetch_specs.is_empty() {
                    return Err(Error::MissingRefspec);
                }

                tracing::debug!("writing remote {}", remote.name);
                writeln!(tmp, "[remote \"{}\"]", remote.name)?;
                tracing::debug!("remote.{}.url = {}", remote.name, remote.url);
                writeln!(tmp, "\turl = {}", remote.url)?;

                for spec in remote.fetch_specs.iter() {
                    tracing::debug!("remote.{}.fetch = {}", remote.name, spec);
                    writeln!(tmp, "\tfetch = {}", spec)?;
                }

                for spec in remote.push_specs.iter() {
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
    /// tracked User handle/`PeerId` pairs for that project.
    ///
    /// The tracked users are expected to be retrieved by talking to the
    /// [`crate::git::storage::Storage`].
    #[tracing::instrument(level = "debug", skip(tracked))]
    pub fn from_tracked_users<R, I>(path: Path, local_url: LocalUrl, tracked: I) -> Self
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
        Remote::new(url, name).with_fetch_specs(vec![Refspec {
            src: Reference::heads(None, peer),
            dst: GenericRef::heads(Flat, handle),
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

#[cfg(test)]
mod test {
    use crate::{
        git::{local::url::LocalUrl, Urn},
        keys::SecretKey,
        peer::PeerId,
    };

    use super::*;

    const LOCAL_SEED: [u8; 32] = [
        0, 10, 109, 178, 52, 203, 96, 195, 109, 177, 87, 178, 159, 70, 238, 41, 20, 168, 163, 180,
        197, 235, 118, 84, 216, 231, 56, 80, 83, 31, 227, 102,
    ];
    const LYLA_SEED: [u8; 32] = [
        216, 242, 247, 226, 55, 82, 13, 180, 197, 249, 205, 34, 152, 15, 64, 254, 37, 87, 34, 209,
        247, 76, 44, 13, 101, 182, 52, 156, 229, 148, 45, 72,
    ];
    const ROVER_SEED: [u8; 32] = [
        200, 50, 199, 97, 117, 178, 51, 186, 246, 43, 94, 103, 111, 252, 210, 133, 119, 110, 115,
        123, 236, 191, 154, 79, 82, 74, 126, 133, 221, 216, 193, 65,
    ];
    const LINGLING_SEED: [u8; 32] = [
        224, 125, 219, 106, 75, 189, 95, 155, 89, 134, 54, 202, 255, 41, 239, 234, 220, 90, 200,
        19, 199, 63, 69, 225, 97, 15, 124, 168, 168, 238, 124, 83,
    ];

    lazy_static! {
        static ref LYLA_HANDLE: ext::RefLike = reflike!("lyla");
        static ref ROVER_HANDLE: ext::RefLike = reflike!("rover");
        static ref LINGLING_HANDLE: ext::RefLike = reflike!("lingling");
        static ref LOCAL_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LOCAL_SEED));
        static ref LYLA_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LYLA_SEED));
        static ref ROVER_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(ROVER_SEED));
        static ref LINGLING_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LINGLING_SEED));
    }

    #[test]
    fn can_create_and_update() -> Result<(), Error> {
        let tmp_dir = tempfile::tempdir()?;
        let url = LocalUrl::from(Urn::new(git2::Oid::zero().into()));

        // Start with an empty config to catch corner-cases where git2::Config does not
        // create a file yet.
        let config = {
            let include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
            let path = include.file_path();
            let config = git2::Config::open(&path)?;
            include.save()?;

            config
        };

        let remote_lyla = format!("{}@{}", *LYLA_HANDLE, *LYLA_PEER_ID);
        {
            let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
            include.add_remote(url.clone(), *LYLA_PEER_ID, (*LYLA_HANDLE).clone());
            include.save()?;
        };

        assert_matches!(
            config
                .get_entry(&format!("remote.{}.url", remote_lyla))?
                .value(),
            Some(_)
        );
        assert_matches!(
            config
                .get_entry(&format!("remote.{}.fetch", remote_lyla))?
                .value(),
            Some(_)
        );

        let remote_rover = format!("{}@{}", *ROVER_HANDLE, *ROVER_PEER_ID);
        {
            let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
            include.add_remote(url.clone(), *LYLA_PEER_ID, (*LYLA_HANDLE).clone());
            include.add_remote(url.clone(), *ROVER_PEER_ID, (*ROVER_HANDLE).clone());
            include.save()?;
        };

        assert_matches!(
            config
                .get_entry(&format!("remote.{}.url", remote_lyla))?
                .value(),
            Some(_)
        );
        assert_matches!(
            config
                .get_entry(&format!("remote.{}.fetch", remote_lyla))?
                .value(),
            Some(_)
        );

        assert_matches!(
            config
                .get_entry(&format!("remote.{}.url", remote_rover))?
                .value(),
            Some(_)
        );
        assert_matches!(
            config
                .get_entry(&format!("remote.{}.fetch", remote_rover))?
                .value(),
            Some(_)
        );

        // The tracking graph changed entirely.
        let remote_lingling = format!("{}@{}", *LINGLING_HANDLE, *LINGLING_PEER_ID);

        {
            let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
            include.add_remote(url, *LINGLING_PEER_ID, (*LINGLING_HANDLE).clone());
            include.save()?;
        };

        assert_matches!(
            config
                .get_entry(&format!("remote.{}.url", remote_lingling))?
                .value(),
            Some(_)
        );
        assert!(config
            .get_entry(&format!("remote.{}.url", remote_lyla))
            .is_err());
        assert!(config
            .get_entry(&format!("remote.{}.url", remote_rover))
            .is_err());

        Ok(())
    }
}
