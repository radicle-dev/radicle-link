// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[macro_use]
extern crate async_trait;

pub mod data;
pub use data::Data;

pub mod track;
pub use track::Track;

pub mod hook;
pub use hook::{Hooks, Notification};

mod sealed;

pub trait Display: sealed::Sealed {
    fn display(&self) -> String;
}

pub trait IsZero {
    fn is_zero(&self) -> bool;
}

/// The updated summary of the `old` and `new` revisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Updated {
    /// Both `old` and `new` are the zero revision.
    Zero,
    /// `old` is the zero revision and `new` is a non-zero revision.
    Created,
    /// `old` is a non-zero revision and `new` is the zero revision.
    Deleted,
    /// Both `old` and `new` are non-zero revisions.
    Changed,
    /// Both `old` and `new` are the same non-zero revision.
    NoChange,
}

#[cfg(feature = "git")]
mod git {
    use git2::Oid;
    use radicle_git_ext as ext;

    use super::IsZero;

    impl IsZero for Oid {
        fn is_zero(&self) -> bool {
            self == &Oid::zero()
        }
    }

    impl IsZero for ext::Oid {
        fn is_zero(&self) -> bool {
            git2::Oid::from(*self).is_zero()
        }
    }
}
