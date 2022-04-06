// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashMap},
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    pin::Pin,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use futures::{
    future,
    stream::{BoxStream, StreamExt as _, TryStreamExt as _},
};
use if_watch::IfWatcher;
use link_async::Spawner;
use nonempty::NonEmpty;
use parking_lot::RwLock;
use quinn::{NewConnection, TransportConfig};
use socket2::{Domain, Protocol, Socket, Type};

use super::{BoxedIncomingStreams, Connection, Conntrack, Error, Result};
use crate::{
    net::{
        connection::{CloseReason, LocalAddr, LocalPeer},
        tls,
        x509,
        Network,
        PROTOCOL_VERSION,
    },
    PeerId,
    Signer,
};

pub type IncomingConnections<'a> = BoxStream<'a, Result<(Connection, BoxedIncomingStreams<'a>)>>;

pub struct BoundEndpoint<'a, const R: usize> {
    pub endpoint: Endpoint<R>,
    pub incoming: IncomingConnections<'a>,
}

impl<'a, const R: usize> LocalPeer for BoundEndpoint<'a, R> {
    fn local_peer_id(&self) -> PeerId {
        self.endpoint.local_peer_id()
    }
}

impl<'a, const R: usize> LocalAddr for BoundEndpoint<'a, R> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.endpoint.listen_addrs()
    }
}

pub enum Ingress<'a> {
    /// The [`Connection`] obtained from calling
    /// [`ConnectPeer::connect`]. It is implied that the
    /// [`BoxedIncomingStreams`] for this connection are dispatched
    /// elsewhere for handling.
    Remote(Connection),
    /// The [`Connection`] and [`BoxedIncomingStreams`] obtained from
    /// calling [`ConnectPeer::connect`]. This is intended to be
    /// handed back directly from an endpoint and the streams are
    /// handled locally.
    Local {
        conn: Connection,
        streams: BoxedIncomingStreams<'a>,
    },
}

impl<'a> Ingress<'a> {
    pub fn connection(&self) -> &Connection {
        match &self {
            Ingress::Remote(conn) => conn,
            Ingress::Local { conn, .. } => conn,
        }
    }
}

/// Attempt to connect to a remote peer's address, giving back an
/// [`Ingress`], which is guaranteed to have a [`Connection`], but may
/// also contain [`BoxedIncomingStreams`].
#[async_trait]
pub trait ConnectPeer
where
    Self: Clone,
{
    async fn connect<'a, Addrs>(&self, peer: PeerId, addrs: Addrs) -> Option<Ingress<'a>>
    where
        Addrs: IntoIterator<Item = SocketAddr> + Send,
        Addrs::IntoIter: Send;
}

/// A QUIC endpoint.
///
/// `R` is the number of reservations for outgoing unidirectional streams, see
/// [`Connection::borrow_uni`].
#[derive(Clone)]
pub struct Endpoint<const R: usize> {
    peer_id: PeerId,
    endpoint: quinn::Endpoint,
    listen_addrs: Arc<RwLock<BTreeSet<SocketAddr>>>,
    conntrack: Conntrack,
    _refcount: Arc<()>,
}

impl<const R: usize> Endpoint<R> {
    pub async fn bind<'a, S>(
        signer: S,
        spawner: &Spawner,
        listen_addr: SocketAddr,
        advertised_addrs: Option<NonEmpty<SocketAddr>>,
        network: Network,
    ) -> Result<BoundEndpoint<'a, R>>
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
                    ifwatch(spawner, listen_addr, Arc::downgrade(&listen_addrs)).await?
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
            _refcount: Arc::new(()),
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
                    let (conn, streams) =
                        Connection::new(Some(conntrack.clone()), R, remote_peer, conn);
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

    pub fn connected_peers(&self) -> HashMap<PeerId, Vec<SocketAddr>> {
        self.conntrack.connected_peers()
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
        let (conn, streams) = Connection::new(Some(self.conntrack.clone()), R, peer, conn);
        self.conntrack.connected(&conn);

        Ok((conn, streams.boxed()))
    }

    pub fn get_connection(&self, to: PeerId) -> Option<Connection> {
        self.conntrack.get(to)
    }

    pub fn disconnect(&self, peer: &PeerId) {
        self.conntrack.disconnect_peer(peer)
    }

    pub fn close(&self) {
        tracing::debug!(
            connections = self.conntrack.total(),
            "endpoint shutdown requested"
        );
        let reason = CloseReason::ServerShutdown;
        self.endpoint
            .close((reason as u32).into(), reason.reason_phrase());
        self.conntrack.disconnect_all();
    }

    pub async fn wait_idle(&self) {
        self.endpoint.wait_idle().await
    }
}

