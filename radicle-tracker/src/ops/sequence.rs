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

#![allow(clippy::new_without_default)]

use crate::ops::{id::Gen, Apply};
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

/// An `Op` is the operation that can be produced from an [`Appendage`], or
/// applied to an [`Appendage`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op<Mod, O, A> {
    /// Append the value (`val`) to the `Appendage`, ensuring it occurs at the
    /// supplied index (`ix`).
    Append { id: O, ix: usize, val: A },
    /// Modify the item at the index (`ix`) of the `Appendage` using the
    /// supplied operation (`op`).
    Modify { id: O, ix: usize, op: Mod },
}

impl<Mod, O, A> Op<Mod, O, A> {
    pub fn ix(&self) -> usize {
        match self {
            Op::Append { ix, .. } => *ix,
            Op::Modify { ix, .. } => *ix,
        }
    }

    pub fn id(&self) -> &O {
        match self {
            Op::Append { id, .. } => &id,
            Op::Modify { id, .. } => &id,
        }
    }
}

/// An `Appendage` is an [`Appendable`] data structure, which ensure that
/// appending and modification is commutative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrdSequence<O, T> {
    pub(crate) val: Vec<(O, T)>,
}

impl<O, T> Deref for OrdSequence<O, T> {
    type Target = Vec<(O, T)>;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

impl<O, T> DerefMut for OrdSequence<O, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl<O, T> OrdSequence<O, T> {
    pub fn new() -> Self {
        OrdSequence { val: vec![] }
    }

    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.val.iter().map(|(_, t)| t).cloned().collect()
    }

    fn find_item(&self, ord: &O) -> Result<usize, usize>
    where
        O: Ord + Clone,
    {
        self.val
            .binary_search_by_key(ord, |(other, _)| other.clone())
    }

    /// Append a new `item` to the `Appendage`. We get back the [`Op`] to pass
    /// onto other `Appendage`s.
    pub fn append<M>(&mut self, item: T) -> Op<M, O, T>
    where
        T: Clone,
        O: Gen + Clone,
    {
        let id = O::gen();
        self.val.push((id.clone(), item.clone()));
        Op::Append {
            id,
            ix: self.val.len() - 1,
            val: item,
        }
    }

    /// Modify the item at `ix` in the `Appendage`. We get back the [`Op`] to
    /// pass onto other `Appendage`s, but only if the index existed since we
    /// cannot modify non-existent items.
    pub fn modify<M>(&mut self, ix: usize, modify: M) -> Result<Op<M, O, T>, Error<T::Error>>
    where
        T: Apply<Op = M> + Clone,
        M: Clone,
        O: Clone,
    {
        match self.val.get_mut(ix) {
            None => Err(Error::IndexOutOfBounds(ix)),
            Some((id, val)) => {
                val.apply(modify.clone())?;
                Ok(Op::Modify {
                    id: id.clone(),
                    ix,
                    op: modify,
                })
            },
        }
    }

    pub fn apply<M>(&mut self, op: Op<M, O, T>) -> Result<(), Error<T::Error>>
    where
        T: Apply<Op = M> + Ord,
        O: Ord + Clone,
    {
        match op {
            Op::Append { id, val, .. } => match self.find_item(&id) {
                Err(ix) => {
                    self.val.insert(ix, (id, val));
                    Ok(())
                },
                Ok(ix) => {
                    // ix exists because of find_item. qed.
                    let item = &self.val[ix];
                    if item.0 < id {
                        self.val.insert(ix + 1, (id, val));
                    } else {
                        self.val.insert(ix, (id, val));
                    }

                    Ok(())
                },
            },
            Op::Modify { id, ix, op } => match self.find_item(&id) {
                Ok(ix) => {
                    // ix exists because of find_item. qed.
                    let (_, item) = &mut self.val[ix];
                    item.apply(op)?;
                    Ok(())
                },
                Err(_) => Err(Error::IndexOutOfBounds(ix)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::id::UniqueTimestamp;
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
        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let append2 = left.append(Int::new(2));

        let mut right = OrdSequence::new();
        right.apply(append1)?;
        right.apply(append2)?;

        assert_eq!(left, right);

        Ok(())
    }

    #[test]
    fn out_of_order_appends_fail() -> TestResult {
        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let append2 = left.append(Int::new(2));

        let mut right = OrdSequence::new();
        right.apply(append2)?;
        right.apply(append1)?;

        assert_eq!(left, right);

        Ok(())
    }

    #[test]
    fn sync_appends_and_edits() -> TestResult {
        let expected = vec![Int::new(1), Int { id: 2, val: 44 }];

        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let append2 = left.append(Int::new(2));
        let edit = left.modify(1, Add(42))?;

        let mut right = OrdSequence::new();
        right.apply(append1)?;
        right.apply(append2)?;
        right.apply(edit)?;

        assert_eq!(left.to_vec(), expected);
        assert_eq!(right.to_vec(), expected);
        assert_eq!(left, right);
        Ok(())
    }

    #[test]
    fn concurrent_appends_lt() -> TestResult {
        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));

        let mut right = OrdSequence::new();
        let append2 = right.append(Int::new(2));

        left.apply(append2)?;
        right.apply(append1)?;

        assert_eq!(left, right);
        Ok(())
    }

    #[test]
    fn concurrent_appends_gt() -> TestResult {
        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(2));

        let mut right = OrdSequence::new();
        let append2 = right.append(Int::new(1));

        left.apply(append2)?;
        right.apply(append1)?;

        assert_eq!(left, right);
        Ok(())
    }

    #[test]
    fn concurrent_appends_with_edits() -> TestResult {
        let mut left: OrdSequence<UniqueTimestamp, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let edit1 = left.modify(append1.ix(), Add(2))?;

        let mut right = OrdSequence::new();
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
