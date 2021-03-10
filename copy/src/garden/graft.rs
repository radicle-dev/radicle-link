// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, ffi, path::PathBuf};

use either::Either;

use librad::{
    git::{
        identities::{self, Person, Project},
        local::{transport::CanOpenStorage, url::LocalUrl},
        types::{
            remote::{LocalPushspec, Remote},
            Flat,
            Force,
            GenericRef,
            Reference,
            Refspec,
        },
        Urn,
    },
    git_ext::{self, OneLevel, Qualified, RefLike},
    peer::PeerId,
    refspec_pattern,
};

use crate::git;

/// When checking out a working copy, we can run into several I/O failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Git error when checking out the project.
    #[error(transparent)]
    Git(#[from] git::Error),

    #[error(transparent)]
    Identities(#[from] Box<identities::Error>),

    #[error("the project, at `{0}`, does not have default branch set")]
    MissingDefaultBranch(Urn),

    #[error(transparent)]
    Ref(#[from] git_ext::name::Error),

    /// An error occurred in the local transport.
    #[error(transparent)]
    Transport(#[from] librad::git::local::transport::Error),
}

impl From<identities::Error> for Error {
    fn from(e: identities::Error) -> Self {
        Self::Identities(Box::new(e))
    }
}

/// Based off of the `Fork`, clone the project using the provided inputs.
///
/// ## Local Clone
///
/// If the `Fork` is `Local` this means that we are cloning based off the user's
/// own project and so the `url` used to clone will be built from the user's
/// `PeerId`. The only remote that will be created is `rad` remote, pointing to
/// the `url` built from the provided `urn` and the user's `PeerId`.
///
/// ## Remote Clone
///
/// If the `Fork` is `Remote` this means that we are cloning based off of a
/// peer's project.
/// Due to this we need to point the remote to the specific remote in our
/// project's hierarchy. What this means is that we need to set up a fetch
/// refspec in the form of `refs/remotes/<peer_id>/heads/*` where the name of
/// the remote is given by `<user_handle>@<peer_id>` -- this keeps in line with
/// [`librad::git::include`]. To finalise the setup of the clone, we also want
/// to add the `rad` remote, which is the designated remote the user pushes
/// their own work to update their monorepo for this project. To do this, we
/// create a `url` that is built using the provided `urn` and the user's
/// `PeerId` and create the `rad` remote. Finally, we initialise the
/// `default_branch` of the proejct -- think upstream branch in git. We do this
/// by pushing to the `rad` remote. This means that the working copy will be now
/// setup where when we open it up we see the initial branch as being
/// `default_branch`.
///
/// To illustrate further, the `config` of the final repository will look
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
/// ```
pub fn graft<F>(
    open_storage: F,
    project: &Project,
    from: Either<Local, Peer>,
) -> Result<git2::Repository, Error>
where
    F: CanOpenStorage + Clone + 'static,
{
    let default_branch = OneLevel::from(RefLike::try_from(
        project
            .subject()
            .default_branch
            .as_ref()
            .ok_or_else(|| Error::MissingDefaultBranch(project.urn()))?
            .as_str(),
    )?);

    let (repo, rad) = match from {
        Either::Left(local) => local.graft(open_storage)?,
        Either::Right(peer) => peer.graft(open_storage)?,
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
    pub fn new(project: &Project, path: PathBuf) -> Self {
        Self {
            url: LocalUrl::from(project.urn()),
            path: resolve_path(project, path),
        }
    }

    fn graft<F>(self, open_storage: F) -> Result<(git2::Repository, Remote<LocalUrl>), Error>
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
    pub fn new(project: &Project, remote: (Person, PeerId), path: PathBuf) -> Result<Self, Error> {
        let default_branch = project
            .subject()
            .default_branch
            .as_ref()
            .ok_or_else(|| Error::MissingDefaultBranch(project.urn()))?;
        let default_branch = OneLevel::from(RefLike::try_from(default_branch.as_str())?);
        Ok(Self {
            url: LocalUrl::from(project.urn()),
            remote,
            default_branch,
            path: resolve_path(project, path),
        })
    }

    fn graft<F>(self, open_storage: F) -> Result<(git2::Repository, Remote<LocalUrl>), Error>
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
                open_storage,
                &repo,
                LocalPushspec::Matching {
                    pattern: Qualified::from(self.default_branch).into(),
                    force: Force::False,
                },
            )?;
            rad
        };

        Ok((repo, rad))
    }
}

fn resolve_path(project: &Project, path: PathBuf) -> PathBuf {
    let name = &project.subject().name;

    // Check if the path provided ends in the 'directory_name' provided. If not we
    // create the full path to that name.
    let project_path: PathBuf =
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
            });
    project_path
}
