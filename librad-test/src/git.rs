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

use std::{convert::TryFrom, time::Duration};

use librad::{
    git::{
        local::{
            transport::{self, with_local_transport, CanOpenStorage},
            url::LocalUrl,
        },
        types::remote::Remote,
    },
    git_ext::reference::RefLike,
};

#[tracing::instrument(skip(repo), err)]
pub fn create_commit(
    repo: &git2::Repository,
    on_branch: RefLike,
) -> Result<git2::Oid, git2::Error> {
    let empty_tree = {
        let mut index = repo.index()?;
        let oid = index.write_tree()?;
        repo.find_tree(oid).unwrap()
    };
    let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
    repo.commit(
        Some(on_branch.as_str()),
        &author,
        &author,
        "Initial commit",
        &empty_tree,
        &[],
    )
}

#[tracing::instrument(skip(open_storage, repo), err)]
pub fn push<F>(
    open_storage: F,
    repo: &git2::Repository,
    remote: Remote<LocalUrl>,
) -> Result<Vec<(RefLike, git2::Oid)>, transport::Error>
where
    F: CanOpenStorage + 'static,
{
    let refspecs = remote
        .push_specs
        .iter()
        .map(|spec| spec.as_refspec())
        .collect::<Vec<_>>();

    with_local_transport(
        open_storage,
        repo,
        remote,
        Duration::from_secs(5),
        |git_remote| {
            let mut updated_refs = Vec::new();
            let mut remote_callbacks = git2::RemoteCallbacks::new();
            remote_callbacks.push_update_reference(|refname, maybe_error| match maybe_error {
                None => {
                    let rev = repo.find_reference(refname)?.target().unwrap();
                    let refname = RefLike::try_from(refname).unwrap();
                    updated_refs.push((refname, rev));

                    Ok(())
                },

                Some(err) => Err(git2::Error::from_str(&format!(
                    "Remote rejected {}: {}",
                    refname, err
                ))),
            });

            git_remote.push(
                &refspecs,
                Some(git2::PushOptions::new().remote_callbacks(remote_callbacks)),
            )?;

            Ok(updated_refs)
        },
    )
    .flatten()
}

#[tracing::instrument(skip(open_storage, repo), err)]
pub fn fetch<F>(
    open_storage: F,
    repo: &git2::Repository,
    remote: Remote<LocalUrl>,
) -> Result<Vec<(RefLike, git2::Oid)>, transport::Error>
where
    F: CanOpenStorage + 'static,
{
    let refspecs = remote
        .fetch_specs
        .iter()
        .map(|spec| spec.as_refspec())
        .collect::<Vec<_>>();

    with_local_transport(
        open_storage,
        repo,
        remote,
        Duration::from_secs(5),
        |git_remote| {
            let mut updated_refs = Vec::new();
            let mut remote_callbacks = git2::RemoteCallbacks::new();
            remote_callbacks.update_tips(|refname, _old, new| {
                if let Ok(refname) = RefLike::try_from(refname) {
                    updated_refs.push((refname, new))
                }

                true
            });

            git_remote.fetch(
                &refspecs,
                Some(
                    git2::FetchOptions::new()
                        .prune(git2::FetchPrune::On)
                        .update_fetchhead(false)
                        .download_tags(git2::AutotagOption::None)
                        .remote_callbacks(remote_callbacks),
                ),
                None,
            )?;

            Ok(updated_refs)
        },
    )
    .flatten()
}
