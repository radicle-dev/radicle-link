// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    convert::TryInto,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use git_ref::{
    file::{self, iter::LooseThenPacked, Transaction, WriteReflog},
    packed,
    FullName,
    PartialNameRef,
    Reference,
    Target,
};
use parking_lot::RwLock;

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum Open {
        #[error("failed to take a snapshot of packed-refs")]
        Snapshot(#[from] Snapshot),

        #[error(transparent)]
        Io(#[from] io::Error),
    }

    #[derive(Debug, Error)]
    pub enum Snapshot {
        #[error("failed to lock packed-refs")]
        Lock(#[from] git_lock::acquire::Error),

        #[error("failed to open packed-refs")]
        Open(#[from] packed::buffer::open::Error),

        #[error(transparent)]
        Io(#[from] io::Error),
    }

    #[derive(Debug, Error)]
    pub enum Follow {
        #[error("cyclic symref: {0:?}")]
        Cycle(FullName),

        #[error("reference {0:?} not found")]
        NotFound(FullName),

        #[error("max symref depth {0} exceeded")]
        DepthLimitExceeded(usize),

        #[error(transparent)]
        Find(#[from] file::find::Error),
    }
}

/// Threadsafe refdb with shareable `packed-refs` memory buffer.
///
/// Packed refs are a delicate business: they are written by an external
/// process, [`git-pack-refs`], _or_ when a packed ref is deleted. It may also
/// be that no `packed-refs` currently exist.
///
/// The only way we can be certain to operate on a consistent view of what is
/// committed to disk is to check if the `packed-refs` file has changed since we
/// last read it. This would be quite expensive to do for small operations.
/// Thus, the caller is responsible for determining just how much they can
/// afford to see possibly out-of-date data: the [`Refdb::snapshot`] method
/// checks if the previously loaded `packed-refs` appear to be out-of-date, and
/// reloads them if necessary. The resulting [`Snapshot`] contains a pointer to
/// an immutable memory buffer of the packed refs which can be shared between
/// threads, or cloned.
///
/// [`git-pack-refs`]: https://git-scm.com/docs/git-pack-refs
#[derive(Clone)]
pub struct Refdb {
    store: file::Store,
    packed: Arc<RwLock<Option<Packed>>>,
}

impl Refdb {
    pub fn open(git_dir: impl Into<PathBuf>) -> Result<Self, error::Open> {
        let store = file::Store::at(git_dir, WriteReflog::Normal);
        let packed = Arc::new(RwLock::new(Packed::open(store.packed_refs_path())?));
        Ok(Self { store, packed })
    }

    pub fn snapshot(&self) -> Result<Snapshot, error::Snapshot> {
        let read = self.packed.read();
        match &*read {
            None => {
                drop(read);
                // always modified, because it was None and now is Some
                self.reload(|_| true)
            },

            Some(packed) => {
                if packed.is_modified()? {
                    let mtime = packed.mtime;
                    drop(read);
                    // we don't care what the mtime is, only that we have a
                    // different value than before
                    self.reload(|packed1| packed1.mtime != mtime)
                } else {
                    Ok(Snapshot {
                        store: self.store.clone(),
                        packed: Some(packed.buf.clone()),
                    })
                }
            },
        }
    }

    fn reload<F>(&self, modified_while_blocked: F) -> Result<Snapshot, error::Snapshot>
    where
        F: FnOnce(&Packed) -> bool,
    {
        let mut write = self.packed.write();
        if let Some(packed) = &*write {
            if modified_while_blocked(packed) {
                return Ok(Snapshot {
                    store: self.store.clone(),
                    packed: Some(packed.buf.clone()),
                });
            }
        }

        match Packed::open(self.store.packed_refs_path())? {
            Some(packed) => {
                let buf = packed.buf.clone();
                *write = Some(packed);
                Ok(Snapshot {
                    store: self.store.clone(),
                    packed: Some(buf),
                })
            },

            None => {
                *write = None;
                Ok(Snapshot {
                    store: self.store.clone(),
                    packed: None,
                })
            },
        }
    }
}

#[derive(Clone)]
pub struct Snapshot {
    store: file::Store,
    packed: Option<Arc<packed::Buffer>>,
}

impl Snapshot {
    pub fn find<'a, N, E>(&self, name: N) -> Result<Option<Reference>, file::find::Error>
    where
        N: TryInto<PartialNameRef<'a>, Error = E>,
        file::find::Error: From<E>,
    {
        self.store
            .try_find(name, self.packed.as_ref().map(|arc| arc.as_ref()))
    }

    pub fn transaction(&self) -> Transaction {
        self.store.transaction()
    }

    pub fn iter(&self, prefix: Option<impl AsRef<Path>>) -> io::Result<LooseThenPacked> {
        let packed = self.packed.as_ref().map(|arc| arc.as_ref());
        match prefix {
            None => self.store.iter(packed),
            Some(p) => self.store.iter_prefixed(packed, p),
        }
    }

    /// Follow a symbolic reference until a direct reference is found.
    ///
    /// If `symref` is a direct reference, a copy of it is returned. No more
    /// than five symbolic references will be followed, and cyclic
    /// references are detected. Both result in an error to be returned.
    ///
    /// Note that following is not the same as "peeling": no access to the
    /// object database is made, and thus no assumptions about the kind of
    /// object the reference ultimately points to can be made.
    pub fn follow(&self, symref: &Reference) -> Result<Reference, error::Follow> {
        match &symref.target {
            Target::Peeled(_) => Ok(symref.clone()),
            Target::Symbolic(name) => {
                let mut seen = BTreeSet::new();
                seen.insert(symref.name.clone());

                let mut next = self
                    .find(name.to_partial())?
                    .ok_or_else(|| error::Follow::NotFound(name.clone()))?;
                seen.insert(name.clone());

                const MAX_DEPTH: usize = 5;
                loop {
                    match next.target {
                        Target::Peeled(_) => return Ok(next),
                        Target::Symbolic(sym) => {
                            if seen.len() + 1 > MAX_DEPTH {
                                return Err(error::Follow::DepthLimitExceeded(MAX_DEPTH));
                            }

                            if seen.contains(&sym) {
                                return Err(error::Follow::Cycle(sym));
                            }
                            next = self
                                .find(sym.to_partial())?
                                .ok_or_else(|| error::Follow::NotFound(sym.clone()))?;
                            seen.insert(sym);
                        },
                    }
                }
            },
        }
    }
}

struct Packed {
    buf: Arc<packed::Buffer>,
    path: PathBuf,
    mtime: SystemTime,
}

impl Packed {
    fn open(path: PathBuf) -> Result<Option<Self>, error::Snapshot> {
        use git_lock::{acquire, Marker};

        let _lock = Marker::acquire_to_hold_resource(&path, acquire::Fail::Immediately, None)?;
        match path.metadata() {
            // `git-lock` will happily lock a non-existent file
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),

            Ok(meta) => {
                let mtime = meta.modified()?;
                let buf = Arc::new(packed::Buffer::open(&path, 32 * 1024)?);
                Ok(Some(Self { buf, path, mtime }))
            },
        }
    }

    fn is_modified(&self) -> io::Result<bool> {
        match self.path.metadata() {
            // it existed before, so gone is modified
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(e),

            Ok(meta) => {
                let mtime = meta.modified()?;
                Ok(self.mtime == mtime)
            },
        }
    }
}
