// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{
    identities::{
        git::{error, VerificationError},
        Identities,
    },
    SecretKey,
};

use crate::librad::git::{repo, Device};

lazy_static! {
    static ref DESKTOP: SecretKey = SecretKey::from_seed([
        143, 47, 243, 180, 88, 210, 28, 210, 95, 46, 192, 56, 51, 195, 64, 222, 206, 58, 197, 225,
        9, 65, 102, 201, 120, 103, 253, 204, 96, 186, 112, 5
    ]);
    static ref LAPTOP: SecretKey = SecretKey::from_seed([
        30, 242, 189, 126, 37, 140, 20, 42, 81, 142, 241, 147, 125, 104, 39, 52, 116, 251, 203,
        128, 121, 28, 90, 176, 119, 91, 59, 205, 180, 97, 134, 185
    ]);
    static ref PALMTOP: SecretKey = SecretKey::from_seed([
        175, 193, 135, 176, 191, 147, 253, 103, 100, 182, 201, 116, 62, 99, 240, 24, 224, 48, 170,
        34, 124, 181, 132, 3, 192, 82, 110, 111, 22, 22, 113, 200
    ]);
}

#[test]
fn create() -> anyhow::Result<()> {
    let repo = repo()?;
    {
        Device::new(&*DESKTOP, Identities::from(&*repo))?.assert_verifies()
    }
}

#[test]
fn update() -> anyhow::Result<()> {
    let repo = repo()?;
    {
        let desktop = Device::new(&*DESKTOP, Identities::from(&*repo))?.update(Some(
            vec![DESKTOP.public(), LAPTOP.public()]
                .into_iter()
                .collect(),
        ))?;
        desktop.assert_no_quorum()?;

        // Gotta confirm from laptop
        let laptop = Device::create_from(&*LAPTOP, &desktop)?;
        laptop.assert_verifies()?;

        // Now that should be a fast-forward on the desktop
        desktop.update_from(&laptop)?.assert_verifies()
    }
}

#[test]
fn revoke_a_deux() -> anyhow::Result<()> {
    let repo = repo()?;
    {
        let desktop = Device::new(&*DESKTOP, Identities::from(&*repo))?.update(Some(
            vec![DESKTOP.public(), LAPTOP.public()]
                .into_iter()
                .collect(),
        ))?;

        // Kick out desktop
        let laptop = Device::create_from(&*LAPTOP, &desktop)?;
        let laptop_revokes_desktop = laptop
            .clone()
            .update(Some(Some(LAPTOP.public()).into_iter().collect()))?;
        // Cannot do that unilaterally -- laptop is now invalid
        assert_matches!(
            laptop_revokes_desktop.verify(),
            Err(error::VerifyPerson::Verification(
                VerificationError::ParentQuorum
            ))
        );

        // Ack confirmation, and then revocation
        let desktop = desktop
            .update_from(&laptop)?
            .update_from(&laptop_revokes_desktop)?;
        desktop.assert_verifies()?;

        // Now laptop turns valid again
        let laptop = laptop.update_from(&desktop)?;
        laptop.assert_verifies()
    }
}

#[test]
fn revoke_a_trois() -> anyhow::Result<()> {
    let repo = repo()?;
    {
        let desktop = Device::new(&*DESKTOP, Identities::from(&*repo))?.update(Some(
            vec![DESKTOP.public(), LAPTOP.public(), PALMTOP.public()]
                .into_iter()
                .collect(),
        ))?;

        // We don't have to ask palmtop for it to be added
        let laptop = Device::create_from(&*LAPTOP, &desktop)?;
        laptop.assert_verifies()?;

        let desktop = desktop.update_from(&laptop)?;
        desktop.assert_verifies()?;

        // And we don't have to ask it to be removed either
        let desktop = desktop.update(Some(
            vec![DESKTOP.public(), LAPTOP.public()]
                .into_iter()
                .collect(),
        ))?;

        let laptop = laptop.update_from(&desktop)?;
        laptop.assert_verifies()?;
        let desktop = desktop.update_from(&laptop)?;
        desktop.assert_verifies()
    }
}
