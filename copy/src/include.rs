// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom as _, path::PathBuf};

use librad::{
    git::{
        identities::{self, Project},
        include::{self, Include},
        local::url::LocalUrl,
        storage::Storage,
    },
    git_ext,
    identities::relations,
    paths::Paths,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Identities(#[from] identities::Error),

    #[error(transparent)]
    Include(#[from] include::Error),

    #[error(transparent)]
    Ref(#[from] git_ext::name::Error),

    #[error(transparent)]
    Relations(#[from] identities::relations::Error),
}

/// Update the include file for the given `project`.
///
/// It looks at the tracked peers of the `project` and creates an entry for each
/// one in an include file. The file can be located by using
/// [`Paths::git_includes_dir`], and the name of the file will be the `Urn`.
pub fn update(storage: &Storage, paths: &Paths, project: &Project) -> Result<PathBuf, Error> {
    let urn = project.urn();
    let url = LocalUrl::from(urn.clone());
    let tracked = identities::relations::tracked(storage, &urn)?;
    let include = Include::from_tracked_persons(
        paths.git_includes_dir().to_path_buf(),
        url,
        tracked
            .into_iter()
            .filter_map(|peer| {
                relations::Peer::replicated_remote(peer).map(|(p, u)| {
                    git_ext::RefLike::try_from(u.subject().name.to_string()).map(|r| (r, p))
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let path = include.file_path();
    include.save()?;

    tracing::info!("updated include file @ '{}'", path.display());
    Ok(path)
}
