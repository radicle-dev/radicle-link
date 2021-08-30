// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, ops::Deref};

use librad::{
    git::{
        identities::{self, Person, Project},
        replication::{self, ReplicateResult},
        storage::{fetcher, Storage},
    },
    identities::{delegation, payload},
    net::{connection::LocalInfo, peer::Peer},
    Signer,
};

pub struct TestPerson {
    pub owner: Person,
}

impl TestPerson {
    pub fn create(storage: &Storage) -> anyhow::Result<Self> {
        let peer_id = storage.peer_id();
        let alice = identities::person::create(
            storage,
            payload::Person {
                name: "alice".into(),
            },
            Some(*peer_id.as_public_key()).into_iter().collect(),
        )?;

        Ok(Self { owner: alice })
    }

    pub fn update(self, storage: &Storage) -> anyhow::Result<Self> {
        let payload = payload::Person {
            name: "alice-laptop".into(),
        }
        .into();
        let owner =
            identities::person::update(storage, &self.owner.urn(), None, Some(payload), None)?;
        Ok(Self { owner })
    }

    /// Pull (fetch or clone) the project from known running peer `A` to peer
    /// `B`.
    pub async fn pull<A, B, S>(&self, from: &A, to: &B) -> anyhow::Result<ReplicateResult>
    where
        A: Deref<Target = Peer<S>> + LocalInfo<Addr = SocketAddr>,
        B: Deref<Target = Peer<S>>,

        S: Signer + Clone,
    {
        let remote_peer = from.local_peer_id();
        let remote_addrs = from.listen_addrs();
        let urn = self.owner.urn();
        let cfg = to.protocol_config().replication;
        let res = to
            .using_storage(move |storage| {
                let fetcher = fetcher::PeerToPeer::new(urn, remote_peer, remote_addrs)
                    .build(storage)
                    .unwrap()
                    .unwrap();
                replication::replicate(storage, fetcher, cfg, None)
            })
            .await??;
        Ok(res)
    }
}

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
            Some(*peer_id.as_public_key()).into_iter().collect(),
        )?;
        let local_id = identities::local::load(storage, alice.urn())?
            .expect("local id must exist as we just created it");
        let proj = identities::project::create(
            storage,
            local_id,
            payload::Project {
                name: "radicle-link".into(),
                description: Some("pea two pea".into()),
                default_branch: Some("next".into()),
            },
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
            payload::Project {
                name: "radicle-link".into(),
                description: Some("pea two pea".into()),
                default_branch: Some("next".into()),
            },
            delegation::Indirect::from(person.owner.clone()),
        )?;

        Ok(Self {
            owner: person.owner,
            project: proj,
        })
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
    pub async fn pull<A, B, S>(&self, from: &A, to: &B) -> anyhow::Result<ReplicateResult>
    where
        A: Deref<Target = Peer<S>> + LocalInfo<Addr = SocketAddr>,
        B: Deref<Target = Peer<S>>,

        S: Signer + Clone,
    {
        let remote_peer = from.local_peer_id();
        let remote_addrs = from.listen_addrs();
        let urn = self.project.urn();
        let cfg = to.protocol_config().replication;
        let res = to
            .using_storage(move |storage| -> anyhow::Result<ReplicateResult> {
                let fetcher = fetcher::PeerToPeer::new(urn, remote_peer, remote_addrs)
                    .build(storage)
                    .expect("creating a git2 remote should not normally fail")?;
                Ok(replication::replicate(storage, fetcher, cfg, None)?)
            })
            .await??;
        Ok(res)
    }
}

pub fn create_test_project(storage: &Storage) -> Result<TestProject, anyhow::Error> {
    TestProject::create(storage)
}
