// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    hash::{Hash, Hasher as _},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, RwLock, Weak},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{channel::mpsc, stream::StreamExt as _, SinkExt as _};
use madness::{
    dns::{PacketBuilder, RData, ResourceRecord},
    service::{Query, ServiceDiscovery},
    MdnsService,
    Packet,
};
use rustc_hash::FxHashMap;

use super::Discovery;
use crate::peer::PeerId;

const SERVICE_NAME: &str = "_radicle-link._udp.local";

pub struct Mdns {
    _query: ServiceDiscovery,
    _cache: Arc<RwLock<Discoveries<PeerId, u64>>>,
    chan: mpsc::Receiver<(PeerId, Vec<SocketAddr>)>,
}

impl Mdns {
    pub fn new(
        local_peer: PeerId,
        listen_addrs: Vec<SocketAddr>,
        query_interval: Duration,
        cache_ttl: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut srv = MdnsService::new(true)?;
        srv.register(SERVICE_NAME);
        let query = srv.discover(SERVICE_NAME, query_interval);

        let ptr = format!("{}.{}", local_peer, SERVICE_NAME);
        let cname = format!("{}.radicle.local", local_peer);
        let cache = Arc::new(RwLock::new(Discoveries::new(cmp::max(
            cache_ttl,
            query_interval,
        ))));
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(run(
            srv,
            ptr,
            cname,
            listen_addrs,
            Arc::downgrade(&cache),
            tx,
        ));

        Ok(Self {
            _query: query,
            _cache: cache,
            chan: rx,
        })
    }

    #[cfg(test)]
    pub fn is_pending(&mut self) -> bool {
        use futures::Stream as _;

        let waker = futures::task::noop_waker();
        matches!(
            Pin::new(&mut self.chan).poll_next(&mut Context::from_waker(&waker)),
            Poll::Pending
        )
    }
}

#[tracing::instrument(skip(srv, ptr, cname, listen_addrs, cache, tx))]
async fn run(
    mut srv: MdnsService,
    ptr: String,
    cname: String,
    listen_addrs: Vec<SocketAddr>,
    cache: Weak<RwLock<Discoveries<PeerId, u64>>>,
    mut tx: mpsc::Sender<(PeerId, Vec<SocketAddr>)>,
) {
    use std::collections::hash_map::DefaultHasher;

    tracing::info!("starting mDNS service discovery");

    loop {
        let packet = srv.next().await;
        tracing::trace!(packet = ?packet, "received");
        match Weak::upgrade(&cache) {
            None => {
                tracing::info!("stopping the madness");
                return;
            },

            Some(cache) => match packet {
                Packet::Query(queries) => {
                    for query in queries {
                        respond(&ptr, &cname, &listen_addrs, &mut srv, query)
                    }
                },
                Packet::Response(resp) => {
                    for (peer, addrs) in discover(&cname, resp) {
                        let addrs_hash = {
                            let mut hasher = DefaultHasher::new();
                            addrs.hash(&mut hasher);
                            hasher.finish()
                        };
                        let discovered = cache.write().unwrap().insert(peer, addrs_hash);
                        match discovered {
                            Discovered::New((peer, _)) | Discovered::Refreshed((peer, _)) => {
                                tracing::debug!("{} is new or refreshed", peer);
                                if let Err(e) = tx.send((peer, addrs)).await {
                                    tracing::warn!(err = ?e, "channel send error");
                                }
                            },

                            Discovered::Known => {
                                tracing::trace!("ignoring known peer {}", peer);
                            },
                        }
                    }
                },
            },
        }
    }
}

impl futures::stream::Stream for Mdns {
    type Item = (PeerId, Vec<SocketAddr>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.chan.poll_next_unpin(cx)
    }
}

impl Discovery for Mdns {
    type Addr = SocketAddr;
    type Stream = Self;

    fn discover(self) -> Self::Stream {
        self
    }
}

struct Timestamped<T> {
    timestamp: Instant,
    inner: T,
}

impl<T> PartialOrd for Timestamped<T> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.timestamp.cmp(&other.timestamp).reverse())
    }
}

impl<T> Ord for Timestamped<T> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.timestamp.cmp(&other.timestamp).reverse()
    }
}

impl<T> PartialEq for Timestamped<T> {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp.eq(&other.timestamp)
    }
}

impl<T> Eq for Timestamped<T> {}

