// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::{self, Display};

use git_ext as ext;
use multihash::Multihash;

use crate::{hash::Hash, identities::git::Urn};

pub trait AsNamespace {
    fn as_namespace(&self) -> String;
}

pub type Legacy = Hash;

impl AsNamespace for Legacy {
    fn as_namespace(&self) -> String {
        self.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Namespace(ext::Oid);

impl AsNamespace for Namespace {
    fn as_namespace(&self) -> String {
        self.to_string()
    }
}

impl From<ext::Oid> for Namespace {
    fn from(oid: ext::Oid) -> Self {
        Self(oid)
    }
}

impl From<git2::Oid> for Namespace {
    fn from(oid: git2::Oid) -> Self {
        Self::from(ext::Oid::from(oid))
    }
}

impl From<Urn> for Namespace {
    fn from(urn: Urn) -> Self {
        Self::from(urn.id)
    }
}

impl From<&Urn> for Namespace {
    fn from(urn: &Urn) -> Self {
        Self::from(urn.id)
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&multibase::encode(
            multibase::Base::Base32Z,
            Multihash::from(&self.0),
        ))
    }
}
