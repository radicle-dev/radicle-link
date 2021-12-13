// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![cfg_attr(feature = "nightly", feature(try_trait_v2))]

extern crate radicle_std_ext as std_ext;

mod spawn;
pub use spawn::{Cancelled, JoinError, Spawner, Stats, Task};

mod time;
pub use time::{interval, sleep, timeout, Elapsed};

pub mod tasks;
