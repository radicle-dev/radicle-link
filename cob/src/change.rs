// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{change_metadata, trailers, History, TypeName};

use link_crypto::BoxedSigner;
use link_identities::sign::Signatures;

use std::{convert::TryFrom, fmt};

use serde::{Deserialize, Serialize};

/// A single change in the change graph.
pub struct Change {
    /// The commit where this change lives
    commit: git2::Oid,
    /// The OID of the tree the commit points at, we need this to validate the
    /// signatures
    revision: git2::Oid,
    /// The OID of the parent commit of this change which points at the author
    /// identity
    author_commit: git2::Oid,
    /// The OID of the parent commit which points at the schema_commit
    schema_commit: git2::Oid,
    /// The manifest
    manifest: Manifest,
    /// The actual changes this change carries
    history: History,
    /// The signatures of this change
    signatures: Signatures,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Change from commit {}", self.commit)
    }
}

pub mod error {
    use super::{change_metadata, trailers};
    use link_crypto::BoxedSignError;
    use link_identities::git::error::Signatures;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Create {
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Signer(#[from] BoxedSignError),
        #[error(transparent)]
        Metadata(#[from] change_metadata::CreateError),
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
        InvalidMetadata(#[from] change_metadata::LoadError),
        #[error(transparent)]
        SchemaCommitTrailer(#[from] trailers::error::InvalidSchemaTrailer),
    }
}

pub struct NewChangeSpec {
    pub(crate) schema_commit: git2::Oid,
    pub(crate) typename: TypeName,
    pub(crate) tips: Option<Vec<git2::Oid>>,
    pub(crate) message: Option<String>,
    pub(crate) history: History,
}

impl Change {
    /// Create a change in the git repo according to the spec
    pub fn create(
        author_identity_commit: git2::Oid,
        repo: &git2::Repository,
        signer: &BoxedSigner,
        spec: NewChangeSpec,
    ) -> Result<Change, error::Create> {
        let manifest = Manifest {
            typename: spec.typename,
            history_type: HistoryType::from_history(&spec.history),
        };

        let mut tb = repo.treebuilder(None)?;
        // SAFETY: we're serializing to an in memory buffer so the only source of
        // errors here is a programming error, which we can't recover from
        let serialized_manifest = toml::ser::to_vec(&manifest).unwrap();
        let manifest_oid = repo.blob(&serialized_manifest)?;
        tb.insert("manifest.toml", manifest_oid, git2::FileMode::Blob.into())?;

        let change_blob = repo.blob(spec.history.as_bytes())?;
        tb.insert("change", change_blob, git2::FileMode::Blob.into())?;

        let revision = tb.write()?;

        let schema_trailer = trailers::SchemaCommitTrailer::from(spec.schema_commit).into();

        let mut tips = spec.tips.clone().unwrap_or_else(Vec::new);
        tips.push(spec.schema_commit);

        let metadata = change_metadata::ChangeMetadata::create(
            revision,
            tips,
            spec.message.unwrap_or_else(|| "new change".to_string()),
            vec![schema_trailer],
            author_identity_commit,
            signer,
            repo,
        )?;

        Ok(Change {
            revision,
            schema_commit: spec.schema_commit,
            commit: metadata.commit,
            manifest,
            history: spec.history,
            author_commit: metadata.author_commit,
            signatures: metadata.signatures,
        })
    }

    /// Load a change from the given commit
    pub fn load(repo: &git2::Repository, commit: &git2::Commit) -> Result<Change, error::Load> {
        let metadata = change_metadata::ChangeMetadata::try_from(commit)?;

        let tree = commit.tree()?;
        let manifest_tree_entry = tree
            .get_name("manifest.toml")
            .ok_or(error::Load::NoManifest)?;
        let manifest_object = manifest_tree_entry.to_object(repo)?;
        let manifest_blob = manifest_object
            .as_blob()
            .ok_or(error::Load::ManifestIsNotBlob)?;
        let manifest: Manifest =
            toml::de::from_slice(manifest_blob.content()).map_err(error::Load::InvalidManifest)?;

        let history = match manifest.history_type {
            HistoryType::Automerge => {
                let history_tree_entry = tree.get_name("change").ok_or(error::Load::NoChange)?;
                let history_object = history_tree_entry.to_object(repo)?;
                let history_blob = history_object.as_blob().ok_or(error::Load::ChangeNotBlob)?;
                History::Automerge(history_blob.content().into())
            },
        };

        let schema_commit_trailer = trailers::SchemaCommitTrailer::try_from(metadata.trailers)?;

        Ok(Change {
            schema_commit: schema_commit_trailer.oid(),
            revision: commit.tree_id(),
            commit: commit.id(),
            manifest,
            history,
            author_commit: metadata.author_commit,
            signatures: metadata.signatures,
        })
    }

    pub fn commit(&self) -> &git2::Oid {
        &self.commit
    }

    pub fn author_commit(&self) -> &git2::Oid {
        &self.author_commit
    }

    pub fn typename(&self) -> &TypeName {
        &self.manifest.typename
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn schema_commit(&self) -> &git2::Oid {
        &self.schema_commit
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
pub enum HistoryType {
    Automerge,
}

impl HistoryType {
    fn from_history(history: &History) -> HistoryType {
        match history {
            History::Automerge(..) => HistoryType::Automerge,
        }
    }
}

impl fmt::Display for HistoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistoryType::Automerge => f.write_str("automerge"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    typename: TypeName,
    history_type: HistoryType,
}
