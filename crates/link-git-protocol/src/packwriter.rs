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
use git_repository::{odb::pack, Progress};

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

/// The default [`PackWriter`].
///
/// Writes the packfile into the given output directory, along with a v2
/// index. The packfile is verified.
pub struct Standard {
    git_dir: PathBuf,
    opt: Options,
    stop: Arc<AtomicBool>,
}

impl Standard {
    pub fn new(git_dir: impl AsRef<Path>, opt: Options, stop: Arc<AtomicBool>) -> Self {
        Self {
            git_dir: git_dir.as_ref().to_owned(),
            opt,
            stop,
        }
    }
}

impl Drop for Standard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl PackWriter for Standard {
    type Output = PackReceived;

    fn write_pack(
        &self,
        pack: impl AsyncBufRead + Unpin,
        prog: impl Progress,
    ) -> io::Result<Self::Output> {
        use git_repository::odb::{
            linked::Store,
            pack::{bundle::write::Options, data::input::Mode, index::Version, Bundle},
            FindExt as _,
        };

        let odb =
            Store::at(self.git_dir.clone()).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let opts = Options {
            thread_limit: self.opt.max_indexer_threads,
            index_kind: Version::V2,
            iteration_mode: Mode::Verify,
        };
        Bundle::write_to_directory(
            BlockOn::new(TryTake::new(pack, self.opt.max_pack_bytes)),
            Some(self.git_dir.join("objects").join("pack")),
            prog,
            &self.stop,
            Some(Box::new(move |oid, buf| {
                odb.find(oid, buf, &mut pack::cache::Never).ok()
            })),
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
