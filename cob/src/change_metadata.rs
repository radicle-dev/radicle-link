// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_trailers::{parse as parse_trailers, Error as TrailerError, OwnedTrailer, Trailer};
use link_crypto::{BoxedSignError, BoxedSigner};
use link_identities::sign::{error::Signatures as SignaturesError, Signatures};

use thiserror::Error as ThisError;

use std::convert::TryFrom;

/// We represent both changes to a collaborative object and changes to the
/// objects schema as commits. `ChangeMetadata` captures the metadata which is
/// common to both object changes and schema changes
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
    /// The OID of the parent commit which points at the identity this change
    /// was authorized with respect to at the time the change was authored.
    pub(super) authorizing_identity_commit: git2::Oid,
    /// The trailers of the commit. We need to hold on to these so more specific
    /// processing can be performed by specific change types. E.g. the
    /// parsing of the `SchemaCommitTrailer` in `Change::load`
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
    AuthorTrailer(#[from] super::trailers::error::InvalidAuthorTrailer),
    #[error(transparent)]
    AuthorizingIdentityTrailer(#[from] super::trailers::error::InvalidAuthorizingIdentityTrailer),
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
        let owned_trailers: Vec<OwnedTrailer> = trailers.iter().map(OwnedTrailer::from).collect();
        let author_commit_trailer =
            super::trailers::AuthorCommitTrailer::try_from(&owned_trailers[..])?;
        let authorizing_identity_trailer =
            super::trailers::AuthorizingIdentityCommitTrailer::try_from(&owned_trailers[..])?;
        let signatures = Signatures::try_from(trailers)?;
        Ok(ChangeMetadata {
            commit: commit.id(),
            revision: commit.tree_id(),
            signatures,
            author_commit: author_commit_trailer.oid(),
            authorizing_identity_commit: authorizing_identity_trailer.oid(),
            trailers: owned_trailers,
        })
    }
}

pub struct CreateMetadataArgs<'a> {
    pub revision: git2::Oid,
    pub tips: Vec<git2::Oid>,
    pub message: String,
    pub extra_trailers: Vec<Trailer<'a>>,
    pub authorizing_identity_commit: git2::Oid,
    pub author_identity_commit: git2::Oid,
    pub signer: BoxedSigner,
    pub repo: &'a git2::Repository,
}

impl ChangeMetadata {
    /// Create a commit in the underlying repository and return the
    /// corresponding metadata
    pub fn create(
        CreateMetadataArgs {
            revision,
            tips,
            message,
            extra_trailers,
            authorizing_identity_commit,
            author_identity_commit,
            signer,
            repo,
        }: CreateMetadataArgs<'_>,
    ) -> Result<ChangeMetadata, CreateError> {
        let owned_trailers = extra_trailers.iter().map(OwnedTrailer::from).collect();

        let author_commit = repo.find_commit(author_identity_commit)?;
        let tree = repo.find_tree(revision)?;

        let author = repo.signature()?;

        let signatures = link_identities::git::sign(&signer, revision.into())?.into();
        let mut parent_commits = Vec::new();
        let tip_commits = tips
            .iter()
            .map(|o| repo.find_commit(*o))
            .collect::<Result<Vec<git2::Commit>, git2::Error>>()?;
        parent_commits.extend(tip_commits);
        parent_commits.push(author_commit.clone());

        let mut trailers = extra_trailers.clone();
        trailers.push(super::trailers::AuthorCommitTrailer::from(author_commit.id()).into());
        trailers.push(
            super::trailers::AuthorizingIdentityCommitTrailer::from(authorizing_identity_commit)
                .into(),
        );

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
            authorizing_identity_commit,
            signatures,
            trailers: owned_trailers,
        })
    }

    pub fn valid_signatures(&self) -> bool {
        for (key, sig) in self.signatures.iter() {
            if !key.verify(sig, self.revision.as_bytes()) {
                return false;
            }
        }
        true
    }
}
