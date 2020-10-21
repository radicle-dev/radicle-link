// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::collections::BTreeMap;

use super::*;
use crate::keys::SecretKey;

lazy_static! {
    static ref CHEYENNE_DESKTOP: SecretKey = SecretKey::from_seed([
        52, 5, 211, 193, 252, 179, 147, 197, 221, 38, 181, 200, 74, 100, 104, 208, 241, 143, 156,
        130, 118, 94, 82, 173, 18, 164, 96, 77, 81, 82, 182, 149
    ]);
    static ref CHEYENNE_LAPTOP: SecretKey = SecretKey::from_seed([
        197, 91, 169, 54, 48, 99, 79, 3, 69, 255, 168, 206, 253, 179, 132, 174, 11, 44, 130, 185,
        181, 169, 203, 221, 41, 75, 222, 216, 113, 131, 19, 240
    ]);
    static ref CHEYENNE_PALMTOP: SecretKey = SecretKey::from_seed([
        210, 223, 197, 162, 13, 216, 81, 37, 28, 172, 247, 158, 217, 134, 126, 46, 155, 121, 206,
        198, 75, 64, 219, 199, 205, 75, 53, 63, 63, 120, 147, 27
    ]);
    static ref DYLAN: SecretKey = SecretKey::from_seed([
        188, 166, 161, 203, 144, 68, 64, 48, 105, 98, 55, 215, 50, 154, 43, 236, 168, 133, 230, 36,
        134, 79, 175, 109, 234, 123, 23, 114, 61, 82, 96, 52
    ]);
}

#[test]
fn create() -> anyhow::Result<()> {
    let repo = common::repo()?;
    {
        let dylan = common::Device::new(&*DYLAN, Git::new(&repo))?;
        common::Project::new(dylan.clone())?.assert_verifies(|urn| {
            if urn == dylan.current().urn() {
                Ok(*dylan.current().content_id)
            } else {
                unreachable!()
            }
        })
    }
}

#[test]
fn update() -> anyhow::Result<()> {
    let repo = common::repo()?;
    {
        let cheyenne = common::Device::new(&*CHEYENNE_DESKTOP, Git::new(&repo))?;
        let dylan = common::Device::new(&*DYLAN, Git::new(&repo))?;

        let heads = current_heads_from(vec![&cheyenne, &dylan]);

        // Cheyenne's view
        let project = {
            let update = IndirectDelegation::try_from_iter(vec![
                Right(cheyenne.current().clone()),
                Right(dylan.current().clone()),
            ])?;
            common::Project::new(cheyenne)?.update(update)
        }?;
        project.assert_no_quorum()?;

        // Dylan's view
        let project = common::Project::create_from(dylan, &project)?;
        project.assert_verifies(lookup(&heads))
    }
}

/// Revoke by just removing a delegation at the top-level
#[test]
fn revoke() -> anyhow::Result<()> {
    let repo = common::repo()?;
    {
        let cheyenne = common::Device::new_with(
            &*CHEYENNE_DESKTOP,
            Git::new(&repo),
            payload::User {
                name: "cheyenne".into(),
            },
        )?;
        let dylan = common::Device::new_with(
            &*DYLAN,
            Git::new(&repo),
            payload::User {
                name: "dylan".into(),
            },
        )?;

        let cheyennes = {
            let update = IndirectDelegation::try_from_iter(vec![
                Right(cheyenne.current().clone()),
                Right(dylan.current().clone()),
            ])?;
            common::Project::new(cheyenne.clone())?.update(update)
        }?;
        cheyennes.assert_no_quorum()?;

        let heads = current_heads_from(vec![&cheyenne, &dylan]);

        let dylans = common::Project::create_from(dylan, &cheyennes)?;
        dylans.assert_verifies(lookup(&heads))?;

        let cheyennes =
            cheyennes
                .update_from(&dylans)?
                .update(IndirectDelegation::try_from_iter(Some(Right(
                    cheyenne.current().clone(),
                )))?)?;
        assert_matches!(
            cheyennes.verify(lookup(&heads)),
            Err(error::VerifyProject::Verification(
                VerificationError::ParentQuorum
            ))
        );

        let dylans = dylans.update_from(&cheyennes)?;
        dylans.assert_verifies(lookup(&heads))?;

        cheyennes
            .update_from(&dylans)?
            .assert_verifies(lookup(&heads))
    }
}

