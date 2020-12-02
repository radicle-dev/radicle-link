// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::mpsc, stream::StreamExt as _, SinkExt as _};
use madness::{
    dns::{PacketBuilder, RData, ResourceRecord},
    service::{Query, ServiceDiscovery},
    MdnsService,
    Packet,
};

use super::Discovery;
use crate::peer::PeerId;

const SERVICE_NAME: &str = "_radicle-link._udp.local";

pub struct Mdns {
    _query: ServiceDiscovery,
    chan: mpsc::Receiver<(PeerId, Vec<SocketAddr>)>,
}

impl Mdns {
    pub fn new(
        local_peer: PeerId,
        listen_addrs: Vec<SocketAddr>,
        query_interval: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut srv = MdnsService::new(true)?;
        srv.register(SERVICE_NAME);
        let query = srv.discover(SERVICE_NAME, query_interval);

        let ptr = format!("{}.{}", local_peer, SERVICE_NAME);
        let cname = format!("{}.radicle.local", local_peer);

        let (mut tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            tracing::info!("starting mDNS service discovery");
            loop {
                let packet = srv.next().await;
                match packet {
                    Packet::Query(queries) => {
                        for query in queries {
                            respond(&ptr, &cname, &listen_addrs, &mut srv, query)
                        }
                    },
                    Packet::Response(resp) => {
                        for info in discover(&cname, resp) {
                            if tx.send(info).await.is_err() {
                                tracing::info!("stopping the madness");
                                return;
                            }
                        }
                    },
                }
            }
        });

        Ok(Self {
            _query: query,
            chan: rx,
        })
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
    use std::net::Ipv4Addr;

    use crate::{keys::SecretKey, peer::PeerId};

    // `madness` uses tokio, so no choice here -- MADNESS!
    #[tokio::test]
    // CI doesn't support multicast UDP
    #[ignore]
    async fn ohai() {
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

        let interval = Duration::from_secs(10);

        let mut lolek = Mdns::new(lolek_id, lolek_addrs.clone(), interval).unwrap();
        let mut bolek = Mdns::new(bolek_id, bolek_addrs.clone(), interval).unwrap();

        let (lolek_disco, bolek_disco) = futures::join!(lolek.next(), bolek.next());
        assert_eq!(lolek_disco, Some((bolek_id, bolek_addrs)));
        assert_eq!(bolek_disco, Some((lolek_id, lolek_addrs)));
    }
}
