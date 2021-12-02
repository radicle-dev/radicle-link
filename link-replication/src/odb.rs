// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::path::Path;

use link_git::{
    object,
    odb,
    protocol::{oid, ObjectId},
};

pub struct Object<'a> {
    pub kind: object::Kind,
    pub data: &'a [u8],
}

impl<'a> From<odb::Object<'a>> for Object<'a> {
    fn from(odb::Object { kind, data, .. }: odb::Object<'a>) -> Self {
        Self { kind, data }
    }
}

impl<'a> From<Object<'a>> for (object::Kind, &'a [u8]) {
    fn from(Object { kind, data }: Object<'a>) -> Self {
        (kind, data)
    }
}

pub trait Odb {
    type LookupError: std::error::Error + Send + Sync + 'static;
    type RevwalkError: std::error::Error + Send + Sync + 'static;
    type AddPackError: std::error::Error + Send + Sync + 'static;

    /// Test if the given [`oid`] is present in any of the [`Odb`]'s backends.
    ///
    /// May return false negatives if the [`Odb`] hasn't loaded a packfile yet.
    /// It is advisable to call [`Odb::add_pack`] explicitly where possible.
    ///
    /// Note that this behaves like [`std::path::Path::is_file`]: I/O errors
    /// translate to `false`.
    fn contains(&self, oid: impl AsRef<oid>) -> bool;

    fn lookup<'a>(
        &self,
        oid: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
    ) -> Result<Option<Object<'a>>, Self::LookupError>;

    fn is_in_ancestry_path(
        &self,
        new: impl Into<ObjectId>,
        old: impl Into<ObjectId>,
    ) -> Result<bool, Self::RevwalkError>;

    /// Make the [`Odb`] aware of a packfile.
    ///
    /// The [`Path`] may point to either the pack (_*.pack_) or index (_*.idx_).
    fn add_pack(&self, path: impl AsRef<Path>) -> Result<(), Self::AddPackError>;
}
