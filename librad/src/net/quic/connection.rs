// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{io, net::SocketAddr};

use futures::stream::{BoxStream, TryStreamExt as _};
use quinn::NewConnection;

use super::{BidiStream, Error, RecvStream, Result, SendStream};
use crate::{
    net::connection::{CloseReason, RemoteAddr, RemotePeer},
    PeerId,
};

mod tracking;
pub use tracking::Conntrack;

pub struct IncomingStreams<'a> {
    pub bidi: BoxStream<'a, Result<BidiStream>>,
    pub uni: BoxStream<'a, Result<RecvStream>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionId(usize);

#[derive(Clone)]
pub struct Connection {
    peer: PeerId,
    conn: quinn::Connection,
    track: Conntrack,
}

impl Connection {
    pub(super) fn new<'a>(
        remote_peer: PeerId,
        track: Conntrack,
        NewConnection {
            connection,
            bi_streams,
            uni_streams,
            ..
        }: NewConnection,
    ) -> (Self, IncomingStreams<'a>) {
        let conn = Self {
            peer: remote_peer,
            conn: connection,
            track: track.clone(),
        };
        let conn_id = conn.id();
        let bidi = {
            let conn = conn.clone();
            let track = track.clone();
            bi_streams
                .map_ok(move |(send, recv)| {
                    conn.tickle();
                    BidiStream {
                        conn: conn.clone(),
                        send: SendStream {
                            conn: conn.clone(),
                            send,
                        },
                        recv: RecvStream {
                            conn: conn.clone(),
                            recv,
                        },
                    }
                })
                .map_err(move |e| {
                    track.disconnect(&conn_id, CloseReason::ConnectionError);
                    Error::from(e)
                })
        };
        let uni = {
            let conn = conn.clone();
            uni_streams
                .map_ok(move |recv| {
                    conn.tickle();
                    RecvStream {
                        conn: conn.clone(),
                        recv,
                    }
                })
                .map_err(move |e| {
                    track.disconnect(&conn_id, CloseReason::ConnectionError);
                    Error::from(e)
                })
        };

        let incoming = IncomingStreams {
            bidi: Box::pin(bidi),
            uni: Box::pin(uni),
        };

        (conn, incoming)
    }

    pub(super) fn existing(remote_peer: PeerId, track: Conntrack, conn: quinn::Connection) -> Self {
        Self {
            peer: remote_peer,
            conn,
            track,
        }
    }

    pub fn id(&self) -> ConnectionId {
        ConnectionId(self.conn.stable_id())
    }

    pub async fn open_bidi(&self) -> Result<BidiStream> {
        let (send, recv) = self.conn.open_bi().await.map_err(|e| {
            self.track
                .disconnect(&self.id(), CloseReason::ConnectionError);
            e
        })?;
        self.tickle();

        Ok(BidiStream {
            conn: self.clone(),
            recv: RecvStream {
                conn: self.clone(),
                recv,
            },
            send: SendStream {
                conn: self.clone(),
                send,
            },
        })
    }

    pub async fn open_uni(&self) -> Result<SendStream> {
        let send = self.conn.open_uni().await.map_err(|e| {
            self.track
                .disconnect(&self.id(), CloseReason::ConnectionError);
            e
        })?;
        self.tickle();

        Ok(SendStream {
            conn: self.clone(),
            send,
        })
    }

    pub fn close(self, reason: CloseReason) {
        self.track.disconnect(&self.id(), reason);
    }

    #[tracing::instrument(skip(self, e))]
    pub(super) fn on_stream_error(&self, e: &io::Error) {
        tracing::warn!(err = ?e, "stream error");
        self.track
            .disconnect(&self.id(), CloseReason::ConnectionError);
    }

    pub fn tickle(&self) {
        self.track.tickle(&self.id())
    }

    pub fn stable_id(&self) -> usize {
        self.conn.stable_id()
    }
}

impl RemotePeer for Connection {
    fn remote_peer_id(&self) -> PeerId {
        self.peer
    }
}

impl RemoteAddr for Connection {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> SocketAddr {
        self.conn.remote_address()
    }
}
