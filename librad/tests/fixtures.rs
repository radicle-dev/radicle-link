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

use std::ops::{Deref, DerefMut};

use async_trait::async_trait;

use librad::{
    keys::PublicKey,
    meta::{
        entity::{Draft, Error, Resolver},
        Project,
        User,
    },
    uri::RadUrn,
};

pub struct Alice(User<Draft>);

impl Alice {
    pub fn new(pk: PublicKey) -> Self {
        Self(User::<Draft>::create("alice".to_owned(), pk).unwrap())
    }
}

impl Deref for Alice {
    type Target = User<Draft>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Alice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait]
impl Resolver<User<Draft>> for Alice {
    async fn resolve(&self, _uri: &RadUrn) -> Result<User<Draft>, Error> {
        Ok(self.0.clone())
    }

    async fn resolve_revision(&self, _uri: &RadUrn, _revision: u64) -> Result<User<Draft>, Error> {
        Ok(self.0.clone())
    }
}

pub struct Radicle(Project<Draft>);

impl Radicle {
    pub fn new(owner: &User<Draft>) -> Self {
        Self(Project::<Draft>::create("radicle".to_owned(), owner.urn()).unwrap())
    }
}

impl Deref for Radicle {
    type Target = Project<Draft>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Radicle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
