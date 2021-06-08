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
pub struct RefsEvent {
    pub path: PathBuf,
    pub kind: RefsEventKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefsEventKind {
    Create,
    Remove,
    Update,
}

pub struct Watch<'a> {
    pub(super) storage: &'a Storage,
}

impl<'a> Watch<'a> {
    pub fn refs(&self) -> Result<(Watcher, impl Iterator<Item = RefsEvent>), Error> {
        use notify::{DebouncedEvent::*, RecursiveMode::Recursive};

        let repo_path = self.storage.path().to_owned();
        let reflogs_path = repo_path.join("logs");

        if !reflogs_path.exists() {
            fs::create_dir(&reflogs_path)?;
        }

        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::watcher(tx, DEBOUNCE_DELAY)?;
        watcher.watch(&reflogs_path, Recursive)?;

        fn relpath(repo: &Path, p: PathBuf) -> PathBuf {
            match p.strip_prefix(repo) {
                Ok(rel) => rel.to_path_buf(),
                Err(_) => p,
            }
        }

        let rx = rx.into_iter().filter_map(move |evt| match evt {
            Create(path) => Some(RefsEvent {
                path: relpath(&reflogs_path, path),
                kind: RefsEventKind::Create,
            }),
            Remove(path) => Some(RefsEvent {
                path: relpath(&reflogs_path, path),
                kind: RefsEventKind::Remove,
            }),
            Write(path) => Some(RefsEvent {
                path: relpath(&reflogs_path, path),
                kind: RefsEventKind::Update,
            }),
            _ => None,
        });

        Ok((Watcher(Arc::new(watcher)), rx))
    }
}
