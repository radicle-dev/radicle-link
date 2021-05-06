// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fs;

use librad::profile::{load_profile_id, Error, Profile};

#[test]
fn load_profile_id_test() {
    let tempdir = tempfile::tempdir().unwrap();

    let id1 = load_profile_id(tempdir.path()).unwrap();
    let id2 = load_profile_id(tempdir.path()).unwrap();
    assert_eq!(id2, id1);
    fs::remove_dir_all(tempdir.path()).unwrap();

    let id3 = load_profile_id(tempdir.path()).unwrap();
    assert_ne!(id3, id1);
}

#[test]
fn profile_paths() {
    let tempdir = tempfile::tempdir().unwrap();

    let id = "foo";

    let profile_id_path = tempdir.path().join("active_profile");
    std::fs::write(profile_id_path, id).unwrap();

    let profile = Profile::from_root(tempdir.path(), None).unwrap();
    assert_eq!(profile.id(), id);
    assert!(profile
        .paths()
        .git_dir()
        .starts_with(tempdir.path().join(id)));
}

#[test]
fn invalid_profile_id_from_file() {
    let tempdir = tempfile::tempdir().unwrap();

    let profile_id_path = tempdir.path().join("active_profile");
    let invalid_id = "foo/bar";
    std::fs::write(profile_id_path, invalid_id).unwrap();

    let result = load_profile_id(tempdir.path());
    assert!(matches!(
        result,
        Err(Error::InvalidProfileIdFromFile { .. })
    ));
}

#[test]
fn load_profile_strip() {
    let tempdir = tempfile::tempdir().unwrap();

    let profile_id_path = tempdir.path().join("active_profile");
    let content = "foo\nbar";
    std::fs::write(profile_id_path, content).unwrap();
    let id = load_profile_id(tempdir.path()).unwrap();
    assert_eq!(id, "foo");
}

#[test]
fn empty_profile_file() {
    let tempdir = tempfile::tempdir().unwrap();

    let profile_id_path = tempdir.path().join("active_profile");
    let content = "";
    std::fs::write(profile_id_path, content).unwrap();
    let err = load_profile_id(tempdir.path()).unwrap_err();
    assert!(matches!(err, Error::InvalidProfileIdFromFile { .. }))
}
