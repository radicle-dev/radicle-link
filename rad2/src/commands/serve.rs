use async_std::task;
use failure::Fail;
use log::info;

use librad::{
    keys::storage::{FileStorage, Pinentry, Storage},
    net::p2p,
    paths::Paths,
    project::Project,
};

use crate::error::Error;

pub fn run<F, P>(paths: Paths, pin: F) -> Result<(), Error<P::Error>>
where
    F: FnOnce(&'static str) -> P,
    P: Pinentry,
    P::Error: Fail,
{
    let key = FileStorage::new(paths.clone()).get_device_key(pin("Unlock your key store:"))?;
    let worker = p2p::Worker::new(key, None).unwrap();
    let service = worker.service().clone();

    Project::list(&paths).for_each(|pid| {
        info!("Serving project {}", pid);
        service.have(pid)
    });

    task::block_on(worker).map_err(|e| e.into())
}
