// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! appendage
//! /əˈpɛndɪdʒ/
//! noun: appendage; plural noun: appendages
//! 1. a thing that is added or attached to something larger or more important.
//!
//! An [`Appendage`] is a data structure that is aware of how to
//! [`Appendable::append`] items such that it can be done commutatively. It does
//! not stop there, however, since it is also aware of how to modify these items
//! commutatively as well. The only caveat is that it must be fed all the
//! operations that exist. That is to say, two appendages will only be equal if
//! they have seen the exact same operations and have applied them.

// You can go away, Clippy, unless you can suggest how to fix this "complex type".
#![allow(clippy::type_complexity)]

use crate::ops::Apply;
use nonempty::NonEmpty;
use std::{
    error,
    fmt,
    ops::{Deref, DerefMut},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<Modify> {
    IndexOutOfBounds(usize),
    IndexExists(usize),
    Modify(Modify),
}

// Writing by hand because of: https://github.com/dtolnay/thiserror/issues/79
impl<Modify: fmt::Display> fmt::Display for Error<Modify> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::IndexOutOfBounds(ix) => write!(f, "index {0} is out of bounds", ix),
            Error::IndexExists(ix) => write!(f, "index {} already exists", ix),
            Error::Modify(m) => write!(f, "{}", m),
        }
    }
}

impl<Modify: error::Error> error::Error for Error<Modify> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Modify(m) => m.source(),
            _ => None,
        }
    }
}

impl<Modify> From<Modify> for Error<Modify> {
    fn from(m: Modify) -> Self {
        Error::Modify(m)
    }
}

/// A data structure is `Appendable` if it is aware of how to append an item,
/// access an item by index, and compute the length of the data structure.
pub trait Appendable {
    /// The inner item of the data structure.
    type Item;

    /// Append an item to the end of the data structure.
    fn append(&mut self, item: Self::Item);

    /// Insert the `item` at the provided `index`, shifting elements to the
    /// right.
    fn insert(&mut self, index: usize, item: Self::Item);

    /// Index the data structure, failing if the index does not exist.
    fn ix_mut(&mut self, index: usize) -> Option<&mut Self::Item>;

    /// Compute the length of the data structure.
    fn len(&self) -> usize;

    /// Check if the data structure is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<A> Appendable for Vec<A> {
    type Item = A;

    fn append(&mut self, item: Self::Item) {
        self.push(item)
    }

    fn insert(&mut self, index: usize, item: Self::Item) {
        self.insert(index, item)
    }

    fn ix_mut(&mut self, index: usize) -> Option<&mut Self::Item> {
        self.get_mut(index)
    }

    fn len(&self) -> usize {
        self.len()
    }
}

impl<A> Appendable for NonEmpty<A> {
    type Item = A;

    fn append(&mut self, item: Self::Item) {
        self.push(item)
    }

    fn ix_mut(&mut self, index: usize) -> Option<&mut Self::Item> {
        self.get_mut(index)
    }

    fn insert(&mut self, index: usize, item: Self::Item) {
        self.insert(index, item)
    }

    fn len(&self) -> usize {
        self.len()
    }
}

/// An `Op` is the operation that can be produced from an [`Appendage`], or
/// applied to an [`Appendage`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op<Mod, A> {
    /// Append the value (`val`) to the `Appendage`, ensuring it occurs at the
    /// supplied index (`ix`).
    Append { ix: usize, val: A },
    /// Modify the item at the index (`ix`) of the `Appendage` using the
    /// supplied operation (`op`).
    Modify { ix: usize, op: Mod },
}

impl<Mod, A> Op<Mod, A> {
    pub fn ix(&self) -> usize {
        match self {
            Op::Append { ix, .. } => *ix,
            Op::Modify { ix, .. } => *ix,
        }
    }
}

/// An `Appendage` is an [`Appendable`] data structure, which ensure that
/// appending and modification is commutative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Appendage<V: Appendable> {
    pub val: V,
}

impl<V: Appendable> Deref for Appendage<V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

impl<V: Appendable> DerefMut for Appendage<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl<Mod, V> Apply for Appendage<V>
where
    V: Appendable,
    V::Item: Apply<Op = Mod> + Ord,
{
    type Op = Op<Mod, V::Item>;
    type Error = Error<<V::Item as Apply>::Error>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        Appendage::apply(self, op)
    }
}

impl<V: Appendable> Appendage<V> {
    /// Create a new `Appendage` starting off with the value provided.
    pub fn new(val: V) -> Self {
        Appendage { val }
    }

    /// Append a new `item` to the `Appendage`. We get back the [`Op`] to pass
    /// onto other `Appendage`s.
    pub fn append<M>(&mut self, item: V::Item) -> Op<M, V::Item>
    where
        V::Item: Clone,
    {
        self.val.append(item.clone());
        Op::Append {
            ix: self.val.len() - 1,
            val: item,
        }
    }

    /// Modify the item at `ix` in the `Appendage`. We get back the [`Op`] to
    /// pass onto other `Appendage`s, but only if the index existed since we
    /// cannot modify non-existent items.
    pub fn modify<M>(
        &mut self,
        ix: usize,
        modify: M,
    ) -> Result<Op<M, V::Item>, Error<<V::Item as Apply>::Error>>
    where
        V::Item: Apply<Op = M> + Clone,
        M: Clone,
    {
        match self.val.ix_mut(ix) {
            None => Err(Error::IndexOutOfBounds(ix)),
            Some(item) => {
                item.apply(modify.clone())?;
                Ok(Op::Modify { ix, op: modify })
            },
        }
    }

