// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{borrow::Cow, net::SocketAddr};

use futures::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, BufWriter},
    SinkExt as _,
    StreamExt as _,
};
use futures_codec::FramedRead;
use thiserror::Error;
use typenum::Unsigned as _;

use crate::{
    git::storage,
    identities::xor,
    net::{
        connection::Duplex,
        protocol::{
            cache,
            interrogation::{self, Request, Response},
            io::{self, codec},
            State,
        },
        quic,
        upgrade::{self, Upgraded},
    },
};

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Cbor(#[from] minicbor::encode::Error<std::io::Error>),
}

lazy_static! {
    static ref INTERNAL_ERROR: Vec<u8> =
        encode(&Response::Error(interrogation::Error::Internal)).unwrap();
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
    const BUFSIZ: usize = xor::MaxFingerprints::USIZE * 3;

    let remote_addr = stream.remote_addr();

    let (recv, send) = stream.into_stream().split();
    let recv = BufReader::with_capacity(BUFSIZ, recv);
    let send = BufWriter::with_capacity(BUFSIZ, send);

    let mut recv = FramedRead::new(recv, codec::Codec::<interrogation::Request>::new());
    if let Some(x) = recv.next().await {
        match x {
            Err(e) => tracing::warn!(err = ?e, "interrogation recv error"),
            Ok(req) => {
                let resp = {
                    let res = state
                        .spawner
                        .clone()
                        .spawn_blocking(move || {
                            handle_request(&state.endpoint, &state.caches.urns, remote_addr, req)
                                .map(Cow::from)
                                .unwrap_or_else(|e| {
                                    tracing::error!(err = ?e, "error handling request");
                                    match e {
                                        Error::Cbor(_) => Cow::from(&*INTERNAL_ERROR),
                                    }
                                })
                        })
                        .await;
                    match res {
                        Err(e) => {
                            drop(e.into_cancelled());
                            return;
                        },
                        Ok(resp) => resp,
                    }
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
    urns: &cache::urns::Filter,
    remote_addr: SocketAddr,
    req: interrogation::Request,
) -> Result<Vec<u8>, Error> {
    use either::Either::*;

    match req {
        Request::GetAdvertisement => {
            Left(Response::Advertisement(io::peer_advertisement(endpoint)()))
        },
        Request::EchoAddr => Left(Response::YourAddr(remote_addr)),
        Request::GetUrns => {
            let urns = urns.get();
            Right(encode(&Response::<SocketAddr>::Urns(Cow::Borrowed(&urns))))
        },
    }
    .right_or_else(|resp| encode(&resp))
}

fn encode(resp: &interrogation::Response<SocketAddr>) -> Result<Vec<u8>, Error> {
    Ok(minicbor::to_vec(resp)?)
}
