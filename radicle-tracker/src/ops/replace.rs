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
use std::convert::Infallible;

/// A data structure that allows you to track the replacement of some value `A`.
/// The change to the value is tracked via the `Marker`.
///
/// A higher `Marker` will always be accepted as the most up to date
/// replacement. If the two `Marker`s are equal, and the values are also equal,
/// then there is no change. If the values are different, however, the conflicts
/// are kept track of for that replacement. When there are conflicts, and a
/// higher `Marker` is found again, then the conflicts are cleared.
///
/// **N.B.**: A `Replace` is its own `Op` for its [`Apply`] instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Replace<Marker: Ord, A> {
    /// An ordered marker for keeping track of replacements.
    pub marker: Marker,
    /// The current value.
    pub val: A,
    conflicts: Vec<A>,
}

impl<Marker: Ord, A: Eq> Replace<Marker, A> {
    /// Create a new `Replace` with the initial value.
    /// The `Marker` is set to its [`Default`] value.
    pub fn new(val: A) -> Self
    where
        Marker: Default,
    {
        Self::new_with_marker(val, Marker::default())
    }

    /// Create a new `Replace` with the provided value and marker.
    pub fn new_with_marker(val: A, marker: Marker) -> Self
    where
        Marker: Default,
    {
        Replace {
            marker,
            val,
            conflicts: vec![],
        }
    }

    /// Replace the current value with the provided one, if the passed marker is
    /// greater than the current one.
    pub fn replace(&mut self, marker: Marker, val: A) {
        if self.marker < marker {
            self.marker = marker;
            self.val = val;
            self.conflicts = vec![];
        } else if self.marker == marker && self.val != val {
            self.conflicts.push(val);
        }
    }

    /// Peek at the conflicts for this `Replace`.
    pub fn conflicts(&self) -> &[A] {
        &self.conflicts
    }
}

impl<Marker: Ord, A: Eq> Apply for Replace<Marker, A> {
    type Op = Replace<Marker, A>;
    type Error = Infallible;

    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error> {
        self.replace(op.marker, op.val);
        Ok(())
    }
}

#[cfg(test)]
pub mod strategy {
    use super::*;
    use proptest::prelude::*;

    pub fn replace_strategy() -> impl Strategy<Value = Replace<usize, String>> {
        (any::<usize>(), "[a-z]*").prop_map(|(marker, val)| Replace {
            marker,
            val: val.to_string(),
            conflicts: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{strategy::replace_strategy, *};
    use crate::ops::absurd;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn idempotent(mut x in replace_strategy()) {
            let original = x.clone();
            x.apply(x.clone()).unwrap_or_else(absurd);
            assert_eq!(original, x);
        }

        #[test]
        fn commutative((x, y) in (replace_strategy(), replace_strategy())) {
            let mut x_result = x.clone();
            let mut y_result = y.clone();
            x_result.apply(y).unwrap_or_else(absurd);
            y_result.apply(x).unwrap_or_else(absurd);

            assert_eq!(x_result, y_result);
        }

        #[test]
        fn associative((x, mut y, z) in (replace_strategy(), replace_strategy(), replace_strategy())) {
            let mut left = x.clone();
            let mut right = x.clone();

            left.apply(y.clone()).unwrap_or_else(absurd);
            left.apply(z.clone()).unwrap_or_else(absurd);

            y.apply(z).unwrap_or_else(absurd);
            right.apply(y).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }
    }

    #[test]
    fn commutative_ints() {
        // Start with 1
        let mut left: Replace<usize, u8> = Replace::new(1);
        // Replace 1 with 2
        left.replace(left.marker + 1, 2);
        let r1 = left.clone();

        // Replace 2 with 3
        left.replace(left.marker + 1, 2);
        let r2 = left.clone();

        let mut right = Replace::new(1);
        right.apply(r2).expect("absurdium");
        right.apply(r1).expect("absurdium");

        assert_eq!(left, right);
    }

    #[test]
    fn concurrent_replace() {
        // Left starts with 1
        let mut left: Replace<usize, u8> = Replace::new(1);

        // Replace 1 with 2
        left.replace(left.marker + 1, 2);

        // Right also starts with 1
        let mut right = Replace::new(1);

        // Replace 1 with 3
        right.replace(right.marker + 1, 3);

        left.apply(right.clone()).expect("absurdium");
        right.apply(left.clone()).expect("absurdium");

        // Concurrent replace will store conflicts locally.
        // The user should be expected try resolve the conflicts.
        assert!(left != right);
        assert_eq!(left.conflicts, vec![3]);
        assert_eq!(right.conflicts, vec![2]);

        // One way is to apply a higher marker.
        right.replace(right.marker + 1, 3);
        left.apply(right.clone()).expect("absurdium");
        assert_eq!(left, right);
    }
}
