use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use futures::StreamExt as _;
use lazy_static::lazy_static;

use librad::{
    git,
    keys::SecretKey,
    net::{discovery, peer::PeerConfig},
    paths::Paths,
};
use librad_test::rad::testnet;

use seed::{Mode, Node, Signer};

lazy_static! {
    static ref LOCALHOST_ANY: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0));
}

#[tokio::test]
async fn replicates_between_async() -> Result<(), Box<dyn std::error::Error>> {
    let seed_tmp = tempfile::tempdir()?;
    let seed_listen_addr = *LOCALHOST_ANY;

    let (seed_peer_id, seed_events) = {
        let paths = Paths::from_root(seed_tmp.path())?;
        let key = SecretKey::new();
        let gossip_params = Default::default();
        let disco = discovery::Static::new(vec![]);
        let storage_config = Default::default();

        git::storage::Storage::init(&paths, key)?;

        let config = PeerConfig {
            signer: Signer::from(key),
            paths,
            listen_addr: seed_listen_addr,
            gossip_params,
            disco,
            storage_config,
        };
        let peer = config.try_into_peer().await?;

        let (api, run_loop) = peer.accept()?;
        let peer_id = api.peer_id().clone();
        let events = api.protocol().subscribe().await;
        let events = events.peekable();

        tokio::spawn(run_loop);
        tokio::spawn(Node::event_loop(api, &Mode::TrackEverything));

        (peer_id, events)
    };

    {
        let tmp = tempfile::tempdir()?;
        let paths = Paths::from_root(tmp.path())?;
        let key = SecretKey::new();
        let gossip_params = Default::default();
        let disco = discovery::Static::new(vec![(seed_peer_id, seed_listen_addr)]);
        let storage_config = Default::default();

        git::storage::Storage::init(&paths, key)?;

        let config = PeerConfig {
            signer: Signer::from(key),
            paths,
            listen_addr: seed_listen_addr,
            gossip_params,
            disco,
            storage_config,
        };
        let peer = config.try_into_peer().await?;

        let (api, run_loop) = peer.accept()?;

        // wait connected

        // create project
        // push commit
        // wait for seed to have cloned
    }

    {
        // Set up peer2
        // clone project from seed
        // profit
    }

    Ok(())
}
