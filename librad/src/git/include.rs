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
    io,
    marker::PhantomData,
    path::{self, PathBuf},
};

use crate::{
    git::{
        local::url::LocalUrl,
        types::{remote::Remote, AsRefspec, FlatRef, Force},
    },
    meta::user::User,
    peer::PeerId,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Git(#[from] git2::Error),
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
    pub remotes: Vec<Remote<LocalUrl>>,
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

    /// Writes the contents of the [`git2::Config`] of the include file to disk.
    pub fn save(self) -> Result<(), Error>
    where
        Path: AsRef<path::Path>,
    {
        let tmp_dir = tempfile::tempdir()?;
        let tmp_path = tmp_dir.path().join(self.local_url.repo.to_string());

        let mut config = git2::Config::open(&tmp_path)?;

        for remote in &self.remotes {
            let (key, url) = url_entry(&remote);
            config.set_str(&key, &url.to_string())?;

            let (key, fetch) = fetch_entry(&remote);
            config.set_str(&key, &fetch)?;
        }

        std::fs::rename(tmp_path, self.file_path())?;

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
            .join(self.local_url.repo.to_string())
            .with_extension("inc")
    }

    /// Generate an include file by giving it a `RadUrn` for a project and the
    /// tracked `User`/`PeerId` pairs for that project.
    ///
    /// The tracked users are expected to be retrieved by talking to the
    /// [`crate::git::storage::Storage`].
    pub fn from_tracked_users<S>(
        path: Path,
        local_url: LocalUrl,
        tracked: impl Iterator<Item = (User<S>, PeerId)>,
    ) -> Self {
        let remotes = tracked
            .map(|(user, peer)| Remote::new(local_url.clone(), format!("{}@{}", user.name(), peer)))
            .collect();
        Self {
            remotes,
            path,
            local_url,
        }
    }
}

fn remote_prefix(remote: &Remote<LocalUrl>) -> String {
    format!("remote.{}", remote.name)
}

fn url_entry(remote: &Remote<LocalUrl>) -> (String, &LocalUrl) {
    let key = remote_prefix(&remote);
    (format!("{}.url", key), &remote.url)
}

fn fetch_entry(remote: &Remote<LocalUrl>) -> (String, String) {
    let key = remote_prefix(&remote);
    (
        format!("{}.fetch", key),
        match &remote.fetch_spec {
            Some(spec) => spec.as_refspec(),
            None => {
                let peer_id = &remote.url.local_peer_id;
                let heads: FlatRef<PeerId, _> = FlatRef::heads(PhantomData, Some(peer_id.clone()));
                let heads = heads.with_name("heads/*");
                let remotes: FlatRef<String, _> =
                    FlatRef::heads(PhantomData, Some(remote.name.clone()));

                remotes.refspec(heads, Force::True).as_refspec()
            },
        },
    )
}

#[cfg(test)]
mod test {
    use crate::{git::local::url::LocalUrl, hash::Hash, keys::SecretKey, peer::PeerId};

    use super::*;

    #[test]
    fn can_create_and_update() -> Result<(), Error> {
        let tmp_dir = tempfile::tempdir()?;

        let key = SecretKey::new();
        let peer_id = PeerId::from(key);
        let repo = Hash::hash(b"meow-meow");
        let url = LocalUrl {
            repo,
            local_peer_id: peer_id,
        };
        let remote_lyla = {
            let key = SecretKey::new();
            let peer_id = PeerId::from(key);
            format!("lyla@{}", peer_id)
        };

        let include_path = {
            let include = Include {
                path: tmp_dir.path().to_path_buf(),
                remotes: vec![Remote::new(url.clone(), remote_lyla.clone())],
                local_url: url.clone(),
            };
            let path = include.file_path();
            include.save()?;

            path
        };

        {
            let config = git2::Config::open(&include_path)?;
            assert_matches!(
                config
                    .get_entry(&format!("remote.{}.url", remote_lyla))?
                    .value(),
                Some(_)
            );
        }

        let remote_rover = {
            let key = SecretKey::new();
            let peer_id = PeerId::from(key);
            format!("rover@{}", peer_id)
        };

        {
            let include = Include {
                path: tmp_dir.path().to_path_buf(),
                remotes: vec![
                    Remote::new(url.clone(), remote_lyla.clone()),
                    Remote::new(url.clone(), remote_rover.clone()),
                ],
                local_url: url.clone(),
            };
            include.save()?;
        };

        {
            let config = git2::Config::open(&include_path)?;
            assert_matches!(
                config
                    .get_entry(&format!("remote.{}.url", remote_lyla))?
                    .value(),
                Some(_)
            );
            assert_matches!(
                config
                    .get_entry(&format!("remote.{}.url", remote_rover))?
                    .value(),
                Some(_)
            );
        };

        // The tracking graph changed entirely.
        let remote_lingling = {
            let key = SecretKey::new();
            let peer_id = PeerId::from(key);
            format!("lingling@{}", peer_id)
        };

        {
            let include = Include {
                path: tmp_dir.path().to_path_buf(),
                remotes: vec![Remote::new(url.clone(), remote_lingling.clone())],
                local_url: url,
            };
            include.save()?;
        };

        let config = git2::Config::open(&include_path)?;
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
