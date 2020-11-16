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

use std::io;

use anyhow::anyhow;

use super::*;
use crate::keys::SecretKey;

use librad_test::tempdir::WithTmpDir;

type TmpRepo = WithTmpDir<git2::Repository>;

pub(super) fn repo() -> anyhow::Result<TmpRepo> {
    Ok(WithTmpDir::new(|path| {
        let setup = || {
            let repo = git2::Repository::init(path)?;

            // We need to set user info to _something_, but that doesn't have to
            // be valid, as we're using a shared repo with many keys
            let mut config = repo.config()?;
            config.set_str("user.name", "shared")?;
            config.set_str("user.email", "not.relevant@for.testing")?;
            Ok(repo)
        };
        setup().map_err(|e: git2::Error| io::Error::new(io::ErrorKind::Other, e))
    })?)
}

#[derive(Clone)]
pub(super) struct Device<'a> {
    key: &'a SecretKey,
    git: Identities<'a, Person>,
    cur: Person,
}

impl<'a> Device<'a> {
    pub fn new(key: &'a SecretKey, git: Identities<'a, Person>) -> anyhow::Result<Self> {
        Self::new_with(
            key,
            git,
            payload::Person {
                name: "dylan".into(),
            },
        )
    }

    pub fn new_with(
        key: &'a SecretKey,
        git: Identities<'a, Person>,
        payload: payload::Person,
    ) -> anyhow::Result<Self> {
        let cur = git.create(
            payload.into(),
            Some(key.public()).into_iter().collect(),
            key,
        )?;

        Ok(Self { key, git, cur })
    }

    pub fn create_from(key: &'a SecretKey, other: &Device<'a>) -> anyhow::Result<Self> {
        let cur = other
            .git
            .create_from(Verifying::from(other.cur.clone()).signed()?, key)?;

        Ok(Self {
            key,
            cur,
            git: Identities::from(other.git.repo),
        })
    }

    pub fn current(&self) -> &Person {
        &self.cur
    }

    pub fn update(
        self,
        delegations: impl Into<Option<delegation::Direct>>,
    ) -> anyhow::Result<Self> {
        let cur = self.git.update(
            Verifying::from(self.cur).signed()?,
            None,
            delegations,
            self.key,
        )?;

        Ok(Self { cur, ..self })
    }

    pub fn update_from(self, other: &Device<'a>) -> anyhow::Result<Self> {
        let cur = self.git.update_from(
            Verifying::from(self.cur).signed()?,
            Verifying::from(other.cur.clone()).signed()?,
            self.key,
        )?;

        Ok(Self { cur, ..self })
    }

    pub fn verify(&self) -> Result<VerifiedPerson, error::VerifyPerson> {
        Ok(self.git.verify(*self.cur.content_id)?)
    }

    pub fn assert_verifies(&self) -> anyhow::Result<()> {
        let verified = self.verify()?.into_inner();
        anyhow::ensure!(
            verified == self.cur,
            anyhow!(
                "verified head `{}` is not current head `{}`",
                verified.content_id,
                self.cur.content_id
            )
        );

        Ok(())
    }

    pub fn assert_no_quorum(&self) -> anyhow::Result<()> {
        let quorum = Verifying::from(self.cur.clone()).signed()?.quorum();
        anyhow::ensure!(
            matches!(quorum, Err(VerificationError::Quorum)),
            anyhow!(
                "expected {} to not reach quorum, instead this happened: {:?}",
                self.cur.content_id,
                quorum
            )
        );

        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Project<'a> {
    dev: Device<'a>,
    cur: super::Project,
}

impl<'a> Project<'a> {
    pub fn new(dev: Device<'a>) -> anyhow::Result<Self> {
        let cur = dev.git.as_project().create(
            payload::Project {
                name: "haskell-emoji".into(),
                description: Some("The Most Interesting Software Project In The World".into()),
                default_branch: Some("\u{1F32F}".into()),
            }
            .into(),
            IndirectDelegation::try_from_iter(Some(Right(dev.cur.clone())))?,
            dev.key,
        )?;

        Ok(Self { dev, cur })
    }

    pub fn create_from(dev: Device<'a>, other: &Project<'a>) -> anyhow::Result<Self> {
        let cur = dev
            .git
            .as_project()
            .create_from(Verifying::from(other.cur.clone()).signed()?, dev.key)?;

        Ok(Self { dev, cur })
    }

    pub fn current(&self) -> &super::Project {
        &self.cur
    }

    pub fn update(
        self,
        delegations: impl Into<Option<IndirectDelegation>>,
    ) -> anyhow::Result<Self> {
        let cur = self.dev.git.as_project().update(
            Verifying::from(self.cur).signed()?,
            None,
            delegations,
            self.dev.key,
        )?;

        Ok(Self { cur, ..self })
    }

    pub fn update_from(self, other: &Project<'a>) -> anyhow::Result<Self> {
        let cur = self.dev.git.as_project().update_from(
            Verifying::from(self.cur).signed()?,
            Verifying::from(other.cur.clone()).signed()?,
            self.dev.key,
        )?;

        Ok(Self { cur, ..self })
    }

    pub fn verify<F>(&self, lookup: F) -> Result<VerifiedProject, error::VerifyProject>
    where
        F: Fn(Urn) -> Result<git2::Oid, !>,
    {
        Ok(self
            .dev
            .git
            .as_project()
            .verify(*self.cur.content_id, lookup)?)
    }

    pub fn assert_verifies<F>(&self, lookup: F) -> anyhow::Result<()>
    where
        F: Fn(Urn) -> Result<git2::Oid, !>,
    {
        let verified = self.verify(lookup)?.into_inner();
        anyhow::ensure!(
            verified == self.cur,
            anyhow!(
                "verified head `{}` is not current head `{}`",
                verified.content_id,
                self.cur.content_id
            )
        );

        Ok(())
    }

    pub fn assert_no_quorum(&self) -> anyhow::Result<()> {
        let quorum = Verifying::from(self.cur.clone()).signed()?.quorum();
        anyhow::ensure!(
            matches!(quorum, Err(VerificationError::Quorum)),
            anyhow!(
                "expected {} to not reach quorum, instead this happened: {:?}",
                self.cur.content_id,
                quorum
            )
        );

        Ok(())
    }
}