#[derive(Debug)]
enum Discovered<T> {
    Known,
    New(T),
    Refreshed(T),
}

struct Discoveries<K, V> {
    all: FxHashMap<K, (Instant, V)>,
    hip: BinaryHeap<Timestamped<K>>,
    ttl: Duration,
}

impl<K, V> Discoveries<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    pub fn new(ttl: Duration) -> Self {
        Self {
            all: Default::default(),
            hip: Default::default(),
            ttl,
        }
    }

    pub fn insert(&mut self, k: K, v: V) -> Discovered<(K, V)> {
        use std::collections::hash_map::Entry::*;
        use Discovered::*;

        let now = Instant::now();
        self.evict(now - self.ttl);
        match self.all.entry(k) {
            Occupied(mut entry) => {
                let prev = entry.get_mut();
                if prev.1 != v {
                    *prev = (now, v.clone());
                    Refreshed((entry.key().clone(), v))
                } else {
                    Known
                }
            },

            Vacant(entry) => {
                let key = entry.key().clone();
                entry.insert((now, v.clone()));
                self.hip.push(Timestamped {
                    timestamp: now,
                    inner: key.clone(),
                });
                New((key, v))
            },
        }
    }

    fn evict(&mut self, deadline: Instant) {
        while let Some(Timestamped { timestamp, .. }) = self.hip.peek() {
            if *timestamp > deadline {
                return;
            }

            let key = self.hip.pop().expect("I peeked, therefore I pop").inner;
            let value_timestamp = self.all[&key].0;
            if value_timestamp > deadline {
                self.hip.push(Timestamped {
                    timestamp: value_timestamp,
                    inner: key,
                });
            } else {
                self.all.remove(&key);
            }
        }
    }
}

fn respond(
    ptr: &str,
    cname: &str,
    listen_addrs: &[SocketAddr],
    srv: &mut MdnsService,
    query: Query,
) {
    use std::net::SocketAddr::*;

    if query.is_meta_service_query() {
        let mut packet = PacketBuilder::new();
        packet.header_mut().set_id(rand::random()).set_query(false);
        packet.add_answer(ResourceRecord::IN(
            madness::META_QUERY_SERVICE,
            RData::ptr(SERVICE_NAME),
        ));
        let packet = packet.build();
        srv.enqueue_response(packet);
    } else if query.name.as_str() == SERVICE_NAME {
        let mut packet = PacketBuilder::new();
        packet.add_answer(ResourceRecord::IN(SERVICE_NAME, RData::ptr(&ptr)));

        for addr in listen_addrs {
            packet.add_answer(ResourceRecord::IN(
                &ptr,
                RData::srv(addr.port(), 0, 0, &cname),
            ));
            match addr {
                V4(ip4) => {
                    packet.add_answer(
                        ResourceRecord::IN(&cname, RData::a(*ip4.ip()))
                            .set_ttl(Duration::from_secs(300)),
                    );
                },
                V6(ip6) => {
                    packet.add_answer(
                        ResourceRecord::IN(&cname, RData::aaaa(*ip6.ip()))
                            .set_ttl(Duration::from_secs(300)),
                    );
                },
            }
        }
        packet.header_mut().set_id(rand::random()).set_query(false);
        let packet = packet.build();
        srv.enqueue_response(packet);
    }
}

