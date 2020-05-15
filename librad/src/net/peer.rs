// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{
    future::Future,
    io,
    net::{SocketAddr, ToSocketAddrs},
};

use thiserror::Error;

use crate::{
    git::{self, repo, server::GitServer, storage::Storage as GitStorage},
    keys::{PublicKey, SecretKey},
    net::{
        connection::LocalInfo,
        discovery,
        gossip::{self, LocalStorage, PutResult},
        protocol::{self, Protocol},
        quic::{self, BoundEndpoint, Endpoint},
    },
    paths::Paths,
    peer::PeerId,
    uri::{self, RadUrn},
};

pub mod types;
pub use types::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GitFetchError {
    #[error("Already have {0}")]
    KnownObject(git2::Oid),

    #[error(transparent)]
    Repo(#[from] repo::Error),

    #[error(transparent)]
    Store(#[from] git::storage::Error),
}

#[derive(Debug, Error)]
#[error("Failed to bind to {addr}")]
pub struct BindError {
    addr: SocketAddr,
    source: quic::Error,
}

/// A stateful network peer.
///
/// Implements [`LocalStorage`]. A [`Peer`] can be bound to one or more
/// [`SocketAddr`]esses.
#[derive(Clone)]
pub struct Peer {
    key: SecretKey,
    paths: Paths,
    git: GitStorage,
}

impl Peer {
    pub fn new(paths: Paths, git: GitStorage) -> Self {
        Self {
            key: git.key.clone(),
            paths,
            git,
        }
    }

    pub fn init(paths: Paths, key: SecretKey) -> Result<Self, git::storage::Error> {
        let git = GitStorage::init(&paths, key.clone())?;
        Ok(Self { key, paths, git })
    }

    pub fn public_key(&self) -> PublicKey {
        self.key.public()
    }

    /// Bind to the given [`SocketAddr`].
    ///
    /// Calling `bind` will cause the process to listen on the given address,
    /// but that won't have any effect (except perhaps for filling up kernel
    /// buffers) until the returned [`BoundPeer`] is run. The reason for
    /// this intermediate bootstrapping step is that we may want to bind to
    /// a random port, and later retrieve which port was actually chosen by the
    /// kernel.
    pub async fn bind<'a>(self, addr: SocketAddr) -> Result<BoundPeer<'a>, BindError> {
        let peer_id = PeerId::from(&self.key);
        let git = GitServer::new(&self.paths);
        let endpoint = Endpoint::bind(&self.key, addr)
            .await
            .map_err(|e| BindError { addr, source: e })?;
        let gossip = gossip::Protocol::new(
            &peer_id,
            gossip::PeerAdvertisement::new(endpoint.local_addr().unwrap()),
            gossip::MembershipParams::default(),
            self,
        );
        let protocol = Protocol::new(gossip, git);
        git::transport::register().register_stream_factory(&peer_id, Box::new(protocol.clone()));

        Ok(BoundPeer {
            peer_id,
            endpoint,
            protocol,
        })
    }

    pub fn git(&self) -> &GitStorage {
        &self.git
    }

    /// Update a git repo
    pub fn git_fetch(
        &self,
        from: &PeerId,
        urn: RadUrn,
        head: git2::Oid,
    ) -> Result<(), GitFetchError> {
        if self.git.has_commit(&urn, head)? {
            return Err(GitFetchError::KnownObject(head));
        }

        self.git
            .clone()
            .open_repo(urn)?
            .fetch(from)
            .map_err(|e| e.into())
    }

    /// Determine if we have the given object locally
    pub fn git_has(&self, urn: RadUrn, head: git2::Oid) -> bool {
        self.git.has_commit(&urn, head).unwrap_or(false)
    }
}

impl LocalStorage for Peer {
    type Update = Gossip;

    fn put(&self, provider: &PeerId, has: Self::Update) -> PutResult {
        match has.urn.proto {
            uri::Protocol::Git => {
                let Rev::Git(head) = has.rev;
                let res = self.git_fetch(provider, has.urn, head);

                match res {
                    Ok(()) => PutResult::Applied,
                    Err(e) => match e {
                        GitFetchError::KnownObject(_) => PutResult::Stale,
                        GitFetchError::Repo(repo::Error::NoSuchUrn(_)) => PutResult::Uninteresting,
                        _ => PutResult::Error,
                    },
                }
            },
        }
    }

    fn ask(&self, want: Self::Update) -> bool {
        match want.urn.proto {
            uri::Protocol::Git => {
                let Rev::Git(head) = want.rev;
                self.git_has(want.urn, head)
            },
        }
    }
}

/// A [`Peer`] bound to a particular [`SocketAddr`] using [`Peer::bind`] and
/// ready to be [`BoundPeer::run`]
pub struct BoundPeer<'a> {
    peer_id: PeerId,
    endpoint: BoundEndpoint<'a>,
    protocol: Protocol<Peer, Gossip>,
}

impl<'a> BoundPeer<'a> {
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Inspect the bound address before calling `run`.
    ///
    /// Useful, for example, to obtain the actual address after having bound the
    /// peer to `0.0.0.0:0`.
    pub fn bound_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }

    /// Obtain a [`Handle`] to the underlying [`Protocol`], so downstream
    /// communication is possible after calling `run`.
    pub fn handle(&self) -> Handle {
        Handle(self.protocol.clone())
    }

    /// Run the protocol stack, bootstrapping from `known_peers`.
    ///
    /// This consumes `self`, and does not terminate unless and until the
    /// supplied `shutdown` future resolves.
    pub async fn run<P, S, F>(mut self, known_peers: P, shutdown: F)
    where
        P: IntoIterator<Item = (PeerId, S)>,
        S: ToSocketAddrs,
        F: Future<Output = ()> + Send + Unpin,
    {
        let disco = discovery::Static::new(known_peers).into_stream();
        self.protocol.run(self.endpoint, disco, shutdown).await
    }
}

/// A handle to the [`Protocol`] of a running [`BoundPeer`]
pub struct Handle(Protocol<Peer, Gossip>);

impl Handle {
    pub async fn announce(&self, have: Gossip) {
        self.0.announce(have).await
    }

    pub async fn query(&self, want: Gossip) -> impl futures::Stream<Item = gossip::Has<Gossip>> {
        self.0.query(want).await
    }

    pub async fn subscribe(&self) -> impl futures::Stream<Item = protocol::ProtocolEvent> {
        self.0.subscribe().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{hash::Hash, uri::Path};

    #[test]
    fn test_rev_serde() {
        let rev = Rev::Git(git2::Oid::hash_object(git2::ObjectType::Commit, b"chrzbrr").unwrap());
        assert_eq!(
            rev,
            serde_cbor::from_slice(&serde_cbor::to_vec(&rev).unwrap()).unwrap()
        )
    }

    #[test]
    fn test_gossip_serde() {
        let rev = Rev::Git(git2::Oid::hash_object(git2::ObjectType::Commit, b"chrzbrr").unwrap());
        let gossip = Gossip::new(Hash::hash(b"cerveza coronita"), Path::new(), rev);
        assert_eq!(
            gossip,
            serde_cbor::from_slice(&serde_cbor::to_vec(&gossip).unwrap()).unwrap()
        )
    }
}
