use std::{net::SocketAddr, ops::Deref};

use librad::{
    git::{
        identities::{self, Person, Project},
        storage::Storage,
    },
    identities::{
        delegation::{self, Direct},
        payload,
    },
    net::{connection::LocalInfo, peer::Peer, replication},
    Signer,
};
use tracing::{info, instrument};

use super::TestPerson;

pub struct TestProject {
    pub owner: Person,
    pub project: Project,
}

impl TestProject {
    pub fn create(storage: &Storage) -> anyhow::Result<Self> {
        let peer_id = storage.peer_id();
        let alice = identities::person::create(
            storage,
            payload::Person {
                name: "alice".into(),
            },
            Direct::new(*peer_id.as_public_key()),
        )?;
        let local_id = identities::local::load(storage, alice.urn())?
            .expect("local id must exist as we just created it");
        let proj = identities::project::create(
            storage,
            local_id,
            Self::default_payload(),
            delegation::Indirect::from(alice.clone()),
        )?;

        Ok(Self {
            owner: alice,
            project: proj,
        })
    }

    pub fn from_test_person(storage: &Storage, person: TestPerson) -> anyhow::Result<Self> {
        let local_id = identities::local::load(storage, person.owner.urn())?
            .expect("local id must exist as we just created it");
        let proj = identities::project::create(
            storage,
            local_id,
            Self::default_payload(),
            delegation::Indirect::from(person.owner.clone()),
        )?;

        Ok(Self {
            owner: person.owner,
            project: proj,
        })
    }

    pub fn default_payload() -> payload::Project {
        payload::Project {
            name: "radicle-link".into(),
            description: Some("pea two pea".into()),
            default_branch: Some("next".into()),
        }
    }

    pub fn from_project_payload(
        storage: &Storage,
        owner: Person,
        payload: payload::Project,
    ) -> anyhow::Result<Self> {
        let local_id = identities::local::load(storage, owner.urn())?
            .expect("local id must exist as we just created it");
        let proj = identities::project::create(
            storage,
            local_id,
            payload,
            delegation::Indirect::from(owner.clone()),
        )?;

        Ok(Self {
            owner,
            project: proj,
        })
    }

    /// Pull (fetch or clone) the project from known running peer `A` to peer
    /// `B`.
    #[instrument(name = "test_project", skip(self, from, to))]
    pub async fn pull<A, B, S>(&self, from: &A, to: &B) -> anyhow::Result<replication::Success>
    where
        A: Deref<Target = Peer<S>> + LocalInfo<Addr = SocketAddr>,
        B: Deref<Target = Peer<S>>,

        S: Signer + Clone,
    {
        let remote_peer = from.local_peer_id();
        let remote_addrs = from.listen_addrs();
        let urn = self.project.urn();

        info!("pull from {} to {}", remote_peer, to.peer_id());

        Ok(to.replicate((remote_peer, remote_addrs), urn, None).await?)
    }
}
