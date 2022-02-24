// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either::Left;

use it_helpers::tmp;
use librad::{
    git::identities,
    identities::{delegation, payload, SomeIdentity},
    SecretKey,
};

use crate::librad::git;

lazy_static! {
    static ref DYLAN: SecretKey = SecretKey::from_seed([
        188, 166, 161, 203, 144, 68, 64, 48, 105, 98, 55, 215, 50, 154, 43, 236, 168, 133, 230, 36,
        134, 79, 175, 109, 234, 123, 23, 114, 61, 82, 96, 52
    ]);
}

#[test]
fn create_anonymous() -> anyhow::Result<()> {
    let storage = tmp::storage(DYLAN.clone());
    let whoami = git::dylan(&storage, &DYLAN)?;
    let proj = identities::project::create(
        &storage,
        whoami,
        payload::Project {
            name: "reMarkable 3".into(),
            description: Some("The next big thing in e-ink technology".into()),
            default_branch: Some("eink".into()),
        },
        delegation::Indirect::try_from_iter(Some(Left(DYLAN.public()))).unwrap(),
    )?;
    assert_eq!(
        Some(proj.urn()),
        identities::project::get(&storage.read_only(), &proj.urn())?.map(|proj| proj.urn())
    );
    assert_eq!(
        Some(proj.urn()),
        identities::any::get(&storage.read_only(), &proj.urn())?
            .and_then(SomeIdentity::project)
            .map(|proj| proj.urn())
    );
    Ok(())
}
