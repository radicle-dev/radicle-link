use rose_tree::{NodeIndex, RoseTree, ROOT};
use std::{collections::HashMap, hash};

/// Laws:
/// Thread::new(comment).first() === comment
/// thread.reply(comment).delete(comment) === thread
/// Thread::new(comment).delete(comment) === None
/// Thread::new(comment).edit(f, comment) === Thread::new(f(comment).unwrap())
pub struct Thread<A> {
    lut: HashMap<Path, NodeIndex>,
    tree: RoseTree<A>,
}

#[derive(Debug, PartialEq, Eq, hash::Hash, Clone)]
pub struct Path(Vec<u32>);

impl Path {
    pub fn new(index: u32) -> Self {
        Path(vec![index])
    }

    pub fn push(&mut self, index: u32) {
        self.0.push(index);
    }
}

impl<A> Thread<A> {
    pub fn new(a: A) -> (Self, Path)
    where
        A: Eq,
    {
        let (tree, root) = RoseTree::new(a);
        let mut lut = HashMap::new();
        let path = Path::new(0);
        lut.insert(path.clone(), root);
        (Thread { lut, tree }, path)
    }

    pub fn delete(&mut self, path: &Path) -> Result<A, ()> {
        match self.lut.remove(&path) {
            Some(ix) => self.tree.remove_node(ix).ok_or(()),
            None => Err(()),
        }
    }

    pub fn edit<F>(&mut self, f: F, path: &Path) -> Option<A>
    where
        F: FnOnce(&mut A) -> Option<A>,
    {
        let ix = self.lut.get(path)?;
        let node = self.tree.node_weight_mut(*ix)?;
        f(node)
    }

    pub fn expand(&self) -> Vec<A>
    where
        A: Clone,
    {
        let mut nodes = vec![];
        for ix in self.tree.children(NodeIndex::new(ROOT)) {
            nodes.push(self.tree.node_weight(ix).unwrap().clone())
        }
        nodes
    }

    pub fn reply(&mut self, path: &Path, a: A) -> Result<Path, ()> {
        match self.lut.get(path) {
            Some(ix) => {
                let new_ix = self.tree.add_child(*ix, a);
                let mut new_path = path.clone();
                new_path.push(0);
                self.lut.insert(new_path.clone(), new_ix);
                Ok(new_path)
            },
            None => Err(()),
        }
    }

    /* This is tricky because basically we want to calculate
     * the sub-LUT of a thread and create a new RoseTree
    pub fn sub_thread(&self, path: &Path) -> Option<RoseTree> {
        let ix = self.lut.get(path)?;
        self.tree.node_weight(*ix)
    }
    */

    pub fn first(&self) -> &A {
        &self.tree[NodeIndex::new(ROOT)]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
