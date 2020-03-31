// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{
    error::Error,
    iter::Iterator,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    time::Duration,
};

use futures::stream::{self, StreamExt};
use git2::{build::RepoBuilder, FetchOptions, RemoteCallbacks, Repository};
use serde::{Deserialize, Serialize};
use tempfile::{tempdir, TempDir};
use tokio::task;
use tracing;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use librad::{
    git::{self, server::GitServer, transport::RadTransport, GitProject},
    keys::device,
    meta,
    net::{
        connection::LocalInfo,
        discovery,
        gossip,
        protocol::Protocol,
        quic::{BoundEndpoint, Endpoint},
    },
    paths::Paths,
    peer::PeerId,
    project::{Project, ProjectId},
    sync::Monitor,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProjectCreated {
    id: ProjectId,
}

#[derive(Clone)]
struct MiniPeer {
    name: String,
    key: device::Key,
    paths: Paths,
}

impl MiniPeer {
    fn peer_id(&self) -> PeerId {
        PeerId::from(self.key.clone())
    }

    fn create_repo<P: AsRef<Path>>(&self, path: P) -> Result<git2::Repository, Box<dyn Error>> {
        let repo = git2::Repository::init(path)?;
        {
            let sig = git2::Signature::now(&self.name, &format!("{}@{}", self.name, self.key))?;
            let tree = {
                let mut index = repo.index()?;
                let tree_id = index.write_tree()?;
                repo.find_tree(tree_id)?
            };

            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        }

        Ok(repo)
    }
}

impl gossip::LocalStorage for MiniPeer {
    type Update = ProjectCreated;

    fn put(&self, provider: &PeerId, ProjectCreated { id }: ProjectCreated) -> gossip::PutResult {
        let span = tracing::info_span!("MiniPeer::put", project.id = %id);
        let _guard = span.enter();

        let repo_path = self.paths.projects_dir().join(id.path(&self.paths));
        if Repository::open_bare(&repo_path).is_ok() {
            // Note: we would want to fetch here
            tracing::info!("{}: Project {} already present", self.peer_id(), id);
            gossip::PutResult::Stale
        } else {
            tracing::info!(
                "{}: Cloning project {} from {} into {}",
                self.peer_id(),
                id,
                provider,
                repo_path.display()
            );

            let mut callbacks = RemoteCallbacks::new();
            callbacks.sideband_progress(|bytes| {
                eprintln!("{}", String::from_utf8_lossy(&bytes));
                true
            });
            let mut fetch_opts = FetchOptions::new();
            fetch_opts.remote_callbacks(callbacks);

            RepoBuilder::new()
                .bare(true)
                .fetch_options(fetch_opts)
                .clone(
                    &format!("rad://{}@{}/{}", self.peer_id(), provider, id),
                    &repo_path,
                )
                .map(|repo| {
                    tracing::info!(
                        msg = "Successfully Cloned",
                        repo.path = %repo.path().display(),
                        repo.head = ?repo.head().unwrap().peel_to_commit().unwrap().id(),
                    );

                    gossip::PutResult::Applied
                })
                .unwrap_or_else(|e| {
                    tracing::warn!(msg = "Error cloning", error = %e);
                    gossip::PutResult::Error
                })
        }
    }

    fn ask(&self, ProjectCreated { id }: &ProjectCreated) -> bool {
        self.paths
            .projects_dir()
            .join(id.path(&self.paths))
            .exists()
    }
}

struct Bootstrap<'a> {
    peer: MiniPeer,
    proto: Protocol<MiniPeer, ProjectCreated>,
    endpoint: BoundEndpoint<'a>,
}

async fn bootstrap<'a>(
    name: &str,
    key: device::Key,
    root: &Path,
) -> Result<Bootstrap<'a>, Box<dyn Error>> {
    let peer = MiniPeer {
        name: name.into(),
        key: key.clone(),
        paths: Paths::from_root(root)?,
    };

    let git = GitServer {
        export: peer.paths.projects_dir().into(),
    };

    let endpoint = Endpoint::bind(
        &key,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0)),
    )
    .await?;

    let gossip = gossip::Protocol::new(
        &PeerId::from(key),
        gossip::PeerAdvertisement::new(endpoint.local_addr().unwrap()),
        gossip::MembershipParams::default(),
        peer.clone(),
    );

    let proto = Protocol::new(gossip, git);

    Ok(Bootstrap {
        peer,
        proto,
        endpoint,
    })
}

struct Spawned {
    tmp: TempDir,
    peer: MiniPeer,
    proto: Protocol<MiniPeer, ProjectCreated>,
    endpoint: Endpoint,
}

async fn spawn(
    prev: Option<(PeerId, Endpoint)>,
    transport: RadTransport,
    shutdown: Monitor<()>,
    i: usize,
) -> Spawned {
    let tmp = tempdir().unwrap();
    let Bootstrap {
        peer,
        proto,
        endpoint,
    } = bootstrap(&format!("peer{}", i), device::Key::new(), tmp.path())
        .await
        .unwrap();

    let disco = discovery::Static::new(
        prev.map(|(peer0, endpoint0)| vec![(peer0, endpoint0.local_addr().unwrap())])
            .unwrap_or_else(|| vec![]),
    )
    .into_stream();

    transport.register_stream_factory(&peer.peer_id(), Box::new(proto.clone()));

    let endpoint0 = endpoint.endpoint.clone();

    let _ = task::spawn({
        let mut proto = proto.clone();
        async move { proto.run(endpoint, disco, shutdown).await }
    });

    Spawned {
        tmp,
        peer,
        proto,
        endpoint: endpoint0,
    }
}

#[tokio::main]
async fn main() {
    librad::init();
    env_logger::init();

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

    let transport = git::transport::register();
    let shutdown = Monitor::new();

    let peer1 = spawn(None, transport.clone(), shutdown.clone(), 1).await;
    let init = (peer1.peer.peer_id(), peer1.endpoint.clone());
    let peers: Vec<Spawned> = stream::unfold((init, 2), |(prev, i)| {
        let transport = transport.clone();
        let shutdown = shutdown.clone();
        async move {
            if i <= 3 {
                let peer = spawn(Some(prev), transport, shutdown, i).await;
                let next = (peer.peer.peer_id(), peer.endpoint.clone());
                Some((peer, (next, i + 1)))
            } else {
                None
            }
        }
    })
    .collect()
    .await;

    // Let it settle for a bit
    tokio::time::delay_for(Duration::from_secs(1)).await;

    println!("Creating project1");
    let project1: ProjectId = {
        let repo = peer1
            .peer
            .create_repo(peer1.tmp.path().join("repo1"))
            .unwrap();
        GitProject::init(
            &peer1.peer.paths,
            &peer1.peer.key,
            &repo,
            meta::Project::new("mini1", &peer1.peer.peer_id()),
            meta::Contributor::new(),
        )
        .unwrap()
        .into()
    };

    println!("Announcing project1");
    peer1
        .proto
        .announce(ProjectCreated {
            id: project1.clone(),
        })
        .await;

    // TODO: replace by subscription on protocol events
    tokio::time::delay_for(Duration::from_secs(2)).await;

    println!("Shutting down");
    shutdown.put(()).await;

    let replicated: Vec<ProjectId> = peers
        .iter()
        .map(|spawned| Project::list(&spawned.peer.paths).collect::<Vec<ProjectId>>())
        .flatten()
        .collect();

    assert_eq!(replicated.len(), peers.len());
    assert!(replicated.iter().all(|project| project == &project1));

    println!("If we got here, all peers have replicated each other's repos");
}
