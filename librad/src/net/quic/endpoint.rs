// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::BTreeSet,
    io,
    net::{SocketAddr, UdpSocket},
    pin::Pin,
    sync::{Arc, Weak},
};

use futures::stream::{BoxStream, StreamExt as _, TryStreamExt as _};
use if_watch::IfWatcher;
use nonempty::NonEmpty;
use parking_lot::RwLock;
use quinn::{NewConnection, TransportConfig};
use socket2::{Domain, Protocol, Socket, Type};
use tracing::Instrument as _;

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

    fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.endpoint.listen_addrs()
    }
}

#[derive(Clone)]
pub struct Endpoint {
    peer_id: PeerId,
    endpoint: quinn::Endpoint,
    listen_addrs: Arc<RwLock<BTreeSet<SocketAddr>>>,
    conntrack: Conntrack,
    refcount: Arc<()>,
}

impl Endpoint {
    pub async fn bind<'a, S>(
        signer: S,
        listen_addr: SocketAddr,
        advertised_addrs: Option<NonEmpty<SocketAddr>>,
        network: Network,
    ) -> Result<BoundEndpoint<'a>>
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let peer_id = PeerId::from_signer(&signer);

        let sock = bind_socket(listen_addr)?;
        let listen_addr = sock.local_addr()?;
        let addrs = {
            let listen_addrs = Arc::new(RwLock::new(BTreeSet::new()));
            match advertised_addrs {
                Some(addrs) => listen_addrs.write().extend(addrs),
                None if listen_addr.ip().is_unspecified() => {
                    ifwatch(listen_addr, Arc::downgrade(&listen_addrs)).await?
                },
                None => listen_addrs.write().extend(Some(listen_addr)),
            }
            listen_addrs
        };

        let (endpoint, incoming) = make_endpoint(signer, sock, alpn(network)).await?;
        let conntrack = Conntrack::new();
        let endpoint = Endpoint {
            peer_id,
            endpoint,
            listen_addrs: addrs,
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

    pub fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.listen_addrs.read().iter().copied().collect()
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

    fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.listen_addrs()
    }
}

// TODO: tune buffer sizes
fn bind_socket(listen_addr: SocketAddr) -> Result<UdpSocket> {
    let sock = Socket::new(
        Domain::for_address(listen_addr),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;
    if listen_addr.is_ipv6() {
        sock.set_only_v6(false)?;
    }
    sock.bind(&socket2::SockAddr::from(listen_addr))?;
    Ok(sock.into())
}

#[tracing::instrument(skip(listen_addrs))]
async fn ifwatch(
    bound_addr: SocketAddr,
    listen_addrs: Weak<RwLock<BTreeSet<SocketAddr>>>,
) -> io::Result<()> {
    use if_watch::{IfEvent::*, IpNet};

    fn same_family(a: &SocketAddr, b: &IpNet) -> bool {
        a.is_ipv4() && b.addr().is_ipv4() || a.is_ipv6() && b.addr().is_ipv6()
    }

    let mut watcher = IfWatcher::new().await?;
    tokio::spawn(
        async move {
            loop {
                match Pin::new(&mut watcher).await {
                    Err(e) => {
                        tracing::warn!(err = ?e, "ifwatcher error");
                        match listen_addrs.upgrade() {
                            None => {
                                tracing::info!("endpoint lost");
                                break;
                            },
                            Some(addrs) => addrs.write().clear(),
                        }
                    },
                    Ok(evt) => match listen_addrs.upgrade() {
                        None => {
                            tracing::info!("endpoint lost");
                            break;
                        },
                        Some(addrs) => match evt {
                            Up(net) => {
                                tracing::debug!("if up {}", net);
                                let new_addr = if same_family(&bound_addr, &net) {
                                    Some(SocketAddr::new(net.addr(), bound_addr.port()))
                                } else {
                                    None
                                };

                                if let Some(addr) = new_addr {
                                    tracing::info!("adding listen addr {}", addr);
                                    addrs.write().insert(addr);
                                }
                            },
                            Down(net) => {
                                tracing::debug!("if down {}", net);
                                if same_family(&bound_addr, &net) {
                                    let addr = SocketAddr::new(net.addr(), bound_addr.port());
                                    tracing::info!("removing listen addr {}", addr);
                                    addrs.write().remove(&addr);
                                }
                            },
                        },
                    },
                }
            }
        }
        .in_current_span(),
    );

    Ok(())
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
            alpn.extend(id.as_ref());
            alpn
        },
    }
}

async fn make_endpoint<S>(
    signer: S,
    sock: UdpSocket,
    alpn: Alpn,
) -> Result<(quinn::Endpoint, quinn::Incoming)>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer.clone(), alpn.clone())?);
    builder.listen(make_server_config(signer, alpn)?);

    Ok(builder.with_socket(sock)?)
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
    quic_config
        // https://tools.ietf.org/html/draft-ietf-quic-transport-11#section-6.5
        .use_stateless_retry(true)
        // below are quinn defaults
        .retry_token_lifetime(15_000_000) // microseconds
        .migration(true)
        .concurrent_connections(100_000);

    Ok(quic_config)
}
