// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use librad::paths::Paths;

use crate::tempdir::WithTmpDir;

pub type TmpPaths = WithTmpDir<Paths>;

pub fn paths() -> TmpPaths {
    WithTmpDir::new(|path| -> Result<_, io::Error> {
        let paths = Paths::from_root(path)?;
        Ok::<_, io::Error>(paths)
    })
    .unwrap()
}
