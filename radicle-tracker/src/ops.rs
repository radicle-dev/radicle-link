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

/// Sometimes it's necessary to keep track of a unique and orderable identity in
/// our structures. The `id` module gives us primitives to use when this is
/// needed. For example, see the [`crate::ops::sequence`] module.
pub mod id;
/// A data structure that allows us to keep track of replacing values over time.
pub mod replace;
/// Contains `OrdSequence`, a data structure that allows us to append to it,
/// resolving conflicts by an ordered identity on the element.
pub mod sequence;
/// Contains `Set`, a `HashSet` like structure that is limited to inserting and
/// removing items.
pub mod set;
/// A data structure that designates whether something should be visible or
/// hidden from view.
pub mod visibility;

use std::convert::Infallible;

/// `Apply` allows us to apply an operation to a data structure. If something
/// implements `Apply` it should safely grow the structure monotonically or else
/// being "embarassingly" convergent.
pub trait Apply {
    /// The operation to apply to the data structure.
    type Op;

    /// Any error that occurs when attempting to apply the operation to the data
    /// structure. If an error isn't possible then use [`Infallible`].
    type Error;

    /// Apply the operation to the data structure.
    fn apply(&mut self, op: Self::Op) -> Result<(), Self::Error>;
}

/// Since we can never _create_ an [`Infallible`] we can never truly call this
/// function. This means we can call this function, pretending like we get back
/// an `A`.
///
/// This is useful when we want to unwrap a `Result` knowing the `Err` is
/// `Infallible`.
pub(crate) fn absurd<A>(_infallible: Infallible) -> A {
    panic!("Infallible cannot do anything else")
}

/// TODO: Fold the things
pub fn fold_apply<S>(state: &mut S, ops: impl Iterator<Item = S::Op>) -> Result<(), S::Error>
where
    S: Apply,
{
    for op in ops {
        state.apply(op)?
    }

    Ok(())
}
