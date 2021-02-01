// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

use librad::{
    self,
    git::{
        identities::{self, local::LocalIdentity, Project},
        local::{
            transport::{self, CanOpenStorage},
            url::LocalUrl,
        },
        storage::Storage,
    },
    identities::{delegation::Indirect, payload},
    paths::Paths,
    signer::BoxedSigner,
};

pub mod error;
use error::Error;

pub mod existing;
pub mod include;
pub mod new;

mod git;
mod sealed;

pub trait CreateRepo: sealed::Sealed {
    type Error;

    fn init<F>(self, url: LocalUrl, transport: F) -> Result<git2::Repository, Self::Error>
    where
        F: CanOpenStorage + 'static;
}

pub trait AsPayload: sealed::Sealed {
    fn as_payload(&self) -> payload::Project;
}

/// Create a [`Project`] and its working copy from the provided `payload`. The
/// `payload` must implement [`CreateRepo`] and [`AsPayload`], the only types of
/// which are:
///   * [`exisiting::Existing<exisiting::Valid>`]
///   * [`new::New<new::Valid>`]
///
/// This means that to provide a `payload` you must have a valid construction so
/// that we don't end up with bad state.
pub fn init<P>(
    paths: Paths,
    signer: BoxedSigner,
    storage: &Storage,
    whoami: LocalIdentity,
    payload: P,
) -> Result<Project, Error>
where
    P: AsPayload + CreateRepo,
    P::Error: Into<error::Error>,
{
    let project = identities::project::create::<payload::Project>(
        storage,
        whoami.clone(),
        payload.as_payload(),
        Indirect::from(whoami.into_inner().into_inner()),
    )?;

    let transport = transport::Settings {
        paths: paths.clone(),
        signer,
    };
    let url = LocalUrl::from(project.urn());
    let repo = payload.init(url, transport).map_err(|err| err.into())?;

    let path = include::update(storage, &paths, &project)?;
    librad::git::include::set_include_path(&repo, path).map_err(include::Error::from)?;

    Ok(project)
}
