// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use librad_test::tempdir::WithTmpDir;

use crate::{
    git::{
        identities::{self, local},
        storage::Storage,
    },
    identities::payload,
    keys::SecretKey,
    paths::Paths,
};

pub type TmpStorage = WithTmpDir<Storage>;

pub fn storage(signer: SecretKey) -> anyhow::Result<TmpStorage> {
    Ok(WithTmpDir::new(|path| {
        let paths = Paths::from_root(path)?;
        let storage = Storage::open_or_init(&paths, signer)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok::<_, io::Error>(storage)
    })?)
}

pub fn dylan(storage: &Storage, key: &SecretKey) -> anyhow::Result<local::LocalIdentity> {
    let dylan = identities::person::create(
        storage,
        payload::Person {
            name: "dylan".into(),
        },
        Some(key.public()).into_iter().collect(),
    )?;
    local::load(&storage, dylan.urn())?.ok_or_else(|| anyhow::anyhow!("where did dylan go?"))
}
