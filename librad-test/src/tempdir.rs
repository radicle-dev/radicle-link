// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
