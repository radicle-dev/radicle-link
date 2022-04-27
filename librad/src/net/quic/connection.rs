// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    future::Future,
    io,
    iter,
    net::SocketAddr,
    ops::DerefMut,
    pin::Pin,
    result::Result as StdResult,
    sync::Arc,
    task::{Context, Poll},
};

use either::Either;
use futures::{
    lock::{Mutex, MutexGuard},
    stream::{self, BoxStream, Stream, StreamExt as _, TryStreamExt as _},
};
use quinn::NewConnection;
use thiserror::Error;

use super::{BidiStream, Error, RecvStream, Result, SendStream};
use crate::{
    net::connection::{CloseReason, RemoteAddr, RemotePeer},
    PeerId,
};

mod tracking;
pub use tracking::Conntrack;

pub type BoxedIncomingStreams<'a> =
    IncomingStreams<BoxStream<'a, Result<Either<BidiStream, RecvStream>>>>;

#[derive(Clone)]
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
        if let Some(track) = track.as_ref() {
            track.disconnect(&conn_id, CloseReason::ConnectionError)
        }

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

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BorrowUniError<E: std::error::Error + 'static> {
    #[error("opening a new stream failed")]
    Quic(#[from] Error),

    #[error("stream upgrade failed")]
    Upgrade(#[source] E),
}

#[derive(Clone)]
pub struct Connection {
    peer: PeerId,
    conn: quinn::Connection,
    track: Option<Conntrack>,
    send_streams: Arc<Vec<Mutex<Option<SendStream>>>>,
}

impl Connection {
    pub(super) fn new(
        track: Option<Conntrack>,
        reserve_send_streams: usize,
        remote_peer: PeerId,
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
            send_streams: Arc::new(
                iter::repeat_with(Default::default)
                    .take(reserve_send_streams)
                    .collect(),
            ),
        };
        let incoming = incoming_streams(conn.clone(), bi_streams, uni_streams);

        (conn, incoming)
    }

    pub fn id(&self) -> ConnectionId {
        ConnectionId(self.conn.stable_id())
    }

    pub async fn open_bidi(&self) -> Result<BidiStream> {
        let (send, recv) = self.conn.open_bi().await.map_err(|e| {
            self.close(CloseReason::ConnectionError);
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
            self.close(CloseReason::ConnectionError);
            e
        })?;
        self.tickle();

        Ok(SendStream {
            conn: self.clone(),
            send,
        })
    }

    /// Borrow the [`SendStream`] at index `idx` from a fixed-size set of
    /// reservations.
    ///
    /// The reservations are allocated on construction, but the actual streams
    /// are created lazily when requested. The supplied closure is executed
    /// once when the stream is allocated, typically to perform a stream
    /// upgrade.
    ///
    /// The caller is responsible for mapping indices to the expected stream,
    /// and to not exceed the reservation bounds.
    ///
    /// # Panics
    ///
    /// If `idx` is out of bounds.
    pub async fn borrow_uni<I, F, U, E>(
        &self,
        idx: I,
        upgrade: F,
    ) -> StdResult<impl DerefMut<Target = SendStream> + '_, BorrowUniError<E>>
    where
        I: Into<usize>,
        F: FnOnce(SendStream) -> U,
        U: Future<Output = std::result::Result<SendStream, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let mut lck = self.send_streams[idx.into()].lock().await;
        if lck.is_none() {
            let stream = self.open_uni().await.map_err(BorrowUniError::Quic)?;
            *lck = Some(upgrade(stream).await.map_err(BorrowUniError::Upgrade)?);
        }

        Ok(MutexGuard::map(lck, |s| s.as_mut().unwrap()))
    }

    pub fn close(&self, reason: CloseReason) {
        if let Some(track) = self.track.as_ref() {
            track.disconnect(&self.id(), reason)
        }
    }

    #[tracing::instrument(skip(self, e))]
    pub(super) fn on_stream_error(&self, e: &io::Error) {
        tracing::warn!(err = ?e, "stream error");
        self.close(CloseReason::ConnectionError);
    }

    pub fn tickle(&self) {
        if let Some(track) = self.track.as_ref() {
            track.tickle(&self.id())
        }
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
