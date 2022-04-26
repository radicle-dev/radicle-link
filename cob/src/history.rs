// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashMap},
    ops::ControlFlow,
};

use petgraph::visit::Walker as _;

use link_identities::git::Urn;
use radicle_git_ext as ext;

use crate::pruning_fold;

/// The DAG of changes making up the history of a collaborative object.
#[derive(Clone, Debug)]
pub struct History {
    root: EntryId,
    entries: HashMap<EntryId, HistoryEntry>,
    indices: HashMap<EntryId, petgraph::graph::NodeIndex<u32>>,
    graph: petgraph::Graph<HistoryEntry, (), petgraph::Directed, u32>,
}

impl PartialEq for History {
    fn eq(&self, other: &Self) -> bool {
        encoding::RawHistory::from(self).eq(&encoding::RawHistory::from(other))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("no entry for the root ID in the entries")]
    MissingRoot,
}

impl History {
    pub(crate) fn new_from_root<Id: Into<EntryId>>(
        id: Id,
        author: Urn,
        contents: EntryContents,
    ) -> Self {
        let id = id.into();
        let root_entry = HistoryEntry {
            id: id.clone(),
            author,
            children: vec![],
            contents,
        };
        let mut entries = HashMap::new();
        entries.insert(id.clone(), root_entry.clone());
        let NewGraph { graph, indices } = create_petgraph(&root_entry.id, &entries);
        Self {
            root: id,
            entries,
            graph,
            indices,
        }
    }

    pub fn new<Id: Into<EntryId>>(
        root: Id,
        entries: HashMap<EntryId, HistoryEntry>,
    ) -> Result<Self, CreateError> {
        let root = root.into();
        if !entries.contains_key(&root) {
            Err(CreateError::MissingRoot)
        } else {
            let NewGraph { graph, indices } = create_petgraph(&root, &entries);
            Ok(Self {
                root,
                entries,
                graph,
                indices,
            })
        }
    }

    /// A topological (parents before children) traversal of the dependency
    /// graph of this history. This is analagous to
    /// [`std::iter::Iterator::fold`] in that it folds every change into an
    /// accumulator value of type `A`. However, unlike `fold` the function `f`
    /// may prune branches from the dependency graph by returning
    /// `ControlFlow::Break`.
    pub fn traverse<F, A>(&self, init: A, f: F) -> A
    where
        F: for<'r> FnMut(A, &'r HistoryEntry) -> ControlFlow<A, A>,
    {
        let topo = petgraph::visit::Topo::new(&self.graph);
        #[allow(clippy::let_and_return)]
        let items = topo.iter(&self.graph).map(|idx| {
            let node = &self.graph[idx];
            node
        });
        pruning_fold::pruning_fold(init, items, f)
    }

    /// Add a new node to this history. The new node will have all the current
    /// tips of the history as its parents.
    pub(crate) fn extend<Id: Into<EntryId>>(
        &mut self,
        new_id: Id,
        new_author: Urn,
        new_contents: EntryContents,
    ) {
        let tips = self.tips();
        let new_id = new_id.into();
        let new_entry = HistoryEntry::new(
            new_id.clone(),
            new_author,
            std::iter::empty::<git2::Oid>(),
            new_contents,
        );
        let new_ix = self.graph.add_node(new_entry.clone());
        self.entries.insert(new_entry.id().clone(), new_entry);
        for tip in tips {
            if let Some(tip_entry) = self.entries.get_mut(&tip) {
                tip_entry.children.push(new_id.clone());
            }
            let tip_ix = self.indices.get(&tip).unwrap();
            self.graph.update_edge(*tip_ix, new_ix, ());
        }
    }

    pub(crate) fn tips(&self) -> BTreeSet<EntryId> {
        self.graph
            .externals(petgraph::Direction::Outgoing)
            .map(|n| {
                let entry = &self.graph[n];
                entry.id().clone()
            })
            .collect()
    }
}

struct NewGraph {
    graph: petgraph::Graph<HistoryEntry, (), petgraph::Directed, u32>,
    indices: HashMap<EntryId, petgraph::graph::NodeIndex<u32>>,
}

