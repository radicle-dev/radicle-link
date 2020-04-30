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

use crate::ops::Apply;
use std::{collections::HashSet, error, fmt, hash::Hash};

/// Error case(s) for when we apply the operations to the [`Set`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Error<A> {
    /// We tried to remove the element `A` and it was not in the `Set`.
    NonExistentRemove(A),
}

// Writing by hand because of: https://github.com/dtolnay/thiserror/issues/79
impl<A: fmt::Display> fmt::Display for Error<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NonExistentRemove(a) => write!(f, "the item {} does not exist in the set", a),
        }
    }
}

impl<A: fmt::Debug + fmt::Display> error::Error for Error<A> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

/// The operations that can be applied to a [`Set`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Op<A> {
    /// Insert an `A` into the [`Set`].
    Insert(A),
    /// Remove an `A` from the [`Set`].
    Remove(A),
}

/// Similar to a [`HashSet`], where we can only `insert` or `remove` items
/// to/from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set<A: Eq + Hash>(pub(crate) HashSet<A>);

impl<A: Eq + Hash> Set<A> {
    /// Create a new empty `Set`.
    pub fn new() -> Self {
        Set(HashSet::new())
    }

    /// Insert a value into the set. We get back an [`Op`] to signify to others
    /// our insertion into the `Set`.
    pub fn insert(&mut self, a: A) -> Op<A>
    where
        A: Clone,
    {
        self.0.insert(a.clone());
        Op::Insert(a)
    }

    /// Remove a value from the set. We get back an [`Op`] to signify to others
    /// our removal from the `Set`.
    pub fn remove(&mut self, a: A) -> Option<Op<A>> {
        if !self.contains(&a) {
            return None;
        }

        self.0.remove(&a);
        Some(Op::Remove(a))
    }

    /// Check if the `Set` already contains this element.
    pub fn contains(&self, a: &A) -> bool {
        self.0.contains(&a)
    }

    /// Look at the [`HashSet`] for this `Set`.
    pub fn peek(&self) -> &HashSet<A> {
        &self.0
    }
}

impl<A: Eq + Hash> Apply for Set<A> {
    type Op = Op<A>;
    type Error = Error<A>;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Insert(a) => {
                self.0.insert(a);
            },
            Op::Remove(a) => {
                if !self.contains(&a) {
                    return Err(Error::NonExistentRemove(a));
                }

                self.0.remove(&a);
            },
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::{collection::hash_set, prelude::*};
    use std::error;

    fn insert_strategy() -> impl Strategy<Value = Op<String>> {
        "[a-z]*".prop_map(Op::Insert)
    }

    fn set_strategy() -> impl Strategy<Value = Set<String>> {
        hash_set("[a-z]*", 0..50).prop_map(Set)
    }

    // We can guarantee these properties if we are only inserting into the set
    proptest! {
        #[test]
        fn idempotent((mut set, x) in (set_strategy(), insert_strategy())) {
            let mut left = set.clone();
            left.apply(x.clone()).expect("Failed to apply x");

            set.apply(x.clone()).expect("Failed to apply x");
            set.apply(x).expect("Failed to apply x");
            assert_eq!(left, set);
        }

        #[test]
        fn commutative((set, x, y) in (set_strategy(), insert_strategy(), insert_strategy())) {
            let mut left = set.clone();
            left.apply(x.clone()).expect("Failed to apply x");
            left.apply(y.clone()).expect("Failed to apply y");

            let mut right = set.clone();
            right.apply(y).expect("Failed to apply y");
            right.apply(x).expect("Failed to apply x");

            assert_eq!(left, right);
        }

        #[test]
        fn associative((set, x, y, z) in (set_strategy(), insert_strategy(), insert_strategy(), insert_strategy())) {
            let mut left = set.clone();
            left.apply(x.clone()).expect("Failed to apply x");
            left.apply(y.clone()).expect("Failed to apply y");
            left.apply(z.clone()).expect("Failed to apply z");

            let mut right = set.clone();
            right.apply(y).expect("Failed to apply y");
            right.apply(z).expect("Failed to apply z");
            right.apply(x).expect("Failed to apply x");

            assert_eq!(left, right);
        }
    }

    #[test]
    fn delete_only_after_insert() -> Result<(), Box<dyn error::Error + 'static>> {
        let mut left = Set::new();

        // We can insert "a" and we can also remove it.
        let insert_a = left.insert("a");
        let remove_a = left.remove("a");
        assert!(remove_a.is_some());

        // We can't remove "b" because it's not in the set.
        let remove_b = left.remove("b");
        assert!(remove_b.is_none());

        // We apply the same operations in order to get the same set.
        let mut right = Set::new();
        right.apply(insert_a)?;
        right.apply(remove_a.clone().unwrap())?;
        assert_eq!(left, right);

        // However if they're out of order then it fails.
        let mut out_of_order = Set::new();
        let removal_error = out_of_order.apply(remove_a.unwrap());
        assert!(removal_error.is_err());

        Ok(())
    }
}
