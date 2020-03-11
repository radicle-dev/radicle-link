use std::{error::Error, iter::Iterator, path::Path, time::Duration};

use futures::stream::{self, StreamExt};
use git2::Repository;
use log::{info, warn};
use tempfile::{tempdir, TempDir};
use tokio::task;

use librad::{
    git::{self, server::GitServer, transport::RadTransport, GitProject},
    keys::device,
    meta,
    net::{
        connection::{BoundEndpoint, Endpoint},
        discovery,
        protocol::{rad, Protocol},
    },
    paths::Paths,
    peer::PeerId,
    project::{Project, ProjectId},
    util::monitor::Monitor,
};

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

impl rad::LocalStorage for MiniPeer {
    fn put(
        &self,
        provider: &PeerId,
        rad::Update::Project { project, .. }: rad::Update,
    ) -> rad::PutResult {
        info!("LocalStorage::put: {}", project);
        let repo = self.paths.projects_dir().join(project.path(&self.paths));
        if repo.exists() {
            info!("Project {} already present", project);
            rad::PutResult::Stale
        } else {
            info!("Cloning project {}", project);
            let res = Repository::clone(
                &format!("rad://{}@{}/{}", self.peer_id(), provider, project),
                repo,
            );

            if let Err(ref e) = res {
                warn!("Error cloning: {}", e);
            }

            res.map(|_| rad::PutResult::Applied)
                .unwrap_or(rad::PutResult::Error)
        }
    }

    fn ask(&self, rad::Update::Project { project, .. }: &rad::Update) -> bool {
        self.paths
            .projects_dir()
            .join(project.path(&self.paths))
            .exists()
    }
}

struct Bootstrap<'a> {
    peer: MiniPeer,
    proto: Protocol<MiniPeer>,
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

    let endpoint = Endpoint::bind(&key, "127.0.0.1:0".parse().unwrap()).await?;

    let rad = rad::Protocol::new(
        &PeerId::from(key),
        rad::PeerAdvertisement::new(endpoint.endpoint.local_addr().unwrap()),
        rad::MembershipParams::default(),
        peer.clone(),
    );

    let proto = Protocol::new(rad, git);

    Ok(Bootstrap {
        peer,
        proto,
        endpoint,
    })
}

struct Spawned {
    tmp: TempDir,
    peer: MiniPeer,
    proto: Protocol<MiniPeer>,
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
    );

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
        .announce(rad::Update::Project {
            project: project1.clone(),
            head: None,
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
