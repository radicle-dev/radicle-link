// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Borrow,
    collections::{BTreeSet, HashMap},
    ops::ControlFlow,
};

pub(crate) trait GraphNode {
    type Id: Clone + Eq + Ord + std::hash::Hash + std::fmt::Debug;

    fn id(&self) -> &Self::Id;
    fn child_ids(&self) -> &[Self::Id];
}

/// Fold a topological sort of a directed acyclic graph, pruning some branches.
///
/// `items` must be an iterator over the nodes of the graph in topological
/// order. Assuming this is the case `fold` will only be called with nodes whose
/// ancestors have already been evaluated. Returning `ControlFlow::Break(..)`
/// from `fold` will omit evaluation of the current node and consequently omit
/// processing of any nodes who have the current node as an ancestor.
pub(crate) fn pruning_fold<'a, BN, Node: 'a, It: Iterator<Item = BN>, F, O>(
    init: O,
    items: It,
    mut f: F,
) -> O
where
    Node: GraphNode,
    BN: Borrow<Node> + 'a,
    F: for<'r> FnMut(O, &'r Node) -> std::ops::ControlFlow<O, O>,
{
    let mut rejected = RejectedNodes::new();
    let mut state = init;
    for node in items {
        // There can be multiple paths to a change so in a topological traversal we
        // might encounter a change which we have already rejected
        // previously
        if rejected.is_rejected(node.borrow().id()) {
            continue;
        }
        if let Some(rejected_ancestor) = rejected.rejected_ancestor(node.borrow().id()) {
            let ancestor = rejected_ancestor.clone();
            tracing::warn!(id=?node.borrow().id(), ?rejected_ancestor, "rejecting node because an ancestor change was rejected");
            for child in node.borrow().child_ids() {
                rejected.transitively_reject(child, &ancestor);
            }
            continue;
        }
        state = match f(state, node.borrow()) {
            ControlFlow::Continue(state) => state,
            ControlFlow::Break(state) => {
                rejected.directly_reject(node.borrow().id(), node.borrow().child_ids());
                state
            },
        };
    }
    state
}

struct RejectedNodes<NodeId> {
    /// Changes which are directly rejected by the fold function
    direct: BTreeSet<NodeId>,
    /// A map from node IDs to the IDs of ancestor nodes which are
    /// direct rejections
    transitive: HashMap<NodeId, NodeId>,
}

impl<NodeId: Clone + Eq + Ord + std::hash::Hash> RejectedNodes<NodeId> {
    fn new() -> RejectedNodes<NodeId> {
        RejectedNodes {
            direct: BTreeSet::new(),
            transitive: HashMap::new(),
        }
    }

    fn rejected_ancestor(&self, node: &NodeId) -> Option<&NodeId> {
        self.transitive.get(node)
    }

    fn is_rejected(&self, node: &NodeId) -> bool {
        self.direct.contains(node)
    }

    fn directly_reject(&mut self, node: &NodeId, children: &[NodeId]) {
        self.direct.insert(node.clone());
        for child in children {
            self.transitive.insert(child.clone(), node.clone());
        }
    }

    fn transitively_reject(&mut self, child: &NodeId, rejected_ancestor: &NodeId) {
        self.transitive
            .insert(child.clone(), rejected_ancestor.clone());
    }
}
