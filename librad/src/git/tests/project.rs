// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either::Left;

use super::*;
use crate::{
    git::identities,
    identities::{delegation, payload, SomeIdentity},
    keys::SecretKey,
};

lazy_static! {
    static ref DYLAN: SecretKey = SecretKey::from_seed([
        188, 166, 161, 203, 144, 68, 64, 48, 105, 98, 55, 215, 50, 154, 43, 236, 168, 133, 230, 36,
        134, 79, 175, 109, 234, 123, 23, 114, 61, 82, 96, 52
    ]);
}

#[test]
fn create_anonymous() -> anyhow::Result<()> {
    let storage = common::storage(DYLAN.clone())?;
    let whoami = common::dylan(&storage, &DYLAN)?;
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
        Some(proj.clone()),
        identities::project::get(&storage, &proj.urn())?
    );
    assert_eq!(
        Some(proj.clone()),
        identities::any::get(&storage, &proj.urn())?.and_then(SomeIdentity::project)
    );
    Ok(())
}
