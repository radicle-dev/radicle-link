// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#[macro_use]
extern crate lazy_static;
extern crate radicle_std_ext as std_ext;

use std::fmt;

use librad::{git::Urn, PeerId};
use thiserror::Error;

pub mod cli;

pub mod any;
pub mod local;
pub mod person;
pub mod project;
pub mod rad_refs;
pub mod refs;
pub mod tracking;

pub mod display;
mod field;
pub mod git;

#[derive(Debug, Error)]
#[error("no default identity was found, perhaps you need to set one")]
pub struct MissingDefaultIdentity;

#[derive(Debug, Error)]
pub struct NotFound {
    urn: Urn,
    peer: Option<PeerId>,
}

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.peer {
            Some(peer) => write!(
                f,
                "the URN `{}` did not exist for the peer `{}`",
                self.urn, peer
            ),
            None => write!(f, "the URN `{}` did not exist", self.urn),
        }
    }
}
