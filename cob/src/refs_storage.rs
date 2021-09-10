// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::{ObjectId, TypeName};

use git2::Reference;
use link_identities::git::Urn;

/// Encapsulates the layout of references to collaborative objects in a
/// repository. This is necessary in order to factor out any dependency on
/// librad
pub trait RefsStorage {
    type Error: std::error::Error;

    /// Get all references which point to a head of the change graph for a
    /// particular object
    fn object_references(
        &self,
        identity_urn: &Urn,
        typename: &TypeName,
        oid: &ObjectId,
    ) -> Result<Vec<Reference<'_>>, Self::Error>;

    /// Get all references to objects of a given type within a particular
    /// identity
    fn type_references(
        &self,
        identity_urn: &Urn,
        typename: &TypeName,
    ) -> Result<Vec<(ObjectId, Reference<'_>)>, Self::Error>;

    /// Update a ref to a particular collaborative object
    fn update_ref(
        &self,
        identity_urn: &Urn,
        typename: &TypeName,
        object_id: ObjectId,
        new_commit: git2::Oid,
    ) -> Result<(), Self::Error>;
}
