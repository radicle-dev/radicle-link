// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use librad::{git::storage::Storage, paths::Paths, SecretKey};

use crate::tempdir::WithTmpDir;

pub mod config;

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
