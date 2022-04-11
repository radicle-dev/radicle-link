// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{
    schema_change,
    AuthorizingIdentity,
    Change,
    CollaborativeObject,
    IdentityStorage,
    ObjectId,
    Schema,
    SchemaChange,
    TypeName,
};
use link_identities::git::Urn;
use petgraph::{
    visit::{EdgeRef, Topo, Walker},
    EdgeDirection,
};
use thiserror::Error as ThisError;

use std::{
    collections::{hash_map::Entry, BTreeSet, HashMap},
    convert::TryInto,
};

mod evaluation;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("missing commit: {0}")]
    MissingRevision(git2::Oid),
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    LoadSchema(#[from] schema_change::error::Load),
    #[error("schema change is authorized by an incorrect identity URN, expected {expected} but was {actual}")]
    SchemaAuthorizingUrnIncorrect { expected: Urn, actual: Urn },
    #[error("no authorizing identity found for schema change")]
    NoSchemaAuthorizingIdentityFound,
    #[error("invalid signature on schema change")]
    InvalidSchemaSignatures,
}

/// The graph of changes for a particular collaborative object
pub(super) struct ChangeGraph<'a> {
    repo: &'a git2::Repository,
    object_id: ObjectId,
    authorizing_identity: &'a dyn AuthorizingIdentity,
    graph: petgraph::Graph<Change, ()>,
    schema_change: SchemaChange,
}

impl<'a> ChangeGraph<'a> {
    /// Load the change graph from the underlying git store by walking
    /// backwards from references to the object
    #[tracing::instrument(skip(repo, tip_refs, authorizing_identity))]
    pub(super) fn load<'b, 'c>(
        tip_refs: impl Iterator<Item = &'b git2::Reference<'b>>,
        repo: &'c git2::Repository,
        authorizing_identity: &'c dyn AuthorizingIdentity,
        typename: &TypeName,
        oid: &ObjectId,
    ) -> Result<Option<ChangeGraph<'c>>, Error> {
        let mut builder = GraphBuilder::default();
        let mut edges_to_process: Vec<(git2::Commit, git2::Oid)> = Vec::new();
        let tip_refs: Vec<&git2::Reference<'_>> = tip_refs.collect();
        let ref_names: Vec<&str> = tip_refs.iter().filter_map(|r| r.name()).collect();
        tracing::trace!(refs=?ref_names, "loading object from references");

        // Populate the initial set of edges_to_process from the refs we have
        for reference in tip_refs {
            let commit = reference.peel_to_commit()?;
            match Change::load(repo, &commit) {
                Ok(change) => {
                    let new_edges = builder.add_change(commit, change);
                    edges_to_process.extend(new_edges);
                },
                Err(e) => {
                    tracing::warn!(err=?e, commit=?commit.id(), reference=?reference.name(), "unable to load change from reference");
                },
            }
        }

