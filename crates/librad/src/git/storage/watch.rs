// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

use notify::Watcher as _;
use thiserror::Error;

use super::Storage;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Notify(#[from] notify::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A handle to a filesystem watcher.
///
/// If and when this value is dropped, the corresponding events iterator will
/// return `None`. Note, however, that this is subject to the debounce delay of
/// the watcher.
#[derive(Clone)]
pub struct Watcher(Arc<notify::RecommendedWatcher>);

#[derive(Debug)]
pub struct NamespaceEvent {
    pub path: PathBuf,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum EventKind {
    Create,
    Remove,
    Update,
}

/// Watch a [`Storage`] for changes.
///
/// Implemented in terms of filesystem events, and so are emitted regardless of
/// which process or [`Storage`] instance causes them.
pub struct Watch<'a> {
    pub(super) storage: &'a Storage,
}

impl<'a> Watch<'a> {
    /// Watch for creation or removal of a namespace.
    ///
    /// Implemented by watching `$GIT_DIR/logs/refs/namespaces` for directory
    /// events. Note that:
    ///
    /// * reflogs must be enabled for the repository
    /// * reflogs for at least one ref within the namespace (eg. `rad/id`) must
    ///   be created
    /// * the directory `$GIT_DIR/logs/refs/namespaces` is created if it doesn't
    ///   exist
    /// * the directory is watched _non-recursively_, as this tends to miss
    ///   events with most filesystem event backends
    ///
    /// By default [`super::Config`] sets `core.logAllRefUpdates` to `true`
    /// (**not** "always"), and refs created by this library will have a
    /// corresponding reflog created. It is currently unlikely that
    /// [`EventKind`]s other than [`EventKind::Create`] will be emitted.
    pub fn namespaces(&self) -> Result<(Watcher, impl Iterator<Item = NamespaceEvent>), Error> {
        use notify::{Op, RawEvent, RecursiveMode::NonRecursive};

        fn is_namespace(p: &Path) -> bool {
            let mut iter = p.iter().take(4);
            iter.next() == Some("refs".as_ref())
                && iter.next() == Some("namespaces".as_ref())
                && iter.next().is_some()
                && iter.next().is_none()
        }

        let repo_path = self.storage.path().to_owned();
        let reflogs_path = repo_path.join("logs");
        let namespaces_path = reflogs_path.join("refs/namespaces");

        if !namespaces_path.exists() {
            fs::create_dir_all(&namespaces_path)?;
        }

        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::raw_watcher(tx)?;
        watcher.watch(&namespaces_path, NonRecursive)?;

        let rx = rx.into_iter().filter_map(move |evt| {
            tracing::trace!("{:?}", evt);

            match evt {
                RawEvent {
                    path: Some(path),
                    op: Ok(op),
                    cookie: _,
                } if path.is_dir() => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    if is_namespace(path) {
                        let kind = if op.contains(Op::CREATE) {
                            EventKind::Create
                        } else if op.contains(Op::REMOVE) {
                            EventKind::Remove
                        } else {
                            EventKind::Update
                        };
                        Some(NamespaceEvent {
                            path: path.to_path_buf(),
                            kind,
                        })
                    } else {
                        tracing::trace!("not a namespace");
                        None
                    }
                },

                _ => None,
            }
        });

        Ok((Watcher(Arc::new(watcher)), rx))
    }
}
