// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, collections::BTreeMap, iter::FromIterator};

/// A simplified representation of a git tree, intended mainly to be created
/// from literals.
///
/// # Example
///
/// ```
/// use radicle_git_ext::tree::{Tree, blob};
///
/// let my_tree = vec![
///     ("README", blob(b"awe")),
///     ("src", vec![("main.rs", blob(b"fn main() {}"))].into_iter().collect()),
/// ]
/// .into_iter()
/// .collect::<Tree>();
///
/// assert_eq!(
///     format!("{:?}", my_tree),
///     "Tree(\
///         {\
///             \"README\": Blob(\
///                 [\
///                     97, \
///                     119, \
///                     101\
///                 ]\
///             ), \
///             \"src\": Tree(\
///                 Tree(\
///                     {\
///                         \"main.rs\": Blob(\
///                             [\
///                                 102, \
///                                 110, \
///                                 32, \
///                                 109, \
///                                 97, \
///                                 105, \
///                                 110, \
///                                 40, \
///                                 41, \
///                                 32, \
///                                 123, \
///                                 125\
///                             ]\
///                         )\
///                     }\
///                 )\
///             )\
///         }\
///     )"
/// )
/// ```
#[derive(Clone, Debug)]
pub struct Tree<'a>(BTreeMap<Cow<'a, str>, Node<'a>>);

impl Tree<'_> {
    pub fn write(&self, repo: &git2::Repository) -> Result<git2::Oid, git2::Error> {
        use Node::*;

        let mut builder = repo.treebuilder(None)?;
        for (name, node) in &self.0 {
            match node {
                Blob(data) => {
                    let oid = repo.blob(data)?;
                    builder.insert(name.as_ref(), oid, git2::FileMode::Blob.into())?;
                },
                Tree(sub) => {
                    let oid = sub.write(repo)?;
                    builder.insert(name.as_ref(), oid, git2::FileMode::Tree.into())?;
                },
            }
        }

        builder.write()
    }
}

impl<'a> From<BTreeMap<Cow<'a, str>, Node<'a>>> for Tree<'a> {
    fn from(map: BTreeMap<Cow<'a, str>, Node<'a>>) -> Self {
        Self(map)
    }
}

impl<'a, K, N> FromIterator<(K, N)> for Tree<'a>
where
    K: Into<Cow<'a, str>>,
    N: Into<Node<'a>>,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, N)>,
    {
        Self(
            iter.into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

#[derive(Clone, Debug)]
pub enum Node<'a> {
    Blob(Cow<'a, [u8]>),
    Tree(Tree<'a>),
}

pub fn blob(slice: &[u8]) -> Node {
    Node::from(slice)
}

impl<'a> From<&'a [u8]> for Node<'a> {
    fn from(slice: &'a [u8]) -> Self {
        Self::from(Cow::Borrowed(slice))
    }
}

impl<'a> From<Cow<'a, [u8]>> for Node<'a> {
    fn from(bytes: Cow<'a, [u8]>) -> Self {
        Self::Blob(bytes)
    }
}

impl<'a> From<Tree<'a>> for Node<'a> {
    fn from(tree: Tree<'a>) -> Self {
        Self::Tree(tree)
    }
}

impl<'a, K, N> FromIterator<(K, N)> for Node<'a>
where
    K: Into<Cow<'a, str>>,
    N: Into<Node<'a>>,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, N)>,
    {
        Self::Tree(iter.into_iter().collect())
    }
}
