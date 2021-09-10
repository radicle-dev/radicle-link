// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashSet, convert::TryFrom, fmt, str::FromStr};

use either::Either;
use serde::{Deserialize, Serialize};

use link_crypto::BoxedSigner;
use link_identities::git::{Person, Project, Urn};

mod change_metadata;
mod trailers;

mod change_graph;
use change_graph::ChangeGraph;

mod schema;
use schema::Schema;

mod change;
use change::Change;

mod identity_cache;
use identity_cache::IdentityCache;

mod schema_change;
use schema_change::SchemaChange;

mod refs_storage;
pub use refs_storage::RefsStorage;

#[derive(Clone, Debug)]
pub enum History {
    Automerge(Vec<u8>),
}

impl History {
    fn as_bytes(&self) -> &[u8] {
        match self {
            History::Automerge(h) => h,
        }
    }
}

pub struct NewObjectSpec {
    pub schema_json: serde_json::Value,
    pub history: History,
    pub typename: String,
    pub message: Option<String>,
}

impl NewObjectSpec {
    fn typename(&self) -> TypeName {
        TypeName(self.typename.clone())
    }

    fn change_spec(&self, schema_commit: git2::Oid) -> change::NewChangeSpec {
        change::NewChangeSpec {
            schema_commit,
            typename: self.typename(),
            tips: None,
            message: self.message.clone(),
            history: self.history.clone(),
        }
    }
}

pub struct UpdateObjectSpec {
    pub object_id: ObjectId,
    pub typename: TypeName,
    pub message: Option<String>,
    pub changes: History,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeName(String);

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl FromStr for TypeName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(TypeName(s.to_string()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(git2::Oid);

impl FromStr for ObjectId {
    type Err = error::ParseObjectId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        git2::Oid::from_str(s)
            .map(ObjectId)
            .map_err(error::ParseObjectId::from)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for ObjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_string().as_str())
    }
}

impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        ObjectId::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

impl From<&git2::Oid> for ObjectId {
    fn from(oid: &git2::Oid) -> Self {
        ObjectId(*oid)
    }
}

#[derive(Debug, Clone)]
pub struct CollaborativeObject {
    containing_identity: Either<Person, Project>,
    typename: TypeName,
    history: History,
    id: ObjectId,
    schema: Schema,
}

impl CollaborativeObject {
    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn id(&self) -> &ObjectId {
        &self.id
    }

