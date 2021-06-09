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

pub const DEBOUNCE_DELAY: Duration = Duration::from_secs(1);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EventKind {
    Create,
    Remove,
    Update,
}

pub struct Watch<'a> {
    pub(super) storage: &'a Storage,
}

impl<'a> Watch<'a> {
    pub fn namespaces(&self) -> Result<(Watcher, impl Iterator<Item = NamespaceEvent>), Error> {
        fn is_namespace(p: &Path) -> bool {
            let mut iter = p.iter().take(6);
            iter.next() == Some("refs".as_ref())
                && iter.next() == Some("namespaces".as_ref())
                && iter.next().is_some()
                && iter.next() == Some("rad".as_ref())
                && iter.next() == Some("id".as_ref())
                && iter.next().is_none()
        }

        let (watcher, rx) = self.reflogs()?;
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

    pub fn reflogs(&self) -> Result<(Watcher, impl Iterator<Item = ReflogEvent>), Error> {
        use notify::{DebouncedEvent::*, RecursiveMode::Recursive};

        let repo_path = self.storage.path().to_owned();
        let reflogs_path = repo_path.join("logs");

        if !reflogs_path.exists() {
            fs::create_dir(&reflogs_path)?;
        }

        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::watcher(tx, DEBOUNCE_DELAY)?;
        watcher.watch(&reflogs_path, Recursive)?;

        let rx = rx.into_iter().filter_map(move |evt| {
            tracing::trace!("reflog event: {:?}", evt);
            match evt {
                Create(path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    path.is_file().then(|| ReflogEvent {
                        path: path.to_path_buf(),
                        kind: EventKind::Create,
                    })
                },
                Remove(path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    path.is_file().then(|| ReflogEvent {
                        path: path.to_path_buf(),
                        kind: EventKind::Remove,
                    })
                },
                Write(path) | Rename(_, path) => {
                    let path = path.strip_prefix(&reflogs_path).ok()?;
                    path.is_file().then(|| ReflogEvent {
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
