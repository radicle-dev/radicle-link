// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! [`list`] all the [`Patch`]es for project.

use either::Either;
use librad::{
    git::types::{namespace::Namespace, Reference, RefsCategory},
    peer::PeerId,
    signer::BoxedSigner,
};
use radicle_git_ext::RefspecPattern;

use crate::{
    project::{self, peer},
    state,
    Person,
};

const TAG_PREFIX: &str = "radicle-patch/";

/// A patch is a change set that a user wants the maintainer to merge into a
/// projects default branch.
///
/// A patch is represented by an annotated tag, prefixed with `radicle-patch/`.
#[derive(Debug, Clone)]
pub struct Patch {
    /// ID of a patch. This is the portion of the tag name followin the
    /// `radicle-patch/` prefix.
    pub id: String,
    /// Peer that the patch originated from
    pub peer: project::Peer<peer::Status<Person>>,
    /// Message attached to the patch. This is the message of the annotated tag.
    pub message: Option<String>,
    /// Head commit that this
    pub commit: git2::Oid,
    /// The merge base of [`Patch::commit`] and the head commit of the first
    /// maintainers default branch.
    pub merge_base: Option<git2::Oid>,
}

impl Patch {
    /// Returns true if [`Patch::commit`] is contained in the base branch
    /// history. This is the case when [`Patch::commit`] is the same as
    /// [`Patch::merge_base`].
    #[must_use]
    pub fn merged(&self) -> bool {
        self.merge_base == Some(self.commit)
    }
}

/// List all patches for the given project.
///
/// # Errors
/// * Cannot access the monorepo
/// * Cannot find references within the monorepo
pub async fn list(
    peer: &crate::net::peer::Peer<BoxedSigner>,
    project_urn: crate::Urn,
) -> Result<Vec<Patch>, state::Error> {
    let mut patches = Vec::new();
    let monorepo_path = state::monorepo(peer);
    let monorepo = git2::Repository::open(monorepo_path)?;
    let namespace = Namespace::from(project_urn.clone());
    let default_branch_head_commit = {
        let project = state::get_project(peer, project_urn.clone())
            .await?
            .ok_or_else(|| state::Error::ProjectNotFound(project_urn.clone()))?;
        let maintainer = project
            .delegations()
            .iter()
            .flat_map(|either| match either {
                Either::Left(pk) => Either::Left(std::iter::once(PeerId::from(*pk))),
                Either::Right(indirect) => {
                    Either::Right(indirect.delegations().iter().map(|pk| PeerId::from(*pk)))
                },
            })
            .next()
            .expect("missing delegation");
        // the `remote` for `get_branch` is set to the first maintainer, if the current
        // `peer` is that maintainer, `get_branch` will catch that and search
        // the local peers directories. The `branch` is set to `None` as
        // `get_branch` will then fall back to the default branch.
        let default_branch =
            state::get_branch(peer, project_urn.clone(), Some(maintainer), None).await?;
        monorepo
            .find_reference(&default_branch.to_string())?
            .peel_to_commit()?
            .id()
    };

    for project_peer in state::list_project_peers(peer, project_urn.clone()).await? {
        let remote = match project_peer {
            project::Peer::Local { .. } => None,
            project::Peer::Remote { peer_id, .. } => Some(peer_id),
        };
        let ref_pattern = Reference {
            remote,
            category: RefsCategory::Tags,
            name: format!("{}*", TAG_PREFIX)
                .parse::<RefspecPattern>()
                .expect("invalid refspec pattern"),
            namespace: Some(namespace.clone()),
        };
        let refs = ref_pattern.references(&monorepo)?;
        for r in refs {
            let r = r?;
            match patch_from_ref(
                &monorepo,
                default_branch_head_commit,
                project_peer.clone(),
                &r,
            ) {
                Ok(patch) => patches.push(patch),
                Err(err) => {
                    log::warn!("failed to get patch from ref: {:?}", err);
                },
            }
        }
    }
    Ok(patches)
}

#[derive(thiserror::Error, Debug)]
enum PatchExtractError {
    #[error("cannot peel reference to tag")]
    PeelToTagFailed(#[source] git2::Error),
    #[error("failed to determine merge base")]
    FailedToDetermineMergeBase(#[source] git2::Error),
    #[error("tag target object is not a commit")]
    InvalidObjectType,
    #[error("tag name is not valid UTF-8")]
    InvalidName(#[source] std::str::Utf8Error),
}

fn patch_from_ref<'repo>(
    monorepo: &'repo git2::Repository,
    default_branch_head_commit: git2::Oid,
    peer: project::Peer<peer::Status<Person>>,
    reference: &git2::Reference<'repo>,
) -> Result<Patch, PatchExtractError> {
    let tag = reference
        .peel_to_tag()
        .map_err(PatchExtractError::PeelToTagFailed)?;
    let commit = tag.target_id();

    let merge_base = match monorepo.merge_base(commit, default_branch_head_commit) {
        Ok(merge_base) => Some(merge_base),
        Err(err) => {
            if err.code() == git2::ErrorCode::NotFound {
                None
            } else {
                return Err(PatchExtractError::FailedToDetermineMergeBase(err));
            }
        },
    };

    let tag_name = std::str::from_utf8(tag.name_bytes()).map_err(PatchExtractError::InvalidName)?;

    let id = tag_name
        .strip_prefix(TAG_PREFIX)
        // This can only fail if our ref pattern is wrong
        .expect("invalid prefix");

    if tag.target_type() != Some(git2::ObjectType::Commit) {
        return Err(PatchExtractError::InvalidObjectType);
    }

    Ok(Patch {
        id: id.to_owned(),
        peer,
        message: tag.message().map(String::from),
        commit,
        merge_base,
    })
}
