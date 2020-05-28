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

use std::{borrow::Borrow, ops::Deref};

use TryCow::*;

/// A fallible version of [`std::borrow::ToOwned`]
pub trait TryToOwned {
    type Owned: Borrow<Self>;
    type Error: std::error::Error;

    fn try_to_owned(&self) -> Result<Self::Owned, Self::Error>;
}

/// A fallible version of [`std::borrow::Cow`]
///
/// Instead of [`std::borrow::ToOwned`], turning a borrowed [`TryCow`] value
/// into an owned one requires [`TryToOwned`], thus the conversion may fail.
pub enum TryCow<'a, B>
where
    B: 'a + TryToOwned + ?Sized,
{
    Borrowed(&'a B),
    Owned(<B as TryToOwned>::Owned),
}

impl<T: TryToOwned> TryCow<'_, T> {
    pub fn try_into_owned(self) -> Result<<T as TryToOwned>::Owned, <T as TryToOwned>::Error> {
        match self {
            Borrowed(borrowed) => borrowed.try_to_owned(),
            Owned(owned) => Ok(owned),
        }
    }

    pub fn try_to_mut(
        &mut self,
    ) -> Result<&mut <T as TryToOwned>::Owned, <T as TryToOwned>::Error> {
        match *self {
            Borrowed(borrowed) => {
                *self = borrowed.try_to_owned().map(Owned)?;
                match *self {
                    Borrowed(..) => unreachable!(),
                    Owned(ref mut owned) => Ok(owned),
                }
            },

            Owned(ref mut owned) => Ok(owned),
        }
    }
}

impl<T: TryToOwned + ?Sized> Deref for TryCow<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match *self {
            Borrowed(borrowed) => borrowed,
            Owned(ref owned) => owned.borrow(),
        }
    }
}

impl<T: TryToOwned + ?Sized> AsRef<T> for TryCow<'_, T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<'a, T: TryToOwned + ?Sized> From<&'a T> for TryCow<'a, T> {
    fn from(t: &'a T) -> Self {
        Borrowed(t)
    }
}
