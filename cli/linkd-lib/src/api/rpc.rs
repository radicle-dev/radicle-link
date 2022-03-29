// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use futures::{future::FutureExt, stream::FuturesUnordered};
use std::{marker::PhantomData, panic, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use tokio::{
    net::{UnixListener, UnixStream},
    sync::mpsc::{channel, Sender},
};

use librad::{
    net::{peer::Peer, protocol::RequestPullGuard},
    Signer,
};
use link_async::{incoming::UnixListenerExt, Spawner};

use super::{
    announce,
    io::{self, SocketTransportError, Transport},
    messages,
    request_pull,
};

pub fn tasks<S, G>(
    spawner: Arc<Spawner>,
    peer: Peer<S, G>,
    socket: &UnixListener,
    announce_wait_time: Duration,
) -> impl futures::stream::Stream<Item = link_async::Task<()>> + Send + '_
where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    socket
        .incoming()
        .map(move |stream| match stream {
            Ok(stream) => {
                tracing::debug!("new connection");
                Some(spawner.spawn(rpc(
                    spawner.clone(),
                    peer.clone(),
                    stream,
                    announce_wait_time,
                )))
            },
            Err(e) => {
                tracing::error!(err=?e, "error accepting connection");
                None
            },
        })
        .take_while(|e| futures::future::ready(e.is_some()))
        .filter_map(futures::future::ready)
}

const MAX_IN_FLIGHT_REQUESTS: usize = 20;

async fn rpc<S, G>(
    spawner: Arc<Spawner>,
    peer: Peer<S, G>,
    stream: UnixStream,
    announce_wait_time: Duration,
) where
    S: Signer + Clone,
    G: RequestPullGuard,
{
    let mut running_handlers = FuturesUnordered::new();
    let mut transport: io::SocketTransport = stream.into();
    // TODO: What should the buffer size be here?
    let (sx, mut rx) = channel(10);

    loop {
        let next = if running_handlers.len() < MAX_IN_FLIGHT_REQUESTS {
            transport.recv_request()
        } else {
            futures::future::pending().boxed()
        };
        let mut next_complete = running_handlers.next().fuse();
        futures::select! {
            next = next.fuse() => {
                match next {
                    Ok(Some(next)) => {
                        let handler = {
                            let peer = peer.clone();
                            spawner.spawn(match next.payload {
                                messages::RequestPayload::Announce(p) => {
                                    let mut listener =
                                        Listener::announce(next.mode, sx.clone());
                                    tracing::info!(?p, "dispatching request");
                                    listener.ack().await;
                                    listener.handle(peer, announce_wait_time, p).boxed()
                                },
                                messages::RequestPayload::RequestPull(p) => {
                                    let mut listener = Listener::request_pull(next.mode, sx.clone());
                                    tracing::info!(?p, "dispatching request");
                                    listener.ack().await;
                                    listener.handle(peer, p).boxed()
                                }
                            })
                        };
                        running_handlers.push(handler);
                    },
                    Ok(None) => {
                        tracing::info!("closing connection");
                        break;
                    },
                    Err(e) => {
                        match e {
                            SocketTransportError::DecodeFailed => {
                                tracing::error!(err=?e, "failed to decode message, ignoring");
                            },
                            e => {
                                tracing::error!(err=?e, "error receiving message, closing");
                                break;
                            },
                        }
                    }
                }
            },
            next_complete = next_complete => {
                if let Some(task) = next_complete {
                    handle_task_complete(task);
                }
            },
            resp = rx.recv().fuse() => {
                match resp {
                    Some(response) => {
                        match transport.send_response(response).await {
                            Ok(()) => {},
                            Err(e) => {
                                tracing::error!(err=?e, "error sending response");
                            }
                        }
                    },
                    None => {
                        tracing::error!("response channel closed");
                        break;
                    }
                }
            }
        }
    }
    while let Some(complete) = running_handlers.next().await {
        handle_task_complete(complete);
    }
}

fn handle_task_complete(task_result: Result<(), link_async::JoinError>) {
    match task_result {
        Ok(_) => (),
        Err(e) => {
            if e.is_panic() {
                panic::resume_unwind(e.into_panic());
            } else {
                tracing::warn!("task unexpectedly cancelled");
            }
        },
    }
}

struct Listener<P> {
    request_id: messages::RequestId,
    send: Sender<messages::Response<messages::SomeSuccess>>,
    interest: ListenerInterest,
    _marker: PhantomData<P>,
}

enum ListenerInterest {
    AckOnly,
    ProgressAndResult,
}

