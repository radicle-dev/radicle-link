// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::Duration,
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
/// return `None`. Note, however, that this is subject to the
/// [`DEBOUNCE_DELAY`].
#[derive(Clone)]
pub struct Watcher(Arc<notify::RecommendedWatcher>);

#[derive(Debug)]
pub struct NamespaceEvent {
    pub path: PathBuf,
    pub kind: EventKind,
}

#[derive(Debug)]
pub struct ReflogEvent {
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
    /// Implemented in terms of [`Self::reflogs`], meaning that reflogs need to
    /// be enabled, and a reflog for a `rad/id` ref needs to be created.
    /// This is the default.
    ///
    /// Note: `EventKind::Update` events will **not** be emitted.
    pub fn namespaces(
        &self,
        debounce: Duration,
    ) -> Result<(Watcher, impl Iterator<Item = NamespaceEvent>), Error> {
        fn is_namespace(p: &Path) -> bool {
            let mut iter = p.iter().take(7);
            iter.next() == Some("refs".as_ref())
                && iter.next() == Some("namespaces".as_ref())
                && iter.next().is_some()
                && iter.next() == Some("refs".as_ref())
                && iter.next() == Some("rad".as_ref())
                && iter.next() == Some("id".as_ref())
                && iter.next().is_none()
        }

        let (watcher, rx) = self.reflogs(debounce)?;
        let rx = rx.filter_map(move |ReflogEvent { path, kind }| {
            if matches!(kind, EventKind::Create | EventKind::Remove) {
                is_namespace(&path).then(|| NamespaceEvent {
                    path: path.iter().take(3).collect(),
                    kind,
                })
            } else {
                None
            }
        });

        Ok((watcher, rx))
    }

    /// Watch the reflog.
    ///
    /// Requires the reflog to be enabled on the backing repository. By default,
    /// refs created by this library will also create corresponding reflogs.
    /// Currently, refs created by other tools (eg. `git push`) will **not**
    /// create reflogs.
    pub fn reflogs(
        &self,
        debounce: Duration,
    ) -> Result<(Watcher, impl Iterator<Item = ReflogEvent>), Error> {
        use notify::{DebouncedEvent::*, RecursiveMode::Recursive};

        let repo_path = self.storage.path().to_owned();
        let reflogs_path = repo_path.join("logs");

        if !reflogs_path.exists() {
            fs::create_dir(&reflogs_path)?;
        }

        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::watcher(tx, debounce)?;
        watcher.watch(&reflogs_path, Recursive)?;

        fn is_ref(p: &Path) -> bool {
            !p.ends_with(".lock") && p.is_file()
        }

        let rx = rx.into_iter().filter_map(move |evt| {
            tracing::trace!("reflog event: {:?}", evt);
            match evt {
                Create(path) if is_ref(&path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    Some(ReflogEvent {
                        path: path.to_path_buf(),
                        kind: EventKind::Create,
                    })
                },
                Remove(path) if is_ref(&path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    Some(ReflogEvent {
                        path: path.to_path_buf(),
                        kind: EventKind::Remove,
                    })
                },
                Write(path) | Rename(_, path) if is_ref(&path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    Some(ReflogEvent {
                        path: path.to_path_buf(),
                        kind: EventKind::Update,
                    })
                },
                _ => None,
            }
        });

        Ok((Watcher(Arc::new(watcher)), rx))
    }
}
