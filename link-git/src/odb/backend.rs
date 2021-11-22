// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use git_hash::oid;
use git_pack::{cache::DecodeEntry, data::Object};

use super::{index, pack, window};

pub type Loose = git_odb::loose::Store;

pub struct Packed<I, D> {
    pub index: I,
    pub data: D,
}

impl<I, D> Packed<I, D>
where
    I: index::Index,
    D: window::Cache,
{
    pub fn contains(&self, id: impl AsRef<oid>) -> bool {
        self.index.contains(id)
    }

    pub fn find<'a>(
        &self,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, index::error::Lookup<pack::error::Data>> {
        self.index
            .lookup(|info| self.data.get(info), id, buf, cache)
    }
}
