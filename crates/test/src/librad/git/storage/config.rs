// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::{Deref, DerefMut};

use librad::{git::storage::config::Config, SecretKey};

use crate::tempdir::WithTmpDir;

pub struct TmpConfig<'a> {
    pub repo: git2::Repository,
    config: Config<'a, SecretKey>,
}

impl<'a> Deref for TmpConfig<'a> {
    type Target = Config<'a, SecretKey>;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl<'a> DerefMut for TmpConfig<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
    }
}

type TmpState<'a> = WithTmpDir<TmpConfig<'a>>;

pub fn setup(key: &SecretKey) -> TmpState {
    WithTmpDir::new::<_, anyhow::Error>(|path| {
        let mut repo = git2::Repository::init_bare(path)?;
        let config = Config::init(&mut repo, key)?;
        Ok(TmpConfig { repo, config })
    })
    .unwrap()
}
