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

use crate::metadata::clock::TimeDiff;
use std::{cmp::Ordering, hash::Hash, time::SystemTime};
use uuid::Uuid;

/// Magically generate a thing.
pub trait Gen {
    /// Abrakedabra!
    fn gen() -> Self;
}

/// A unique identifier consisting of a blob of bytes (for some meaning of
/// unique).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unique {
    blob: Vec<u8>,
}

impl Unique {
    /// Peek at the bytes of the unique identifier.
    pub fn val(&self) -> &[u8] {
        &self.blob
    }
}

impl Gen for Unique {
    fn gen() -> Self {
        Unique {
            blob: Uuid::new_v4().as_bytes().to_vec(),
        }
    }
}

/// A combination of [`Unique`] identifier along with a [`TimeDiff`].
///
/// The ordering of a `UniqueTimestamp` first relies on its `TimeDiff` falling
/// back to the identifier if the times were equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueTimestamp {
    unique: Unique,
    time: TimeDiff,
}

impl PartialOrd for UniqueTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for UniqueTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.time.cmp(&other.time) {
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
            Ordering::Equal => self.unique.cmp(&other.unique),
        }
    }
}

impl UniqueTimestamp {
    /// Peek at the identifier bytes and the `TimeDiff`.
    pub fn val(&self) -> (&[u8], &TimeDiff) {
        (self.unique.val(), &self.time)
    }

    /// Peek at the `TimeDiff`.
    pub fn at(&self) -> &TimeDiff {
        &self.time
    }
}

impl Gen for UniqueTimestamp {
    fn gen() -> Self {
        UniqueTimestamp {
            unique: Unique::gen(),
            time: TimeDiff::from(SystemTime::now()),
        }
    }
}
