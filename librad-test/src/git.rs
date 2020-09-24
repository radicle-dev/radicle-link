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

use librad::git::{local::url::LocalUrl, types::remote::Remote};

pub fn initial_commit(
    repo: &git2::Repository,
    remote: Remote<LocalUrl>,
    reference: &str,
    remote_callbacks: Option<git2::RemoteCallbacks>,
) -> Result<git2::Oid, git2::Error> {
    let mut remote = remote.create(&repo)?;

    let commit_id = {
        let empty_tree = {
            let mut index = repo.index()?;
            let oid = index.write_tree()?;
            repo.find_tree(oid).unwrap()
        };
        let author = git2::Signature::now("The Animal", "animal@muppets.com").unwrap();
        repo.commit(
            Some(reference),
            &author,
            &author,
            "Initial commit",
            &empty_tree,
            &[],
        )?
    };

    let mut opts = git2::PushOptions::new();
    let opts = match remote_callbacks {
        Some(cb) => opts.remote_callbacks(cb),
        None => &mut opts,
    };
    remote.push(&[reference], Some(opts))?;

    Ok(commit_id)
}
