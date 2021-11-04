// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use super::{
    change_metadata::{self, ChangeMetadata, CreateMetadataArgs},
    Schema,
};

use link_crypto::BoxedSigner;

pub mod error {
    use super::{super::schema::error as schema_error, change_metadata};
    use thiserror::Error as ThisError;

    #[derive(Debug, ThisError)]
    pub enum Create {
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Commit(#[from] change_metadata::CreateError),
    }

    #[derive(Debug, ThisError)]
    pub enum Load {
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Metadata(#[from] change_metadata::LoadError),
        #[error("no schema.json in commit tree")]
        NoSchemaJson,
        #[error("schema.json was not a blob")]
        SchemaNotBlob,
        #[error("invalid schema in schema.json: {0}")]
        InvalidSchema(#[from] schema_error::Parse),
    }
}

pub(super) struct SchemaChange {
    metadata: ChangeMetadata,
    schema: Schema,
}

const SCHEMA_BLOB_NAME: &str = "schema.json";

impl SchemaChange {
    pub fn create(
        authorizing_identity_commit: git2::Oid,
        author_identity_commit: git2::Oid,
        repo: &git2::Repository,
        signer: &BoxedSigner,
        schema: Schema,
    ) -> Result<SchemaChange, error::Create> {
        let mut tb = repo.treebuilder(None)?;
        let schema_oid = repo.blob(&schema.json_bytes())?;
        tb.insert(SCHEMA_BLOB_NAME, schema_oid, git2::FileMode::Blob.into())?;

        let revision = tb.write()?;

        let metadata = ChangeMetadata::create(CreateMetadataArgs {
            revision,
            tips: Vec::new(),
            message: "create schema".to_string(),
            extra_trailers: Vec::new(),
            authorizing_identity_commit,
            author_identity_commit,
            signer: signer.clone(),
            repo,
        })?;

        Ok(SchemaChange { metadata, schema })
    }

    pub fn load(
        commit_id: git2::Oid,
        repo: &git2::Repository,
    ) -> Result<SchemaChange, error::Load> {
        let commit = repo.find_commit(commit_id)?;
        let metadata = change_metadata::ChangeMetadata::try_from(&commit)?;
        let tree = repo.find_tree(metadata.revision)?;

        let schema_tree_entry = tree
            .get_name(SCHEMA_BLOB_NAME)
            .ok_or(error::Load::NoSchemaJson)?;
        let schema_object = schema_tree_entry.to_object(repo)?;
        let schema_blob = schema_object.as_blob().ok_or(error::Load::SchemaNotBlob)?;
        let schema = Schema::try_from(schema_blob.content())?;

        Ok(SchemaChange { metadata, schema })
    }

    pub fn commit(&self) -> git2::Oid {
        self.metadata.commit
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn valid_signatures(&self) -> bool {
        self.metadata.valid_signatures()
    }
}
