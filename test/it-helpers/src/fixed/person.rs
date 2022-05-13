use std::{net::SocketAddr, ops::Deref};

use librad::{
    git::{
        identities::{self, local::LocalIdentity, Person},
        storage::Storage,
    },
    identities::{delegation::Direct, payload},
    net::{connection::LocalInfo, peer::Peer, protocol::RequestPullGuard, replication},
    Signer,
};
use tracing::{info, instrument};

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
            Direct::new(*peer_id.as_public_key()),
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

    pub fn local(&self, storage: &Storage) -> anyhow::Result<Option<LocalIdentity>> {
        Ok(identities::local::load(storage, self.owner.urn())?)
    }

    /// Pull (fetch or clone) the project from known running peer `A` to peer
    /// `B`.
    #[instrument(name = "test_person", skip(self, from, to), err)]
    pub async fn pull<A, B, S, Auth>(
        &self,
        from: &A,
        to: &B,
    ) -> anyhow::Result<replication::Success>
    where
        A: Deref<Target = Peer<S, Auth>> + LocalInfo<Addr = SocketAddr>,
        B: Deref<Target = Peer<S, Auth>>,

        S: Signer + Clone,
        Auth: RequestPullGuard,
    {
        let remote_peer = from.local_peer_id();
        let remote_addrs = from.listen_addrs();
        let urn = self.owner.urn();

        info!("pull from {} to {}", remote_peer, to.peer_id());

        Ok(to
            .client()?
            .replicate((remote_peer, remote_addrs), urn, None)
            .await?)
    }
}
