use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub type Capabilities = HashSet<Capability>;

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Seed,
    GitDaemon { port: u16 },
}
