// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(unused)]

use std::{convert::TryFrom, io, marker::PhantomData, path::PathBuf};

use git_ext::{self as ext, is_not_found_err};
use std_ext::result::ResultExt as _;
use thiserror::Error;

use super::{super::identities::local::LocalIdentity, Storage};
use crate::{
    identities::{
        git::{Identities, Urn, VerifiedPerson},
        urn,
    },
    keys::SecretKey,
    peer::{self, PeerId},
    signer::{BoxedSigner, Signer},
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
    Urn(#[from] urn::error::FromStr<ext::oid::FromMultihashError>),

    #[error(transparent)]
    Git(#[from] git2::Error),
}

/// The _local_ config for the give [`git2::Repository`].
///
/// This is typically `$GIT_DIR/.git/config` for non-bare, and `$GIT_DIR/config`
/// for bare repositories.
pub fn path(repo: &git2::Repository) -> PathBuf {
    repo.path().join("config")
}

pub struct Config<'a, S> {
    inner: git2::Config,
    signer: &'a S,
}

impl<'a> TryFrom<&'a Storage> for Config<'a, BoxedSigner> {
    type Error = Error;

    fn try_from(storage: &'a Storage) -> Result<Self, Self::Error> {
        let inner = git2::Config::open(&storage.config_path())?;
        let mut this = Self {
            inner,
            signer: storage.signer(),
        };
        this.guard_key_change()?;
        this.ensure_reflog()?;

        Ok(this)
    }
}

impl TryFrom<&git2::Repository> for Config<'_, PhantomData<!>> {
    type Error = git2::Error;

    fn try_from(repo: &git2::Repository) -> Result<Self, Self::Error> {
        let inner = git2::Config::open(&self::path(repo))?.snapshot()?;
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
            |err| matches!(err, Error::Git(e) if is_not_found_err(e)),
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

    fn ensure_reflog(&mut self) -> Result<(), Error> {
        if let Err(e) = self.inner.get_bool("core.logAllRefUpdates") {
            return if is_not_found_err(&e) {
                Ok(self.inner.set_bool("core.logAllRefUpdates", true)?)
            } else {
                Err(e.into())
            };
        }

        Ok(())
    }

    // TODO(finto): changed this from `pub(super)` to `pub`, but should it be hidden
    // and we change the creation test?
    pub fn init(repo: &mut git2::Repository, signer: &'a S) -> Result<Self, Error> {
        let peer_id = PeerId::from_signer(signer);
        let config = git2::Config::open(&self::path(repo))?;
        let mut this = Config {
            inner: config,
            signer,
        };
        this.guard_key_change()?;
        this.ensure_reflog()?;
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
                self.set_user_info(&user.subject().name)?;

                Ok(())
            },
        }
    }

    pub(crate) fn as_raw(&self) -> &git2::Config {
        &self.inner
    }

    pub(crate) fn as_raw_mut(&mut self) -> &mut git2::Config {
        &mut self.inner
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