        // Process edges until we have no more to process
        while let Some((parent_commit, child_commit_id)) = edges_to_process.pop() {
            tracing::trace!(?parent_commit, ?child_commit_id, "loading change");
            match Change::load(repo, &parent_commit) {
                Ok(change) => {
                    let parent_commit_id = parent_commit.id();
                    let new_edges = builder.add_change(parent_commit, change);
                    builder.add_edge(child_commit_id, parent_commit_id);
                    edges_to_process.extend(new_edges);
                },
                Err(e) => {
                    tracing::warn!(err=?e, commit=?parent_commit.id(), "unable to load changetree from commit");
                },
            }
        }
        builder.build(repo, *oid, authorizing_identity)
    }

    /// Given a graph evaluate it to produce a collaborative object. This will
    /// filter out branches of the graph which do not have valid signatures,
    /// or which do not have permission to make a change, or which make a
    /// change which invalidates the schema of the object
    pub(super) fn evaluate<I: IdentityStorage>(&self, identities: &I) -> CollaborativeObject {
        let mut roots: Vec<petgraph::graph::NodeIndex<u32>> = self
            .graph
            .externals(petgraph::Direction::Incoming)
            .collect();
        roots.sort();
        // This is okay because we check that the graph has a root node in
        // GraphBuilder::build
        let root = roots.first().unwrap();
        let typename = {
            let first_node = &self.graph[*root];
            first_node.typename().clone()
        };
        let evaluating = evaluation::Evaluating::new(
            identities,
            self.authorizing_identity,
            self.repo,
            self.schema().clone(),
        );
        let topo = Topo::new(&self.graph);
        let items = topo.iter(&self.graph).map(|idx| {
            let node = &self.graph[idx];
            let outgoing_edges = self.graph.edges_directed(idx, EdgeDirection::Outgoing);
            let child_commits: Vec<git2::Oid> = outgoing_edges
                .map(|e| *self.graph[e.target()].commit())
                .collect();
            (node, child_commits)
        });
        let history = {
            let root_change = &self.graph[*root];
            evaluating.evaluate(*root_change.commit(), items)
        };
        CollaborativeObject {
            authorizing_identity_urn: self.authorizing_identity.urn(),
            typename,
            history,
            id: self.object_id,
            schema: self.schema_change.schema().clone(),
        }
    }

    /// Get the tips of the collaborative object
    pub(super) fn tips(&self) -> BTreeSet<git2::Oid> {
        self.graph
            .externals(petgraph::Direction::Outgoing)
            .map(|n| {
                let change = &self.graph[n];
                *change.commit()
            })
            .collect()
    }

    pub(super) fn number_of_nodes(&self) -> u64 {
        self.graph.node_count().try_into().unwrap()
    }

    pub(super) fn graphviz(&self) -> String {
        let for_display = self.graph.map(|_ix, n| n.to_string(), |_ix, _e| "");
        petgraph::dot::Dot::new(&for_display).to_string()
    }

    pub(super) fn schema_commit(&self) -> git2::Oid {
        self.schema_change.commit()
    }

    pub(super) fn schema(&self) -> &Schema {
        self.schema_change.schema()
    }
}

struct GraphBuilder {
    node_indices: HashMap<git2::Oid, petgraph::graph::NodeIndex<u32>>,
    graph: petgraph::Graph<Change, ()>,
}

impl Default for GraphBuilder {
    fn default() -> Self {
        GraphBuilder {
            node_indices: HashMap::new(),
            graph: petgraph::graph::Graph::new(),
        }
    }
}

impl GraphBuilder {
    /// Add a change to the graph which we are building up, returning any edges
    /// corresponding to the parents of this node in the change graph
    fn add_change<'a>(
        &mut self,
        commit: git2::Commit<'a>,
        change: Change,
    ) -> Vec<(git2::Commit<'a>, git2::Oid)> {
        let author_commit = change.author_commit();
        let schema_commit = change.schema_commit();
        let authorizing_identity_commit = change.authorizing_identity_commit();
        if let Entry::Vacant(e) = self.node_indices.entry(commit.id()) {
            let ix = self.graph.add_node(change);
            e.insert(ix);
        }
        commit
            .parents()
            .filter_map(|parent| {
                if parent.id() != author_commit
                    && parent.id() != schema_commit
                    && parent.id() != authorizing_identity_commit
                    && !self.has_edge(parent.id(), commit.id())
                {
                    Some((parent, commit.id()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_edge(&mut self, parent_id: git2::Oid, child_id: git2::Oid) -> bool {
        let parent_ix = self.node_indices.get(&parent_id);
        let child_ix = self.node_indices.get(&child_id);
        match (parent_ix, child_ix) {
            (Some(parent_ix), Some(child_ix)) => self.graph.contains_edge(*parent_ix, *child_ix),
            _ => false,
        }
    }

    fn add_edge(&mut self, child: git2::Oid, parent: git2::Oid) {
        // This panics if the child or parent ids are not in the graph already
        let child_id = self.node_indices.get(&child).unwrap();
        let parent_id = self.node_indices.get(&parent).unwrap();
        self.graph.update_edge(*parent_id, *child_id, ());
    }

    fn build<'b>(
        self,
        repo: &'b git2::Repository,
        object_id: ObjectId,
        authorizing_identity: &'b dyn AuthorizingIdentity,
    ) -> Result<Option<ChangeGraph<'b>>, Error> {
        if let Some(root) = self.graph.externals(petgraph::Direction::Incoming).next() {
            let root_change = &self.graph[root];
            let schema_change = SchemaChange::load(root_change.schema_commit(), repo)?;
            if !schema_change.valid_signatures() {
                return Err(Error::InvalidSchemaSignatures);
            }
            Ok(Some(ChangeGraph {
                repo,
                schema_change,
                object_id,
                authorizing_identity,
                graph: self.graph,
            }))
        } else {
            Ok(None)
        }
    }
}