/// Revoke a key on a user, while keeping the project unchanged
#[test]
fn revoke_indirect() -> anyhow::Result<()> {
    let repo = common::repo()?;
    {
        let cheyenne_desktop = common::Device::new_with(
            &*CHEYENNE_DESKTOP,
            Git::new(&repo),
            payload::User {
                name: "cheyenne".into(),
            },
        )?
        .update(Some(
            vec![CHEYENNE_DESKTOP.public(), CHEYENNE_LAPTOP.public()]
                .into_iter()
                .collect(),
        ))?;

        let cheyenne_laptop = common::Device::create_from(&*CHEYENNE_LAPTOP, &cheyenne_desktop)?;
        let cheyenne_desktop = cheyenne_desktop.update_from(&cheyenne_laptop)?;
        cheyenne_desktop.assert_verifies()?;

        let dylan = common::Device::new_with(
            &*DYLAN,
            Git::new(&repo),
            payload::User {
                name: "dylan".into(),
            },
        )?;

        let cheyenne_project = {
            let update = IndirectDelegation::try_from_iter(vec![
                Right(cheyenne_desktop.current().clone()),
                Right(dylan.current().clone()),
            ])?;
            common::Project::new(cheyenne_desktop.clone())?.update(update)
        }?;
        let dylan_project = common::Project::create_from(dylan.clone(), &cheyenne_project)?;

        let heads = current_heads_from(vec![&cheyenne_desktop, &dylan]);
        dylan_project.assert_verifies(lookup(&heads))?;

        // Swap lap with palm
        let cheyenne_desktop = cheyenne_desktop.update(Some(
            vec![CHEYENNE_DESKTOP.public(), CHEYENNE_PALMTOP.public()]
                .into_iter()
                .collect(),
        ))?;
        let cheyenne_palmtop = common::Device::create_from(&*CHEYENNE_PALMTOP, &cheyenne_desktop)?;
        // Doesn't check out
        assert_matches!(
            cheyenne_palmtop.verify(),
            Err(error::VerifyUser::Verification(
                VerificationError::ParentQuorum
            ))
        );
        // Hence `dylan_project` doesn't check out either
        let heads = current_heads_from(vec![&cheyenne_palmtop, &dylan]);
        assert_matches!(
            dylan_project.verify(lookup(&heads)),
            Err(error::VerifyProject::VerifyUser(
                error::VerifyUser::Verification(VerificationError::ParentQuorum)
            ))
        );

        Ok(())
    }
}

#[test]
fn double_vote() -> anyhow::Result<()> {
    let repo = common::repo()?;
    {
        let cheyenne_desktop = common::Device::new_with(
            &*CHEYENNE_DESKTOP,
            Git::new(&repo),
            payload::User {
                name: "cheyenne".into(),
            },
        )?
        .update(Some(
            vec![CHEYENNE_DESKTOP.public(), CHEYENNE_LAPTOP.public()]
                .into_iter()
                .collect(),
        ))?;

        let cheyenne_laptop = common::Device::create_from(&*CHEYENNE_LAPTOP, &cheyenne_desktop)?;
        let cheyenne_desktop = cheyenne_desktop.update_from(&cheyenne_laptop)?;
        cheyenne_desktop.assert_verifies()?;

        let dylan = common::Device::new_with(
            &*DYLAN,
            Git::new(&repo),
            payload::User {
                name: "dylan".into(),
            },
        )?;

        let cheyenne_project = {
            let update = IndirectDelegation::try_from_iter(vec![
                Right(cheyenne_desktop.current().clone()),
                Right(dylan.current().clone()),
            ])?;
            common::Project::new(cheyenne_desktop.clone())?.update(update)
        }?;
        let dylan_project = common::Project::create_from(dylan.clone(), &cheyenne_project)?;

        let heads = current_heads_from(vec![&cheyenne_desktop, &dylan]);
        dylan_project.assert_verifies(lookup(&heads))?;

        let cheyenne_project = cheyenne_project.update_from(&dylan_project)?.update(
            IndirectDelegation::try_from_iter(vec![Right(cheyenne_desktop.current().clone())])?,
        )?;
        // That doesn't pass parent-quorum
        assert_matches!(
            cheyenne_project.verify(lookup(&heads)),
            Err(error::VerifyProject::Verification(
                VerificationError::ParentQuorum
            ))
        );
        // Still doesn't pass if we try to confirm on the laptop
        //
        // XXX(kim): dang, we don't actually reach the `DoubleVote` error,
        // because `cheyenne_project` errors out first. Dunno how to trigger
        // this at this level.
        let cheyenne_project_laptop =
            common::Project::create_from(cheyenne_laptop, &cheyenne_project)?;
        assert_matches!(
            cheyenne_project_laptop.verify(lookup(&heads)),
            Err(error::VerifyProject::Verification(
                VerificationError::ParentQuorum
            ))
        );
        // In case dylan confirms anyway, `cheyenne_project_laptop` gets ignored
        //
        // FIXME(kim): There is a nice footgun opportunity here: if we merge
        // `cheyenne_project_laptop`, we'll end up at the previous revision (ie.
        // `dylan_project` above) because first-parent will just ignore
        // cheyenne's detour. Not sure if we can fix the `Git` operations to
        // ensure we don't merge more than one commit from a concurrent lineage.
        let dylan_project = dylan_project.update_from(&cheyenne_project)?;
        dylan_project.assert_verifies(lookup(&heads))?;
        let dylan_stupid = dylan_project
            .clone()
            .update_from(&cheyenne_project_laptop)?;
        assert_eq!(
            &dylan_stupid.verify(lookup(&heads))?.into_inner(),
            dylan_project.current()
        );

        Ok(())
    }
}

fn current_heads_from<'a>(
    devs: impl IntoIterator<Item = &'a common::Device<'a>>,
) -> BTreeMap<Urn, git2::Oid> {
    devs.into_iter()
        .map(|dev| {
            let cur = dev.current();
            (cur.urn(), *cur.content_id)
        })
        .collect()
}

fn lookup<'a>(map: &'a BTreeMap<Urn, git2::Oid>) -> impl Fn(Urn) -> Result<git2::Oid, !> + 'a {
    move |urn| Ok(*map.get(&urn).unwrap())
}
