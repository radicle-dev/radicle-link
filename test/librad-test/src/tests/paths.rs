// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tempfile::tempdir;

use librad::paths::Paths;

/// Note: not testing the system paths flavour, as that would only be
/// meaningful on a pristine system with properly set $HOME.
#[test]
fn test_initialises_paths() {
    let tmp = tempdir().unwrap();
    let paths = Paths::from_root(tmp.path()).unwrap();
    assert!(paths.all_dirs().all(|path| path.exists()))
}

/// Test we indeed create everything under the root dir -
/// airquotes-chroot-airquotes.
#[test]
fn test_chroot() {
    let tmp = tempdir().unwrap();
    let paths = Paths::from_root(tmp.path()).unwrap();
    assert!(paths
        .all_dirs()
        .all(|path| { path.ancestors().any(|parent| parent == tmp.path()) }))
}
