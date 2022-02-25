// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_ext::{is_not_found_err, reference};
use std_ext::result::ResultExt as _;

use super::{
    refs::{self, Refs},
    types::Namespace,
};

pub use super::{Storage, Urn};
pub use git_ext::Tree;

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum QuickCommit {
        #[error("failed to update refs signature")]
        Sigrefs(#[from] refs::stored::Error),

        #[error(transparent)]
        Git(#[from] git2::Error),
    }
}

/// Quickly create a commit in the namespace and and on top of the branch
/// described by [`Urn`], operating directly on [`Storage`].
///
/// This function is provided mainly for integration testing purposes, where the
/// ceremony of a working copy is often undesired.
///
/// Note that the given [`Urn`]'s `path` will be sanitised such that it points
/// to the locally-owned `refs/heads`. That is, `refs/remotes/xx/heads/pu` would
/// be interpreted as `refs/namespaces/<urn>/refs/heads/xx/heads/pu`.
///
/// In other words, you can only use this function to commit to your own
/// branches, but if you try to circumvent this restriction, the result may be
/// surprising.
///
/// The default branch if the [`Urn`] contains no `path` is "master".
#[tracing::instrument(
    level = "debug",
    skip(storage, urn, tree, message),
    fields(urn = %urn),
    err
)]
pub fn quick_commit(
    storage: &Storage,
    urn: &Urn,
    tree: Tree,
    message: &str,
) -> Result<git2::Oid, error::QuickCommit> {
    let repo = storage.as_raw();

    let author = repo.signature()?;
    let branch = {
        let path =
            reference::OneLevel::from(urn.path.clone().unwrap_or_else(|| reflike!("master")));
        reflike!("refs/namespaces")
            .join(Namespace::from(urn))
            .join(path.into_qualified(reflike!("heads")))
    };
    let parent = repo
        .find_reference(branch.as_str())
        .and_then(|ref_| ref_.peel_to_commit())
        .map(Some)
        .or_matches::<git2::Error, _, _>(is_not_found_err, || Ok(None))?;
    let tree = {
        let oid = tree.write(repo)?;
        repo.find_tree(oid)?
    };

    let oid = repo.commit(
        Some(branch.as_str()),
        &author,
        &author,
        message,
        &tree,
        &parent.as_ref().into_iter().collect::<Vec<_>>(),
    )?;
    tracing::debug!(oid = %oid, branch = %branch.as_str(), "quick commit created");

    Refs::update(storage, urn)?;

    Ok(oid)
}
