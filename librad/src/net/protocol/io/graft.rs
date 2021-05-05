// Copyright © 2019-2021 The Radicle Foundation <hello@radicle.foundation>
// Copyright © 2021      The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::Duration;

use crate::{git::replication, net::protocol::state};

pub mod error;

mod rere;
pub use rere::{is_interesting, rere};

mod scheduled;
pub(in crate::net::protocol) use scheduled::{Env, Grafting, Progress, Queue, Scheduler};

#[derive(Clone, Debug)]
pub struct Config {
    pub replication: replication::Config,
    pub fetch_slot_wait_timeout: Duration,
}

impl From<state::StateConfig> for Config {
    fn from(c: state::StateConfig) -> Self {
        Self {
            replication: c.replication,
            fetch_slot_wait_timeout: c.fetch.fetch_slot_wait_timeout,
        }
    }
}
