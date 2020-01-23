use std::{io, path::Path, sync::Arc, thread, time::Duration};

use async_std::task;
use failure::Fail;
use log::{info, warn};
use structopt::StructOpt;

use libp2p::multiaddr::Protocol;

use librad::{
    keys::storage::{FileStorage, Pinentry, Storage},
    net::{p2p, tcp},
    paths::Paths,
    project::ProjectId,
};

use crate::error::Error;

#[derive(StructOpt)]
pub struct Options {
    /// The project to fetch
    project: ProjectId,

    #[structopt(long, default_value = "5")]
    max_retries: u8,
}

pub fn run<F, P>(paths: Paths, opts: Options, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let key = FileStorage::new(paths).get_device_key(pin("Unlock your key store:"))?;
    let worker = p2p::Worker::new(key, None).unwrap();
    let service = worker.service().clone();

    info!("Joining the network");
    task::spawn(worker);

    info!("Finding peers providing project {}", opts.project);
    let providers = get_providers(service, &opts.project, opts.max_retries)?;

    for provider in providers {
        for addr in provider.addrs {
            let gitaddr = addr.replace(1, |_| Some(Protocol::Tcp(9418))).unwrap();
            match tcp::multiaddr_to_socketaddr(&gitaddr) {
                Ok(saddr) => {
                    info!(
                        "Trying to clone {} from {} at {}",
                        opts.project, provider.peer, saddr
                    );
                    if git2::Repository::clone(
                        &format!("git://{}/{}", &saddr, &opts.project),
                        Path::new(&format!("/tmp/{}", opts.project)),
                    )
                    .is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(e) => warn!("Could not connect to {} at {}: {}", provider.peer, addr, e),
            }
        }
    }

    Ok(())
}

fn get_providers<E: Fail>(
    srv: Arc<p2p::Service>,
    pid: &ProjectId,
    retries: u8,
) -> Result<Vec<p2p::Provider>, Error<E>> {
    let query = || {
        task::block_on(async { srv.providers(pid).await }).map_err(|_| {
            Error::Io(io::Error::new(
                io::ErrorKind::Other,
                "Providers query cancelled",
            ))
        })
    };

    let mut providers;
    let mut attempts = 0;
    loop {
        attempts += 1;
        providers = query()?;
        if !providers.is_empty() {
            break;
        } else if attempts < retries {
            info!("No providers found, retrying ({}/{})", attempts, retries);
            thread::sleep(Duration::from_secs(1));
            continue;
        } else {
            break;
        }
    }

    Ok(providers)
}
