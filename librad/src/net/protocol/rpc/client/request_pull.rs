// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{
    self,
    future::{self, BoxFuture},
    stream::BoxStream,
    FutureExt as _,
    Stream,
    StreamExt as _,
};

use crate::{
    git::Urn,
    net::{
        protocol::{self, request_pull},
        quic,
    },
    paths::Paths,
};

use super::{error, streams};

/// A series of request-pull responses.
///
/// Use [`futures::StreamExt::next`] to get the next response from the
/// `RequestPull` stream. The responses will be finished once the next result
/// will either one of the following:
///   * `None` was returned
///   * A successful response, [`request_pull::Response::Success`]
///   * An error response, [`request_pull::Response::Error`]
///   * An error,  [`error::RequestPull`]
pub struct RequestPull {
    resp: BoxStream<'static, Result<request_pull::Response, error::RequestPull>>,
    repl: BoxFuture<'static, Result<(), error::Incoming>>,
}

trait AssertSend: Send {}
impl AssertSend for RequestPull {}

impl RequestPull {
    pub async fn new(
        conn: quic::Connection,
        streams: Option<quic::BoxedIncomingStreams<'static>>,
        urn: Urn,
        paths: Arc<Paths>,
    ) -> Result<Self, error::RequestPull> {
        let resp = protocol::io::send::multi_response(
            &conn,
            protocol::request_pull::Request { urn },
            protocol::request_pull::FRAMED_BUFSIZ,
        )
        .await?
        .map(|i| i.map_err(error::RequestPull::from))
        .boxed();

        let repl = match streams {
            Some(streams) => streams::git(paths, streams).boxed(),
            None => future::pending().boxed(),
        };

        Ok(Self { resp, repl })
    }
}

impl Stream for RequestPull {
    type Item = Result<request_pull::Response, error::RequestPull>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(Err(e)) = self.repl.poll_unpin(cx) {
            return Poll::Ready(Some(Err(e.into())));
        }

        self.resp.poll_next_unpin(cx)
    }
}
