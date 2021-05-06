// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
