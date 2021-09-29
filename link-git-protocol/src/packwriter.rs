// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_lite::io::{AsyncBufRead, BlockOn};
use git_repository::{
    hash::ObjectId,
    odb::{self, pack},
    Progress,
};

use crate::take::TryTake;

#[cfg(feature = "git2")]
pub use libgit::Libgit;

/// What to do with the `packfile` response.
///
/// _This is mostly the same as [`git_repository::protocol::fetch::Delegate`],
/// but without incurring the
/// [`git_repository::protocol::fetch::DelegateBlocking`] super-trait
/// constraint. We can simply make [`crate::fetch::Fetch`] parametric over the
/// packfile sink._
pub trait PackWriter {
    type Output;

    fn write_pack(
        &self,
        pack: impl AsyncBufRead + Unpin,
        progress: impl Progress,
    ) -> io::Result<Self::Output>;
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// How many threads the packfile indexer is allowed to spawn. `None` means
    /// unlimited.
    pub max_indexer_threads: Option<usize>,
    /// The maximum size in bytes of the packfile.
    ///
    /// If the remote sends a larger file, the transfer will be aborted.
    pub max_pack_bytes: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_indexer_threads: Some(1),
            max_pack_bytes: u64::MAX,
        }
    }
}

#[cfg(feature = "git2")]
pub mod libgit {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    pub struct PackReceived {
        pub objects: usize,
        pub local_objects: usize,
        pub deltas: usize,
    }

    impl From<git2::Progress<'_>> for PackReceived {
        fn from(p: git2::Progress<'_>) -> Self {
            Self {
                objects: p.indexed_objects(),
                local_objects: p.local_objects(),
                deltas: p.indexed_deltas(),
            }
        }
    }

    pub struct Libgit {
        opt: Options,
        repo: git2::Repository,
        stop: Arc<AtomicBool>,
    }

    impl Libgit {
        pub fn new(opt: Options, repo: git2::Repository, stop: Arc<AtomicBool>) -> Self {
            Self { opt, repo, stop }
        }

        fn guard_cancelled(&self) -> io::Result<()> {
            if self.stop.load(Ordering::Acquire) {
                Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
            } else {
                Ok(())
            }
        }
    }

    impl PackWriter for Libgit {
        type Output = Option<PackReceived>;

        fn write_pack(
            &self,
            pack: impl AsyncBufRead + Unpin,
            _: impl Progress,
        ) -> io::Result<Self::Output> {
            let mut out = None;

            let odb = self.repo.odb().map_err(io_error)?;
            let mut writer = odb.packwriter().map_err(io_error)?;

            self.guard_cancelled()?;
            io::copy(
                &mut BlockOn::new(TryTake::new(pack, self.opt.max_pack_bytes)),
                &mut writer,
            )?;

            self.guard_cancelled()?;
            writer
                .progress(|p| {
                    out = Some(p.to_owned());
                    true
                })
                .commit()
                .map(|_| ())
                .map_err(io_error)?;
            // Convince borrowchk that `out` can not possibly be borrowed anymore
            drop(writer);

            Ok(out.map(Into::into))
        }
    }

    fn io_error(e: git2::Error) -> io::Error {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

pub type PackReceived = pack::bundle::write::Outcome;

/// A lookup function to help "thicken" thin packs by finding missing base
/// objects.
///
/// The impl provided for [`odb::linked::Store`] does not use any pack caching.
pub trait Thickener {
    fn find_object<'a>(&self, id: ObjectId, buf: &'a mut Vec<u8>)
        -> Option<pack::data::Object<'a>>;
}

impl Thickener for odb::linked::Store {
    fn find_object<'a>(
        &self,
        id: ObjectId,
        buf: &'a mut Vec<u8>,
    ) -> Option<pack::data::Object<'a>> {
        use git_repository::prelude::FindExt as _;
        self.find(id, buf, &mut pack::cache::Never).ok()
    }
}

/// A factory spewing out new [`Thickener`]s with static lifetimes.
///
/// `gitoxide` doesn't currently allow us to initialise thickening lazily (the
/// pack file may not be thin after all), but requires a static lookup function.
/// Instead of initialising a new [`odb::linked::Store`] for every pack stream,
/// users may share a pre-initialised object database provided appropriate
/// thread safety measures.
pub trait BuildThickener {
    type Error: std::error::Error + Send + Sync + 'static;
    type Thick: Thickener + 'static;

    fn build_thickener(&self) -> Result<Self::Thick, Self::Error>;
}

pub struct StandardThickener {
    git_dir: PathBuf,
}

impl StandardThickener {
    pub fn new(git_dir: impl Into<PathBuf>) -> Self {
        let git_dir = git_dir.into();
        Self { git_dir }
    }
}

impl BuildThickener for StandardThickener {
    type Error = odb::linked::init::Error;
    type Thick = odb::linked::Store;

    fn build_thickener(&self) -> Result<Self::Thick, Self::Error> {
        odb::linked::Store::at(self.git_dir.join("objects"))
    }
}

/// The default [`PackWriter`].
///
/// Writes the packfile into the given output directory, along with a v2
/// index. The packfile is verified.
pub struct Standard<F> {
    git_dir: PathBuf,
    opt: Options,
    thick: F,
    stop: Arc<AtomicBool>,
}

impl<F> Standard<F> {
    pub fn new(git_dir: impl AsRef<Path>, opt: Options, thick: F, stop: Arc<AtomicBool>) -> Self {
        Self {
            git_dir: git_dir.as_ref().to_owned(),
            opt,
            thick,
            stop,
        }
    }
}

impl<F> Drop for Standard<F> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl<F: BuildThickener> PackWriter for Standard<F> {
    type Output = PackReceived;

    fn write_pack(
        &self,
        pack: impl AsyncBufRead + Unpin,
        prog: impl Progress,
    ) -> io::Result<Self::Output> {
        use pack::{bundle::write::Options, data::input::Mode, index::Version, Bundle};

        let opts = Options {
            thread_limit: self.opt.max_indexer_threads,
            index_kind: Version::V2,
            iteration_mode: Mode::Verify,
        };
        let thickener = self
            .thick
            .build_thickener()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Bundle::write_to_directory(
            BlockOn::new(TryTake::new(pack, self.opt.max_pack_bytes)),
            Some(self.git_dir.join("objects").join("pack")),
            prog,
            &self.stop,
            Some(Box::new(move |oid, buf| thickener.find_object(oid, buf))),
            opts,
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

/// No-op [`PackWriter`] which just drains the input.
pub struct Discard;

impl PackWriter for Discard {
    type Output = u64;

    fn write_pack(
        &self,
        pack: impl AsyncBufRead + Unpin,
        _: impl Progress,
    ) -> io::Result<Self::Output> {
        io::copy(&mut BlockOn::new(pack), &mut io::sink())
    }
}
