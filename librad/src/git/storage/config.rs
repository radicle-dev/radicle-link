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

use std::{convert::TryFrom, io};

use thiserror::Error;

use crate::{
    git::ext::is_not_found_err,
    internal::result::ResultExt,
    keys::SecretKey,
    meta::{entity::Verified, user::User},
    peer::{self, PeerId},
    uri::{self, RadUrn},
};

const CONFIG_USER_NAME: &str = "user.name";
const CONFIG_USER_EMAIL: &str = "user.email";
const CONFIG_RAD_SELF: &str = "rad.self";
const CONFIG_RAD_PEER_ID: &str = "rad.peerid";

#[derive(Debug, Error)]
pub enum Error {
    #[error("Supplied user entity is not signed by the local key")]
    NotSignedBySelf,

    #[error("Entity must be  signed with an owned key")]
    OwnedKeyRequired,

    #[error("Configuration key {config_key} is not set")]
    Unset { config_key: &'static str },

    #[error(transparent)]
    Peer(#[from] peer::conversion::Error),

    #[error(transparent)]
    Urn(#[from] uri::rad_urn::ParseError),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct Config {
    inner: git2::Config,
}

impl<'a> TryFrom<&'a git2::Repository> for Config {
    type Error = git2::Error;

    fn try_from(repo: &'a git2::Repository) -> Result<Self, Self::Error> {
        let inner = repo.config()?;
        Ok(Self { inner })
    }
}

impl Config {
    pub(super) fn init<U>(
        repo: &mut git2::Repository,
        signer: &SecretKey,
        user: U,
    ) -> Result<Self, Error>
    where
        U: Into<Option<User<Verified>>>,
    {
        let mut config = repo.config()?;
        let peer_id = PeerId::from(signer);
        let user = user.into();

        let mut this = Config { inner: config };
        this.set_peer_id(&peer_id)?;
        this.set_user_info(user.as_ref().map(|u| u.name()).unwrap_or("radicle"))?;
        this.set_user(user)?;

        Ok(this)
    }

    pub fn user_name(&self) -> Result<String, Error> {
        self.inner.get_string(CONFIG_USER_NAME).map_err(Error::from)
    }

    pub fn user_email(&self) -> Result<String, Error> {
        self.inner
            .get_string(CONFIG_USER_EMAIL)
            .map_err(Error::from)
    }

    fn set_user_info(&mut self, name: &str) -> Result<(), Error> {
        let peer_id = self.peer_id()?;
        self.inner.set_str(CONFIG_USER_NAME, name)?;
        self.inner
            .set_str(CONFIG_USER_EMAIL, &format!("{}@{}", name, peer_id))?;

        Ok(())
    }

    pub fn peer_id(&self) -> Result<PeerId, Error> {
        self.inner
            .get_string(CONFIG_RAD_PEER_ID)
            .map_err(Error::from)
            .and_then(|peer_id| peer_id.parse().map_err(Error::from))
    }

    fn set_peer_id(&mut self, peer_id: &PeerId) -> Result<(), Error> {
        self.inner
            .set_str(CONFIG_RAD_PEER_ID, &peer_id.to_string())
            .map_err(Error::from)
    }

    /// Set the default [`User`] identity.
    ///
    /// Passing [`Option::None`] removes the setting.
    ///
    /// # Invariants
    ///
    /// ## Unchecked
    ///
    /// The caller must ensure that the entity is persisted.
    ///
    /// ## Checked
    ///
    /// An error is returned if:
    ///
    /// * The [`User`] is not signed by the configured [`PeerId`]'s key
    /// * The signature of the configured key is not owned by the [`User`] (ie.
    ///   the local key refers to a different entity)
    pub fn set_user<U>(&mut self, user: U) -> Result<(), Error>
    where
        U: Into<Option<User<Verified>>>,
    {
        match user.into() {
            None => self
                .inner
                .remove(CONFIG_RAD_SELF)
                .or_matches(is_not_found_err, || Ok(())),

            Some(user) => {
                self.guard_user_valid(&user)?;
                self.inner
                    .set_str(CONFIG_RAD_SELF, &user.urn().to_string())
                    .map_err(Error::from)?;
                self.set_user_info(user.name())?;

                Ok(())
            },
        }
    }

    /// Validation rules as described for [`Config::set_user`]
    pub fn guard_user_valid<S>(&self, user: &User<S>) -> Result<(), Error>
    where
        S: Clone,
    {
        let peer_id = self.peer_id()?;
        user.keys()
            .get(peer_id.as_public_key())
            .ok_or(Error::NotSignedBySelf)?;
        Ok(())
    }

    pub fn user(&self) -> Result<RadUrn, Error> {
        let urn = self
            .inner
            .get_string(CONFIG_RAD_SELF)
            .or_matches(is_not_found_err, || {
                Err(Error::Unset {
                    config_key: CONFIG_RAD_SELF,
                })
            })?;

        urn.parse().map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Deref;

    use tempfile::tempdir;

    use crate::{keys::SecretKey, meta::entity::Draft, test::ConstResolver};
    use librad_test::tempdir::WithTmpDir;

    struct TmpConfig {
        repo: git2::Repository,
        config: Config,
    }

    impl Deref for TmpConfig {
        type Target = Config;

        fn deref(&self) -> &Self::Target {
            &self.config
        }
    }

    type TmpState = WithTmpDir<TmpConfig>;

    fn setup(key: &SecretKey) -> TmpState {
        WithTmpDir::new::<_, Error>(|path| {
            let mut repo = git2::Repository::init_bare(path)?;
            let config = Config::init(&mut repo, key, None)?;
            Ok(TmpConfig { repo, config })
        })
        .unwrap()
    }

    #[test]
    fn test_init() {
        let key = SecretKey::new();
        let config = setup(&key);

        assert_eq!(config.peer_id().unwrap(), PeerId::from(&key));
        assert!(matches!(
            config.user(),
            Err(Error::Unset {
                config_key: CONFIG_RAD_SELF
            })
        ))
    }

    // FIXME: This test is impossible to write
    // (the error condition is impossible with this refactor)
    /*
    #[test]
    fn test_guard_user_unsigned() {
        let key = SecretKey::new();
        let config = setup(&key);

        let alice = User::<Draft>::create("alice".to_owned(), key.public()).unwrap();
        assert!(matches!(
            config.guard_user_valid(&alice),
            Err(Error::NotSignedBySelf)
        ))
    }
    */

    // FIXME: This test is impossible to write
    // (the error condition is impossible with this refactor)
    /*
    #[async_test]
    async fn test_guard_user_not_self_signed() {

        let key = SecretKey::new();
        let config = setup(&key);

        let mut alice = User::<Draft>::create("alice".to_owned(), key.public()).unwrap();
        {
            let bob = User::<Draft>::create("bob".to_owned(), key.public()).unwrap();
            alice
                .sign_by_user(&key, &bob.clone().as_verified())
                .unwrap();
        }

        assert!(matches!(
            config.guard_user_valid(&alice),
            Err(Error::OwnedKeyRequired)
        ))
    }
    */

    #[async_test]
    async fn test_guard_user_valid() {
        let key = SecretKey::new();
        let config = setup(&key);

        let mut alice = User::<Draft>::create("alice".to_owned(), key.public()).unwrap();
        {
            alice.sign_owned(&key).unwrap();
        }

        assert!(matches!(config.guard_user_valid(&alice), Ok(())))
    }
}
