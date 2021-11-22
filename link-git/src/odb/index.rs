// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::VecDeque, fs, io, iter::FromIterator, path::Path, sync::Arc};

use arc_swap::ArcSwap;
use git_hash::oid;
use git_pack::{
    cache::DecodeEntry,
    data::{Object, ResolvedBase},
};
use parking_lot::Mutex;
use tracing::trace;

use super::pack;

pub use git_pack::index::File as IndexFile;

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Discover {
        #[error(transparent)]
        Index(#[from] pack::error::Index),

        #[error(transparent)]
        Io(#[from] io::Error),
    }

    #[derive(Debug, Error)]
    pub enum Lookup<E> {
        #[error(transparent)]
        Lookup(E),

        #[error(transparent)]
        Decode(#[from] git_pack::data::decode_entry::Error),
    }
}

pub trait Index {
    fn contains(&self, id: impl AsRef<oid>) -> bool;

    fn lookup<'a, F, E>(
        &self,
        pack_cache: F,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, error::Lookup<E>>
    where
        F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>;
}

/// Attempt to load all pack index files from the provided `GIT_DIR`.
///
/// The returned [`Vec`] is sorted by the file modification time (earlier
/// first).
pub fn discover(git_dir: impl AsRef<Path>) -> Result<Vec<pack::Index>, error::Discover> {
    let pack_dir = git_dir.as_ref().join("objects").join("pack");

    let mut paths = Vec::new();
    trace!("discovering packs at {}", pack_dir.display());
    for entry in fs::read_dir(&pack_dir)? {
        let entry = entry?;
        let path = entry.path();
        trace!("{}", path.display());
        let meta = entry.metadata()?;
        if meta.file_type().is_file() && path.extension().unwrap_or_default() == "idx" {
            let mtime = meta.modified()?;
            paths.push((path, mtime));
        }
    }
    paths.sort_by(|(_, mtime_a), (_, mtime_b)| mtime_a.cmp(mtime_b));

    let indices = paths
        .into_iter()
        .map(|(path, _)| Ok(pack::Index::open(path)?))
        .collect::<Result<_, error::Discover>>()?;

    Ok(indices)
}

/// An [`Index`] which can be shared between threads.
///
/// Writes are guarded by a [`Mutex`], while reads are lock-free (and mostly
/// wait-free). [`Shared`] does not automatically detect changes on the
/// filesystem.
///
/// Lookup methods traverse the set of indices in reverse order, so the iterator
/// to construct a [`Shared`] from via its [`FromIterator`] impl should yield
/// elements an appropriate order. Usually, more recently created indices are
/// more likely to be accessed than older ones.
pub struct Shared {
    indices: ArcSwap<im::Vector<Arc<pack::Index>>>,
    write: Mutex<()>,
}

impl FromIterator<pack::Index> for Shared {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = pack::Index>,
    {
        Self {
            indices: ArcSwap::new(Arc::new(iter.into_iter().map(Arc::new).collect())),
            write: Mutex::new(()),
        }
    }
}

impl Shared {
    /// Add a newly discovered [`pack::Index`].
    ///
    /// This index will be considered first by subsequent lookups.
    pub fn push(&self, idx: pack::Index) {
        let lock = self.write.lock();
        let mut new = self.indices.load_full();
        Arc::make_mut(&mut new).push_back(Arc::new(idx));
        self.indices.store(new);
        drop(lock)
    }

    pub fn remove(&self, info: &pack::Info) {
        let lock = self.write.lock();
        let mut new = self.indices.load_full();
        Arc::make_mut(&mut new).retain(|idx| &idx.info != info);
        self.indices.store(new);
        drop(lock)
    }

    pub fn clear(&self) {
        let lock = self.write.lock();
        self.indices.store(Arc::new(im::Vector::new()));
        drop(lock)
    }

    pub fn replace<T>(&self, iter: T)
    where
        T: IntoIterator<Item = pack::Index>,
    {
        let lock = self.write.lock();
        self.indices
            .store(Arc::new(iter.into_iter().map(Arc::new).collect()));
        drop(lock)
    }

    pub fn is_empty(&self) -> bool {
        self.indices.load().is_empty()
    }

    pub fn len(&self) -> usize {
        self.indices.load().len()
    }

    pub fn contains(&self, id: impl AsRef<oid>) -> bool {
        for idx in self.indices.load().iter().rev() {
            if idx.contains(&id) {
                return true;
            }
        }

        false
    }

    pub fn lookup<'a, F, E>(
        &self,
        pack_cache: F,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, error::Lookup<E>>
    where
        F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>,
    {
        for idx in self.indices.load().iter().rev() {
            if let Some(ofs) = idx.ofs(&id) {
                let data = pack_cache(&idx.info).map_err(error::Lookup::Lookup)?;
                let pack = data.file();
                let entry = pack.entry(ofs);
                let obj = pack
                    .decode_entry(
                        entry,
                        buf,
                        |id, _| idx.ofs(id).map(|ofs| ResolvedBase::InPack(pack.entry(ofs))),
                        cache,
                    )
                    .map(move |out| Object {
                        kind: out.kind,
                        data: buf.as_slice(),
                        pack_location: None,
                    })?;

                return Ok(Some(obj));
            }
        }

        Ok(None)
    }
}

impl Index for Shared {
    fn contains(&self, id: impl AsRef<oid>) -> bool {
        self.contains(id)
    }

    fn lookup<'a, F, E>(
        &self,
        pack_cache: F,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, error::Lookup<E>>
    where
        F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>,
    {
        self.lookup(pack_cache, id, buf, cache)
    }
}

/// A simple [`Index`] which can not be modified concurrently.
///
/// Lookup functions traverse the inner [`VecDeque`] in reverse order, so
/// indices which are more likely to contain the requested object should be
/// placed at the end of the [`VecDeque`].
pub struct Static {
    pub indices: VecDeque<pack::Index>,
}

impl Static {
    pub fn contains(&self, id: impl AsRef<oid>) -> bool {
        for idx in self.indices.iter().rev() {
            if idx.contains(&id) {
                return true;
            }
        }

        false
    }

    pub fn lookup<'a, F, E>(
        &self,
        pack_cache: F,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, error::Lookup<E>>
    where
        F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>,
    {
        for idx in self.indices.iter().rev() {
            if let Some(ofs) = idx.ofs(&id) {
                let data = pack_cache(&idx.info).map_err(error::Lookup::Lookup)?;
                let pack = data.file();
                let entry = pack.entry(ofs);
                let obj = pack
                    .decode_entry(
                        entry,
                        buf,
                        |id, _| idx.ofs(id).map(|ofs| ResolvedBase::InPack(pack.entry(ofs))),
                        cache,
                    )
                    .map(move |out| Object {
                        kind: out.kind,
                        data: buf.as_slice(),
                        pack_location: None,
                    })?;

                return Ok(Some(obj));
            }
        }

        Ok(None)
    }
}

impl Index for Static {
    fn contains(&self, id: impl AsRef<oid>) -> bool {
        self.contains(id)
    }

    fn lookup<'a, F, E>(
        &self,
        pack_cache: F,
        id: impl AsRef<oid>,
        buf: &'a mut Vec<u8>,
        cache: &mut impl DecodeEntry,
    ) -> Result<Option<Object<'a>>, error::Lookup<E>>
    where
        F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>,
    {
        self.lookup(pack_cache, id, buf, cache)
    }
}
