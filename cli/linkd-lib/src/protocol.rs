// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, panic, time::Duration};

use futures::{future::FutureExt as _, pin_mut, select};
use tokio::{sync::mpsc, time::sleep};
use tracing::{error, info, instrument};

use librad::{
    net::{self, discovery::Discovery, peer::Peer, protocol::RequestPullGuard},
    Signer,
};

#[instrument(name = "protocol subroutine", skip(disco, peer, shutdown_rx))]
pub async fn routine<D, S, G>(
    peer: Peer<S, G>,
    disco: D,
    mut shutdown_rx: mpsc::Receiver<()>,
) -> anyhow::Result<()>
where
    D: Discovery<Addr = SocketAddr> + Clone + 'static,
    S: Signer + Clone,
    G: RequestPullGuard,
{
    let shutdown = shutdown_rx.recv().fuse();
    futures::pin_mut!(shutdown);

    loop {
        match peer.bind().await {
            Ok(bound) => {
                let (stop, run) = bound.accept(disco.clone().discover());
                let run = run.fuse();
                pin_mut!(run);

                let res = select! {
                    _ = shutdown => {
                        stop();
                        run.await
                    }
                    res = run => res
                };

                match res {
                    Err(net::protocol::io::error::Accept::Done) => {
                        info!("network endpoint shut down");
                        break;
                    },
                    Err(err) => {
                        error!(?err, "accept error");
                    },
                    Ok(never) => unreachable!("absurd: {}", never),
                }
            },
            Err(err) => {
                error!(?err, "bind error");

                let sleep = sleep(Duration::from_secs(2)).fuse();
                pin_mut!(sleep);
                select! {
                    _ = sleep => {},
                    _ = shutdown => {
                        break;
                    }
                }
            },
        }
    }

    Ok(())
}
