use std::{net, path::PathBuf};

use tracing_subscriber::FmtSubscriber;

use librad::{peer::PeerId, uri::RadUrn};
use radicle_seed::{Mode, Node, NodeConfig, Signer};

use argh::FromArgs;

#[derive(FromArgs)]
/// Radicle Seed.
pub struct Options {
    /// track the specified peers only
    #[argh(option)]
    pub track_peers: Vec<PeerId>,

    /// track the specified URNs only
    #[argh(option)]
    pub track_urns: Vec<RadUrn>,

    /// listen on the following address for peer connections
    #[argh(option)]
    pub listen: Option<net::SocketAddr>,

    /// log level (default: info)
    #[argh(option, default = "tracing::Level::INFO")]
    pub log: tracing::Level,

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
    let subscriber = FmtSubscriber::builder().with_max_level(opts.log).finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting tracing subscriber should succeed");

    let signer = match Signer::new(std::io::stdin()) {
        Ok(signer) => signer,
        Err(err) => panic!("invalid key was supplied to stdin: {}", err),
    };

    let config = NodeConfig {
        listen_addr: opts.listen.unwrap_or(NodeConfig::default().listen_addr),
        root: opts.root,
        mode: if !opts.track_peers.is_empty() {
            Mode::TrackPeers(opts.track_peers.into_iter().collect())
        } else if !opts.track_urns.is_empty() {
            Mode::TrackUrns(opts.track_urns.into_iter().collect())
        } else {
            Mode::TrackEverything
        },
        signer,
    };
    let node = Node::new(config).unwrap();

    node.run().await.unwrap();
}