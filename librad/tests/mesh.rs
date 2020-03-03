use std::{
    error::Error,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use git2::Repository;

use librad::{
    git,
    keys::device,
    net::{
        connection::Endpoint,
        protocol::{rad, Protocol},
    },
    peer::PeerId,
};

type Repo = String;

#[derive(Clone)]
struct MiniPeer {
    name: String,
    repos: PathBuf,
}

impl MiniPeer {
    fn create_repo(&self, name: &str) -> Result<(), Box<dyn Error>> {
        let repo = git2::Repository::init_bare(self.repos.join(name))?;
        let sig = git2::Signature::now(&self.name, &format!("{}@leboeuf.xyz", self.name))?;
        let tree = {
            let mut index = repo.index()?;
            let tree_id = index.write_tree()?;
            repo.find_tree(tree_id)?
        };

        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        Ok(())
    }

    fn make(
        key: device::Key,
        repos: &Path,
    ) -> Result<(Self, SocketAddr, Protocol<Repo, Self>), Box<dyn Error>> {
        let mini_peer = MiniPeer {
            name: key.to_string(),
            repos: repos.into(),
        };

        let git = git::server::GitServer {
            export: repos.into(),
        };

        let endpoint = Endpoint::new(key, "127.0.0.1:0".parse().unwrap())?;

        //let rad = rad::Protocol::new(&PeerId::from(key),
        unimplemented!()
    }
}

#[async_trait]
impl rad::LocalStorage<Repo> for MiniPeer {
    async fn put(&self, provider: &PeerId, has: Repo) -> rad::PutResult {
        let repo = self.repos.join(has.clone());
        if repo.exists() {
            rad::PutResult::Stale
        } else {
            let url = format!("rad://{}/{}", provider, has);
            match Repository::clone(&url, repo) {
                Ok(_) => rad::PutResult::Applied,
                Err(_) => rad::PutResult::Error,
            }
        }
    }

    async fn ask(&self, want: &Repo) -> bool {
        self.repos.join(want).exists()
    }
}