    /// Apply the [`Op`] to the `Appendage`. The operations are commutative, as
    /// long as all the operations are passed. The `Appendage` will hold the
    /// state necessary to carry out the operation if it can't do it right
    /// away.
    ///
    /// For example: we appended `x` and then `y` on `Appendage` `A`. If we then
    /// apply the `Op` to append `y` on an `Appendage` `B`, it will have to
    /// wait for the `Op` to append `x` to reach a convergent state with
    /// `A`.
    ///
    /// TODO: Well crap. If we get an append and a modify,
    /// and a concurrent append happens, how do we disambiguate
    /// what the modify was modifying?
    /// Is this just another form of Replace?
    pub fn apply<M>(&mut self, op: Op<M, V::Item>) -> Result<(), Error<<V::Item as Apply>::Error>>
    where
        V::Item: Apply<Op = M> + Ord,
    {
        match op {
            Op::Append { ix, val } => match self.val.ix_mut(ix) {
                None => {
                    if ix == self.val.len() {
                        self.val.append(val);
                        Ok(())
                    } else {
                        Err(Error::IndexOutOfBounds(ix))
                    }
                },
                Some(other) => {
                    if val < *other {
                        self.val.insert(ix, val);
                        Ok(())
                    } else {
                        self.val.insert(ix + 1, val);
                        Ok(())
                    }
                },
            },
            Op::Modify { ix, op } => match self.val.ix_mut(ix) {
                None => Err(Error::IndexOutOfBounds(ix)),
                Some(v) => {
                    v.apply(op)?;
                    Ok(())
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cmp::Ordering, convert::Infallible, error};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Int {
        id: usize,
        val: u32,
    }

    impl Int {
        fn new(i: u32) -> Self {
            Int {
                id: i as usize,
                val: i,
            }
        }
    }

    impl PartialOrd for Int {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            self.id.partial_cmp(&other.id)
        }
    }

    impl Ord for Int {
        fn cmp(&self, other: &Self) -> Ordering {
            self.id.cmp(&other.id)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Add(u32);

    impl Apply for Int {
        type Op = Add;
        type Error = Infallible;

        fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
            self.val += op.0;
            Ok(())
        }
    }

    type TestResult = Result<(), Box<dyn error::Error + 'static>>;

    #[test]
    fn sync_appends() -> TestResult {
        let mut left = Appendage::new(vec![Int::new(1)]);
        let append1 = left.append(Int::new(2));
        let append2 = left.append(Int::new(3));

        let mut right = Appendage::new(vec![Int::new(1)]);
        right.apply(append1)?;
        right.apply(append2)?;

        assert_eq!(left, right);

        Ok(())
    }

    #[test]
    fn out_of_order_appends_fail() -> TestResult {
        let mut left = Appendage::new(vec![Int::new(1)]);
        let append1 = left.append(Int::new(2));
        let append2 = left.append(Int::new(3));

        let mut right = Appendage::new(vec![Int::new(1)]);
        let failed_append = right.apply(append2.clone());
        assert!(failed_append.is_err());

        right.apply(append1)?;
        right.apply(append2)?;

        assert_eq!(left, right);

        Ok(())
    }

    #[test]
    fn sync_appends_and_edits() -> TestResult {
        let expected = vec![Int::new(1), Int::new(2), Int { id: 3, val: 45 }];

        let mut left = Appendage::new(vec![Int::new(1)]);
        let append1 = left.append(Int::new(2));
        let append2 = left.append(Int::new(3));
        let edit = left.modify(2, Add(42))?;

        let mut right = Appendage::new(vec![Int::new(1)]);
        right.apply(append1)?;
        right.apply(append2)?;
        right.apply(edit)?;

        assert_eq!(left.val, expected);
        assert_eq!(right.val, expected);
        assert_eq!(left, right);
        Ok(())
    }

    #[test]
    fn concurrent_appends_lt() -> TestResult {
        let mut left = Appendage::new(vec![]);
        let append1 = left.append(Int::new(1));

        let mut right = Appendage::new(vec![]);
        let append2 = right.append(Int::new(2));

        left.apply(append2)?;
        right.apply(append1)?;

        assert_eq!(left, right);
        Ok(())
    }

    #[test]
    fn concurrent_appends_gt() -> TestResult {
        let mut left = Appendage::new(vec![]);
        let append1 = left.append(Int::new(2));

        let mut right = Appendage::new(vec![]);
        let append2 = right.append(Int::new(1));

        left.apply(append2)?;
        right.apply(append1)?;

        assert_eq!(left, right);
        Ok(())
    }

    // TODO: test case fails because we can't identify what
    // we modified in the sequence
    #[test]
    fn concurrent_appends_with_edits() -> TestResult {
        let mut left = Appendage::new(vec![]);
        let append1 = left.append(Int::new(1));
        let edit1 = left.modify(append1.ix(), Add(2))?;

        let mut right = Appendage::new(vec![]);
        let append2 = right.append(Int::new(2));
        let edit2 = right.modify(append2.ix(), Add(3))?;

        left.apply(append2)?;
        left.apply(edit2)?;
        right.apply(append1)?;
        right.apply(edit1)?;

        assert_eq!(left, right);
        Ok(())
    }
}
