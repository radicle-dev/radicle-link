// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::time::Duration;

use librad::{net::protocol::event::downstream::MembershipInfo, PeerId};
use thiserror::Error;
use tokio::{
    sync::{
        mpsc::{self, error::TrySendError},
        oneshot,
    },
    time,
};

use crate::Project;

/// An error returned by the [`NodeHandle`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NodeError {
    #[error("request response failed")]
    RequestResponseFailed(#[from] oneshot::error::RecvError),

    #[error("request send failed: channel full")]
    RequestSendFailedChannelFull,

    #[error("request send failed: channel closed")]
    RequestSendFailedChannelClosed,

    #[error("request failed: the node disconnected")]
    RequestFailed,

    #[error("request timed out")]
    RequestTimeout(#[from] time::error::Elapsed),
}

impl NodeError {
    fn from_send_error<T>(try_send_error: TrySendError<T>) -> Self {
        match try_send_error {
            TrySendError::Full(_) => Self::RequestSendFailedChannelFull,
            TrySendError::Closed(_) => Self::RequestSendFailedChannelClosed,
        }
    }
}

/// Handle used to interact with the seed node.
pub struct NodeHandle {
    channel: mpsc::Sender<Request>,
    timeout: Duration,
}

impl NodeHandle {
    pub(crate) fn new(channel: mpsc::Sender<Request>, timeout: Duration) -> Self {
        Self { channel, timeout }
    }

    pub async fn get_membership(&mut self) -> Result<MembershipInfo, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.channel
            .try_send(Request::GetMembership(tx))
            .map_err(NodeError::from_send_error)?;

        time::timeout(self.timeout, rx)
            .await?
            .map_err(NodeError::from)
    }

    /// Get all local projects.
    pub async fn get_projects(&mut self) -> Result<Vec<Project>, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.channel
            .try_send(Request::GetProjects(tx))
            .map_err(NodeError::from_send_error)?;

        time::timeout(self.timeout, rx)
            .await?
            .map_err(NodeError::from)
    }

    /// Get currently connected peers.
    pub async fn get_peers(&mut self) -> Result<Vec<PeerId>, NodeError> {
        let (tx, rx) = oneshot::channel();
        self.channel
            .try_send(Request::GetPeers(tx))
            .map_err(NodeError::from_send_error)?;

        time::timeout(self.timeout, rx)
            .await?
            .map_err(NodeError::from)
    }
}

/// User request to the seed node.
pub enum Request {
    /// Get current membership info.
    GetMembership(oneshot::Sender<MembershipInfo>),
    /// Get local projects.
    GetProjects(oneshot::Sender<Vec<Project>>),
    /// Get connected peers.
    GetPeers(oneshot::Sender<Vec<PeerId>>),
}
