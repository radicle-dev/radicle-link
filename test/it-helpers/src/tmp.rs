// Copyright © 2021 The Radicle Link Contributors
// SPDX-License-Identifier: GPLv3-or-later

use std::io;

use librad::{git::storage::Storage, paths::Paths, SecretKey};
use test_helpers::tempdir::WithTmpDir;

pub type TmpPaths = WithTmpDir<Paths>;

pub fn paths() -> TmpPaths {
    WithTmpDir::new(|path| -> Result<_, io::Error> {
        let paths = Paths::from_root(path)?;
        Ok::<_, io::Error>(paths)
    })
    .unwrap()
}

type TmpRepo = WithTmpDir<git2::Repository>;

pub fn repo() -> anyhow::Result<TmpRepo> {
    Ok(WithTmpDir::new(|path| {
        let setup = || {
            let repo = git2::Repository::init(path)?;

            // We need to set user info to _something_, but that doesn't have to
            // be valid, as we're using a shared repo with many keys
            let mut config = repo.config()?;
            config.set_str("user.name", "shared")?;
            config.set_str("user.email", "not.relevant@for.testing")?;
            Ok(repo)
        };
        setup().map_err(|e: git2::Error| io::Error::new(io::ErrorKind::Other, e))
    })?)
}

pub type TmpStorage = WithTmpDir<Storage>;

pub fn storage(signer: SecretKey) -> TmpStorage {
    WithTmpDir::new(|path| -> Result<_, io::Error> {
        let paths = Paths::from_root(path)?;
        let storage =
            Storage::open(&paths, signer).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok::<_, io::Error>(storage)
    })
    .unwrap()
}
