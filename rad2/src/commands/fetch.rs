use std::{io, sync::Arc, thread, time::Duration};

use async_std::task;
use failure::Fail;
use log::info;
use structopt::StructOpt;

use librad::{
    keys::storage::{FileStorage, Pinentry, Storage},
    net::p2p,
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

    let pid = opts.project.clone();
    let mut providers;
    let mut attempts = 0;
    loop {
        attempts += 1;
        providers = get_providers(service.clone(), pid.clone())?;
        if !providers.is_empty() {
            break;
        } else if attempts < opts.max_retries {
            info!(
                "No providers found, retrying ({}/{})",
                attempts, opts.max_retries
            );
            thread::sleep(Duration::from_secs(1));
            continue;
        } else {
            break;
        }
    }

    providers
        .iter()
        .for_each(|p| println!("Found provider for {}: {:#?}", opts.project, p));

    Ok(())
}

fn get_providers<E: Fail>(
    srv: Arc<p2p::Service>,
    pid: ProjectId,
) -> Result<Vec<p2p::Provider>, Error<E>> {
    task::block_on(async { srv.providers(pid).await }).map_err(|_| {
        Error::Io(io::Error::new(
            io::ErrorKind::Other,
            "Providers query cancelled",
        ))
    })
}
