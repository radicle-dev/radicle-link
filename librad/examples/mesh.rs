use std::{error::Error, net::SocketAddr, path::Path};

use async_trait::async_trait;
use futures::channel::oneshot;
use git2::Repository;
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
        quic,
    },
    paths::Paths,
    peer::PeerId,
    project::{Project, ProjectId},
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

#[async_trait]
impl rad::LocalStorage for MiniPeer {
    async fn put(
        &self,
        provider: &PeerId,
        rad::Update::Project { project, .. }: rad::Update,
    ) -> rad::PutResult {
        let repo = self.paths.projects_dir().join(project.path(&self.paths));
        if repo.exists() {
            rad::PutResult::Stale
        } else {
            Repository::clone(&format!("rad://{}/{}", provider, project), repo)
                .map(|_| rad::PutResult::Applied)
                .unwrap_or(rad::PutResult::Error)
        }
    }

    async fn ask(&self, rad::Update::Project { project, .. }: &rad::Update) -> bool {
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
        rad::PeerAdvertisement::new(endpoint.endpoint.local_addr().unwrap().port()),
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

fn main() {
    librad::init();

    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        println!("enter");
        {
            let key = device::Key::new();
            let mut builder = quinn::Endpoint::builder();
            builder.default_client_config(quic::make_client_config(&key));
            builder.listen(quic::make_server_config(&key));

            let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let _ = builder.bind(&addr).unwrap();
        }
        println!("sandman");

        let tmp1 = tempdir().unwrap();
        println!("boostrapping peer1");
        let Bootstrap {
            peer: peer1,
            proto: proto1,
            endpoint: endpoint1,
        } = bootstrap("peer1", device::Key::new(), tmp1.path())
            .await
            .expect("Could not boostrap peer1");

        println!("peer1 ready");

        let tmp2 = tempdir().unwrap();
        println!("boostrapping peer3");
        let Bootstrap {
            peer: peer2,
            proto: mut proto2,
            endpoint: endpoint2,
        } = bootstrap("peer2", device::Key::new(), tmp2.path())
            .await
            .expect("Could not boostrap peer2");

        println!("peer2 ready");

        let tmp3 = tempdir().unwrap();
        println!("boostrapping peer3");
        let Bootstrap {
            peer: peer3,
            proto: mut proto3,
            endpoint: endpoint3,
        } = bootstrap("peer3", device::Key::new(), tmp3.path())
            .await
            .expect("Could not boostrap peer3");

        println!("peer3 ready");

        let disco1 = discovery::Static::<SocketAddr>::new(vec![]);
        let disco2 = discovery::Static::new(vec![
            (peer1.peer_id(), endpoint1.local_addr().unwrap()),
            (peer3.peer_id(), endpoint3.local_addr().unwrap()),
        ]);
        let disco3 = discovery::Static::new(vec![
            (peer1.peer_id(), endpoint1.local_addr().unwrap()),
            (peer2.peer_id(), endpoint2.local_addr().unwrap()),
        ]);

        let transport = git::transport::register();
        transport.register_stream_factory(Box::new(proto1.clone()));
        transport.register_stream_factory(Box::new(proto2.clone()));
        transport.register_stream_factory(Box::new(proto3.clone()));

        let p1 = {
            let (tx, rx) = oneshot::channel();
            task::spawn({
                let mut proto1 = proto1.clone();
                async move { proto1.run(endpoint1, disco1, rx).await }
            });
            tx
        };
        let p2 = {
            let (tx, rx) = oneshot::channel();
            task::spawn(async move { proto2.run(endpoint2, disco2, rx).await });
            tx
        };
        let p3 = {
            let (tx, rx) = oneshot::channel();
            task::spawn(async move { proto3.run(endpoint3, disco3, rx).await });
            tx
        };

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

        proto1
            .announce(rad::Update::Project {
                project: project1,
                head: None,
            })
            .await;

        assert_eq!(
            Project::list(&peer1.paths).collect::<Vec<ProjectId>>(),
            Project::list(&peer2.paths).collect::<Vec<ProjectId>>()
        );

        p1.send(()).unwrap();
        p2.send(()).unwrap();
        p3.send(()).unwrap();
    });
}
