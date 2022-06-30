// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{trailers, EntryContents, HistoryType, TypeName};

use git_trailers::{parse as parse_trailers, OwnedTrailer};
use link_crypto::BoxedSigner;
use link_identities::sign::Signatures;

use std::{convert::TryFrom, fmt};

use serde::{Deserialize, Serialize};

/// A single change in the change graph. The layout of changes in the repository
/// is specified in the RFC (docs/rfc/0662-collaborative-objects.adoc)
/// under "Change Commits".
pub struct Change {
    /// The commit where this change lives
    commit: git2::Oid,
    /// The OID of the tree the commit points at, we need this to validate the
    /// signatures
    revision: git2::Oid,
    /// The signatures of this change
    signatures: Signatures,
    /// The OID of the parent commit of this change which points at the author
    /// identity
    author_commit: git2::Oid,
    /// The OID of the parent commit of this change which points at a schema.
    /// Schemas are no longer used but older implementations include a
    /// schema commit as a parent of the change and to stay backwards
    /// compatible we must exclude these commits when loading a change.
    schema_commit: Option<git2::Oid>,
    /// The OID of the parent commit which points at the identity this change
    /// was authorized with respect to at the time the change was authored.
    authorizing_identity_commit: git2::Oid,
    /// The manifest
    manifest: Manifest,
    /// The actual changes this change carries
    contents: EntryContents,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Change from commit {}", self.commit())
    }
}

