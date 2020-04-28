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

pub trait Gen {
    fn gen() -> Self;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unique {
    blob: Vec<u8>,
}

impl Unique {
    pub fn new(id: &[u8]) -> Option<Self> {
        if id.is_empty() {
            return None;
        }

        Some(Unique { blob: id.to_vec() })
    }

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    time: TimeDiff,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueTimestamp {
    unique: Unique,
    time: Timestamp,
}

impl PartialOrd for UniqueTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.time.partial_cmp(&other.time)
    }
}

impl Ord for UniqueTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time)
    }
}

impl UniqueTimestamp {
    pub fn new(id: &[u8], time: Timestamp) -> Option<Self> {
        Unique::new(id).map(|unique| UniqueTimestamp { unique, time })
    }

    pub fn val(&self) -> (&[u8], &Timestamp) {
        (self.unique.val(), &self.time)
    }

    pub fn at(&self) -> &Timestamp {
        &self.time
    }
}

impl Gen for UniqueTimestamp {
    fn gen() -> Self {
        UniqueTimestamp {
            unique: Unique::gen(),
            time: Timestamp {
                time: TimeDiff::from(SystemTime::now()),
            },
        }
    }
}
