// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{net::SocketAddr, sync::Arc};

use parking_lot::Mutex;
pub use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast as tincan, mpsc, oneshot::Receiver};

use super::{
    error,
    event::{self, Downstream},
    gossip,
    info::PeerAdvertisement,
    interrogation,
    request_pull,
};
use crate::{git::Urn, identities::xor::Xor, net::quic, PeerId};

pub struct Connected(pub(crate) quic::Connection);

#[derive(Clone)]
pub struct TinCans {
    pub(super) downstream: tincan::Sender<event::Downstream>,
    pub(super) upstream: tincan::Sender<event::Upstream>,
}

impl TinCans {
    pub fn new() -> Self {
        Self {
            downstream: tincan::channel(16).0,
            upstream: tincan::channel(16).0,
        }
    }

    pub fn announce(&self, have: gossip::Payload) -> Result<(), gossip::Payload> {
        use event::downstream::Gossip::Announce;

        self.downstream
            .send(Downstream::Gossip(Announce(have)))
            .and(Ok(()))
            .map_err(|tincan::error::SendError(e)| match e {
                Downstream::Gossip(g) => g.payload(),
                _ => unreachable!(),
            })
    }

    pub fn query(&self, want: gossip::Payload) -> Result<(), gossip::Payload> {
        use event::downstream::Gossip::Query;

        self.downstream
            .send(Downstream::Gossip(Query(want)))
            .and(Ok(()))
            .map_err(|tincan::error::SendError(e)| match e {
                Downstream::Gossip(g) => g.payload(),
                _ => unreachable!(),
            })
    }

    pub async fn connected_peers(&self) -> Vec<PeerId> {
        use event::downstream::Info::*;

        let (tx, rx) = replier();
        if let Err(tincan::error::SendError(e)) =
            self.downstream.send(Downstream::Info(ConnectedPeers(tx)))
        {
            match e {
                Downstream::Info(ConnectedPeers(reply)) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(vec![])
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or_default()
    }

    pub async fn membership(&self) -> event::downstream::MembershipInfo {
        use event::downstream::{Info::*, MembershipInfo};

        let (tx, rx) = replier();
        if let Err(tincan::error::SendError(e)) =
            self.downstream.send(Downstream::Info(Membership(tx)))
        {
            match e {
                Downstream::Info(Membership(reply)) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(MembershipInfo::default())
                        .ok();
                },
                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or_default()
    }

    pub async fn stats(&self) -> event::downstream::Stats {
        use event::downstream::{Info::*, Stats};

        let (tx, rx) = replier();
        if let Err(tincan::error::SendError(e)) = self.downstream.send(Downstream::Info(Stats(tx)))
        {
            match e {
                Downstream::Info(Stats(reply)) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(Stats::default())
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or_default()
    }

    pub fn interrogate(&self, peer: PeerId, conn: quic::Connection) -> Interrogation {
        Interrogation {
            peer,
            conn,
            chan: self.downstream.clone(),
        }
    }

    pub async fn request_pull(&self, urn: Urn, conn: quic::Connection) -> RequestPull {
        let (tx, rx) = multi_replier();
        if let Err(tincan::error::SendError(e)) =
            self.downstream
                .send(Downstream::RequestPull(event::downstream::RequestPull {
                    conn,
                    request: request_pull::Request { urn },
                    reply: tx,
                }))
        {
            match e {
                Downstream::RequestPull(event::downstream::RequestPull { reply, .. }) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(Err(error::RequestPull::Unavailable))
                        .await
                        .ok();
                },
                _ => unreachable!(),
            }
        }

        RequestPull { reply: rx }
    }

    pub async fn connect(&self, peer: impl Into<(PeerId, Vec<SocketAddr>)>) -> Option<Connected> {
        use event::downstream::Connect;

        let (tx, rx) = replier();
        if let Err(tincan::error::SendError(e)) =
            self.downstream.send(Downstream::Connect(Connect {
                peer: peer.into(),
                reply: tx,
            }))
        {
            match e {
                Downstream::Connect(Connect { reply, .. }) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(None)
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.ok().flatten().map(Connected)
    }

    pub fn subscribe(&self) -> impl futures::Stream<Item = Result<event::Upstream, RecvError>> {
        let mut r = self.upstream.subscribe();
        async_stream::stream! { loop { yield r.recv().await } }
    }

    pub(crate) fn emit(&self, evt: impl Into<event::Upstream>) {
        self.upstream.send(evt.into()).ok();
    }
}

impl Default for TinCans {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Interrogation {
    peer: PeerId,
    conn: quic::Connection,
    chan: tincan::Sender<event::Downstream>,
}

impl Interrogation {
    /// Ask the interrogated peer to send its [`PeerAdvertisement`].
    pub async fn peer_advertisement(
        &self,
    ) -> Result<PeerAdvertisement<SocketAddr>, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::GetAdvertisement)
            .await
            .and_then(|resp| match resp {
                Response::Advertisement(ad) => Ok(ad),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    /// Ask the interrogated peer to send back the [`SocketAddr`] the local peer
    /// appears to have.
    pub async fn echo_addr(&self) -> Result<SocketAddr, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::EchoAddr)
            .await
            .and_then(|resp| match resp {
                Response::YourAddr(ad) => Ok(ad),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    /// Ask the interrogated peer to send the complete list of URNs it has.
    ///
    /// The response is compactly encoded as an [`Xor`] filter, with a very
    /// small false positive probability.
    pub async fn urns(&self) -> Result<Xor, error::Interrogation> {
        use interrogation::{Request, Response};

        self.request(Request::GetUrns)
            .await
            .and_then(|resp| match resp {
                Response::Urns(urns) => Ok(urns.into_owned()),
                Response::Error(e) => Err(error::Interrogation::ErrorResponse(e)),
                _ => Err(error::Interrogation::InvalidResponse),
            })
    }

    async fn request(
        &self,
        request: interrogation::Request,
    ) -> Result<interrogation::Response<'static, SocketAddr>, error::Interrogation> {
        use event::downstream::Interrogation;

        let (tx, rx) = replier();
        let msg = Downstream::Interrogation(Interrogation {
            conn: self.conn.clone(),
            peer: self.peer,
            request,
            reply: tx,
        });
        if let Err(tincan::error::SendError(e)) = self.chan.send(msg) {
            match e {
                Downstream::Interrogation(Interrogation { reply, .. }) => {
                    reply
                        .lock()
                        .take()
                        .expect("if chan send failed, there can't be another contender")
                        .send(Err(error::Interrogation::Unavailable))
                        .ok();
                },

                _ => unreachable!(),
            }
        }

        rx.await.unwrap_or(Err(error::Interrogation::Unavailable))
    }
}

pub struct RequestPull {
    reply: mpsc::Receiver<Result<request_pull::Response, error::RequestPull>>,
}

impl RequestPull {
    pub async fn next(&mut self) -> Option<Result<request_pull::Response, error::RequestPull>> {
        self.reply.recv().await
    }
}

fn replier<T>() -> (event::downstream::Reply<T>, Receiver<T>) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    (Arc::new(Mutex::new(Some(tx))), rx)
}

fn multi_replier<T>() -> (event::downstream::MultiReply<T>, mpsc::Receiver<T>) {
    let (tx, rx) = tokio::sync::mpsc::channel(3);
    (Arc::new(Mutex::new(Some(tx))), rx)
}
