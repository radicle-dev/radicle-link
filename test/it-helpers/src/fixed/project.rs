use std::{net::SocketAddr, ops::Deref};

use librad::{
    git::{
        identities::{self, Person, Project},
        storage::Storage,
        types::{Namespace, Reference},
        Urn,
    },
    identities::{
        delegation::{self, Direct},
        payload,
    },
    net::{
        connection::LocalInfo,
        peer::{Peer, RequestPullGuard},
        replication,
    },
    Signer,
};
use tracing::{info, instrument};

use crate::testnet::RunningTestPeer;

use super::TestPerson;

pub struct TestProject {
    pub owner: Person,
    pub project: Project,
}

impl TestProject {
    pub fn create(storage: &Storage) -> anyhow::Result<Self> {
        Self::create_with_payload(storage, Self::default_payload())
    }

    pub fn create_with_payload(
        storage: &Storage,
        payload: payload::Project,
    ) -> anyhow::Result<Self> {
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
            payload,
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
        let urn = self.project.urn();

        info!("pull from {} to {}", remote_peer, to.peer_id());

        Ok(to
            .client()?
            .replicate((remote_peer, remote_addrs), urn, None)
            .await?)
    }

    /// Add maintainers to a TestProject
    ///
    /// The `home` argument must be a peer which is already a delegate of the
    /// project. The [`Maintainers`] struct which is returned can be used to
    /// add maintainers using [`Maintainers::add`] before calling
    /// [`Maintainers::setup`] to perform the cross signing which adds the
    /// delegates to the project.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use it_helpers::{testnet::RunningTestPeer, fixed::{TestProject, TestPerson}};
    /// # async fn doit() {
    /// let peer: RunningTestPeer = unimplemented!();
    /// let peer2: RunningTestPeer = unimplemented!();
    ///
    /// let project = peer.using_storage(TestProject::create).await.unwrap().unwrap();
    /// let other_person = peer2.using_storage(TestPerson::create).await.unwrap().unwrap();
    /// project.maintainers(&peer).add(&other_person, &peer2).setup().await.unwrap()
    /// # }
    /// ```
    pub fn maintainers<'a>(&'a self, home: &'a RunningTestPeer) -> Maintainers<'a> {
        Maintainers {
            project: self,
            home,
            other_maintainers: Vec::new(),
        }
    }
}

pub struct Maintainers<'a> {
    project: &'a TestProject,
    home: &'a RunningTestPeer,
    other_maintainers: Vec<(&'a RunningTestPeer, &'a TestPerson)>,
}

impl<'a> Maintainers<'a> {
    pub fn add(mut self, person: &'a TestPerson, peer: &'a RunningTestPeer) -> Self {
        self.other_maintainers.push((peer, person));
        self
    }

    /// Perform the cross signing necessary to add all the maintainers to the
    /// project.
    ///
    /// What this does is the following:
    /// * Track each of the maintainers remotes for the given peer on the `home`
    ///   peer
    /// * Add all of the `Person` identities as indirect delegates of the
    ///   projects on the home peer
    /// * For each maintainer:
    ///     * Pull the updated document into the maintainers peer and `update`
    ///       the document
    ///     * Pull the updated document back into the home peer
    ///     * On the home peer `update` and `merge` the document
    /// * Finally pull the completed document back into each of the maintainer
    ///   peers
    pub async fn setup(self) -> anyhow::Result<()> {
        // make sure the home peer has all the other identities
        for (peer, testperson) in &self.other_maintainers {
            self.home
                .track(self.project.project.urn(), Some(peer.peer_id()))
                .await?;
            testperson.pull(*peer, self.home).await?;
        }
        // Add the other identities as delegates of the project
        self.home
            .using_storage({
                let urn = self.project.project.urn();
                let owners = std::iter::once(self.project.owner.clone())
                    .chain(self.other_maintainers.iter().map(|(_, m)| m.owner.clone()))
                    .map(either::Either::Right)
                    .collect::<Vec<_>>();
                move |storage| -> Result<(), anyhow::Error> {
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        librad::identities::delegation::Indirect::try_from_iter(owners).unwrap(),
                    )?;
                    identities::project::verify(storage, &urn)?;
                    Ok(())
                }
            })
            .await??;

        // For each maintainer, sign the updated document and merge it back into the
        // home peer
        for (peer, _) in &self.other_maintainers {
            // pull the document into the maintainer peer
            self.project.pull(self.home, *peer).await?;
            // Sign the project document using the maintiners peer
            peer.using_storage({
                let urn = self.project.project.urn();
                let peer_id = self.home.peer_id();
                let rad =
                    Urn::try_from(Reference::rad_id(Namespace::from(&urn)).with_remote(peer_id))
                        .unwrap();
                move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
                    let project = identities::project::get(&storage, &rad)?.unwrap();
                    identities::project::update(
                        storage,
                        &urn,
                        None,
                        None,
                        project.delegations().clone(),
                    )?;
                    identities::project::merge(storage, &urn, peer_id)?;
                    Ok(identities::project::verify(storage, &urn)?)
                }
            })
            .await??;

            // pull the signed update back into the home peer
            self.project.pull(*peer, self.home).await?;

            // Merge the signed update into peer1
            self.home
                .using_storage({
                    let urn = self.project.project.urn();
                    let peer_id = peer.peer_id();
                    move |storage| -> Result<Option<identities::VerifiedProject>, anyhow::Error> {
                        identities::project::merge(storage, &urn, peer_id)?;
                        Ok(identities::project::verify(storage, &urn)?)
                    }
                })
                .await??;
        }

        // pull the finished document back to the maintainer peers
        for (peer, _) in self.other_maintainers {
            self.project.pull(self.home, peer).await?;
        }
        Ok(())
    }
}