fn discover(cname: &str, resp: mdns::Response) -> impl Iterator<Item = (PeerId, Vec<SocketAddr>)> {
    use mdns::{Record, RecordKind::*};

    let mut ports = BTreeMap::<PeerId, BTreeSet<u16>>::new();
    let mut addrs = BTreeMap::<PeerId, BTreeSet<IpAddr>>::new();
    let mut add_addr = |peer, addr| {
        addrs
            .entry(peer)
            .and_modify(|addrs| {
                addrs.insert(addr);
            })
            .or_insert_with(|| vec![addr].into_iter().collect());
    };

    fn peer_from_domain(dom: &str) -> Option<PeerId> {
        dom.split('.').next().and_then(|first| first.parse().ok())
    }

    for Record { name, kind, .. } in resp.records() {
        match kind {
            SRV { target, port, .. } if target != cname && name.ends_with(SERVICE_NAME) => {
                if let Some(peer) = peer_from_domain(target) {
                    ports
                        .entry(peer)
                        .and_modify(|ports| {
                            ports.insert(*port);
                        })
                        .or_insert_with(|| vec![*port].into_iter().collect());
                }
            },
            A(ip4) if name != cname => {
                if let Some(peer) = peer_from_domain(name) {
                    add_addr(peer, IpAddr::from(*ip4));
                }
            },
            AAAA(ip6) if name != cname => {
                if let Some(peer) = peer_from_domain(name) {
                    add_addr(peer, IpAddr::from(*ip6));
                }
            },

            _ => (),
        }
    }

    addrs.into_iter().filter_map(move |(peer, addrs)| {
        ports.get(&peer).map(|ports| {
            let addrs = addrs
                .iter()
                .flat_map(|addr| ports.iter().map(move |port| SocketAddr::new(*addr, *port)))
                .collect();
            (peer, addrs)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, net::Ipv4Addr};

    use crate::{keys::SecretKey, peer::PeerId};

    // Watch out for MADNESS:
    //
    // * `madness` requires the tokio runtime (hence the tokio::test)
    // * CI (actually: docker) doesn't support multicast UDP (hence the ignore)
    // * `madness` doesn't support specifying the multicast address or port, so
    //     * the test must run in an isolated environment
    //     * we cannot have more than one test function running in parallel
    #[tokio::test]
    #[ignore]
    async fn smoke() {
        librad_test::logging::init();

        let lolek_id = PeerId::from(SecretKey::new());
        let lolek_addrs = vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            666,
        )];
        let bolek_id = PeerId::from(SecretKey::new());
        let bolek_addrs = vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            777,
        )];
        let tola_id = PeerId::from(SecretKey::new());
        let tola_addrs = vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            888,
        )];

        let interval = Duration::from_millis(500);
        let ttl = Duration::from_secs(60);

        let mut lolek = Mdns::new(lolek_id, lolek_addrs.clone(), interval, ttl).unwrap();
        let mut bolek = Mdns::new(bolek_id, bolek_addrs.clone(), interval, ttl).unwrap();

        // assert that lolek and bolek discover each other, and nobody else
        let (lolek_disco, bolek_disco) = futures::join!(lolek.next(), bolek.next());
        assert_eq!(lolek_disco, Some((bolek_id, bolek_addrs.clone())));
        assert_eq!(bolek_disco, Some((lolek_id, lolek_addrs.clone())));
        assert!(bolek.is_pending());

        // assert that a new peer is discovered, while the previous one (still
        // running) is detected as already seen
        let _tola = Mdns::new(tola_id, tola_addrs.clone(), interval, ttl).unwrap();
        let bolek_disco = bolek.next().await;
        assert_eq!(
            bolek_disco,
            Some((tola_id, tola_addrs.clone())),
            "lolek should be cached"
        );
        assert!(bolek.is_pending());

        // assert that, when reset, bolek will see the survivors
        let mut bolek = Mdns::new(bolek_id, bolek_addrs.clone(), interval, ttl).unwrap();
        let mut bolek_disco = HashSet::new();
        for _ in 0..2 {
            bolek_disco.insert(bolek.next().await.unwrap());
        }
        assert_eq!(
            bolek_disco,
            [
                (lolek_id, lolek_addrs.clone()),
                (tola_id, tola_addrs.clone())
            ]
            .iter()
            .cloned()
            .collect(),
            "cache should have been reset"
        );
        assert!(bolek.is_pending());

        // assert that the cache ttl is observed by effectively disabling it
        let mut bolek = Mdns::new(bolek_id, bolek_addrs.clone(), interval, interval).unwrap();
        for _ in 0..2 {
            let mut bolek_disco = HashSet::new();
            for _ in 0..2 {
                bolek_disco.insert(bolek.next().await.unwrap());
            }
            assert_eq!(
                bolek_disco,
                [
                    (lolek_id, lolek_addrs.clone()),
                    (tola_id, tola_addrs.clone())
                ]
                .iter()
                .cloned()
                .collect(),
            );
            assert!(bolek.is_pending())
        }

        // assert that advertising new addresses will re-discover that peer
        let _lolek = Mdns::new(lolek_id, tola_addrs.clone(), interval, ttl).unwrap();
        let bolek_disco = bolek.next().await;
        assert_eq!(
            bolek_disco,
            Some((lolek_id, tola_addrs.clone())),
            "lolek should've been determined refreshed"
        );
        assert!(bolek.is_pending());
    }
}