fn create_petgraph<'a>(root: &'a EntryId, entries: &'a HashMap<EntryId, HistoryEntry>) -> NewGraph {
    let mut graph = petgraph::Graph::new();
    let mut indices = HashMap::<EntryId, petgraph::graph::NodeIndex<u32>>::new();
    let root = entries.get(root).unwrap().clone();
    let root_ix = graph.add_node(root.clone());
    indices.insert(root.id.clone(), root_ix);
    let mut to_process = vec![root];
    while let Some(entry) = to_process.pop() {
        let entry_ix = indices[&entry.id];
        for child_id in entry.children {
            let child = entries[&child_id].clone();
            let child_ix = graph.add_node(child.clone());
            indices.insert(child.id.clone(), child_ix);
            graph.update_edge(entry_ix, child_ix, ());
            to_process.push(child.clone());
        }
    }
    NewGraph { graph, indices }
}

#[derive(Clone, Debug, PartialEq, Hash, Eq, minicbor::Encode, minicbor::Decode)]
pub enum EntryContents {
    #[n(0)]
    Automerge(
        #[cbor(with = "minicbor::bytes")]
        #[n(0)]
        Vec<u8>,
    ),
}

#[derive(serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode)]
pub enum HistoryType {
    #[n(1)]
    Automerge,
}

impl From<&EntryContents> for HistoryType {
    fn from(c: &EntryContents) -> Self {
        match c {
            EntryContents::Automerge(..) => HistoryType::Automerge,
        }
    }
}

impl AsRef<[u8]> for EntryContents {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Automerge(bytes) => bytes,
        }
    }
}

/// A unique identifier for a history entry.
#[derive(
    Clone, Debug, PartialEq, Hash, Eq, PartialOrd, Ord, minicbor::Decode, minicbor::Encode,
)]
#[cbor(transparent)]
pub struct EntryId(#[n(0)] ext::Oid);

impl From<git2::Oid> for EntryId {
    fn from(id: git2::Oid) -> Self {
        Self(id.into())
    }
}

/// One entry in the dependency graph for a change
#[derive(Clone, Debug, PartialEq, Hash, minicbor::Encode, minicbor::Decode)]
pub struct HistoryEntry {
    #[n(0)]
    id: EntryId,
    #[n(1)]
    author: Urn,
    #[n(2)]
    children: Vec<EntryId>,
    #[n(3)]
    contents: EntryContents,
}

impl HistoryEntry {
    pub fn new<Id1: Into<EntryId>, Id2: Into<EntryId>, ChildIds: IntoIterator<Item = Id2>>(
        id: Id1,
        author: Urn,
        children: ChildIds,
        contents: EntryContents,
    ) -> Self {
        Self {
            id: id.into(),
            author,
            children: children.into_iter().map(|id| id.into()).collect(),
            contents,
        }
    }

    /// The ids of the changes this change depends on
    pub fn children(&self) -> impl Iterator<Item = &EntryId> {
        self.children.iter()
    }

    /// The URN of the identity which signed this change
    pub fn author(&self) -> &Urn {
        &self.author
    }

    /// The contents of this change
    pub fn contents(&self) -> &EntryContents {
        &self.contents
    }

    pub fn id(&self) -> &EntryId {
        &self.id
    }
}

impl pruning_fold::GraphNode for HistoryEntry {
    type Id = EntryId;

    fn id(&self) -> &Self::Id {
        &self.id
    }

    fn child_ids(&self) -> &[Self::Id] {
        &self.children
    }
}

mod encoding {
    use super::{EntryId, History, HistoryEntry};
    use std::{borrow::Cow, collections::HashMap};

    #[derive(PartialEq, minicbor::Encode, minicbor::Decode)]
    pub(super) struct RawHistory<'a> {
        #[b(0)]
        root: Cow<'a, EntryId>,
        #[b(1)]
        entries: Cow<'a, HashMap<EntryId, HistoryEntry>>,
    }

    impl<'a> From<&'a History> for RawHistory<'a> {
        fn from(h: &'a History) -> Self {
            RawHistory {
                root: Cow::Borrowed(&h.root),
                entries: Cow::Borrowed(&h.entries),
            }
        }
    }

    impl minicbor::Encode for History {
        fn encode<W: minicbor::encode::Write>(
            &self,
            e: &mut minicbor::Encoder<W>,
        ) -> Result<(), minicbor::encode::Error<W::Error>> {
            RawHistory::from(self).encode(e)
        }
    }

    impl<'b> minicbor::Decode<'b> for History {
        fn decode(d: &mut minicbor::Decoder<'b>) -> Result<Self, minicbor::decode::Error> {
            let raw = RawHistory::decode(d)?;
            History::new(raw.root.into_owned(), raw.entries.into_owned())
                .map_err(|e| minicbor::decode::Error::Custom(Box::new(e)))
        }
    }
}
