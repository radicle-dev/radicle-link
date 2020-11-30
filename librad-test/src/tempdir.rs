// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    ops::{Deref, DerefMut},
    path::Path,
};

use tempfile::{tempdir, TempDir};

pub struct WithTmpDir<A> {
    _tmp: TempDir,
    inner: A,
}

impl<A> WithTmpDir<A> {
    pub fn new<F, E>(mk_inner: F) -> Result<Self, E>
    where
        F: FnOnce(&Path) -> Result<A, E>,
        E: From<io::Error>,
    {
        let tmp = tempdir()?;
        let inner = mk_inner(tmp.path())?;
        Ok(Self { _tmp: tmp, inner })
    }
}

impl<A> Deref for WithTmpDir<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A> DerefMut for WithTmpDir<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
