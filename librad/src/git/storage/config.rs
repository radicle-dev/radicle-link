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

#![allow(unused)]

use std::{convert::TryFrom, io, marker::PhantomData};

use git_ext::{self as ext, is_not_found_err};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{super::identities::local::LocalIdentity, Storage};
use crate::{
    identities::{
        git::{Identities, Urn, VerifiedUser},
        urn,
    },
    keys::SecretKey,
    peer::{self, PeerId},
    signer::Signer,
};

const CONFIG_USER_NAME: &str = "user.name";
const CONFIG_USER_EMAIL: &str = "user.email";
const CONFIG_RAD_SELF: &str = "rad.self";
const CONFIG_RAD_PEER_ID: &str = "rad.peerid";

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("storage was already initialised with peer id {0}")]
    AlreadyInitialised(PeerId),

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error(transparent)]
    Urn(#[from] urn::ParseError<ext::oid::FromMultihashError>),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[cfg(test)]
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct Config<'a, S> {
    inner: git2::Config,
    signer: &'a S,
}

impl<'a, S> TryFrom<&'a Storage<S>> for Config<'a, S>
where
    S: Signer,
{
    type Error = Error;

    fn try_from(storage: &'a Storage<S>) -> Result<Self, Self::Error> {
        let inner = storage.as_raw().config()?;
        let this = Self {
            inner,
            signer: storage.signer(),
        };
        this.guard_key_change()?;

        Ok(this)
    }
}

impl TryFrom<&git2::Repository> for Config<'_, PhantomData<!>> {
    type Error = git2::Error;

    fn try_from(repo: &git2::Repository) -> Result<Self, Self::Error> {
        let inner = repo.config()?;
        Ok(Self {
            inner,
            signer: &PhantomData,
        })
    }
}

impl<'a, S> Config<'a, S>
where
    S: Signer,
{
    fn guard_key_change(&self) -> Result<(), Error> {
        let configured_peer_id = self.peer_id().map(Some).or_matches::<Error, _, _>(
            |err| matches!(err, Error::Git(e) if is_not_found_err(&e)),
            || Ok(None),
        )?;
        let signer_peer_id = PeerId::from_signer(self.signer);
        match configured_peer_id {
            Some(initialised_with) if initialised_with != signer_peer_id => {
                Err(Error::AlreadyInitialised(initialised_with))
            },

            _ => Ok(()),
        }
    }

    pub(super) fn init(repo: &mut git2::Repository, signer: &'a S) -> Result<Self, Error> {
        let peer_id = PeerId::from_signer(signer);
        let config = repo.config()?;
        let mut this = Config {
            inner: config,
            signer,
        };
        this.guard_key_change()?;
        this.set_peer_id(PeerId::from_signer(signer))?;
        this.set_user_info("anonymous")?;

        Ok(this)
    }

    fn set_user_info(&mut self, name: &str) -> Result<(), Error> {
        let peer_id = self.peer_id()?;
        self.inner.set_str(CONFIG_USER_NAME, name)?;
        self.inner
            .set_str(CONFIG_USER_EMAIL, &format!("{}@{}", name, peer_id))?;

        Ok(())
    }

    fn set_peer_id(&mut self, peer_id: PeerId) -> Result<(), Error> {
        self.inner
            .set_str(CONFIG_RAD_PEER_ID, &peer_id.to_string())
            .map_err(Error::from)
    }

    /// Set the default identity.
    ///
    /// Passing [`Option::None`] removes the setting.
    pub fn set_user<U>(&mut self, user: U) -> Result<(), Error>
    where
        U: Into<Option<LocalIdentity>>,
    {
        match user.into() {
            None => {
                self.inner
                    .remove(CONFIG_RAD_SELF)
                    .or_matches::<Error, _, _>(is_not_found_err, || Ok(()))?;
                self.set_user_info("anonymous")
            },

            Some(user) => {
                self.inner
                    .set_str(CONFIG_RAD_SELF, &user.urn().to_string())
                    .map_err(Error::from)?;
                self.set_user_info(&user.doc.payload.subject.name)?;

                Ok(())
            },
        }
    }
}

impl<S> Config<'_, S> {
    pub fn user_name(&self) -> Result<String, Error> {
        self.inner.get_string(CONFIG_USER_NAME).map_err(Error::from)
    }

    pub fn user_email(&self) -> Result<String, Error> {
        self.inner
            .get_string(CONFIG_USER_EMAIL)
            .map_err(Error::from)
    }
    pub fn peer_id(&self) -> Result<PeerId, Error> {
        self.inner
            .get_string(CONFIG_RAD_PEER_ID)
            .map_err(Error::from)
            .and_then(|peer_id| peer_id.parse().map_err(Error::from))
    }

    pub fn user(&self) -> Result<Option<Urn>, Error> {
        self.inner
            .get_string(CONFIG_RAD_SELF)
            .map(Some)
            .or_matches::<Error, _, _>(is_not_found_err, || Ok(None))?
            .map(|urn| urn.parse().map_err(Error::from))
            .transpose()
    }
}

impl Config<'_, PhantomData<!>> {
    pub fn readonly(repo: &git2::Repository) -> Result<Self, git2::Error> {
        Self::try_from(repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::{Deref, DerefMut};

    use crate::{
        identities::{self, git::User},
        keys::SecretKey,
    };
    use librad_test::tempdir::WithTmpDir;

    lazy_static! {
        static ref ALICE_KEY: SecretKey = SecretKey::from_seed([
            81, 151, 13, 57, 246, 76, 127, 57, 30, 125, 102, 210, 87, 132, 7, 92, 12, 122, 7, 30,
            202, 71, 235, 169, 66, 199, 172, 11, 97, 50, 173, 150
        ]);
        static ref BOB_KEY: SecretKey = SecretKey::from_seed([
            117, 247, 70, 158, 119, 191, 163, 76, 169, 138, 229, 198, 147, 90, 8, 220, 233, 86,
            170, 139, 85, 5, 233, 64, 1, 58, 193, 241, 12, 87, 14, 60
        ]);
        static ref ALICE_PEER_ID: PeerId = PeerId::from(&*ALICE_KEY);
    }

    struct TmpConfig<'a> {
        repo: git2::Repository,
        config: Config<'a, SecretKey>,
    }

    impl<'a> Deref for TmpConfig<'a> {
        type Target = Config<'a, SecretKey>;

        fn deref(&self) -> &Self::Target {
            &self.config
        }
    }

    impl<'a> DerefMut for TmpConfig<'a> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.config
        }
    }

    type TmpState<'a> = WithTmpDir<TmpConfig<'a>>;

    fn setup(key: &SecretKey) -> TmpState {
        WithTmpDir::new::<_, Error>(|path| {
            let mut repo = git2::Repository::init_bare(path)?;
            let config = Config::init(&mut repo, key)?;
            Ok(TmpConfig { repo, config })
        })
        .unwrap()
    }

    #[test]
    fn init_proper() {
        let config = setup(&*ALICE_KEY);

        assert_eq!(config.peer_id().unwrap(), *ALICE_PEER_ID);
        assert!(config.user().unwrap().is_none())
    }

    #[test]
    fn reinit_with_different_key() {
        let mut alice_config = setup(&*ALICE_KEY);
        let bob_config = Config::init(&mut alice_config.repo, &*BOB_KEY);

        assert_matches!(
            bob_config.map(|_| ()), // map to avoid `Debug` impl
            Err(Error::AlreadyInitialised(pid)) if pid == *ALICE_PEER_ID
        )
    }
}
