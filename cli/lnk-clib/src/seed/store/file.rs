// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs,
    io::{self, BufRead as _},
    marker::PhantomData,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::seed::Seed;

use super::Store;

/// File storage for seeds of the form:
///
/// For the expected format of each entry, see [`Store`].
pub struct FileStore<T> {
    path: PathBuf,
    _marker: PhantomData<T>,
}

impl<T> FileStore<T> {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let path = path.as_ref();
        if !path.exists() {
            fs::File::create(path)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            _marker: PhantomData,
        })
    }

    pub fn iter(&self) -> Result<Iter<T>, io::Error>
    where
        T: FromStr,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        let buf = fs::File::open(self.path.clone())?;
        Ok(Iter {
            inner: io::BufReader::new(buf).lines(),
            _marker: self._marker,
        })
    }
}

pub struct Iter<T> {
    inner: io::Lines<io::BufReader<fs::File>>,
    _marker: PhantomData<T>,
}

impl<T: FromStr> Iterator for Iter<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<Seed<T>, error::Iter>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|seed| {
            seed.map_err(error::Iter::from)
                .and_then(|seed| seed.parse().map_err(error::Iter::from))
        })
    }
}

impl<T: FromStr> Store for FileStore<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    type Scan = io::Error;
    type Iter = error::Iter;
    type Addrs = T;
    type Seeds = Iter<T>;

    fn scan(&self) -> Result<Self::Seeds, Self::Scan> {
        self.iter()
    }
}

pub mod error {
    use std::io;

    use thiserror::Error;

    use crate::seed::error;

    #[derive(Debug, Error)]
    pub enum Iter {
        #[error(transparent)]
        Io(#[from] io::Error),
        #[error(transparent)]
        Parse(#[from] error::Parse),
    }
}
