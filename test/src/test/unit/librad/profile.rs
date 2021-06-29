// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fs;
use tempfile::TempDir;

use librad::profile::{id, Profile, ProfileId, RadHome};

pub struct TempHome {
    tmp: TempDir,
    home: RadHome,
}

fn temp() -> TempHome {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    TempHome {
        tmp,
        home: RadHome::Root(root),
    }
}

#[test]
fn load_profile_id_test() {
    let tmp_home = temp();

    let id1 = ProfileId::load(&tmp_home.home).unwrap();
    let id2 = ProfileId::load(&tmp_home.home).unwrap();
    assert_eq!(id2, id1);
    fs::remove_dir_all(tmp_home.tmp.path()).unwrap();

    let id3 = ProfileId::load(&tmp_home.home).unwrap();
    assert_ne!(id3, id1);
}

#[test]
fn profile_paths() {
    let tmp_home = tempfile::tempdir().unwrap();

    let id = "foo";

    let profile_id_path = tmp_home.path().join("active_profile");
    std::fs::write(profile_id_path, id).unwrap();

    let profile = Profile::from_root(tmp_home.path(), None).unwrap();
    assert_eq!(profile.id(), id);
    assert!(profile
        .paths()
        .git_dir()
        .starts_with(tmp_home.path().join(id)));
}

#[test]
fn invalid_profile_id_from_file() {
    let tmp_home = temp();

    let profile_id_path = tmp_home.tmp.path().join("active_profile");
    let invalid_id = "foo/bar";
    std::fs::write(profile_id_path, invalid_id).unwrap();

    let result = ProfileId::load(&tmp_home.home);
    assert!(matches!(result, Err(id::Error::FromFile { .. })));
}

#[test]
fn load_profile_strip() {
    let tmp_home = temp();

    let profile_id_path = tmp_home.tmp.path().join("active_profile");
    let content = "foo\nbar";
    std::fs::write(profile_id_path, content).unwrap();
    let id = ProfileId::load(&tmp_home.home).unwrap();
    assert_eq!(id, "foo".parse().unwrap());
}

#[test]
fn empty_profile_file() {
    let tmp_home = temp();

    let profile_id_path = tmp_home.tmp.path().join("active_profile");
    let content = "";
    std::fs::write(profile_id_path, content).unwrap();
    let err = ProfileId::load(&tmp_home.home).unwrap_err();
    assert!(matches!(err, id::Error::FromFile { .. }))
}
