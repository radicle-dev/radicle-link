// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::{
        include::{Error, Include},
        local::url::LocalUrl,
        Urn,
    },
    git_ext as ext,
    reflike,
    PeerId,
    SecretKey,
};

const LOCAL_SEED: [u8; 32] = [
    0, 10, 109, 178, 52, 203, 96, 195, 109, 177, 87, 178, 159, 70, 238, 41, 20, 168, 163, 180, 197,
    235, 118, 84, 216, 231, 56, 80, 83, 31, 227, 102,
];
const LYLA_SEED: [u8; 32] = [
    216, 242, 247, 226, 55, 82, 13, 180, 197, 249, 205, 34, 152, 15, 64, 254, 37, 87, 34, 209, 247,
    76, 44, 13, 101, 182, 52, 156, 229, 148, 45, 72,
];
const ROVER_SEED: [u8; 32] = [
    200, 50, 199, 97, 117, 178, 51, 186, 246, 43, 94, 103, 111, 252, 210, 133, 119, 110, 115, 123,
    236, 191, 154, 79, 82, 74, 126, 133, 221, 216, 193, 65,
];
const LINGLING_SEED: [u8; 32] = [
    224, 125, 219, 106, 75, 189, 95, 155, 89, 134, 54, 202, 255, 41, 239, 234, 220, 90, 200, 19,
    199, 63, 69, 225, 97, 15, 124, 168, 168, 238, 124, 83,
];

lazy_static! {
    static ref LYLA_HANDLE: ext::RefLike = reflike!("lyla");
    static ref ROVER_HANDLE: ext::RefLike = reflike!("rover");
    static ref LOCAL_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LOCAL_SEED));
    static ref LYLA_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LYLA_SEED));
    static ref ROVER_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(ROVER_SEED));
    static ref LINGLING_PEER_ID: PeerId = PeerId::from(SecretKey::from_seed(LINGLING_SEED));
}

#[test]
fn can_create_and_update() -> Result<(), Error> {
    let tmp_dir = tempfile::tempdir()?;
    let url = LocalUrl::from(Urn::new(git2::Oid::zero().into()));

    // Start with an empty config to catch corner-cases where git2::Config does not
    // create a file yet.
    let config = {
        let include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
        let path = include.file_path();
        let config = git2::Config::open(&path)?;
        include.save()?;

        config
    };

    let remote_lyla = format!("{}@{}", *LYLA_HANDLE, *LYLA_PEER_ID);
    {
        let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
        include.add_remote(url.clone(), *LYLA_PEER_ID, (*LYLA_HANDLE).clone());
        include.save()?;
    };

    assert_matches!(
        config
            .get_entry(&format!("remote.{}.url", remote_lyla))?
            .value(),
        Some(_)
    );
    assert_matches!(
        config
            .get_entry(&format!("remote.{}.fetch", remote_lyla))?
            .value(),
        Some(_)
    );

    let remote_rover = format!("{}@{}", *ROVER_HANDLE, *ROVER_PEER_ID);
    {
        let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
        include.add_remote(url.clone(), *LYLA_PEER_ID, (*LYLA_HANDLE).clone());
        include.add_remote(url.clone(), *ROVER_PEER_ID, (*ROVER_HANDLE).clone());
        include.save()?;
    };

    assert_matches!(
        config
            .get_entry(&format!("remote.{}.url", remote_lyla))?
            .value(),
        Some(_)
    );
    assert_matches!(
        config
            .get_entry(&format!("remote.{}.fetch", remote_lyla))?
            .value(),
        Some(_)
    );

    assert_matches!(
        config
            .get_entry(&format!("remote.{}.url", remote_rover))?
            .value(),
        Some(_)
    );
    assert_matches!(
        config
            .get_entry(&format!("remote.{}.fetch", remote_rover))?
            .value(),
        Some(_)
    );

    // The tracking graph changed entirely.
    let remote_lingling = format!("{}", *LINGLING_PEER_ID);

    {
        let mut include = Include::new(tmp_dir.path().to_path_buf(), url.clone());
        include.add_remote(url, *LINGLING_PEER_ID, None);
        include.save()?;
    };

    assert_matches!(
        config
            .get_entry(&format!("remote.{}.url", remote_lingling))?
            .value(),
        Some(_)
    );
    assert!(config
        .get_entry(&format!("remote.{}.url", remote_lyla))
        .is_err());
    assert!(config
        .get_entry(&format!("remote.{}.url", remote_rover))
        .is_err());

    Ok(())
}
