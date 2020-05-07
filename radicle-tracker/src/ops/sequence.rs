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

// Clipy is giving out about "i >= 0", but we're looping down from
// an index, and I'd rather not find out what happens if I do "0 - 1".
#![allow(clippy::absurd_extreme_comparisons, unused_comparisons)]

use crate::ops::{
    id::{Gen, UniqueTimestamp},
    Apply,
};
use std::{
    error,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

/// Errors that occur when modifying an [`OrdSequence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<Modify> {
    /// The index provided was out of bounds. This means we could not append or
    /// modify an item.
    IndexOutOfBounds(usize),
    /// The identifier could not be found when trying to apply a modification.
    MissingModificationId(UniqueTimestamp),
    /// The modification operation failed. The generic type represents the
    /// underlying [`Apply`] operation.
    Modify(Modify),
}

// Writing by hand because of: https://github.com/dtolnay/thiserror/issues/79
impl<Modify: fmt::Display> fmt::Display for Error<Modify> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::IndexOutOfBounds(ix) => write!(f, "index {} is out of bounds", ix),
            Error::MissingModificationId(id) => write!(f, "identifier {} was not found", id),
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

/// An `Op` is the operation that can be produced from an [`OrdSequence`], or
/// applied to an [`OrdSequence`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op<Mod, A> {
    /// Append the value (`val`) to the `OrdSequence`, ensuring it occurs at the
    /// supplied index (`ix`).
    Append {
        /// The identifier for this append operation. It is called `O` because
        /// it is needed for falling back to an `O`rdering.
        id: UniqueTimestamp,
        /// The index of where we expect this append to happen. This is to
        /// ensure that we do not append the item at a completely
        /// unrelated index. Note that if it does not end up in the
        /// desired position, it was because two items shared the same index and
        /// fell back to the `id`.
        ix: usize,
        /// The value we wish to append ot the [`OrdSequence`].
        val: A,
    },
    /// Modify the item at the index (`ix`) of the `OrdSequence` using the
    /// supplied operation (`op`).
    Modify {
        /// The identifier for this modify operation. It is called `O` because
        /// it is needed for falling back to an `O`rdering.
        id: UniqueTimestamp,
        /// The value we wish to modify ot the [`OrdSequence`].
        op: Mod,
    },
}

impl<Mod, A> Op<Mod, A> {
    /// Look at the identifier for this operation.
    pub fn id(&self) -> &UniqueTimestamp {
        match self {
            Op::Append { id, .. } => &id,
            Op::Modify { id, .. } => &id,
        }
    }
}

/// An `OrdSequence` is an [`Appendable`] data structure, which ensure that
/// appending and modification is commutative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrdSequence<M, T> {
    pub(crate) val: Vec<(UniqueTimestamp, T)>,

    // Carrying type information for
    // the modification operations.
    _marker: PhantomData<M>,
}

impl<M, T> Deref for OrdSequence<M, T> {
    type Target = Vec<(UniqueTimestamp, T)>;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

impl<M, T> DerefMut for OrdSequence<M, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl<M, T> From<OrdSequence<M, T>> for Vec<(UniqueTimestamp, T)> {
    fn from(sequence: OrdSequence<M, T>) -> Self {
        sequence.val
    }
}

impl<M, T> OrdSequence<M, T> {
    /// Create a new empty `OrdSequence`.
    pub fn new() -> Self {
        OrdSequence {
            val: vec![],
            _marker: PhantomData,
        }
    }

    /// Get the underlying vector of items.
    ///
    /// Note that this removes the identifiers from the items and avoids cloning
    /// them. If we wish to also get the identifiers, we can use `From`
    /// instance.
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.val.iter().map(|(_, t)| t).cloned().collect()
    }

    // Searching through a vector looking for the exact identifier.
    // Not my proudest moment... Very imperative.
    fn search_for_identifier<Update, Bound>(
        &self,
        time: &UniqueTimestamp,
        mut ix: usize,
        bound: Bound,
        update: Update,
    ) -> Option<usize>
    where
        Update: Fn(usize) -> usize,
        Bound: Fn(usize) -> bool,
    {
        while bound(ix) {
            let val = &self.val[ix].0;

            if val == time {
                return Some(ix);
            }

            if val.at() != time.at() {
                break;
            }
            ix = update(ix);
        }

        None
    }

