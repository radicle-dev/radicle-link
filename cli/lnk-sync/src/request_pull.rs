// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;

use futures::StreamExt;
use serde::Serialize;
use thiserror::Error;

use git_ref_format::RefString;
use librad::{
    git::Urn,
    git_ext as ext,
    net::{
        peer::{client, Client},
        protocol::request_pull,
        quic::ConnectPeer,
    },
    Signer,
};
use lnk_clib::seed::Seed;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Client(#[from] client::error::RequestPull),
    #[error(transparent)]
    Response(#[from] request_pull::Error),
}

pub(super) async fn request_pull<S, E>(
    client: &Client<S, E>,
    urn: Urn,
    seed: Seed<Vec<SocketAddr>>,
) -> Result<Option<Success>, Error>
where
    S: Signer + Clone,
    E: ConnectPeer + Clone + Send + Sync + 'static,
{
    let mut req = client.request_pull(seed.clone(), urn.clone()).await?;
    while let Some(res) = req.next().await {
        match res {
            Ok(res) => match res {
                request_pull::Response::Success(succ) => return Ok(Some(succ.into())),
                request_pull::Response::Error(err) => {
                    tracing::error!(err = %err, "request-pull error");
                    return Err(err.into());
                },
                request_pull::Response::Progress(prog) => {
                    println!("{}", prog);
                    tracing::info!("request-pull progress {}", prog);
                    continue;
                },
            },
            Err(err) => {
                tracing::error!(err = %err, "request-pull error");
                return Err(err.into());
            },
        }
    }

    Ok(None)
}

#[derive(Clone, Debug, Serialize)]
pub struct Success {
    pub updated: Vec<Reference>,
    pub pruned: Vec<RefString>,
}

impl From<request_pull::Success> for Success {
    fn from(s: request_pull::Success) -> Self {
        Self {
            updated: s.refs.into_iter().map(|r| r.into()).collect(),
            pruned: s.pruned,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Reference {
    name: RefString,
    target: ext::Oid,
}

impl From<request_pull::Ref> for Reference {
    fn from(request_pull::Ref { name, oid }: request_pull::Ref) -> Self {
        Self { name, target: oid }
    }
}
