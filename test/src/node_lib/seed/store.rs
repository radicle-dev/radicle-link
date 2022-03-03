// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{fmt, fs, io};

use node_lib::seed::{store, Seed};

use crate::tempdir::WithTmpDir;

pub type TmpKVStore<T> = WithTmpDir<store::FileStore<T>>;

pub fn kv_store<T>(init: Vec<Seed<T>>) -> TmpKVStore<T>
where
    T: fmt::Display,
{
    WithTmpDir::new(|path| -> Result<_, io::Error> {
        let path = path.join("seeds");
        fs::write(
            path.clone(),
            init.into_iter()
                .map(|seed| seed.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
        Ok(store::FileStore::new(path))
    })
    .unwrap()
}
