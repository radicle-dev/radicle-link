use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::project::ProjectId;

pub type Capabilities = HashSet<Capability>;

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capability {
    Seed,
    GitDaemon { port: u16 },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PeerInfo {
    pub provided_projects: HashSet<ProjectId>,
    pub capabilities: Capabilities,
}
