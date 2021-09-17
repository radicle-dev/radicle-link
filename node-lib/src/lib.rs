// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![feature(box_patterns)]

pub mod args;

mod cfg;
pub use cfg::{Seed, Seeds};

mod logging;
mod metrics;
pub mod node;
mod protocol;
mod signals;
mod tracking;

#[cfg(unix)]
pub mod socket_activation;
