use std::{net, path::PathBuf};

use librad::peer::PeerId;
use radicle_seed::{Mode, Node, NodeConfig};

use argh::FromArgs;
use futures::executor;
use log;

#[derive(FromArgs)]
/// Radicle Seed.
pub struct Options {
    /// track the specified peers only
    #[argh(option)]
    pub track_peers: Vec<PeerId>,

    /// listen on the following address for peer connections
    #[argh(option)]
    pub listen: Option<net::SocketAddr>,

    /// log level (default: info)
    #[argh(option, default = "log::Level::Info")]
    pub log: log::Level,

    /// radicle root path, for key and git storage
    #[argh(option)]
    pub root: Option<PathBuf>,
}

impl Options {
    pub fn from_env() -> Self {
        argh::from_env()
    }
}

#[tokio::main]
async fn main() {
    let opts = Options::from_env();
    let default = NodeConfig::default();
    let config = NodeConfig {
        listen_addr: opts.listen.unwrap_or(default.listen_addr),
        root: opts.root,
        mode: if opts.track_peers.is_empty() {
            Mode::TrackEverything
        } else {
            Mode::TrackPeers(opts.track_peers)
        },
    };
    let node = Node::new(config).unwrap();

    executor::block_on(node.run()).unwrap();
}