    pub fn typename(&self) -> &TypeName {
        &self.typename
    }
}

pub mod error {
    use super::{change, change_graph::Error as ChangeGraphError, schema, schema_change};
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Create<RefsError: std::error::Error> {
        #[error("Invalid automerge history")]
        InvalidAutomergeHistory,
        #[error(transparent)]
        CreateSchemaChange(#[from] schema_change::error::Create),
        #[error(transparent)]
        CreateChange(#[from] change::error::Create),
        #[error("invalid schema: {0}")]
        InvalidSchema(#[from] schema::error::Parse),
        #[error(transparent)]
        Refs(RefsError),
    }

    #[derive(Debug, Error)]
    pub enum Retrieve<RefsError: std::error::Error> {
        #[error(transparent)]
        ChangeGraph(#[from] ChangeGraphError<RefsError>),
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Refs(RefsError),
    }

    #[derive(Debug, Error)]
    pub enum Update<RefsError: std::error::Error> {
        #[error(transparent)]
        ChangeGraph(#[from] ChangeGraphError<RefsError>),
        #[error("no object found")]
        NoSuchObject,
        #[error(transparent)]
        CreateChange(#[from] change::error::Create),
        #[error(transparent)]
        Refs(RefsError),
    }

    #[derive(Debug, Error)]
    pub enum ParseObjectId {
        #[error(transparent)]
        Git2(#[from] git2::Error),
    }
}

/// Create a collaboratibe object
///
/// The `within_identity` argument specifies the identity this collaborative
/// object will be referenced under.
pub fn create_object<R: RefsStorage>(
    refs_storage: &R,
    repo: &git2::Repository,
    signer: &BoxedSigner,
    author_identity_commit: git2::Oid,
    within_identity: Either<Person, Project>,
    spec: NewObjectSpec,
) -> Result<CollaborativeObject, error::Create<R::Error>> {
    let schema = Schema::try_from(&spec.schema_json)?;
    let schema_change =
        schema_change::SchemaChange::create(author_identity_commit, repo, signer, schema.clone())?;

    let init_change = change::Change::create(
        author_identity_commit,
        repo,
        signer,
        spec.change_spec(schema_change.commit()),
    )
    .map_err(error::Create::from)?;
    let object_id = init_change.commit().into();
    refs_storage
        .update_ref(
            &identity_urn(&within_identity),
            &spec.typename(),
            object_id,
            *(init_change.commit()),
        )
        .map_err(error::Create::Refs)?;
    Ok(CollaborativeObject {
        containing_identity: within_identity,
        typename: spec.typename(),
        history: spec.history,
        schema,
        id: init_change.commit().into(),
    })
}

pub fn retrieve_object<R: RefsStorage>(
    refs_storage: &R,
    repo: &git2::Repository,
    within_identity: Either<Person, Project>,
    typename: &TypeName,
    oid: &ObjectId,
) -> Result<Option<CollaborativeObject>, error::Retrieve<R::Error>> {
    if let Some(graph) = ChangeGraph::load(refs_storage, repo, &within_identity, typename, oid)? {
        let mut identities = IdentityCache::new(repo);
        Ok(Some(graph.evaluate(&mut identities)))
    } else {
        Ok(None)
    }
}

pub fn retrieve_objects<R: RefsStorage>(
    refs_storage: &R,
    repo: &git2::Repository,
    within_identity: Either<Person, Project>,
    typename: &TypeName,
) -> Result<Vec<CollaborativeObject>, error::Retrieve<R::Error>> {
    let oids: HashSet<ObjectId> = refs_storage
        .type_references(&identity_urn(&within_identity), typename)
        .map_err(error::Retrieve::Refs)?
        .into_iter()
        .map(|i| i.0)
        .collect();
    let mut result = Vec::new();
    for oid in oids {
        let mut identities = IdentityCache::new(repo);
        if let Some(object) =
            ChangeGraph::load(refs_storage, repo, &within_identity, typename, &oid)?
                .map(|g| g.evaluate(&mut identities))
        {
            result.push(object);
        }
    }
    Ok(result)
}

pub fn update_object<R: RefsStorage>(
    refs_storage: &R,
    signer: &BoxedSigner,
    repo: &git2::Repository,
    author_identity_commit: git2::Oid,
    within_identity: Either<Person, Project>,
    spec: UpdateObjectSpec,
) -> Result<CollaborativeObject, error::Update<R::Error>> {
    let mut identities = IdentityCache::new(repo);
    if let Some(mut graph) = ChangeGraph::load(
        refs_storage,
        repo,
        &within_identity,
        &spec.typename,
        &spec.object_id,
    )? {
        let object = graph.evaluate(&mut identities);
        let change = change::Change::create(
            author_identity_commit,
            repo,
            signer,
            change::NewChangeSpec {
                tips: Some(graph.tips()),
                schema_commit: graph.schema_commit(),
                history: spec.changes,
                typename: object.typename,
                message: spec.message,
            },
        )?;
        let new_commit = *change.commit();
        graph.extend(change);
        refs_storage
            .update_ref(
                &identity_urn(&within_identity),
                &spec.typename,
                spec.object_id,
                new_commit,
            )
            .map_err(error::Update::Refs)?;
        Ok(graph.evaluate(&mut identities))
    } else {
        Err(error::Update::NoSuchObject)
    }
}

pub fn changegraph_dotviz_for_object<R: RefsStorage>(
    refs_storage: &R,
    repo: &git2::Repository,
    within_identity: Either<Person, Project>,
    typename: &TypeName,
    oid: &ObjectId,
) -> Result<Option<String>, error::Retrieve<R::Error>> {
    let graph = ChangeGraph::load(refs_storage, repo, &within_identity, typename, oid)?;
    Ok(graph.map(|g| g.graphviz()))
}

fn identity_urn(id: &Either<Person, Project>) -> Urn {
    id.clone()
        .map_left(|i| i.urn())
        .map_right(|i| i.urn())
        .into_inner()
}
