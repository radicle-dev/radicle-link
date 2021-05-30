// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fmt::{self, Debug},
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use either::Either;
use futures::stream::{self, BoxStream, Stream, StreamExt as _, TryStreamExt as _};
use quinn::NewConnection;

use super::{BidiStream, Error, RecvStream, Result, SendStream};
use crate::{
    net::connection::{CloseReason, RemoteAddr, RemotePeer},
    PeerId,
};

mod tracking;
pub use tracking::Conntrack;

pub type BoxedIncomingStreams<'a> =
    IncomingStreams<BoxStream<'a, Result<Either<BidiStream, RecvStream>>>>;

pub struct IncomingStreams<S> {
    conn: Connection,
    inner: S,
}

impl<'a, S> IncomingStreams<S>
where
    S: Stream<Item = Result<Either<BidiStream, RecvStream>>> + Send + 'a,
{
    pub fn boxed(self) -> BoxedIncomingStreams<'a> {
        IncomingStreams {
            conn: self.conn,
            inner: Box::pin(self.inner),
        }
    }
}

fn incoming_streams(
    conn: Connection,
    bi_streams: quinn::IncomingBiStreams,
    uni_streams: quinn::IncomingUniStreams,
) -> IncomingStreams<impl Stream<Item = Result<Either<BidiStream, RecvStream>>>> {
    use Either::{Left, Right};

    let conn_id = conn.id();
    let track = conn.track.clone();
    let bidi = {
        let conn = conn.clone();
        bi_streams.map_ok(move |(send, recv)| {
            conn.tickle();
            Left(BidiStream {
                conn: conn.clone(),
                send: SendStream {
                    conn: conn.clone(),
                    send,
                },
                recv: RecvStream {
                    conn: conn.clone(),
                    recv,
                },
            })
        })
    };
    let uni = {
        let conn = conn.clone();
        uni_streams.map_ok(move |recv| {
            conn.tickle();
            Right(RecvStream {
                conn: conn.clone(),
                recv,
            })
        })
    };
    let inner = stream::select(bidi, uni).map_err(move |e| {
        track.disconnect(&conn_id, CloseReason::ConnectionError);
        Error::from(e)
    });

    IncomingStreams { conn, inner }
}

impl<S> Stream for IncomingStreams<S>
where
    S: Stream<Item = Result<Either<BidiStream, RecvStream>>> + Unpin,
{
    type Item = Result<Either<BidiStream, RecvStream>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

impl<S> RemoteAddr for IncomingStreams<S> {
    type Addr = SocketAddr;

    fn remote_addr(&self) -> Self::Addr {
        self.conn.remote_addr()
    }
}

impl<S> RemotePeer for IncomingStreams<S> {
    fn remote_peer_id(&self) -> PeerId {
        self.conn.remote_peer_id()
    }
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
    pub(super) fn new(
        remote_peer: PeerId,
        track: Conntrack,
        NewConnection {
            connection,
            bi_streams,
            uni_streams,
            ..
        }: NewConnection,
    ) -> (
        Self,
        IncomingStreams<impl Stream<Item = Result<Either<BidiStream, RecvStream>>>>,
    ) {
        let conn = Self {
            peer: remote_peer,
            conn: connection,
            track,
        };
        let incoming = incoming_streams(conn.clone(), bi_streams, uni_streams);

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

impl Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Connection")
            .field("peer", &self.peer)
            .field("conn", &self.id())
            .field("track", &self.track)
            .finish()
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
