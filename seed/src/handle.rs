// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::HashMap, net::SocketAddr};

use futures::{channel::mpsc as chan, sink::SinkExt as _, stream::StreamExt as _};
use thiserror::Error;

use librad::peer::PeerId;

use crate::Project;

/// An error returned by the [`NodeHandle`].
#[derive(Debug, Error)]
pub enum NodeError {
    #[error("request failed: the node disconnected")]
    RequestFailed,
}

/// Handle used to interact with the seed node.
pub struct NodeHandle {
    channel: chan::UnboundedSender<Request>,
}

impl NodeHandle {
    pub(crate) fn new(channel: chan::UnboundedSender<Request>) -> Self {
        Self { channel }
    }

    /// Get all local projects.
    pub async fn get_projects(&mut self) -> Result<Vec<Project>, NodeError> {
        let (tx, mut rx) = chan::channel(1);
        self.channel
            .send(Request::GetProjects(tx))
            .await
            .map_err(|_| NodeError::RequestFailed)?;

        rx.next().await.ok_or(NodeError::RequestFailed)
    }

    /// Get currently connected peers.
    pub async fn get_peers(&mut self) -> Result<HashMap<PeerId, SocketAddr>, NodeError> {
        let (tx, mut rx) = chan::channel(1);
        self.channel
            .send(Request::GetPeers(tx))
            .await
            .map_err(|_| NodeError::RequestFailed)?;

        rx.next().await.ok_or(NodeError::RequestFailed)
    }
}

/// User request to the seed node.
pub enum Request {
    /// Get local projects.
    GetProjects(chan::Sender<Vec<Project>>),
    /// Get connected peers.
    GetPeers(chan::Sender<HashMap<PeerId, SocketAddr>>),
}