impl<const R: usize> LocalPeer for Endpoint<R> {
    fn local_peer_id(&self) -> PeerId {
        self.peer_id
    }
}

impl<const R: usize> LocalAddr for Endpoint<R> {
    type Addr = SocketAddr;

    fn listen_addrs(&self) -> Vec<SocketAddr> {
        self.listen_addrs()
    }
}

/// An endpoint that can only establish outbound connections that
/// result in two-way communication.
#[derive(Clone)]
pub struct SendOnly {
    peer_id: PeerId,
    endpoint: quinn::Endpoint,
}

impl SendOnly {
    pub async fn new<S>(signer: S, network: Network) -> Result<Self>
    where
        S: Signer + Clone + Send + Sync + 'static,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let peer_id = PeerId::from_signer(&signer);

        let listen_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0));
        let sock = bind_socket(listen_addr)?;
        let endpoint = make_send_only(signer, sock, alpn(network)).await?;
        Ok(Self { peer_id, endpoint })
    }

    pub async fn connect<'a>(
        &self,
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

        let (conn, streams) = Connection::new(None, 2, peer, conn);
        Ok((conn, streams.boxed()))
    }
}

#[async_trait]
impl ConnectPeer for SendOnly {
    #[tracing::instrument(skip(self, addrs))]
    async fn connect<'a, Addrs>(&self, peer: PeerId, addrs: Addrs) -> Option<Ingress<'a>>
    where
        Addrs: IntoIterator<Item = SocketAddr> + Send,
        Addrs::IntoIter: Send,
    {
        if peer == self.peer_id {
            return None;
        }

        future::select_ok(addrs.into_iter().map(|addr| {
            let endpoint = self.clone();
            tracing::info!(remote_addr = %addr, "establishing connection");
            Box::pin(async move {
                Self::connect(&endpoint, peer, &addr)
                    .await
                    .map_err(|e| {
                        tracing::warn!(err = ?e, remote_addr = %addr, "could not connect");
                        e
                    })
                    .map(|(conn, streams)| Ingress::Local { conn, streams })
            })
        }))
        .await
        .ok()
        .map(|(success, _pending)| success)
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

#[tracing::instrument(skip(spawner, listen_addrs))]
async fn ifwatch(
    spawner: &Spawner,
    bound_addr: SocketAddr,
    listen_addrs: Weak<RwLock<BTreeSet<SocketAddr>>>,
) -> io::Result<()> {
    use if_watch::{IfEvent::*, IpNet};

    fn same_family(a: &SocketAddr, b: &IpNet) -> bool {
        a.is_ipv4() && b.addr().is_ipv4() || a.is_ipv6() && b.addr().is_ipv6()
    }

    let mut watcher = IfWatcher::new().await?;
    spawner
        .spawn(async move {
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
        })
        .detach();

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

async fn make_send_only<S>(signer: S, sock: UdpSocket, alpn: Alpn) -> Result<quinn::Endpoint>
where
    S: Signer + Clone + Send + Sync + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let mut builder = quinn::Endpoint::builder();
    builder.default_client_config(make_client_config(signer, alpn)?);

    Ok(builder.with_socket(sock)?.0)
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
