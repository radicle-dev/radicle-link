use std::error::Error;

use structopt::StructOpt;

use librad::keys::device;
use librad::p2p::swarm;
use librad::paths::Paths;

#[derive(StructOpt)]
pub struct Options {}

pub fn run(_paths: Paths, _opts: Options, key: device::Key) -> Result<(), Box<dyn Error>> {
    swarm::join(key, None)
}
