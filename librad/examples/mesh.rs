use std::{error::Error, net::SocketAddr, path::Path, time::Duration};

use git2::Repository;
use log::{info, warn};
use tempfile::tempdir;
use tokio::task;

use librad::{
    git::{self, server::GitServer, GitProject},
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

#[tokio::main]
async fn main() {
    librad::init();
    env_logger::init();

    let tmp1 = tempdir().unwrap();
    println!("Boostrapping peer1");
    let Bootstrap {
        peer: peer1,
        proto: proto1,
        endpoint: endpoint1,
    } = bootstrap("peer1", device::Key::new(), tmp1.path())
        .await
        .expect("Could not boostrap peer1");

    let tmp2 = tempdir().unwrap();
    println!("Boostrapping peer3");
    let Bootstrap {
        peer: peer2,
        proto: mut proto2,
        endpoint: endpoint2,
    } = bootstrap("peer2", device::Key::new(), tmp2.path())
        .await
        .expect("Could not boostrap peer2");

    let tmp3 = tempdir().unwrap();
    println!("Boostrapping peer3");
    let Bootstrap {
        peer: peer3,
        proto: mut proto3,
        endpoint: endpoint3,
    } = bootstrap("peer3", device::Key::new(), tmp3.path())
        .await
        .expect("Could not boostrap peer3");

    let disco1 = discovery::Static::<SocketAddr>::new(vec![]);
    let disco2 = discovery::Static::new(vec![(peer1.peer_id(), endpoint1.local_addr().unwrap())]);
    let disco3 = discovery::Static::new(vec![
        (peer1.peer_id(), endpoint1.local_addr().unwrap()),
        (peer2.peer_id(), endpoint2.local_addr().unwrap()),
    ]);

    let transport = git::transport::register();
    transport.register_stream_factory(&peer1.peer_id(), Box::new(proto1.clone()));
    transport.register_stream_factory(&peer2.peer_id(), Box::new(proto2.clone()));
    transport.register_stream_factory(&peer3.peer_id(), Box::new(proto3.clone()));

    let shutdown = Monitor::new();

    println!("Spawning peer1");
    let _ = task::spawn({
        let mut proto1 = proto1.clone();
        let shutdown = shutdown.clone();
        async move { proto1.run(endpoint1, disco1, shutdown).await }
    });
    println!("Spawning peer2");
    let _ = task::spawn({
        let shutdown = shutdown.clone();
        async move { proto2.run(endpoint2, disco2, shutdown).await }
    });
    println!("Spawning peer3");
    let _ = task::spawn({
        let shutdown = shutdown.clone();
        async move { proto3.run(endpoint3, disco3, shutdown).await }
    });

    tokio::time::delay_for(Duration::from_secs(1)).await;

    println!("Creating project1");
    let project1 = {
        let repo = peer1.create_repo(tmp1.path().join("repo1")).unwrap();
        GitProject::init(
            &peer1.paths,
            &peer1.key,
            &repo,
            meta::Project::new("mini1", &peer1.peer_id()),
            meta::Contributor::new(),
        )
        .unwrap()
        .into()
    };

    println!("Announcing project1");
    proto1
        .announce(rad::Update::Project {
            project: project1,
            head: None,
        })
        .await;

    // TODO: replace by subscription on protocol events
    tokio::time::delay_for(Duration::from_secs(2)).await;

    println!("Shutting down");
    shutdown.put(()).await;

    assert_eq!(
        Project::list(&peer1.paths).collect::<Vec<ProjectId>>(),
        Project::list(&peer2.paths).collect::<Vec<ProjectId>>()
    );
    assert_eq!(
        Project::list(&peer2.paths).collect::<Vec<ProjectId>>(),
        Project::list(&peer3.paths).collect::<Vec<ProjectId>>()
    );

    println!("If we got here, all peers have replicated each other's repos");
}
