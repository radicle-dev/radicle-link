// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(never_type)]

#[macro_use]
extern crate lazy_static;

use std::fmt;

use thiserror::Error;

use librad::{git::Urn, PeerId};

pub mod cli;

pub mod any;
pub mod local;
pub mod person;
pub mod project;
pub mod rad_refs;
pub mod refs;
pub mod tracking;

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
