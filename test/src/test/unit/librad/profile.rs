// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeSet, fs};
use tempfile::TempDir;

use librad::profile::{id, Error, LnkHome, Profile, ProfileId};

pub struct TempHome {
    tmp: TempDir,
    home: LnkHome,
}

fn temp() -> TempHome {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    TempHome {
        tmp,
        home: LnkHome::Root(root),
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

    let id: ProfileId = "foo".parse().unwrap();

    let profile_id_path = tmp_home.path().join("active_profile");
    std::fs::write(profile_id_path, id.as_str()).unwrap();

    let profile = Profile::from_root(tmp_home.path(), None).unwrap();
    assert_eq!(profile.id(), &id);
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

#[test]
fn new_doesnt_interfere_with_load() {
    let tmp_home = temp();

    let p1 = ProfileId::load(&tmp_home.home).unwrap();
    let p2 = Profile::new(&tmp_home.home).unwrap();

    assert_ne!(&p1, p2.id());

    let active = ProfileId::active(&tmp_home.home).unwrap().unwrap();
    assert_eq!(p1, active);
}

#[test]
fn get_set_load() {
    let tmp_home = temp();

    let p = Profile::new(&tmp_home.home).unwrap();
    Profile::set(&tmp_home.home, p.id().clone()).unwrap();

    let l = ProfileId::load(&tmp_home.home).unwrap();
    assert_eq!(p.id(), &l);

    let g = Profile::get(&tmp_home.home, p.id().clone()).unwrap();
    assert_eq!(Some(p.id()), g.as_ref().map(|g| g.id()));
}

#[test]
fn list_profiles() {
    let n = 10;
    let tmp_home = temp();
    let mut expected = BTreeSet::new();
    for _ in 1..n {
        expected.insert(Profile::new(&tmp_home.home).unwrap().id().clone());
    }

    let profiles = Profile::list(&tmp_home.home)
        .unwrap()
        .into_iter()
        .map(|p| p.id().clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(expected, profiles);
}

#[test]
fn get_profile() {
    let tmp_home = temp();

    let p = Profile::new(&tmp_home.home).unwrap();
    let p1 = Profile::get(&tmp_home.home, p.id().clone()).unwrap();
    assert_eq!(Some(p.id()), p1.as_ref().map(|p| p.id()));

    let p2 = Profile::get(&tmp_home.home, "i-dont-exist".parse().unwrap()).unwrap();
    assert_eq!(p2.as_ref().map(|p| p.id()), None);
}

#[test]
fn set_profile() {
    let tmp_home = temp();

    let p = Profile::new(&tmp_home.home).unwrap();
    let p1 = Profile::set(&tmp_home.home, p.id().clone()).unwrap();
    assert_eq!(p.id(), p1.id());

    let err = Profile::set(&tmp_home.home, "i-dont-exist".parse().unwrap()).unwrap_err();
    assert!(matches!(err, Error::DoesNotExist { .. }));
}
