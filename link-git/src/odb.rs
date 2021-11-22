// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_hash::oid;
use thiserror::Error;

pub mod backend;
pub mod index;
pub mod pack;
pub mod window;

pub use git_pack::{cache, data::Object};

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Packed(#[from] index::error::Lookup<pack::error::Data>),

    #[error(transparent)]
    Loose(#[from] git_odb::loose::find::Error),
}

pub struct Odb<I, D> {
    pub loose: backend::Loose,
    pub packed: backend::Packed<I, D>,
}

impl<I, D> Odb<I, D>
where
    I: index::Index,
    D: window::Cache,
{
    pub fn contains(&self, id: impl AsRef<oid>) -> bool {
        self.packed.contains(id.as_ref()) || self.loose.contains(id)
    }

    pub fn find<'a>(
        &self,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl cache::DecodeEntry,
    ) -> Result<Option<Object<'a>>, Error> {
        let id = id.as_ref();
        if self.packed.contains(id) {
            return self.packed.find(id, buf, cache).map_err(Into::into);
        }
        self.loose.try_find(id, buf).map_err(Into::into)
    }
}
