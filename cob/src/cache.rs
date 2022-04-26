// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::ObjectId;

use minicbor::Decode;
use thiserror::Error;
use tracing::instrument;

use std::{cell::RefCell, collections::BTreeSet, path::PathBuf, rc::Rc};
pub mod cached_change_graph;
pub use cached_change_graph::CachedChangeGraph;

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
    /// We return an `Rc<RefCell<CachedChangeGraph>>`. This is so that changes
    /// can be made by calling `CachedChangeGraph::propose_change`, which
    /// mutates the `CachedChangeGraph`.
    fn load(
        &mut self,
        oid: ObjectId,
        known_refs: &BTreeSet<git2::Oid>,
    ) -> Result<Option<Rc<RefCell<CachedChangeGraph>>>, Error>;

    /// Insert or update an object in the cache
    fn put(&mut self, oid: ObjectId, graph: Rc<RefCell<CachedChangeGraph>>) -> Result<(), Error>;
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
/// Each file contains a CBOR encoding of a `CachedChangeGraph`. This file
/// contains the OIDs of the tips of the graph that were used to generate the
/// object, the validated automerge history that was generated using those tips,
/// the schema and the schema commit OID.
///
/// The `v1` directory means we can easily add a `v2` if we need to change the
/// cache layout in backwards incompatible ways.
///
/// In order to be safe for concurrent use the cache writes new objects by
/// creating a temporary file and then renaming it.
pub struct FileSystemCache {
    dir: PathBuf,
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
        Ok(FileSystemCache { dir })
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
    ) -> Result<Option<Rc<RefCell<CachedChangeGraph>>>, Error> {
        let object_path = self.object_path(oid);
        let bytes = match std::fs::read(&object_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::trace!(object_id=?oid, object_cache_path=?object_path, "no cache found on filesystem for object");
                return Ok(None);
            },
            Err(e) => return Err(e.into()),
        };

        let raw_graph = match CachedChangeGraph::decode(&mut minicbor::Decoder::new(&bytes)) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(err=?e, "error decoding cached change graph");
                return Ok(None);
            },
        };
        let cached_graph = Rc::new(RefCell::new(raw_graph));

        if known_refs == cached_graph.borrow().refs() {
            tracing::trace!(object_id=?oid, "fresh object found in filesystem cache");
            Ok(Some(cached_graph))
        } else {
            tracing::trace!(fresh_refs=?known_refs, cached_refs=?cached_graph.borrow().refs(), "stale object found in filesystem cache");
            Ok(None)
        }
    }

    #[instrument(level = "trace", skip(self, graph))]
    fn put(&mut self, oid: ObjectId, graph: Rc<RefCell<CachedChangeGraph>>) -> Result<(), Error> {
        let tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        {
            let out = std::fs::File::create(&tmp)?;
            let g = graph.borrow();
            let gref: &CachedChangeGraph = &g;
            minicbor::encode(gref, &out)?;
            out.sync_all()?;
        }
        std::fs::rename(tmp, self.object_path(oid))?;
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
    fn put(&mut self, _oid: ObjectId, _graph: Rc<RefCell<CachedChangeGraph>>) -> Result<(), Error> {
        Ok(())
    }

    fn load(
        &mut self,
        _oid: ObjectId,
        _known_tips: &BTreeSet<git2::Oid>,
    ) -> Result<Option<Rc<RefCell<CachedChangeGraph>>>, Error> {
        Ok(None)
    }
}
