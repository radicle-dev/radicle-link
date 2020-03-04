use nonempty::NonEmpty;
use rose_tree::{NodeIndex, RoseTree, ROOT};
use std::{collections::HashMap, hash};

/// Laws:
/// Thread::new(comment).first() === comment
/// thread.reply(comment).delete(comment) === thread
/// Thread::new(comment).delete(comment) === None
/// Thread::new(comment).edit(f, comment) === Thread::new(f(comment).unwrap())
#[derive(Debug, Clone)]
pub struct Thread<A> {
    lut: HashMap<Path, NodeIndex>,
    tree: RoseTree<A>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    MissingPath(Path),
    MissingNode,
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

    pub fn pop(&mut self) -> Option<u32> {
        self.0.pop()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn max_node(paths: &[Self]) -> u32 {
        let mut max_n = 0;

        for path in paths {
            let last = path.clone().pop();
            max_n = max_n.max(last.unwrap_or(max_n));
        }

        max_n
    }

    // Get the prefix keys of path that differ by 1
    pub(crate) fn prefix_keys<'a, P>(&self, paths: P) -> Vec<Path>
    where
        P: Iterator<Item = &'a Path>,
    {
        let mut prefixes = vec![];
        for path in paths {
            let mut prefix = path.clone();
            prefix.pop();
            if *self == prefix {
                prefixes.push(path.clone())
            }
        }

        prefixes
    }
}

impl From<Vec<u32>> for Path {
    fn from(path: Vec<u32>) -> Self {
        Path(path)
    }
}

impl<A> Thread<A> {
    /// Create a new `Thread` with `a` as the root of the `Thread`.
    ///
    /// The return value includes the [`Path`] to reach the root.
    /// This should always be equal to `Path::new(0)`, and can
    /// be used to [`Thread::view`] the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{Path, Thread};
    ///
    /// let (thread, root_path) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// assert_eq!(thread.view(&root_path), Ok(&String::from("Discussing rose trees")));
    /// assert_eq!(root_path, Path::new(0));
    /// ```
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

    /// Reply to an existing `Thread`. The reply is made to where the [`Path`]
    /// points to. For example, if we want to reply to the main thread, we will
    /// always use the "root path", `Path::new(0)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{Path, Thread};
    ///
    /// let (mut thread, root_path) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(&root_path, String::from("I love rose trees!"));
    /// thread.reply(&root_path, String::from("What should we use them for?"));
    ///
    /// // Reply to the 1st comment on the main thread
    /// let mut first_comment_path = root_path.clone();
    /// first_comment_path.push(1);
    /// thread.reply(&first_comment_path, String::from("Did you know rose trees are equivalent to Cofree []?"));
    ///
    /// assert_eq!(thread.view(&root_path), Ok(&String::from("Discussing rose trees")));
    /// assert_eq!(thread.view(&first_comment_path), Ok(&String::from("I love rose trees!")));
    ///
    /// let mut first_of_first_path = first_comment_path.clone();
    /// first_of_first_path.push(1);
    /// assert_eq!(
    ///     thread.view(&first_of_first_path),
    ///     Ok(&String::from("Did you know rose trees are equivalent to Cofree []?"))
    /// );
    /// ```
    pub fn reply(&mut self, path: &Path, a: A) -> Result<Path, Error> {
        match self.lut.get(path) {
            Some(ix) => {
                let new_ix = self.tree.add_child(*ix, a);
                let mut new_path = path.clone();
                let new_node = Path::max_node(&path.prefix_keys(self.lut.keys()));
                new_path.push(new_node + 1);
                self.lut.insert(new_path.clone(), new_ix);
                Ok(new_path)
            },
            None => Err(Error::MissingPath(path.clone())),
        }
    }

    /// Delete a node that exists on the provided [`Path`].
    ///
    /// TODO(fintan): Need to figure out what happens when we delete a node that
    /// has children as a thread. In RoseTree it says that the parent of the
    /// deleted node becomes the parent of all the deleted nodes children.
    /// What would this mean for a comment thread? Maybe we want "immutable"
    /// comments, where comments are marked as deleted but not actually deleted
    /// from the graph.
    ///
    /// # Error
    ///
    /// If the node does not exist on the provided [`Path`].
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{ThreadError, Path, Thread};
    ///
    /// let (mut thread, root_path) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(&root_path, String::from("I love rose trees!"));
    /// thread.reply(&root_path, String::from("What should we use them for?"));
    ///
    /// // Reply to the 1st comment on the main thread
    /// let mut first_comment_path = root_path.clone();
    /// first_comment_path.push(1);
    /// thread.reply(
    ///     &first_comment_path,
    ///     String::from("Did you know rose trees are equivalent to Cofree []?")
    /// );
    ///
    /// // Delete the last comment on the main thread
    /// thread.delete(&Path::from(vec![0, 2]));
    ///
    /// assert_eq!(thread.view(&root_path), Ok(&String::from("Discussing rose trees")));
    /// assert_eq!(thread.view(&first_comment_path), Ok(&String::from("I love rose trees!")));
    /// assert_eq!(
    ///     thread.view(&Path::from(vec![0, 2])),
    ///     Err(ThreadError::MissingPath(Path::from(vec![0,2])))
    /// );
    /// ```
    pub fn delete(&mut self, path: &Path) -> Result<A, Error> {
        match self.lut.remove(&path) {
            Some(ix) => self
                .tree
                .remove_node(ix)
                .ok_or_else(|| Error::MissingPath(path.clone())),
            None => Err(Error::MissingPath(path.clone())),
        }
    }

