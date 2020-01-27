use async_std::task;
use failure::Fail;
use log::info;
use structopt::StructOpt;

use librad::{
    keys::storage::{Pinentry, Storage},
    net::{p2p, protocol::Capability},
    project::Project,
};

use crate::{config::Config, error::Error};

#[derive(StructOpt)]
pub struct Serve {
    #[structopt(long, default_value = "9418", env = "DEFAULT_GIT_PORT")]
    git_port: u16,
}

impl Serve {
    pub fn run<K, P>(self, cfg: Config<K, P>) -> Result<(), Error<P::Error>>
    where
        K: Storage<P>,
        P: Pinentry,
        P::Error: Fail,
    {
        let key = cfg.keystore.get_device_key()?;
        let worker = p2p::Worker::new(
            key,
            None,
            vec![Capability::GitDaemon {
                port: self.git_port,
            }]
            .into_iter()
            .collect(),
        )
        .unwrap();
        let service = worker.service().clone();

        Project::list(&cfg.paths).for_each(|pid| {
            info!("Serving project {}", pid);
            service.have(&pid)
        });

        task::block_on(worker).map_err(|e| e.into())
    }
}