impl From<messages::RequestMode> for ListenerInterest {
    fn from(mode: messages::RequestMode) -> Self {
        match mode {
            messages::RequestMode::FireAndForget => Self::AckOnly,
            messages::RequestMode::ReportProgress => Self::ProgressAndResult,
        }
    }
}

impl<P> Listener<P> {
    async fn ack(&mut self) {
        self.send(messages::ResponsePayload::Ack).await
    }

    async fn error(&mut self, error: String) {
        match self.interest {
            ListenerInterest::AckOnly => {},
            ListenerInterest::ProgressAndResult => {
                self.send(messages::ResponsePayload::Error(error)).await
            },
        }
    }

    async fn progress(&mut self, message: String) {
        match self.interest {
            ListenerInterest::AckOnly => {},
            ListenerInterest::ProgressAndResult => {
                self.send(messages::ResponsePayload::Progress(message))
                    .await
            },
        }
    }

    async fn success(&mut self, payload: messages::SomeSuccess) {
        match self.interest {
            ListenerInterest::AckOnly => {},
            ListenerInterest::ProgressAndResult => {
                self.send(messages::ResponsePayload::Success(payload)).await
            },
        }
    }

    async fn send(&mut self, msg: messages::ResponsePayload<messages::SomeSuccess>) {
        let resp = messages::Response {
            request_id: self.request_id.clone(),
            payload: msg,
        };
        match self.send.send(resp).await {
            Ok(()) => {},
            Err(_) => {
                tracing::error!("error sending response");
            },
        }
    }
}

impl Listener<announce::Response> {
    fn announce(
        mode: messages::RequestMode,
        send: Sender<messages::Response<messages::SomeSuccess>>,
    ) -> Self {
        Self {
            request_id: Default::default(),
            send,
            interest: mode.into(),
            _marker: PhantomData,
        }
    }

    #[tracing::instrument(skip(self, peer))]
    async fn handle<S, G>(
        mut self,
        peer: Peer<S, G>,
        announce_wait_time: Duration,
        announce: announce::Request,
    ) where
        S: Signer + Clone,
        G: RequestPullGuard,
    {
        tracing::info!(rev = ?announce.rev, urn = %announce.urn, "received announce request");
        let gossip_announce = announce.into_gossip(peer.peer_id());
        if peer.connected_peers().await.is_empty() {
            tracing::debug!(wait_time=?announce_wait_time, "No connected peers, waiting a bit");
            self.progress(format!(
                "no connected peers, waiting {} seconds",
                announce_wait_time.as_secs()
            ))
            .await;
            link_async::sleep(announce_wait_time).await;
        }
        let num_connected = peer.connected_peers().await.len();
        self.progress(format!("found {} peers", num_connected))
            .await;
        if peer.announce(gossip_announce).is_err() {
            // This error can occur if there are no recievers in the running peer to handle
            // the announcement message.
            tracing::error!("failed to send message to announcement subroutine");
            self.error("unable to announce".to_string()).await;
        } else {
            self.success(announce::Response.into()).await;
        }
    }
}

impl Listener<request_pull::Response> {
    fn request_pull(
        mode: messages::RequestMode,
        send: Sender<messages::Response<messages::SomeSuccess>>,
    ) -> Self {
        Self {
            request_id: Default::default(),
            send,
            interest: mode.into(),
            _marker: PhantomData,
        }
    }

    #[tracing::instrument(skip(self, peer))]
    async fn handle<S, G>(
        mut self,
        peer: Peer<S, G>,
        request_pull::Request {
            urn,
            peer: remote,
            addrs,
        }: request_pull::Request,
    ) where
        S: Signer + Clone,
        G: RequestPullGuard,
    {
        use librad::net::protocol::request_pull::{Error, Progress, Response};

        tracing::info!(peer = %remote, urn = %urn, "received request-pull");
        match peer.request_pull((remote, addrs), urn.clone()).await {
            Ok(mut rp) => {
                while let Some(resp) = rp.next().await {
                    match resp {
                        Ok(Response::Progress(Progress { message })) => {
                            self.progress(message).await
                        },
                        Ok(Response::Success(success)) => {
                            self.success(request_pull::Response::from(success).into())
                                .await;
                            break;
                        },
                        Ok(Response::Error(Error { message })) => {
                            self.error(format!("request-pull failed: {message}")).await;
                            break;
                        },
                        Err(err) => {
                            self.error(format!("request-pull failed: {err}")).await;
                            break;
                        },
                    }
                }
            },
            Err(err) => {
                tracing::error!(err = %err, "failed to request-pull");
                self.error(format!("unable to request-pull to `{remote}` for `{urn}`",))
                    .await;
            },
        }
    }
}
