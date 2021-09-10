// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_trailers::{parse as parse_trailers, Error as TrailerError, OwnedTrailer, Trailer};
use link_crypto::{BoxedSignError, BoxedSigner};
use link_identities::sign::{error::Signatures as SignaturesError, Signatures};

use thiserror::Error as ThisError;

use std::convert::TryFrom;

pub(super) struct ChangeMetadata {
    /// The commit where this change lives
    pub(super) commit: git2::Oid,
    /// The OID of the tree the commit points at, we need this to validate the
    /// signatures
    pub(super) revision: git2::Oid,
    /// The signatures of this change
    pub(super) signatures: Signatures,
    /// The OID of the parent commit of this change which points at the author
    /// identity
    pub(super) author_commit: git2::Oid,
    pub(super) trailers: Vec<OwnedTrailer>,
}

#[derive(Debug, ThisError)]
pub enum LoadError {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    Signatures(#[from] SignaturesError),
    #[error("non utf-8 characters in commit message")]
    Utf8,
    #[error(transparent)]
    Trailer(#[from] TrailerError),
    #[error(transparent)]
    AuthorTrailer(#[from] super::trailers::InvalidAuthorTrailer),
}

#[derive(Debug, ThisError)]
pub enum CreateError {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    Signer(#[from] BoxedSignError),
}

impl TryFrom<&git2::Commit<'_>> for ChangeMetadata {
    type Error = LoadError;

    fn try_from(commit: &git2::Commit) -> Result<Self, Self::Error> {
        let trailers = commit
            .message()
            .ok_or(LoadError::Utf8)
            .and_then(|s| parse_trailers(s, ":").map_err(|e| e.into()))?;
        let owned_trailers = trailers.iter().map(OwnedTrailer::from).collect();
        let author_commit_trailer = super::trailers::AuthorCommitTrailer::try_from(&trailers)?;
        let signatures = Signatures::try_from(trailers)?;
        Ok(ChangeMetadata {
            commit: commit.id(),
            revision: commit.tree_id(),
            signatures,
            author_commit: author_commit_trailer.oid(),
            trailers: owned_trailers,
        })
    }
}

impl ChangeMetadata {
    pub fn create(
        revision: git2::Oid,
        tips: Vec<git2::Oid>,
        message: String,
        extra_trailers: Vec<Trailer<'_>>,
        author_identity_commit: git2::Oid,
        signer: &BoxedSigner,
        repo: &git2::Repository,
    ) -> Result<ChangeMetadata, CreateError> {
        let owned_trailers = extra_trailers.iter().map(OwnedTrailer::from).collect();

        let author_commit = repo.find_commit(author_identity_commit)?;
        let tree = repo.find_tree(revision)?;

        let author = repo.signature()?;

        let signatures = link_identities::git::sign(signer, revision.into())?.into();
        let mut parent_commits = Vec::new();
        let tip_commits = tips
            .iter()
            .map(|o| repo.find_commit(*o))
            .collect::<Result<Vec<git2::Commit>, git2::Error>>()?;
        parent_commits.extend(tip_commits);
        parent_commits.push(author_commit.clone());

        let mut trailers = extra_trailers.clone();
        trailers.push(super::trailers::AuthorCommitTrailer::from(author_commit.id()).into());

        let commit = repo.commit(
            None,
            &author,
            &author,
            &link_identities::git::sign::CommitMessage::new(
                message.as_str(),
                &signatures,
                trailers,
            )
            .to_string(),
            &tree,
            &(parent_commits.iter().collect::<Vec<&git2::Commit>>())[..],
        )?;

        Ok(ChangeMetadata {
            revision,
            commit,
            author_commit: author_commit.id(),
            signatures,
            trailers: owned_trailers,
        })
    }
}
