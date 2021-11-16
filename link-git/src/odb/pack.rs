// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use git_hash::{oid, ObjectId};
use git_pack::{data, index};
use rustc_hash::FxHasher;
use tracing::warn;

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("failed to load pack data from {path:?}")]
    pub struct Data {
        pub path: PathBuf,
        pub source: data::header::decode::Error,
    }

    #[derive(Debug, Error)]
    #[error("failed to load pack index from {path:?}")]
    pub struct Index {
        pub path: PathBuf,
        pub source: index::init::Error,
    }
}

pub struct Data {
    pub hash: u64,
    hits: AtomicUsize,
    file: data::File,
}

impl Data {
    pub fn hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn file(&self) -> &data::File {
        &self.file
    }
}

impl AsRef<data::File> for Data {
    fn as_ref(&self) -> &data::File {
        self.file()
    }
}

#[derive(Clone, PartialEq)]
pub struct Info {
    pub(super) hash: u64,
    pub data_path: PathBuf,
}

impl Info {
    pub fn data(&self) -> Result<Data, error::Data> {
        let file = data::File::at(&self.data_path).map_err(|source| error::Data {
            path: self.data_path.clone(),
            source,
        })?;
        Ok(Data {
            hash: self.hash,
            hits: AtomicUsize::new(0),
            file,
        })
    }
}

pub struct Index {
    pub info: Info,
    file: index::File,
}

impl Index {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, error::Index> {
        let path = path.as_ref();
        let file = index::File::at(path).map_err(|source| error::Index {
            path: path.to_path_buf(),
            source,
        })?;
        let data_path = path.with_extension("pack");
        let hash = {
            let file_name = path
                .file_name()
                .expect("must have a file name, we opened it")
                .to_string_lossy();
            // XXX: inexplicably, gitoxide omits the "pack-" prefix
            let sha_hex = file_name.strip_prefix("pack-").unwrap_or(&file_name);
            match ObjectId::from_hex(&sha_hex.as_bytes()[..40]) {
                Err(e) => {
                    warn!(
                        "unconventional pack name {:?}, falling back to fxhash: {}",
                        path, e
                    );
                    hash(path)
                },
                Ok(oid) => {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&oid.sha1()[..8]);
                    u64::from_be_bytes(buf)
                },
            }
        };
        let info = Info { hash, data_path };

        Ok(Self { file, info })
    }

    pub fn contains(&self, id: impl AsRef<oid>) -> bool {
        self.file.lookup(id).is_some()
    }

    pub fn ofs(&self, id: impl AsRef<oid>) -> Option<u64> {
        self.file
            .lookup(id)
            .map(|idx| self.file.pack_offset_at_index(idx))
    }
}

fn hash(p: &Path) -> u64 {
    use std::hash::{Hash as _, Hasher as _};

    let mut hasher = FxHasher::default();
    p.hash(&mut hasher);
    hasher.finish()
}
