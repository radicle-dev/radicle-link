// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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