    fn search_and_identify(&self, time: &UniqueTimestamp) -> Option<usize> {
        match self
            .val
            .binary_search_by(|(other, _)| time.cmp_times(other))
        {
            Ok(ix) => {
                // Are the identifiers the exact same?
                if self.val[ix].0 == *time {
                    Some(ix)
                } else {
                    // Otherwise we need to check left and right of the value.
                    // While the timestamps are the same.
                    self.search_for_identifier(&time, ix - 1, |i| i >= 0, |i| i - 1)
                        .or_else(|| {
                            self.search_for_identifier(
                                &time,
                                ix + 1,
                                |i| i < self.val.len(),
                                |i| i + 1,
                            )
                        })
                }
            },
            Err(_) => None,
        }
    }

    /// Append a new `item` to the `OrdSequence`. We get back the [`Op`] to pass
    /// onto other `OrdSequence`s.
    pub fn append(&mut self, item: T) -> Op<M, T>
    where
        T: Clone,
    {
        let id = UniqueTimestamp::gen();
        self.val.push((id.clone(), item.clone()));
        Op::Append {
            id,
            ix: self.val.len() - 1,
            val: item,
        }
    }

    /// Modify the item at `ix` in the `OrdSequence`. We get back the [`Op`] to
    /// pass onto other `OrdSequence`s, but only if the index existed since we
    /// cannot modify non-existent items.
    ///
    /// The modification operation will identify this item by its identifier
    /// rather than its index.
    pub fn modify<F>(&mut self, ix: usize, f: F) -> Result<Op<M, T>, Error<T::Error>>
    where
        T: Apply<Op = M> + Clone,
        M: Clone,
        F: FnOnce(&mut T) -> Result<M, T::Error>,
    {
        match self.val.get_mut(ix) {
            None => Err(Error::IndexOutOfBounds(ix)),
            Some((id, val)) => {
                let op = f(val)?;
                Ok(Op::Modify { id: id.clone(), op })
            },
        }
    }
}

impl<M, T: Apply<Op = M>> Apply for OrdSequence<M, T> {
    type Op = Op<M, T>;
    type Error = Error<T::Error>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Append { ix, id, val } => match self.val.get_mut(ix) {
                None => {
                    if ix >= self.len() {
                        self.val.push((id, val))
                    } else {
                        return Err(Error::IndexOutOfBounds(ix));
                    }
                    Ok(())
                },
                Some(existing) => {
                    // This will end up in non-convergence if the values
                    // are exactly equal.
                    if id < existing.0 {
                        self.val.insert(ix, (id, val))
                    } else {
                        self.val.insert(ix + 1, (id, val))
                    }
                    Ok(())
                },
            },
            // We don't actually look for the index since concurrent items
            // may get placed at a different index.
            Op::Modify { id, op } => match self.search_and_identify(&id) {
                Some(ix) => {
                    // ix exists because of search_and_identify. qed.
                    let (_, item) = &mut self.val[ix];
                    item.apply(op)?;
                    Ok(())
                },
                None => Err(Error::MissingModificationId(id)),
            },
        }
    }
}

#[cfg(test)]
pub mod strategy {
    use super::*;
    use crate::ops::id::UniqueTimestamp;
    use proptest::{collection, prelude::*};
    use std::{fmt, marker::PhantomData};

    pub fn sequence_strategy<M, T>(
        val_strategy: impl Strategy<Value = T>,
    ) -> impl Strategy<Value = OrdSequence<M, T>>
    where
        T: fmt::Debug,
        M: fmt::Debug,
    {
        collection::vec(
            val_strategy.prop_map(|v| (UniqueTimestamp::gen(), v)),
            1..50,
        )
        .prop_map(|mut val| {
            val.sort_by_key(|(t, _)| t.clone());
            OrdSequence {
                val,
                _marker: PhantomData,
            }
        })
    }

    pub fn ascii() -> impl Strategy<Value = String> {
        "[ -.|0-~]+"
    }
}

#[cfg(test)]
mod tests {
    use super::{strategy::*, *};
    use crate::ops::replace::{strategy::replace_strategy, Replace};
    use itertools::{EitherOrBoth, Itertools};
    use pretty_assertions::assert_eq;
    use proptest::{collection::vec, prelude::*};
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

