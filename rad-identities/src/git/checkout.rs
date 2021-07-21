// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, ffi, path::PathBuf};

use either::Either;

use librad::{
    git::{
        identities::{self, Person},
        local::{transport::CanOpenStorage, url::LocalUrl},
        types::{
            remote::{LocalFetchspec, LocalPushspec, Remote},
            Flat,
            Force,
            GenericRef,
            Reference,
            Refspec,
        },
    },
    git_ext::{self, OneLevel, Qualified, RefLike},
    refspec_pattern,
    PeerId,
};

use crate::{
    field::{HasBranch, HasName, HasUrn, MissingDefaultBranch},
    git,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git::Error),

    #[error(transparent)]
    Identities(#[from] Box<identities::Error>),

    #[error(transparent)]
    Missing(#[from] MissingDefaultBranch),

    #[error(transparent)]
    Ref(#[from] git_ext::name::Error),

    #[error(transparent)]
    Transport(#[from] librad::git::local::transport::Error),
}

impl From<identities::Error> for Error {
    fn from(e: identities::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

/// Create a working copy of an identity that exists in storage. The working
/// copy can be based on either a [`Local`] copy, i.e. owned by the local
/// operator, or a [`Peer`]'s copy.
///
/// ## Local
///
/// In the former, a single `rad` remote is create linking the working copy to
/// the storage. The remote's upstream will be the default branch of the
/// identity.
///
/// ## Remote
///
/// In the latter, there will be a remote based on the peer we're checking out
/// from. The working copy will use the reference that is found at
/// `refs/remotes/<peer>/heads/<default branch>`. Two remotes will be created
/// linking the working copy to the storage. One will point to the peer, given
/// by the name `<name>@<peer_id>` (where name is the handle found in the peer's
/// [`Person`] document. The second remote will be the `rad` remote for the
/// operator's own references.
///
/// To illustrate further, the `config` of the working copy will look
/// similar to:
///
/// ```text
/// [remote "rad"]
///     url = rad://hyymr17h1fg5zk7duikgc7xoqonqorhwnxxs98kdb63f9etnsjxxmo@hwd1yrerzpjbmtshsqw6ajokqtqrwaswty6p7kfeer3yt1n76t46iqggzcr.git
///     fetch = +refs/heads/*:refs/remotes/rad/*
/// [remote "banana@hyy36ey56mfayah398n7w4i8hy5ywci43hbyhwf1krfwonc1ur87ch"]
///     url = rad://hyymr17h1fg5zk7duikgc7xoqonqorhwnxxs98kdb63f9etnsjxxmo@hwd1yrerzpjbmtshsqw6ajokqtqrwaswty6p7kfeer3yt1n76t46iqggzcr.git
///     fetch = +refs/remotes/hyy36ey56mfayah398n7w4i8hy5ywci43hbyhwf1krfwonc1ur87ch/heads/*:refs/remotes/banana@hyy36ey56mfayah398n7w4i8hy5ywci43hbyhwf1krfwonc1ur87ch/*
/// [branch "master"]
///     remote = rad
///     merge = refs/heads/master
/// [include]
///     path = /home/user/.config/radicle/git-includes/hwd1yrerzpjbmtshsqw6ajokqtqrwaswty6p7kfeer3yt1n76t46iqggzcr.inc
pub fn checkout<F, I>(
    open_storage: F,
    identity: &I,
    from: Either<Local, Peer>,
) -> Result<git2::Repository, Error>
where
    F: CanOpenStorage + Clone + 'static,
    I: HasBranch + HasUrn,
{
    let default_branch = identity.branch_or_die(identity.urn())?;

    let (repo, rad) = match from {
        Either::Left(local) => local.checkout(open_storage)?,
        Either::Right(peer) => peer.checkout(open_storage)?,
    };

    // Set configurations
    git::set_upstream(&repo, &rad, default_branch.clone())?;
    repo.set_head(Qualified::from(default_branch).as_str())
        .map_err(git::Error::from)?;
    repo.checkout_head(None).map_err(git::Error::from)?;
    Ok(repo)
}

pub struct Local {
    url: LocalUrl,
    path: PathBuf,
}

impl Local {
    pub fn new<I>(identity: &I, path: PathBuf) -> Self
    where
        I: HasName + HasUrn,
    {
        let path = resolve_path(identity, path);
        Self {
            url: LocalUrl::from(identity.urn()),
            path,
        }
    }

    fn checkout<F>(self, open_storage: F) -> Result<(git2::Repository, Remote<LocalUrl>), Error>
    where
        F: CanOpenStorage + 'static,
    {
        let rad = Remote::rad_remote(
            self.url,
            Refspec {
                src: refspec_pattern!("refs/heads/*"),
                dst: refspec_pattern!("refs/remotes/rad/*"),
                force: Force::True,
            },
        );
        Ok(git::clone(&self.path, open_storage, rad)?)
    }
}

pub struct Peer {
    url: LocalUrl,
    remote: (Person, PeerId),
    default_branch: OneLevel,
    path: PathBuf,
}

impl Peer {
    pub fn new<I>(identity: &I, remote: (Person, PeerId), path: PathBuf) -> Result<Self, Error>
    where
        I: HasBranch + HasName + HasUrn,
    {
        let urn = identity.urn();
        let default_branch = identity.branch_or_die(urn.clone())?;
        let path = resolve_path(identity, path);
        Ok(Self {
            url: LocalUrl::from(urn),
            remote,
            default_branch,
            path,
        })
    }

    fn checkout<F>(self, open_storage: F) -> Result<(git2::Repository, Remote<LocalUrl>), Error>
    where
        F: CanOpenStorage + Clone + 'static,
    {
        let (person, peer) = self.remote;
        let handle = &person.subject().name;
        let name =
            RefLike::try_from(format!("{}@{}", handle, peer)).expect("failed to parse remote name");

        let remote = Remote::new(self.url.clone(), name.clone()).with_fetchspecs(vec![Refspec {
            src: Reference::heads(Flat, peer),
            dst: GenericRef::heads(Flat, name),
            force: Force::True,
        }]);

        let (repo, _) = git::clone(&self.path, open_storage.clone(), remote)?;

        // Create a rad remote and push the default branch so we can set it as the
        // upstream.
        let rad = {
            // Create a fetchspec `refs/heads/*:refs/remotes/rad/*`
            let fetchspec = Refspec {
                src: GenericRef::<_, RefLike, _>::heads(Flat, None),
                dst: refspec_pattern!("refs/remotes/rad/*"),
                force: Force::True,
            };
            let mut rad = Remote::rad_remote(self.url, fetchspec);
            rad.save(&repo).map_err(git::Error::Git)?;
            let _ = rad.push(
                open_storage.clone(),
                &repo,
                LocalPushspec::Matching {
                    pattern: Qualified::from(self.default_branch).into(),
                    force: Force::False,
                },
            )?;
            let _ = rad.fetch(open_storage, &repo, LocalFetchspec::Configured)?;
            rad
        };

        Ok((repo, rad))
    }
}

pub fn from_whom<I>(
    identity: &I,
    remote: Option<(Person, PeerId)>,
    path: PathBuf,
) -> Result<Either<Local, Peer>, Error>
where
    I: HasBranch + HasName + HasUrn,
{
    Ok(match remote {
        None => Either::Left(Local::new(identity, path)),
        Some(remote) => Either::Right(Peer::new(identity, remote, path)?),
    })
}

fn resolve_path<I>(identity: &I, path: PathBuf) -> PathBuf
where
    I: HasName,
{
    let name = identity.name();

    // Check if the path provided ends in the 'directory_name' provided. If not we
    // create the full path to that name.
    path.components()
        .next_back()
        .map_or(path.join(&**name), |destination| {
            let destination: &ffi::OsStr = destination.as_ref();
            let name: &ffi::OsStr = name.as_ref();
            if destination == name {
                path.to_path_buf()
            } else {
                path.join(name)
            }
        })
}
