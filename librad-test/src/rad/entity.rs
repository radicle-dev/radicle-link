// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::ops::{Deref, DerefMut};

use librad::{
    keys::PublicKey,
    meta::{
        entity::{Draft, Error, Resolver},
        Project,
        User,
    },
    uri::RadUrn,
};

#[derive(Clone)]
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

impl Resolver<User<Draft>> for Alice {
    fn resolve(&self, _uri: &RadUrn) -> Result<User<Draft>, Error> {
        Ok(self.0.clone())
    }

    fn resolve_revision(&self, _uri: &RadUrn, _revision: u64) -> Result<User<Draft>, Error> {
        Ok(self.0.clone())
    }
}

#[derive(Clone)]
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
