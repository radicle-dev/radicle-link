// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::ObjectId;

use thiserror::Error;
use tracing::instrument;

use std::{cell::RefCell, collections::BTreeSet, path::PathBuf, rc::Rc};
pub mod thin_change_graph;
pub use thin_change_graph::ThinChangeGraph;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SchemaParse(#[from] super::schema::error::Parse),
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    MinicborDecode(#[from] minicbor::decode::Error),
    #[error(transparent)]
    MinicborEncode(#[from] minicbor::encode::Error<std::io::Error>),
}

pub trait Cache {
    /// Load an object from the cache. `known_refs` are the OIDs pointed to by
    /// references to the object that we know about. If these OIDs have not
    /// changed then we may reuse the cached object. This means that if
    /// additional changes are added to the change graph (via replication or
    /// some direct twiddling of the storage) but no references to the object
    /// are changed then we will not see those changes. However, we specify
    /// in the RFC that any peer updating a change must update their ref to
    /// the object, so this should not be a problem.
    ///
    /// We return an `Rc<RefCell<ThinChangeGraph>>`. This is so that changes can
    /// be made by calling `ThinChangeGraph::propose_change`, which mutates
    /// the `ThinChangeGraph`. This allows the `ThinChangeGraph` (via it's
    /// `validated_automerge`) to cache the `automerge::Backend` and
    /// `automerge::Frontend` used to validate changes. This in turn means that
    /// we avoid rebuilding the automerge document from scratch for every
    /// change - instead we just have to rebuild in the case of schema
    /// invalidating changes, which are hopefully rare.
    fn load(
        &mut self,
        oid: ObjectId,
        known_refs: &BTreeSet<git2::Oid>,
    ) -> Result<Option<Rc<RefCell<ThinChangeGraph>>>, Error>;

    /// Insert or update an object in the cache
    fn put(&mut self, oid: ObjectId, graph: Rc<RefCell<ThinChangeGraph>>) -> Result<(), Error>;
}

/// A cache which stores it's objects on the file system. A sort of poor mans
/// database, this cache is designed to be safe for use from concurrent
/// processes and to be easy to upgrade. The layout on disk looks like this:
///
/// ```ignore
/// <cache dir>
/// |- v1
/// |  |- <object 1 id>
/// |  |- <object 2 id>
/// |  ...
/// ```
///
/// Each file contains a CBOR encoding of a `CacheFile`. This file contains the
/// OIDs of the tips of the graph that were used to generate the object, the
/// validated automerge history that was generated using those tips, the schema
/// and the schema commit OID.
///
/// The `v1` directory means we can easily add a `v2` if we need to change the
/// cache layout in backwards incompatible ways.
///
/// In order to be safe for concurrent use the cache writes new objects by
/// creating a temporary file and then renaming it.
pub struct FileSystemCache {
    dir: PathBuf,
    /// An in memory cache of the last 100 objects that were loaded. This is
    /// useful for situations where you're iteratively applying updates -
    /// one after another - to the same object because it avoids hitting the
    /// disk for every update.
    hot_cache: lru::LruCache<ObjectId, Rc<RefCell<ThinChangeGraph>>>,
}

impl FileSystemCache {
    pub fn open<P: Into<PathBuf>>(dir: P) -> Result<FileSystemCache, std::io::Error> {
        // We add a version to the path so that we can support multiple incompatible
        // cache versions at the same time.
        let dir = dir.into().join("v1");
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        tracing::debug!(dir=?dir, "opening cache");
        Ok(FileSystemCache {
            dir,
            hot_cache: lru::LruCache::new(100),
        })
    }

    fn object_path(&self, oid: ObjectId) -> std::path::PathBuf {
        self.dir.join(oid.to_string())
    }
}

impl Cache for FileSystemCache {
    #[instrument(level = "trace", skip(self, known_refs))]
    fn load(
        &mut self,
        oid: ObjectId,
        known_refs: &BTreeSet<git2::Oid>,
    ) -> Result<Option<Rc<RefCell<ThinChangeGraph>>>, Error> {
        if self.hot_cache.contains(&oid) {
            let obj = self.hot_cache.get(&oid).unwrap().clone();
            if known_refs == obj.borrow().refs() {
                tracing::trace!(object_id=?oid, "fresh object found in memory cache");
                return Ok(Some(obj));
            } else {
                tracing::trace!(fresh_refs=?known_refs, cached_refs=?obj.borrow().refs(), "stale object found in memory cache");
                self.hot_cache.pop(&oid);
                return Ok(None);
            }
        }
        let object_path = self.object_path(oid);
        let bytes = match std::fs::read(&object_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::trace!(object_id=?oid, object_cache_path=?object_path, "no cache found on filesystem for object");
                return Ok(None);
            },
            Err(e) => return Err(e.into()),
        };

        //let raw_graph: ThinChangeGraph = minicbor::decode(&bytes)?;
        let raw_graph: ThinChangeGraph = match thin_change_graph::forward_compatible_decode(
            &mut minicbor::Decoder::new(&bytes),
        )? {
            None => {
                tracing::warn!(
                    "cached object found with unknown fields, consider upgrading librad"
                );
                return Ok(None);
            },
            Some(g) => g,
        };
        let thin_graph = Rc::new(RefCell::new(raw_graph));

        if known_refs == thin_graph.borrow().refs() {
            tracing::trace!(object_id=?oid, "fresh object found in filesystem cache");
            self.hot_cache.put(oid, thin_graph.clone());
            Ok(Some(thin_graph))
        } else {
            tracing::trace!(fresh_refs=?known_refs, cached_refs=?thin_graph.borrow().refs(), "stale object found in filesystem cache");
            Ok(None)
        }
    }

    #[instrument(level = "trace", skip(self, graph))]
    fn put(&mut self, oid: ObjectId, graph: Rc<RefCell<ThinChangeGraph>>) -> Result<(), Error> {
        let tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        {
            let out = std::fs::File::create(&tmp)?;
            let g = graph.borrow();
            let gref: &ThinChangeGraph = &g;
            minicbor::encode(gref, &out)?;
            out.sync_all()?;
        }
        std::fs::rename(tmp, self.object_path(oid))?;
        self.hot_cache.put(oid, graph);
        Ok(())
    }
}

pub struct NoOpCache {}

impl NoOpCache {
    pub fn new() -> NoOpCache {
        NoOpCache {}
    }
}

impl Cache for NoOpCache {
    fn put(&mut self, _oid: ObjectId, _graph: Rc<RefCell<ThinChangeGraph>>) -> Result<(), Error> {
        Ok(())
    }

    fn load(
        &mut self,
        _oid: ObjectId,
        _known_tips: &BTreeSet<git2::Oid>,
    ) -> Result<Option<Rc<RefCell<ThinChangeGraph>>>, Error> {
        Ok(None)
    }
}
