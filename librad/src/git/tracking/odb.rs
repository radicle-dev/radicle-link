// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use link_canonical::Canonical as _;

use crate::{
    git::{
        storage::{ReadOnly, ReadOnlyStorage as _, Storage},
        tracking::{
            git::odb::{Read, Write},
            Config,
        },
    },
    git_ext as ext,
};

pub mod error {
    use thiserror::Error;

    use crate::{
        git::{storage::read, tracking::config},
        git_ext as ext,
    };

    #[derive(Debug, Error)]
    pub enum Find {
        #[error("the configuration at `{0}` is not a blob")]
        NotBlob(ext::Oid),
        #[error(transparent)]
        Read(#[from] read::Error),
        #[error("failed to parse config at `{oid}`")]
        Config {
            oid: ext::Oid,
            #[source]
            source: config::error::Parse,
        },
    }

    #[derive(Debug, Error)]
    pub enum Modify {
        #[error(transparent)]
        Find(#[from] Find),
        #[error("no configuration was found at `{0}` to be modified")]
        Missing(ext::Oid),
        #[error(transparent)]
        Write(#[from] Write),
    }

    #[derive(Debug, Error)]
    pub enum Write {
        #[error(transparent)]
        Git(#[from] git2::Error),
    }
}

impl Read for ReadOnly {
    type FindError = error::Find;

    type Oid = ext::Oid;

    fn find_config(&self, oid: &Self::Oid) -> Result<Option<Config>, Self::FindError> {
        match self.find_object(oid)? {
            None => Ok(None),
            Some(obj) => {
                let blob = obj.into_blob().map_err(|_| error::Find::NotBlob(*oid))?;
                Config::try_from(blob.content())
                    .map(Some)
                    .map_err(|err| error::Find::Config {
                        oid: *oid,
                        source: err,
                    })
            },
        }
    }
}

impl Read for Storage {
    type FindError = error::Find;

    type Oid = ext::Oid;

    fn find_config(&self, oid: &Self::Oid) -> Result<Option<Config>, Self::FindError> {
        self.read_only().find_config(oid)
    }
}

impl Write for Storage {
    type ModifyError = error::Modify;
    type WriteError = error::Write;

    type Oid = ext::Oid;

    fn write_config(&self, config: &Config) -> Result<Self::Oid, Self::WriteError> {
        // unwrap is safe since Error is Infallible
        Ok(self
            .as_raw()
            .blob(&config.canonical_form().unwrap())
            .map(ext::Oid::from)?)
    }

    fn modify_config<F>(&self, oid: &Self::Oid, f: F) -> Result<Self::Oid, Self::ModifyError>
    where
        F: FnOnce(Config) -> Config,
    {
        let config = self
            .find_config(oid)?
            .map(Ok)
            .unwrap_or_else(|| Err(error::Modify::Missing(*oid)))?;
        Ok(self.write_config(&f(config))?)
    }
}
