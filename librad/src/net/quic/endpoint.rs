// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use futures::stream::{BoxStream, StreamExt as _, TryStreamExt as _};
use pnet_datalink::interfaces as network_interfaces;
use quinn::{NewConnection, TransportConfig};

use super::{BoxedIncomingStreams, Connection, Conntrack, Error, Result};
use crate::{
    net::{
        connection::{CloseReason, LocalAddr, LocalPeer},
        tls,
        x509,
        Network,
        PROTOCOL_VERSION,
    },
    signer::Signer,
    PeerId,
};

pub type IncomingConnections<'a> = BoxStream<'a, Result<(Connection, BoxedIncomingStreams<'a>)>>;

pub struct BoundEndpoint<'a> {
    pub endpoint: Endpoint,
    pub incoming: IncomingConnections<'a>,
}

impl<'a> LocalPeer for BoundEndpoint<'a> {
    fn local_peer_id(&self) -> PeerId {
        self.endpoint.local_peer_id()
    }
}

impl<'a> LocalAddr for BoundEndpoint<'a> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> io::Result<Vec<SocketAddr>> {
        self.endpoint.listen_addrs()
    }
}

#[derive(Clone)]
pub struct Endpoint {
    peer_id: PeerId,
    endpoint: quinn::Endpoint,
    conntrack: Conntrack,
    refcount: Arc<()>,
}

impl Endpoint {
    pub async fn bind<'a, S>(
        signer: S,
        listen_addr: SocketAddr,
        network: Network,
    ) -> Result<BoundEndpoint<'a>>
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let peer_id = PeerId::from_signer(&signer);
        let (endpoint, incoming) = make_endpoint(signer, listen_addr, alpn(network)).await?;
        let conntrack = Conntrack::new();
        let endpoint = Endpoint {
            peer_id,
            endpoint,
            conntrack: conntrack.clone(),
            refcount: Arc::new(()),
        };
        let incoming = incoming
            .map(Ok)
            .and_then(move |connecting| {
                let conntrack = conntrack.clone();
                async move {
                    let conn = connecting.await?;
                    let remote_peer = remote_peer(&conn)?;
                    debug_assert!(
                        remote_peer != peer_id,
                        "self-connections are prevented in the TLS handshake"
                    );
                    let (conn, streams) = Connection::new(remote_peer, conntrack.clone(), conn);
                    conntrack.connected(&conn);

                    Ok((conn, streams.boxed()))
                }
            })
            .boxed();

        Ok(BoundEndpoint { endpoint, incoming })
    }

    pub fn listen_addrs(&self) -> io::Result<Vec<SocketAddr>> {
        // FIXME: can this really fail?
        let local_addr = self.endpoint.local_addr()?;
        let local_ip = local_addr.ip();

        fn same_family(a: &IpAddr, b: &IpAddr) -> bool {
            matches!((a, b), (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)))
        }

        let addrs = if local_ip.is_unspecified() {
            network_interfaces()
                .iter()
                .filter(|iface| {
                    iface.is_up()
                        && iface
                            .ips
                            .iter()
                            .any(|net| same_family(&net.ip(), &local_ip))
                })
                .flat_map(|iface| {
                    iface
                        .ips
                        .iter()
                        .map(|net| SocketAddr::new(net.ip(), local_addr.port()))
                })
                .collect()
        } else {
            vec![local_addr]
        };

        Ok(addrs)
    }

    pub fn connections_total(&self) -> usize {
        self.conntrack.total()
    }

    pub fn connected_peers(&self) -> usize {
        self.conntrack.num_peers()
    }

    pub fn peers(&self) -> Vec<PeerId> {
        self.conntrack.peers()
    }

    pub async fn connect<'a>(
        &mut self,
        peer: PeerId,
        addr: &SocketAddr,
    ) -> Result<(Connection, BoxedIncomingStreams<'a>)> {
        if peer == self.peer_id {
            return Err(Error::SelfConnect);
        }

        let conn = self
            .endpoint
            .connect(addr, peer.as_dns_name().as_ref().into())?
            .await?;
        let (conn, streams) = Connection::new(peer, self.conntrack.clone(), conn);
        self.conntrack.connected(&conn);

        Ok((conn, streams.boxed()))
    }

    pub fn get_connection(&self, to: PeerId) -> Option<Connection> {
        self.conntrack
            .get(to)
            .map(|conn| Connection::existing(to, self.conntrack.clone(), conn))
    }

    pub fn disconnect(&self, peer: &PeerId) {
        self.conntrack.disconnect_peer(peer)
    }

    // TODO: provide a graceful shutdown using wait_idle with a timeout
    pub fn shutdown(&self) {
        tracing::warn!(
            connections = self.conntrack.total(),
            "endpoint shutdown requested"
        );
        let reason = CloseReason::ServerShutdown;
        self.endpoint
            .close((reason as u32).into(), reason.reason_phrase());
        self.conntrack.disconnect_all()
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if Arc::strong_count(&self.refcount) == 1 {
            self.shutdown()
        }
    }
}

impl LocalPeer for Endpoint {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id
    }
}

impl LocalAddr for Endpoint {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> io::Result<Vec<SocketAddr>> {
        self.listen_addrs()
    }
}

/// Try to extract the remote identity from a newly established connection
fn remote_peer(conn: &NewConnection) -> Result<PeerId> {
    conn.connection
        .peer_identity()
        .map(|certs| {
            let first = certs
                .iter()
                .next()
                .expect("One certificate must have been presented")
                .as_ref();
            x509::Certificate::from_der(first)
                .map(|cert| cert.peer_id())
                .unwrap()
        })
        .ok_or(Error::RemoteIdUnavailable)
}

type Alpn = Vec<u8>;

fn alpn(network: Network) -> Alpn {
    let mut alpn = super::ALPN_PREFIX.to_vec();
    alpn.push(b'/');
    alpn.push(PROTOCOL_VERSION);
    match network {
        Network::Main => alpn,
        Network::Custom(id) => {
            alpn.push(b'/');
            alpn.extend(id);
            alpn
        },
    }
}

async fn make_endpoint<S>(
    signer: S,
    listen_addr: SocketAddr,
    alpn: Alpn,
) -> Result<(quinn::Endpoint, quinn::Incoming)>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer.clone(), alpn.clone())?);
    builder.listen(make_server_config(signer, alpn)?);

    Ok(builder.bind(&listen_addr)?)
}

fn make_client_config<S>(signer: S, alpn: Vec<u8>) -> Result<quinn::ClientConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_client_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = vec![alpn];

    let mut transport_config = TransportConfig::default();
    transport_config
        .keep_alive_interval(Some(super::KEEP_ALIVE_INTERVAL))
        // Set idle timeout anyway, as the default is smaller than our
        // keep-alive
        .max_idle_timeout(Some(super::MAX_IDLE_TIMEOUT))
        .expect("idle timeout is in vetted range");

    let mut quic_config = quinn::ClientConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(transport_config);

    Ok(quic_config)
}

fn make_server_config<S>(signer: S, alpn: Vec<u8>) -> Result<quinn::ServerConfig>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut tls_config = tls::make_server_config(signer).map_err(|e| Error::Signer(Box::new(e)))?;
    tls_config.alpn_protocols = vec![alpn];

    let mut transport_config = TransportConfig::default();
    transport_config
        .max_idle_timeout(Some(super::MAX_IDLE_TIMEOUT))
        .expect("idle timeout is in vetted range");

    let mut quic_config = quinn::ServerConfigBuilder::default().build();
    quic_config.crypto = Arc::new(tls_config);
    quic_config.transport = Arc::new(transport_config);

    Ok(quic_config)
}
