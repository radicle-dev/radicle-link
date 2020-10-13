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

use keystore::sign;

use crate::{
    git::ext::{self, is_not_found_err},
    identities::{
        git::{Identities, Urn, VerifiedUser},
        urn,
    },
    internal::result::ResultExt,
    keys::SecretKey,
    peer::{self, PeerId},
    uri::{self, RadUrn},
};

const CONFIG_USER_NAME: &str = "user.name";
const CONFIG_USER_EMAIL: &str = "user.email";
const CONFIG_RAD_SELF: &str = "rad.self";
const CONFIG_RAD_PEER_ID: &str = "rad.peerid";

#[derive(Debug, Error)]
pub enum Error {
    #[error("storage was already initialised with peer id {0}")]
    AlreadyInitialised(PeerId),

    #[error("supplied user entity is not signed by the local key")]
    NotSignedBySelf,

    #[error("configuration key {config_key} is not set")]
    Unset { config_key: &'static str },

    #[error(transparent)]
    PeerId(#[from] peer::conversion::Error),

    #[error(transparent)]
    Urn(#[from] urn::ParseError<ext::oid::FromMultihashError>),

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
        signer: &impl sign::Signer,
        user: U,
    ) -> Result<Self, Error>
    where
        U: Into<Option<VerifiedUser>>,
    {
        let peer_id = PeerId::from_signer(signer);
        let config = repo.config()?;
        let mut this = Config { inner: config };

        match this.peer_id().map(Some).or_matches::<Error, _, _>(
            |err| matches!(err, Error::Git(e) if is_not_found_err(&e)),
            || Ok(None),
        )? {
            Some(initialised_with) if initialised_with != peer_id => {
                return Err(Error::AlreadyInitialised(initialised_with));
            },
            _ => this.set_peer_id(&peer_id)?,
        }

        match user.into() {
            Some(user) => this.set_user(user),
            None => this.set_user_info("anonymous"),
        }?;

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

    /// Set the default identity.
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
    /// * The [`VerifiedUser`] is not signed by the configured [`PeerId`]'s key
    /// * The signature of the configured key is not owned by the
    ///   [`VerifiedUser`] (ie. the local key refers to a different entity)
    pub fn set_user<U>(&mut self, user: U) -> Result<(), Error>
    where
        U: Into<Option<VerifiedUser>>,
    {
        match user.into() {
            None => {
                self.inner
                    .remove(CONFIG_RAD_SELF)
                    .or_matches::<Error, _, _>(is_not_found_err, || Ok(()))?;
                self.set_user_info("anonymous")
            },

            Some(user) => {
                self.guard_user_valid(&user)?;
                self.inner
                    .set_str(CONFIG_RAD_SELF, &user.urn().to_string())
                    .map_err(Error::from)?;
                self.set_user_info(&user.doc.payload.subject.name)?;

                Ok(())
            },
        }
    }

    /// Validation rules as described for [`Config::set_user`]
    pub fn guard_user_valid(&self, user: &VerifiedUser) -> Result<(), Error> {
        let peer_id = self.peer_id()?;
        if user.signatures.contains_key(peer_id.as_public_key()) {
            Ok(())
        } else {
            Err(Error::NotSignedBySelf)
        }
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

    impl DerefMut for TmpConfig {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.config
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
    fn init_proper() {
        let config = setup(&*ALICE_KEY);

        assert_eq!(config.peer_id().unwrap(), *ALICE_PEER_ID);
        assert_matches!(
            config.user(),
            Err(Error::Unset {
                config_key: CONFIG_RAD_SELF
            })
        )
    }

    #[test]
    fn reinit_with_different_key() {
        let mut alice_config = setup(&*ALICE_KEY);
        let bob_config = Config::init(&mut alice_config.repo, &*BOB_KEY, None);

        assert_matches!(
            bob_config.map(|_| ()), // map to avoid `Debug` impl
            Err(Error::AlreadyInitialised(pid)) if pid == *ALICE_PEER_ID
        )
    }

    #[test]
    fn set_and_unset_user() {
        let mut config = setup(&*ALICE_KEY);

        let alice = {
            let ids = Identities::<User>::from(&config.repo);
            let alice = ids
                .create(
                    identities::payload::User {
                        name: "alice".into(),
                    }
                    .into(),
                    Some(ALICE_KEY.public()).into_iter().collect(),
                    &*ALICE_KEY,
                )
                .unwrap();
            ids.verify(*alice.content_id).unwrap()
        };
        let alice_urn = alice.urn();

        config.set_user(alice).unwrap();
        assert_eq!(Some(alice_urn), config.user().unwrap());

        config.set_user(None).unwrap();
        assert!(config.user().unwrap().is_none())
    }

    #[test]
    fn guard_user_valid() {
        let config = setup(&*ALICE_KEY);

        let alice = {
            let ids = Identities::<User>::from(&config.repo);
            let alice = ids
                .create(
                    identities::payload::User {
                        name: "alice".into(),
                    }
                    .into(),
                    Some(ALICE_KEY.public()).into_iter().collect(),
                    &*ALICE_KEY,
                )
                .unwrap();
            ids.verify(*alice.content_id).unwrap()
        };

        assert_matches!(config.guard_user_valid(&alice), Ok(()))
    }

    #[test]
    fn guard_user_not_self_signed() {
        let config = setup(&*ALICE_KEY);

        let bob = {
            let ids = Identities::<User>::from(&config.repo);
            let bob = ids
                .create(
                    identities::payload::User { name: "bob".into() }.into(),
                    Some(BOB_KEY.public()).into_iter().collect(),
                    &*BOB_KEY,
                )
                .unwrap();
            ids.verify(*bob.content_id).unwrap()
        };

        assert_matches!(config.guard_user_valid(&bob), Err(Error::NotSignedBySelf))
    }
}