        fn add(&mut self, i: u32) -> Result<Add, Infallible> {
            self.val += i;
            Ok(Add(i))
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

    fn oracle_tester<T>(
        mut sequence: OrdSequence<Replace<usize, T>, Replace<usize, T>>,
        appends: Vec<T>,
        modifications: Vec<Replace<usize, T>>,
    ) where
        T: Clone + Eq + fmt::Debug,
    {
        let mut oracle = sequence.clone();
        let mut ops = vec![];

        for item in appends.into_iter().zip_longest(modifications.into_iter()) {
            match item {
                EitherOrBoth::Both(val, op) => {
                    let append = oracle.append(Replace::new(val));
                    let modify = oracle
                        .modify(oracle.len() - 1, |val| {
                            val.replace(op.marker + 1, op.val);
                            Ok(val.clone())
                        })
                        .expect("failed to modify oracle");

                    assert_eq!(
                        modify.id(),
                        append.id(),
                        "the modification id differed from the append id"
                    );

                    ops.push(append);
                    ops.push(modify);
                },
                EitherOrBoth::Left(val) => {
                    let append = oracle.append(Replace::new(val));
                    ops.push(append)
                },
                EitherOrBoth::Right(_) => { /* skip */ },
            }
        }

        for op in ops {
            sequence.apply(op).expect("failed to apply to sequence");
        }

        assert_eq!(sequence, oracle);
    }

    #[test]
    fn oracle_test_debug() {
        let sequence: OrdSequence<Replace<usize, String>, _> = OrdSequence::new();
        let appends = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let modifications = vec![
            Replace::new("d".to_string()),
            Replace::new("e".to_string()),
            Replace::new("f".to_string()),
        ];
        oracle_tester(sequence, appends, modifications)
    }

    fn replace_first(
        sequence: &mut OrdSequence<Replace<usize, String>, Replace<usize, String>>,
        op: Replace<usize, String>,
    ) -> Result<Op<Replace<usize, String>, Replace<usize, String>>, Error<Infallible>> {
        sequence.modify(0, |val| {
            val.replace(op.marker, op.val);
            Ok(val.clone())
        })
    }

    proptest! {
        #[test]
        fn oracle_test((sequence, appends, modifications)
                       in (sequence_strategy(replace_strategy()), vec(ascii(), 1..20), vec(replace_strategy(), 1..20))) {
            oracle_tester(sequence, appends, modifications)
        }

        #[test]
        fn idempotent_modifications(x in replace_strategy()) {
            let mut left = OrdSequence::new();
            left.append(Replace::new("init".to_string()));
            let mut right = left.clone();

            replace_first(&mut left, x.clone()).expect("failed to modify left");

            replace_first(&mut right, x.clone()).expect("failed to modify right");
            replace_first(&mut right, x).expect("failed to modify right");

            assert_eq!(left, right);
        }

        #[test]
        fn commutative_modifications((x, y) in (replace_strategy(), replace_strategy())) {
            let mut left = OrdSequence::new();
            left.append(Replace::new("init".to_string()));
            let mut right = left.clone();

            replace_first(&mut left, x.clone()).expect("failed to modify left");
            replace_first(&mut left, y.clone()).expect("failed to modify left");

            replace_first(&mut right, y).expect("failed to modify right");
            replace_first(&mut right, x).expect("failed to modify right");

            assert_eq!(left, right);
        }

        #[test]
        fn associative_modifications((x, y, z) in (replace_strategy(), replace_strategy(), replace_strategy())) {
            let mut left = OrdSequence::new();
            left.append(Replace::new("init".to_string()));
            let mut right = left.clone();

            replace_first(&mut left, x.clone()).expect("failed to modify left");
            replace_first(&mut left, y.clone()).expect("failed to modify left");
            replace_first(&mut left, z.clone()).expect("failed to modify left");

            replace_first(&mut right, y).expect("failed to modify right");
            replace_first(&mut right, z).expect("failed to modify right");
            replace_first(&mut right, x).expect("failed to modify right");

            assert_eq!(left, right);
        }
    }

    #[test]
    fn sync_appends() -> TestResult {
        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
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
        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let append2 = left.append(Int::new(2));

        let mut right = OrdSequence::new();
        right.apply(append1)?;
        right.apply(append2)?;

        assert_eq!(left, right);

        Ok(())
    }

    #[test]
    fn sync_appends_and_edits() -> TestResult {
        let expected = vec![Int::new(1), Int { id: 2, val: 44 }];

        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let append2 = left.append(Int::new(2));
        let edit = left.modify(1, |val| val.add(42))?;

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
        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
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
        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
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
        let mut left: OrdSequence<Add, Int> = OrdSequence::new();
        let append1 = left.append(Int::new(1));
        let edit1 = left.modify(left.len() - 1, |val| val.add(2))?;

        let mut right = OrdSequence::new();
        let append2 = right.append(Int::new(2));
        let edit2 = right.modify(right.len() - 1, |val| val.add(3))?;

        left.apply(append2)?;
        left.apply(edit2)?;
        right.apply(append1)?;
        right.apply(edit1)?;

        assert_eq!(left, right);
        Ok(())
    }
}
