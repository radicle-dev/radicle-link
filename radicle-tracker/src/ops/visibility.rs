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

use crate::ops::{absurd, Apply};
use std::convert::Infallible;

/// The operation to apply to [`Visibility`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Op {}

/// Determine whether something is visible.
///
/// This can mean hiding a deleted comment or a closed issue, for example.
///
/// This can be thought of as the logical OR operator for Booleans. If
/// `Visibility` becomes `Hidden` it will always stay `Hidden`, in the same way
/// `True || x == True`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    /// Here I am!
    Visible,
    /// I'm hiding...
    Hidden,
}

impl Visibility {
    /// I'm hiding away now.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::ops::visibility::Visibility;
    ///
    /// let mut visible = Visibility::Visible;
    /// visible.hide();
    /// assert_eq!(visible, Visibility::Hidden);
    ///
    /// // And it stays hidden.
    /// visible.hide();
    /// assert_eq!(visible, Visibility::Hidden);
    /// ```
    pub fn hide(&mut self) -> Op {
        self.apply(Op {}).unwrap_or_else(absurd);
        Op {}
    }
}

impl Apply for Visibility {
    type Op = Op;
    type Error = Infallible;

    fn apply(&mut self, _op: Self::Op) -> Result<(), Self::Error> {
        *self = Visibility::Hidden;
        Ok(())
    }
}

#[cfg(test)]
pub mod strategy {
    use super::*;
    use proptest::prelude::*;

    pub fn visibility_strategy() -> impl Strategy<Value = Visibility> {
        prop_oneof![Just(Visibility::Visible), Just(Visibility::Hidden)]
    }
}

#[cfg(test)]
mod tests {
    use super::{strategy::visibility_strategy, *};
    use crate::ops::absurd;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn idempotent(visibility in visibility_strategy()) {
            let mut left = visibility.clone();
            left.apply(Op {}).unwrap_or_else(absurd);

            let mut right = visibility;
            right.apply(Op {}).unwrap_or_else(absurd);
            right.apply(Op {}).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }

        #[test]
        fn commutative(visibility in visibility_strategy()) {
            let x = Op {};
            let y = Op {};

            let mut left = visibility.clone();
            left.apply(x.clone()).unwrap_or_else(absurd);
            left.apply(y.clone()).unwrap_or_else(absurd);

            let mut right = visibility;
            right.apply(y).unwrap_or_else(absurd);
            right.apply(x).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }

        #[test]
        fn associative(visibility in visibility_strategy()) {
            let x = Op {};
            let y = Op {};
            let z = Op {};

            let mut left = visibility.clone();
            left.apply(x.clone()).unwrap_or_else(absurd);
            left.apply(y.clone()).unwrap_or_else(absurd);
            left.apply(z.clone()).unwrap_or_else(absurd);

            let mut right = visibility;
            right.apply(y).unwrap_or_else(absurd);
            right.apply(z).unwrap_or_else(absurd);
            right.apply(x).unwrap_or_else(absurd);

            assert_eq!(left, right);
        }
    }
}
