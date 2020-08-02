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

use std::collections::{BTreeMap, BTreeSet};

use super::{Error, Identity, Revision, Verified};

/// API for a cache or identity revision verification status
pub trait VerificationCache {
    /// Check if the given revision is verified
    fn is_verified(&self, rev: &Revision) -> bool;

    /// Register a newly verified revision
    /// (it errors if the revision happens to be a fork)
    fn register_verified(&mut self, id: &Identity<Verified>) -> Result<(), Error>;
}

/// Cache entry
struct CachedRevision {
    /// The children of this revision
    pub children: BTreeSet<Revision>,
    /// Flag for forked revisions
    pub is_forked: bool,
}

impl CachedRevision {
    /// New cache entry
    pub fn new(is_forked: bool) -> Self {
        Self {
            children: BTreeSet::new(),
            is_forked,
        }
    }

    /// New cache entry with a given child
    pub fn new_with_child(child: Revision) -> Self {
        let mut result = Self {
            children: BTreeSet::new(),
            is_forked: false,
        };
        result.children.insert(child);
        result
    }

    /// adds a child to this entry
    /// (returns true if the insertion causes a fork)
    pub fn add_child(&mut self, child: Revision) -> bool {
        self.children.insert(child);
        let forked = self.children.len() > 1;
        if forked {
            self.is_forked = true;
        }
        forked
    }
}

#[derive(Default)]
/// In memory cache for revision verification status
pub struct MemoryCache {
    revisions: BTreeMap<Revision, CachedRevision>,
}

impl MemoryCache {
    /// Clear the cache
    pub fn clear(&mut self) {
        self.revisions.clear()
    }

    /// Flag a revision as forked and propagate the flag to its children
    fn set_forked(&mut self, start: &Revision) {
        let mut pending = Vec::new();
        pending.push(start.clone());
        while let Some(current) = pending.pop() {
            if let Some(entry) = self.revisions.get_mut(&current) {
                entry.is_forked = true;
                for child in entry.children.iter() {
                    pending.push(child.clone());
                }
            }
        }
    }
}

impl VerificationCache for MemoryCache {
    fn is_verified(&self, rev: &Revision) -> bool {
        self.revisions
            .get(rev)
            .map_or(false, |entry| !entry.is_forked)
    }

    fn register_verified(&mut self, id: &Identity<Verified>) -> Result<(), Error> {
        let mut missing_parent = None;

        let forked = id.doc().replaces().map_or(false, |parent| {
            self.revisions.get_mut(parent).map_or_else(
                || {
                    missing_parent = Some(parent.clone());
                    false
                },
                |parent_entry| parent_entry.add_child(id.revision().clone()),
            )
        });
        if forked {
            if let Some(parent) = id.doc().replaces() {
                self.set_forked(parent)
            }
        }

        if let Some(missing_parent) = missing_parent {
            self.revisions.insert(
                missing_parent,
                CachedRevision::new_with_child(id.revision().clone()),
            );
        }

        if self.revisions.contains_key(id.revision()) {
            if forked {
                self.set_forked(id.revision());
            }
        } else {
            self.revisions
                .insert(id.revision().clone(), CachedRevision::new(forked));
        }

        if forked {
            Err(Error::ForkDetected)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[derive(Default)]
    pub struct NullVerificationCache {}

    impl VerificationCache for NullVerificationCache {
        fn is_verified(&self, _rev: &Revision) -> bool {
            false
        }
        fn register_verified(&mut self, _id: &Identity<Verified>) -> Result<(), Error> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub struct TrueVerificationCache {}

    impl VerificationCache for TrueVerificationCache {
        fn is_verified(&self, _rev: &Revision) -> bool {
            true
        }
        fn register_verified(&mut self, _id: &Identity<Verified>) -> Result<(), Error> {
            Ok(())
        }
    }
}
