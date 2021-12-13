// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::HashMap;

use super::{ObjectId, TypeName};

use git2::Reference;
use link_identities::git::Urn;

/// References to the tips of a collaborative object
#[derive(Default)]
pub struct ObjectRefs<'a> {
    /// The reference (if any) which represents the tip of the changes authored
    /// by the identity which owns the underlying storage
    pub local: Option<Reference<'a>>,
    /// Any references from peers who do not own the underlying storage
    pub remote: Vec<Reference<'a>>,
}

impl<'a> std::fmt::Debug for ObjectRefs<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let local_dbg = self
            .local
            .as_ref()
            .and_then(|r| r.name().map(|n| n.to_string()));
        let remote_dbgs = self
            .remote
            .iter()
            .filter_map(|r| r.name())
            .collect::<Vec<&str>>();
        write!(
            f,
            "ObjectRefs{{local: {:?}, remote: {:?}}}",
            local_dbg, remote_dbgs
        )
    }
}

impl<'a> ObjectRefs<'a> {
    pub fn iter<'b>(&'b self) -> impl Iterator<Item = &'b git2::Reference<'a>> {
        self.local.iter().chain(self.remote.iter())
    }
}

/// Encapsulates the layout of references to collaborative objects in a
/// repository. This is necessary in order to factor out any dependency on
/// librad
pub trait RefsStorage {
    type Error: std::error::Error;

    /// Get all references which point to a head of the change graph for a
    /// particular object
    fn object_references<'a>(
        &'a self,
        identity_urn: &Urn,
        typename: &TypeName,
        oid: &ObjectId,
    ) -> Result<ObjectRefs<'a>, Self::Error>;

    /// Get all references to objects of a given type within a particular
    /// identity
    fn type_references<'a>(
        &'a self,
        identity_urn: &Urn,
        typename: &TypeName,
    ) -> Result<HashMap<ObjectId, ObjectRefs<'a>>, Self::Error>;

    /// Update a ref to a particular collaborative object
    fn update_ref(
        &self,
        identity_urn: &Urn,
        typename: &TypeName,
        object_id: ObjectId,
        new_commit: git2::Oid,
    ) -> Result<(), Self::Error>;
}
