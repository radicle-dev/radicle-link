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

use librad::git_ext::reference::RefLike;

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
