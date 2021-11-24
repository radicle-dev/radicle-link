// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

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

mod metrics;
pub use metrics::{Metrics, Stats, StatsView};

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
        Reload(#[from] Discover),

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

/// An [`Index`] which can be shared between threads.
///
/// [`Shared`] assumes that:
///
/// * newer packs are likely to contain recent objects
/// * lookups tend to favour recent objects
/// * lookups tend to expect the object to be found (the object id is either
///   pointed to by a ref, or linked to by an existing object)
///
/// Thus, it:
///
/// * orders indices found in `GIT_DIR/objects/pack` by modification time, and
///   queries the more recent ones first
/// * attempts to rescan `GIT_DIR/objects/pack` when an object id was _not_
///   found (assuming that this is due to a compaction)
///
/// Unless a reload occurs, lookups are lock-free and mostly wait-free. Writes
/// ([`Shared::push`], [`Shared::reload`]) are guarded by a [`Mutex`].
// TODO: consecutive lookups also tend to resolve to the same pack, so we could
// remember the index into the `im::Vector` where we found a match and look
// there first. This is what libgit2 does, but the heuristic is not necessarily
// true when `Shared` is shared across multiple concurrent link replication
// tasks; per-namespace packs are independent pre-compaction.
pub struct Shared<M> {
    pack_dir: PathBuf,
    indices: ArcSwap<im::Vector<Arc<pack::Index>>>,
    write: Mutex<()>,
    stats: M,
}

impl Shared<()> {
    pub fn open(git_dir: impl AsRef<Path>) -> Result<Self, error::Discover> {
        let pack_dir = git_dir.as_ref().join("objects").join("pack");
        let indices = discover(&pack_dir)?;

        Ok(Self {
            pack_dir,
            indices: ArcSwap::new(Arc::new(indices)),
            write: Mutex::new(()),
            stats: (),
        })
    }
}

impl<M> Shared<M>
where
    M: Metrics,
{
    pub fn with_stats(self) -> Shared<Stats> {
        self.with_metrics(Stats::default())
    }

    pub fn with_metrics<N: Metrics>(self, m: N) -> Shared<N> {
        Shared {
            pack_dir: self.pack_dir,
            indices: self.indices,
            write: self.write,
            stats: m,
        }
    }

    pub fn stats(&self) -> M::Snapshot {
        self.stats.snapshot(self.len())
    }

    /// Add a newly discovered [`pack::Index`].
    ///
    /// This index will be considered first by subsequent lookups. Note that it
    /// is only guaranteed that the index will be visible to readers if it
    /// resides in the `git_dir` this [`Shared`] was initialised with.
    pub fn push(&self, idx: pack::Index) {
        let lock = self.write.lock();
        let mut new = self.indices.load_full();
        Arc::make_mut(&mut new).push_front(Arc::new(idx));
        self.indices.store(new);
        drop(lock);

        self.stats.record_push()
    }

    /// Re-scan the packs directory and replace the in-memory indices with the
    /// result.
    ///
    /// If the application can intercept compaction events, this method can be
    /// used to release memory early. Otherwise it is not required to call this
    /// method, as [`Shared`] manages reloads automatically.
    pub fn reload(&self) -> Result<(), error::Discover> {
        let lock = self.write.lock();
        let indices = discover(&self.pack_dir)?;
        self.indices.store(Arc::new(indices));
        drop(lock);

        self.stats.record_reload();

        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.indices.load().is_empty()
    }

    pub fn len(&self) -> usize {
        self.indices.load().len()
    }

    fn contains(&self, id: impl AsRef<oid>) -> bool {
        for i in 0..2 {
            for idx in self.indices.load().iter() {
                if idx.contains(&id) {
                    self.stats.record_hit();
                    return true;
                }
            }

            if i == 0 && self.reload().is_err() {
                self.stats.record_miss();
                return false;
            }
        }

        self.stats.record_miss();
        false
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
        for i in 0..2 {
            for idx in self.indices.load().iter() {
                if let Some(ofs) = idx.ofs(&id) {
                    self.stats.record_hit();
                    return load_obj(ofs, idx, pack_cache, buf, cache).map(Some);
                }
            }

            if i == 0 {
                self.reload()?;
            }
        }

        self.stats.record_miss();
        Ok(None)
    }
}

fn load_obj<'a, F, E>(
    ofs: u64,
    idx: &pack::Index,
    pack_cache: F,
    buf: &'a mut Vec<u8>,
    cache: &mut impl DecodeEntry,
) -> Result<Object<'a>, error::Lookup<E>>
where
    F: FnOnce(&pack::Info) -> Result<Arc<pack::Data>, E>,
{
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

    Ok(obj)
}

fn discover(pack_dir: impl AsRef<Path>) -> Result<im::Vector<Arc<pack::Index>>, error::Discover> {
    let pack_dir = pack_dir.as_ref();
    let pack_dir_disp = pack_dir.display();
    trace!("discovering packs at {}", pack_dir_disp);
    match fs::read_dir(&pack_dir) {
        Ok(iter) => {
            let mut paths = Vec::new();
            for entry in iter {
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
                .rev()
                .map(|(path, _)| Ok(pack::Index::open(path).map(Arc::new)?))
                .collect::<Result<_, error::Discover>>()?;

            Ok(indices)
        },
        // It's not an error if the directory doesn't exist, the repository
        // could contain only loose objects
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            trace!("not a directory: {}", pack_dir_disp);
            Ok(im::Vector::new())
        },
        Err(e) => Err(e.into()),
    }
}

impl<M> Index for Shared<M>
where
    M: Metrics,
{
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
