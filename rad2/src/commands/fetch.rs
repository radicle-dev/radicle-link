use std::{io, path::Path, sync::Arc, time::Duration};

use async_std::task;
use failure::Fail;
use log::{info, warn};
use retry::{delay, retry};
use structopt::StructOpt;

use libp2p::{multiaddr::Protocol, PeerId};

use librad::{
    keys::storage::{Pinentry, Storage},
    net::{p2p, tcp},
    project::ProjectId,
};

use crate::{config::Config, error::Error};

#[derive(StructOpt)]
pub struct Fetch {
    /// The project to fetch
    project: ProjectId,

    #[structopt(long, default_value = "5")]
    max_retries: usize,
}

impl Fetch {
    pub fn run<K, P>(self, cfg: Config<K, P>) -> Result<(), Error<P::Error>>
    where
        K: Storage<P>,
        P: Pinentry,
        P::Error: Fail,
    {
        let key = cfg.keystore.get_device_key()?;
        let worker = p2p::Worker::new(key, None, Default::default()).unwrap();
        let service = worker.service().clone();

        info!("Joining the network");
        task::spawn(worker);

        info!("Finding peers providing project {}", self.project);
        let providers = get_providers(service.clone(), &self.project, self.max_retries)?;

        info!("Found {} providers", providers.len());
        for provider in providers {
            info!("Finding git port of {}", provider.peer);
            let git_port: Result<Option<u16>, Error<P::Error>> =
                get_peer_info(service.clone(), &provider.peer, self.max_retries).map(|info| {
                    info.capabilities.iter().find_map(|cap| match cap {
                        p2p::Capability::GitDaemon { port } => Some(*port),
                        _ => None,
                    })
                });

            if let Ok(Some(git_port)) = git_port {
                for addr in provider.addrs {
                    let gitaddr = addr.replace(1, |_| Some(Protocol::Tcp(git_port))).unwrap();
                    match tcp::multiaddr_to_socketaddr(&gitaddr) {
                        Ok(saddr) => {
                            info!(
                                "Trying to clone {} from {} at {}",
                                self.project, provider.peer, saddr
                            );
                            if git2::Repository::clone(
                                &format!("git://{}/{}", &saddr, &self.project),
                                Path::new(&format!("/tmp/{}", self.project)),
                            )
                            .is_ok()
                            {
                                return Ok(());
                            }
                        },
                        Err(e) => {
                            warn!("Could not connect to {} at {}: {}", provider.peer, addr, e)
                        },
                    }
                }
            }
        }

        Ok(())
    }
}

fn get_providers<E: Fail>(
    srv: Arc<p2p::Service>,
    pid: &ProjectId,
    retries: usize,
) -> Result<Vec<p2p::Provider>, Error<E>> {
    retry(with_backoff().take(retries), || {
        task::block_on(srv.providers(pid)).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "Providers query cancelled",
            ))
        })
    })
    .map_err(map_retry_error)
}

fn get_peer_info<E: Fail>(
    srv: Arc<p2p::Service>,
    peer: &PeerId,
    retries: usize,
) -> Result<p2p::PeerInfo, Error<E>> {
    retry(with_backoff().take(retries), || {
        task::block_on(srv.peer_info(peer)).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "Capabilities query cancelled",
            ))
        })
    })
    .map_err(map_retry_error)
}

fn with_backoff() -> impl Iterator<Item = Duration> {
    delay::Exponential::from_millis(1000).map(delay::jitter)
}

fn map_retry_error<E: Fail>(err: retry::Error<Error<E>>) -> Error<E> {
    match err {
        retry::Error::Internal(s) => Error::Io(io::Error::new(io::ErrorKind::Other, s)),
        retry::Error::Operation {
            error,
            total_delay,
            tries,
        } => {
            warn!(
                "Gave up after {} attempts and {}sec",
                tries,
                total_delay.as_secs()
            );
            error
        },
    }
}
