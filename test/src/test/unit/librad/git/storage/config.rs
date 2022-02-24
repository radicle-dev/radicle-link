// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    git::storage::config::{Config, Error},
    PeerId,
    SecretKey,
};
use test_helpers::tempdir::WithTmpDir;

lazy_static! {
    static ref ALICE_KEY: SecretKey = SecretKey::from_seed([
        81, 151, 13, 57, 246, 76, 127, 57, 30, 125, 102, 210, 87, 132, 7, 92, 12, 122, 7, 30, 202,
        71, 235, 169, 66, 199, 172, 11, 97, 50, 173, 150
    ]);
    static ref BOB_KEY: SecretKey = SecretKey::from_seed([
        117, 247, 70, 158, 119, 191, 163, 76, 169, 138, 229, 198, 147, 90, 8, 220, 233, 86, 170,
        139, 85, 5, 233, 64, 1, 58, 193, 241, 12, 87, 14, 60
    ]);
    static ref ALICE_PEER_ID: PeerId = PeerId::from(&*ALICE_KEY);
}

struct TmpState<'a> {
    repo: git2::Repository,
    config: Config<'a, SecretKey>,
}

fn tmp_state(key: &SecretKey) -> WithTmpDir<TmpState> {
    WithTmpDir::new::<_, anyhow::Error>(|path| {
        let mut repo = git2::Repository::init_bare(path)?;
        let config = Config::init(&mut repo, key)?;
        Ok(TmpState { repo, config })
    })
    .unwrap()
}

#[test]
fn init_proper() {
    let s = tmp_state(&*ALICE_KEY);
    assert_eq!(s.config.peer_id().unwrap(), *ALICE_PEER_ID);
    assert!(s.config.user().unwrap().is_none())
}

#[test]
fn reinit_with_different_key() {
    let mut alice = tmp_state(&*ALICE_KEY);
    let bob = Config::init(&mut alice.repo, &*BOB_KEY);

    assert_matches!(
        bob.map(|_| ()), // map to avoid `Debug` impl
        Err(Error::AlreadyInitialised(pid)) if pid == *ALICE_PEER_ID
    )
}
