// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io;

use thiserror::Error;

use librad::git::{refs, storage};
use linkd_lib::api;

#[derive(Debug, Error)]
pub enum PostReceive<E: std::error::Error + Send + 'static> {
    #[error(transparent)]
    Announce(#[from] Announce<E>),
    #[error(transparent)]
    Update(#[from] UpdateSignedRefs<E>),
    #[error(transparent)]
    RequestPull(#[from] RequestPull<E>),
    #[error(transparent)]
    Progress(#[from] Progress<E>),
}

#[derive(Debug, Error)]
pub enum Announce<E: std::error::Error + Send + 'static> {
    #[error(transparent)]
    Progress(#[from] Progress<E>),
    #[error("failed to connect to linkd node: {0}")]
    LinkdConnect(#[source] io::Error),
    #[error("linkd rpc transport failed: {0}")]
    LinkdTransport(#[source] api::client::ReplyError<api::io::SocketTransportError>),
    #[error("the linkd node reported an error: {0}")]
    Linkd(String),
}

#[derive(Debug, Error)]
pub enum UpdateSignedRefs<E: std::error::Error + Send + 'static> {
    #[error(transparent)]
    Progress(#[from] Progress<E>),
    #[error("could not open storage: {0}")]
    OpenStorage(#[from] storage::pool::PoolError),
    #[error("error updating refs: {0}")]
    UpdateRefs(#[from] refs::stored::Error),
}

#[derive(Debug, Error)]
pub enum RequestPull<E: std::error::Error + Send + 'static> {
    #[error(transparent)]
    Progress(#[from] Progress<E>),
}

#[derive(Debug, Error)]
#[error("error notifying client of progress: {0}")]
pub struct Progress<E: std::error::Error + Send + 'static>(pub E);
