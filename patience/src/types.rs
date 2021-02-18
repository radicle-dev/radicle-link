// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

use super::sealed;

/// `Queries` is a wrapper around `usize` so that we can differentiate it from
/// [`Clones`].
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Queries {
    /// The max number of queries allowed per request.
    Max(usize),
    /// The max number is infinite, and so we allow the request to never time
    /// out.
    Infinite,
}

impl Queries {
    /// Create a new `Queries` wrapping around `n`.
    #[must_use]
    pub const fn new(n: usize) -> Self {
        Self::Max(n)
    }
}

impl From<Queries> for Option<usize> {
    fn from(other: Queries) -> Self {
        match other {
            Queries::Max(i) => Some(i),
            Queries::Infinite => None,
        }
    }
}

impl Add<usize> for Queries {
    type Output = Self;

    fn add(self, other: usize) -> Self::Output {
        match self {
            Self::Max(i) => Self::Max(i + other),
            Self::Infinite => Self::Infinite,
        }
    }
}

impl AddAssign<usize> for Queries {
    fn add_assign(&mut self, other: usize) {
        match self {
            Self::Max(i) => *i += other,
            Self::Infinite => {},
        }
    }
}

/// `Clones` is a wrapper around `usize` so that we can differentiate it from
/// [`Queries`].
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Clones {
    /// The max number of clones allowed per request.
    Max(usize),
    /// The max number is infinite, and so we allow the request to never time
    /// out.
    Infinite,
}

impl Clones {
    /// Create a new `Clones` wrapping around `n`.
    #[must_use]
    pub const fn new(n: usize) -> Self {
        Self::Max(n)
    }
}

impl From<Clones> for Option<usize> {
    fn from(other: Clones) -> Self {
        match other {
            Clones::Max(i) => Some(i),
            Clones::Infinite => None,
        }
    }
}

impl Add<usize> for Clones {
    type Output = Self;

    fn add(self, other: usize) -> Self::Output {
        match self {
            Self::Max(i) => Self::Max(i + other),
            Self::Infinite => Self::Infinite,
        }
    }
}

impl AddAssign<usize> for Clones {
    fn add_assign(&mut self, other: usize) {
        match self {
            Self::Max(i) => *i += other,
            Self::Infinite => {},
        }
    }
}

/// The number of different attempts a `Request` has made during its lifetime.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempts {
    /// The number of query attempts we have made.
    pub(super) queries: Queries,
    /// The number of clone attempts we have made.
    pub(super) clones: Clones,
}

impl Attempts {
    /// Get a new `Attempts` where the number of queires and clones is initially
    /// `0`.
    #[must_use]
    pub const fn new() -> Self {
        Attempts {
            queries: Queries::Max(0),
            clones: Clones::Max(0),
        }
    }

    /// Construct an `Attempts` where the number of queries and clones is
    /// `Infinite`.
    #[must_use]
    pub const fn infinite() -> Self {
        Attempts {
            queries: Queries::Infinite,
            clones: Clones::Infinite,
        }
    }
}

impl Default for Attempts {
    fn default() -> Self {
        Attempts::new()
    }
}

impl sealed::Sealed for Attempts {}