pub mod error {
    use super::trailers;
    use git_trailers::Error as TrailerError;
    use link_crypto::BoxedSignError;
    use link_identities::sign::error::Signatures;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Create {
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Signer(#[from] BoxedSignError),
    }

    #[derive(Debug, Error)]
    pub enum Load {
        #[error(transparent)]
        Signatures(#[from] Signatures),
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error("No manifest found in commit")]
        NoManifest,
        #[error("Manifest tree entry was not a blog")]
        ManifestIsNotBlob,
        #[error("invalid manifest: {0:?}")]
        InvalidManifest(toml::de::Error),
        #[error("no ./change in commit tree")]
        NoChange,
        #[error("./change was not a blob")]
        ChangeNotBlob,
        #[error(transparent)]
        SchemaCommitTrailer(#[from] trailers::error::InvalidSchemaTrailer),
        #[error(transparent)]
        AuthorTrailer(#[from] trailers::error::InvalidAuthorTrailer),
        #[error(transparent)]
        AuthorizingIdentityTrailer(
            #[from] super::trailers::error::InvalidAuthorizingIdentityTrailer,
        ),
        #[error("non utf-8 characters in commit message")]
        Utf8,
        #[error(transparent)]
        Trailer(#[from] TrailerError),
    }
}

pub struct NewChangeSpec {
    pub(crate) typename: TypeName,
    pub(crate) tips: Option<Vec<git2::Oid>>,
    pub(crate) message: Option<String>,
    pub(crate) contents: EntryContents,
}

const MANIFEST_BLOB_NAME: &str = "manifest.toml";
const CHANGE_BLOB_NAME: &str = "change";

impl Change {
    /// Create a change in the git repo according to the spec
    pub fn create(
        authorizing_identity_commit_id: git2::Oid,
        author_identity_commit_id: git2::Oid,
        repo: &git2::Repository,
        signer: &BoxedSigner,
        spec: NewChangeSpec,
    ) -> Result<Change, error::Create> {
        let manifest = Manifest {
            typename: spec.typename,
            history_type: (&spec.contents).into(),
        };

        let mut tb = repo.treebuilder(None)?;
        // SAFETY: we're serializing to an in memory buffer so the only source of
        // errors here is a programming error, which we can't recover from
        let serialized_manifest = toml::ser::to_vec(&manifest).unwrap();
        let manifest_oid = repo.blob(&serialized_manifest)?;
        tb.insert(
            MANIFEST_BLOB_NAME,
            manifest_oid,
            git2::FileMode::Blob.into(),
        )?;

        let change_blob = repo.blob(spec.contents.as_ref())?;
        tb.insert(CHANGE_BLOB_NAME, change_blob, git2::FileMode::Blob.into())?;

        let revision = tb.write()?;
        let tree = repo.find_tree(revision)?;

        let author_commit = repo.find_commit(author_identity_commit_id)?;
        let author = repo.signature()?;

        let authorizing_identity_commit = repo.find_commit(authorizing_identity_commit_id)?;

        let signatures = link_identities::git::sign(signer, revision.into())?.into();
        let mut parent_commits = spec
            .tips
            .iter()
            .flat_map(|cs| cs.iter())
            .map(|o| repo.find_commit(*o))
            .collect::<Result<Vec<git2::Commit>, git2::Error>>()?;
        parent_commits.push(authorizing_identity_commit);
        parent_commits.push(author_commit);

        let trailers = vec![
            super::trailers::AuthorCommitTrailer::from(author_identity_commit_id).into(),
            super::trailers::AuthorizingIdentityCommitTrailer::from(authorizing_identity_commit_id)
                .into(),
        ];

        let commit = repo.commit(
            None,
            &author,
            &author,
            &link_identities::git::sign::CommitMessage::new(
                spec.message
                    .unwrap_or_else(|| "new change".to_string())
                    .as_str(),
                &signatures,
                trailers,
            )
            .to_string(),
            &tree,
            &(parent_commits.iter().collect::<Vec<&git2::Commit>>())[..],
        )?;

        Ok(Change {
            schema_commit: None,
            manifest,
            contents: spec.contents,
            commit,
            signatures,
            authorizing_identity_commit: authorizing_identity_commit_id,
            author_commit: author_identity_commit_id,
            revision,
        })
    }

    /// Load a change from the given commit
    pub fn load(repo: &git2::Repository, commit: &git2::Commit) -> Result<Change, error::Load> {
        let trailers = commit
            .message()
            .ok_or(error::Load::Utf8)
            .and_then(|s| parse_trailers(s, ":").map_err(|e| e.into()))?;
        let owned_trailers: Vec<OwnedTrailer> = trailers.iter().map(OwnedTrailer::from).collect();
        let author_commit_trailer =
            super::trailers::AuthorCommitTrailer::try_from(&owned_trailers[..])?;
        let authorizing_identity_trailer =
            super::trailers::AuthorizingIdentityCommitTrailer::try_from(&owned_trailers[..])?;

        // We no longer support schema parents but to remain backwards compatible we
        // still load the commit trailer so we know to omit the schema parent
        // commits when evaluating old object histories which still have a
        // schema parent commit
        let schema_commit_trailer =
            match super::trailers::SchemaCommitTrailer::try_from(&owned_trailers[..]) {
                Ok(t) => Some(t),
                Err(super::trailers::error::InvalidSchemaTrailer::NoTrailer) => None,
                Err(e) => return Err(e.into()),
            };
        let signatures = Signatures::try_from(trailers)?;

        let tree = commit.tree()?;
        let manifest_tree_entry = tree
            .get_name(MANIFEST_BLOB_NAME)
            .ok_or(error::Load::NoManifest)?;
        let manifest_object = manifest_tree_entry.to_object(repo)?;
        let manifest_blob = manifest_object
            .as_blob()
            .ok_or(error::Load::ManifestIsNotBlob)?;
        let manifest: Manifest =
            toml::de::from_slice(manifest_blob.content()).map_err(error::Load::InvalidManifest)?;

        let contents = match manifest.history_type {
            HistoryType::Automerge => {
                let contents_tree_entry = tree
                    .get_name(CHANGE_BLOB_NAME)
                    .ok_or(error::Load::NoChange)?;
                let contents_object = contents_tree_entry.to_object(repo)?;
                let contents_blob = contents_object
                    .as_blob()
                    .ok_or(error::Load::ChangeNotBlob)?;
                EntryContents::Automerge(contents_blob.content().into())
            },
        };

        Ok(Change {
            manifest,
            contents,
            commit: commit.id(),
            schema_commit: schema_commit_trailer.map(|s| s.oid()),
            author_commit: author_commit_trailer.oid(),
            authorizing_identity_commit: authorizing_identity_trailer.oid(),
            signatures,
            revision: tree.id(),
        })
    }

    pub fn commit(&self) -> &git2::Oid {
        &self.commit
    }

    pub fn author_commit(&self) -> git2::Oid {
        self.author_commit
    }

    pub fn typename(&self) -> &TypeName {
        &self.manifest.typename
    }

    pub fn contents(&self) -> &EntryContents {
        &self.contents
    }

    pub fn schema_commit(&self) -> Option<git2::Oid> {
        self.schema_commit
    }

    pub fn authorizing_identity_commit(&self) -> git2::Oid {
        self.authorizing_identity_commit
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

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    typename: TypeName,
    history_type: HistoryType,
}