    pub fn edit<F>(&mut self, path: &Path, f: F) -> Result<Option<A>, Error>
    where
        F: FnOnce(&A) -> Option<A>,
    {
        let ix = self
            .lut
            .get(path)
            .ok_or_else(|| Error::MissingPath(path.clone()))?;
        let node = self
            .tree
            .node_weight_mut(*ix)
            .ok_or_else(|| Error::MissingNode)?;
        Ok(f(node))
    }

    pub fn expand(&self) -> NonEmpty<A>
    where
        A: Clone,
    {
        let mut nodes = NonEmpty::new(self.first().clone());
        for ix in self.tree.children(NodeIndex::new(ROOT)) {
            // TODO: this assumes we have an existing index.
            // Technically we should because we're walking over the children...
            nodes.push(self.tree.node_weight(ix).unwrap().clone())
        }
        nodes
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

    pub fn view(&self, path: &Path) -> Result<&A, Error> {
        match self.lut.get(path) {
            Some(ix) => Ok(&self.tree[*ix]),
            None => Err(Error::MissingPath(path.clone())),
        }
    }

    pub fn paths(&self) -> Vec<Path> {
        self.lut.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// forall a. Thread::new(a).first() === a
    fn prop_first_of_new<A: Eq + Clone>(a: A) -> bool {
        *Thread::new(a.clone()).0.first() == a
    }

    /// { new_path = thread.reply(path, comment)?
    ///   thread.delete(new_path)
    /// } === thread
    fn prop_deleting_a_replied_comment_is_noop<A: Clone>(
        thread: &mut Thread<A>,
        path: &Path,
        a: A,
    ) -> Result<bool, Error> {
        let old_thread = thread.clone();
        let new_path = thread.reply(path, a)?;
        thread.delete(&new_path)?;

        // TODO: Also check that all NodeIndexes are equal
        Ok(thread.lut == old_thread.lut)
    }

    /// Thread::new(comment).delete(comment) === None
    fn prop_deleting_root_should_not_be_possible<A: Eq>(a: A) -> bool {
        Thread::new(a).0.delete(&Path::new(0)) == Err(Error::MissingPath(Path::new(0)))
    }

    /// Thread::new(comment).edit(f, comment) ===
    /// Thread::new(f(comment).unwrap())
    fn prop_new_followed_by_edit_is_same_as_editing_followed_by_new<A, F>(a: A, f: &F) -> bool
    where
        A: Eq + Clone,
        F: Fn(&A) -> Option<A>,
    {
        let mut lhs = Thread::new(a.clone()).0;
        lhs.edit(&Path::new(0), f).unwrap();

        let rhs = Thread::new(f(&a).unwrap()).0;

        lhs.lut == rhs.lut
    }

    /// Thread::new(a).1 == Path::new(0)
    fn prop_root_path_is_constant<A>(a: A) -> bool
    where
        A: Eq,
    {
        Thread::new(a).1 == Path::new(0)
    }

    /// let (thread, path) = Thread::new(a)
    /// => thread.view(path) == a
    fn prop_root_followed_by_view<A>(a: A) -> bool
    where
        A: Eq + Clone,
    {
        let (thread, path) = Thread::new(a.clone());
        *thread.view(&path).unwrap() == a
    }

    #[test]
    fn check_first_of_new() {
        assert!(prop_first_of_new("New thread"))
    }

    #[test]
    fn check_root_path_is_constant() {
        assert!(prop_root_path_is_constant("New thread"))
    }

    #[test]
    fn check_root_followed_by_view() {
        assert!(prop_root_followed_by_view("New thread"))
    }

    #[test]
    fn check_deleting_a_replied_comment_is_noop() -> Result<(), Error> {
        let (mut thread, path) = Thread::new("New thread");
        prop_deleting_a_replied_comment_is_noop(&mut thread, &path, "New comment").map(|_| ())
    }

    #[test]
    fn check_deleting_root_should_not_be_possible() {
        assert!(prop_deleting_root_should_not_be_possible("New thread"))
    }

    #[test]
    fn check_new_followed_by_edit_is_same_as_editing_followed_by_new() {
        assert!(
            prop_new_followed_by_edit_is_same_as_editing_followed_by_new("new thread", &|_| {
                Some("edit: New thread")
            })
        )
    }

    mod path {
        use super::*;

        #[test]
        fn test_prefix_keys() {
            let test_path = Path::new(0);
            let test_paths = vec![
                Path::from(vec![0]),
                Path::from(vec![0, 1]),
                Path::from(vec![0, 1, 1]),
                Path::from(vec![0, 2]),
                Path::from(vec![0, 3]),
                Path::from(vec![1]),
            ];

            assert_eq!(
                test_path.prefix_keys(test_paths.iter()),
                vec![
                    Path::from(vec![0, 1]),
                    Path::from(vec![0, 2]),
                    Path::from(vec![0, 3]),
                ]
            );
        }
    }
}
