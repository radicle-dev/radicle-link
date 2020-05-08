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

use crate::ops::{
    absurd,
    visibility::{self, Visibility},
    Apply,
};

/// A value that can be placed in a [`crate::thread::Thread`]. The value is
/// placed alongside a `visibility` to mark that the `Item` can be "deleted".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item<A> {
    /// The value of the `Item`.
    val: A,
    /// If the [`Visibility::`Hidden`] then this `Item` is marked as deleted,
    /// otherwise it was only created.
    visibility: Visibility,
}

impl<A> Item<A> {
    /// Create a new `Item`, where the `visibility` is [`Visibility::Visible`].
    pub fn new(val: A) -> Self {
        Item {
            val,
            visibility: Visibility::Visible,
        }
    }

    /// Modify the value in the `Item`. The function supplied should return the
    /// operation that represents this modification. This operation will
    /// then be embedded in [`Op::Edit`].
    pub fn edit<F, E>(&mut self, f: F) -> Result<Op<E>, A::Error>
    where
        A: Apply<Op = E>,
        F: FnOnce(&mut A) -> Result<E, A::Error>,
    {
        let op = f(&mut self.val)?;
        Ok(Op::Edit(op))
    }

    /// Delete the value in the `Item`. This operation does not perform a hard
    /// deletion of the data. Instead, it "hides" the data. See
    /// [`Visibility`].
    pub fn delete<E>(&mut self) -> Op<E> {
        Op::Delete(self.visibility.hide())
    }

    /// Look at the value of the `Item`.
    pub fn val(&self) -> &A {
        &self.val
    }

    /// Look at the deletion status value of the `Item`.
    pub fn status(&self) -> &Visibility {
        &self.visibility
    }
}

/// The operations that can be applied to an [`Item`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op<E> {
    /// Edit the underlying value of an [`Item`] using some operation `E` that
    /// implements [`Apply`].
    Edit(E),
    /// Delete the [`Item`] by turning the visibility to [`Visibility::Hidden`].
    Delete(visibility::Op),
}

impl<E, A: Apply<Op = E>> Apply for Item<A> {
    type Op = Op<E>;
    type Error = A::Error;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        match op {
            Op::Edit(e) => self.val.apply(e),
            Op::Delete(h) => self.visibility.apply(h).map_err(absurd),
        }
    }
}

#[cfg(test)]
pub mod strategy {
    use super::*;
    use crate::ops::{
        replace::{strategy::replace_strategy, Replace},
        visibility::{self, strategy::visibility_strategy},
    };
    use proptest::prelude::*;

    type ReplaceItem = Item<Replace<usize, String>>;
    type ReplaceOp = Op<Replace<usize, String>>;

    pub fn item_strategy() -> impl Strategy<Value = ReplaceItem> {
        (replace_strategy(), visibility_strategy()).prop_map(|(replace, visibility)| Item {
            val: replace,
            visibility,
        })
    }

    pub fn op_strategy() -> impl Strategy<Value = ReplaceOp> {
        prop_oneof![
            Just(Op::Delete(visibility::Op {})),
            replace_strategy().prop_map(Op::Edit)
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{strategy::*, *};
    use crate::ops::absurd;
    use proptest::prelude::*;

    // Items are idempotent, commutative, and associative as long as the
    // Edit operation also is.
    proptest! {
        #[test]
        fn idempotent((item, x) in (item_strategy(), op_strategy())) {
            let mut left = item.clone();
            left.apply(x.clone()).unwrap_or_else(absurd);

            let mut right = item;
            right.apply(x.clone()).unwrap_or_else(absurd);
            right.apply(x).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }

        #[test]
        fn commutative((item, x, y) in (item_strategy(), op_strategy(), op_strategy())) {
            let mut left = item.clone();
            left.apply(x.clone()).unwrap_or_else(absurd);
            left.apply(y.clone()).unwrap_or_else(absurd);

            let mut right = item;
            right.apply(y).unwrap_or_else(absurd);
            right.apply(x).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }

        #[test]
        fn associative((item, x, y, z) in (item_strategy(), op_strategy(), op_strategy(), op_strategy())) {
            let mut left = item.clone();
            left.apply(x.clone()).unwrap_or_else(absurd);
            left.apply(y.clone()).unwrap_or_else(absurd);
            left.apply(z.clone()).unwrap_or_else(absurd);

            let mut right = item;
            right.apply(y).unwrap_or_else(absurd);
            right.apply(z).unwrap_or_else(absurd);
            right.apply(x).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }
    }
}
