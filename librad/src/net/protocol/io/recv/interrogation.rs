// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    net::SocketAddr,
    panic,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt as _},
    SinkExt as _,
    StreamExt as _,
};
use futures_codec::FramedRead;
use parking_lot::{
    MappedRwLockReadGuard,
    RwLock,
    RwLockReadGuard,
    RwLockUpgradableReadGuard,
    RwLockWriteGuard,
};
use thiserror::Error;

use crate::{
    git::{identities, storage, Storage},
    identities::SomeUrn,
    net::{
        connection::Duplex,
        protocol::{
            info::PeerAdvertisement,
            interrogation::{self, xor, Request, Response, Xor},
            io::{self, codec},
            State,
        },
        quic,
        upgrade::{self, Upgraded},
    },
};

#[derive(Clone, Default)]
pub struct Cache {
    urns: Arc<RwLock<Option<(Instant, Xor)>>>,
}

#[derive(Debug, Error)]
enum Error {
    #[error("the cache is being rebuilt")]
    Refreshing,

    #[error(transparent)]
    BuildUrns(#[from] Box<xor::BuildError<identities::Error>>),

    #[error(transparent)]
    Cbor(#[from] minicbor::encode::Error<std::io::Error>),
}

impl From<xor::BuildError<identities::Error>> for Error {
    fn from(e: xor::BuildError<identities::Error>) -> Self {
        Self::BuildUrns(Box::new(e))
    }
}

lazy_static! {
    static ref INTERNAL_ERROR: Vec<u8> =
        encode(&Response::Error(interrogation::Error::Internal)).unwrap();
    static ref UNAVAILABLE_ERROR: Vec<u8> = encode(&Response::Error(
        interrogation::Error::TemporarilyUnavailable
    ))
    .unwrap();
}

pub(in crate::net::protocol) async fn interrogation<S, T>(
    state: State<S>,
    stream: Upgraded<upgrade::Interrogation, T>,
) where
    S: storage::Pooled + Send + 'static,
    T: Duplex<Addr = SocketAddr>,
    T::Read: AsyncRead + Unpin,
    T::Write: AsyncWrite + Unpin,
{
    let remote_addr = stream.remote_addr();
    let (recv, send) = stream.into_stream().split();
    let mut recv = FramedRead::new(recv, codec::Codec::<interrogation::Request>::new());
    if let Some(x) = recv.next().await {
        match x {
            Err(e) => tracing::warn!(err = ?e, "interrogation recv error"),
            Ok(req) => {
                let resp = match state.storage.get().await {
                    Err(e) => {
                        tracing::error!(err = ?e, "unable to borrow storage");
                        Cow::from(&*INTERNAL_ERROR)
                    },
                    Ok(storage) => {
                        let res = tokio::task::spawn_blocking(move || {
                            handle_request(
                                &state.endpoint,
                                &storage,
                                &state.caches.interrogation,
                                remote_addr,
                                req,
                            )
                            .map(Cow::from)
                            .unwrap_or_else(|e| {
                                tracing::error!(err = ?e, "error handling request");
                                match e {
                                    Error::Refreshing | Error::BuildUrns(_) => {
                                        Cow::from(&*UNAVAILABLE_ERROR)
                                    },
                                    Error::Cbor(_) => Cow::from(&*INTERNAL_ERROR),
                                }
                            })
                        })
                        .await;
                        match res {
                            Err(e) => {
                                if e.is_panic() {
                                    panic::resume_unwind(e.into_panic())
                                } else if e.is_cancelled() {
                                    return;
                                } else {
                                    unreachable!("unexpected task error: {:?}", e)
                                }
                            },
                            Ok(resp) => resp,
                        }
                    },
                };

                if let Err(e) = send.into_sink().send(resp).await {
                    tracing::warn!(err = ?e, "interrogation send error")
                }
            },
        }
    }
}

fn handle_request(
    endpoint: &quic::Endpoint,
    storage: &Storage,
    cache: &Cache,
    remote_addr: SocketAddr,
    req: interrogation::Request,
) -> Result<Vec<u8>, Error> {
    use either::Either::*;

    match req {
        Request::GetAdvertisement => Left(Response::Advertisement(peer_advertisement(endpoint))),
        Request::EchoAddr => Left(Response::YourAddr(remote_addr)),
        Request::GetUrns => {
            let urns = urns(cache, storage)?;
            Right(encode(&Response::<SocketAddr>::Urns(Cow::Borrowed(&urns))))
        },
    }
    .right_or_else(|resp| encode(&resp))
}

fn peer_advertisement(endpoint: &quic::Endpoint) -> PeerAdvertisement<SocketAddr> {
    io::peer_advertisement(endpoint)
}

fn urns<'a>(cache: &'a Cache, storage: &Storage) -> Result<MappedRwLockReadGuard<'a, Xor>, Error> {
    // refresh the cache every couple of minutes, assuming we have the refs in
    // memory anyways
    //
    // TODO: we should eventually have hooks to only refresh when the refs
    // actually changed
    const MAX_AGE: Duration = Duration::from_secs(300);

    // fast path cache hit
    {
        let guard = cache.urns.read();
        if let Some((updated, _)) = &*guard {
            if updated.elapsed() < MAX_AGE {
                return Ok(RwLockReadGuard::map(guard, |x| {
                    x.as_ref().map(|(_, x)| x).unwrap()
                }));
            }
        }
    }

    // take an upgradable read lock, exiting if another cache builder holds it
    match cache.urns.try_upgradable_read() {
        None => Err(Error::Refreshing),
        Some(guard) => {
            // I/O while only holding the read lock, other readers should be
            // able to make progress
            let xor = build_urns(storage)?;
            let mut guard = RwLockUpgradableReadGuard::upgrade(guard);
            *guard = Some((Instant::now(), xor));
            Ok(RwLockReadGuard::map(
                RwLockWriteGuard::downgrade(guard),
                |x| x.as_ref().map(|(_, x)| x).unwrap(),
            ))
        },
    }
}

fn build_urns(storage: &Storage) -> Result<Xor, xor::BuildError<identities::Error>> {
    Xor::try_from_iter(identities::any::list_urns(storage)?.map(|res| res.map(SomeUrn::Git)))
}

fn encode(resp: &interrogation::Response<SocketAddr>) -> Result<Vec<u8>, Error> {
    Ok(minicbor::to_vec(resp)?)
}
