// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt, io, net::SocketAddr, str::FromStr};

use serde::Serialize;

use librad::{net::discovery, PeerId};
use tokio::net::{lookup_host, ToSocketAddrs};

pub mod store;
pub use store::Store;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Seed<Addrs> {
    /// The identifier for the `Seed`.
    pub peer: PeerId,
    /// The network addresses to reach the `Seed` on. It is common for this to
    /// be a `String` address that can be resolved by [`Seed::resolve`] to a
    /// list of [`SocketAddr`].
    pub addrs: Addrs,
    /// Human-friendly label for this `Seed`.
    pub label: Option<String>,
}

impl<Addrs: fmt::Display> fmt::Display for Seed<Addrs> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.label {
            Some(label) => write!(f, "{}@{},{}", self.peer, self.addrs, label),
            None => write!(f, "{}@{}", self.peer, self.addrs),
        }
    }
}

impl<T: FromStr> FromStr for Seed<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    type Err = error::Parse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut components = s.split(&[',', '@']);

        let peer = match components.next() {
            None => return Err(error::Parse::Missing("<peer id>")),
            Some(peer) => peer.parse()?,
        };

        let addrs = match components.next() {
            None => return Err(error::Parse::Missing("<addr>")),
            Some(addr) => addr
                .parse()
                .map_err(|err| error::Parse::Addr(Box::new(err)))?,
        };

        let label = components.next().map(Into::into);

        if let Some(wat) = components.next() {
            return Err(error::Parse::Unexpected(wat.to_string()));
        }

        Ok(Self { peer, addrs, label })
    }
}

impl<T> Seed<T> {
    /// Resolve the `Seed`'s address by calling [`tokio::net::lookup_host`].
    ///
    /// # Errors
    ///
    /// If the addresses returned by `lookup_host` are empty, this will result
    /// in an [`error::Resolve::DnsLookupFailed`].
    pub async fn resolve(&self) -> Result<Seed<Vec<SocketAddr>>, error::Resolve>
    where
        T: Clone + ToSocketAddrs + fmt::Display,
    {
        let addrs = lookup_host(self.addrs.clone()).await?.collect::<Vec<_>>();
        if !addrs.is_empty() {
            Ok(Seed {
                peer: self.peer,
                addrs,
                label: self.label.clone(),
            })
        } else {
            Err(error::Resolve::DnsLookupFailed {
                peer: self.peer,
                addr: self.addrs.to_string(),
            })
        }
    }
}

/// A list of [`Seed`]s that have been resolved.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seeds(pub Vec<Seed<Vec<SocketAddr>>>);

impl Seeds {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Load and resolve the [`Seeds`] from the given `store`.
    ///
    /// If `cutoff` is given then only that number of seeds will be retrieved
    /// from the store and resolved.
    ///
    /// If any seeds failed to be resolved they will be returned alongside the
    /// successful seeds.
    pub async fn load<S, T>(
        store: &S,
        cutoff: impl Into<Option<usize>>,
    ) -> Result<(Seeds, Vec<error::Load>), S::Scan>
    where
        S: Store<Addrs = T>,
        S::Iter: std::error::Error + Send + Sync + 'static,
        T: Clone + fmt::Display + FromStr + ToSocketAddrs,
        T::Err: std::error::Error + Send + Sync + 'static,
    {
        let mut resolved = Vec::new();
        let mut failures = Vec::new();
        let cutoff = cutoff.into();

        for seed in store.scan()? {
            match seed {
                Err(err) => failures.push(error::Load::MalformedSeed(Box::new(err))),
                Ok(seed) => match seed.resolve().await {
                    Ok(r) => {
                        resolved.push(r);
                        if Some(resolved.len()) == cutoff {
                            return Ok((Self(resolved), failures));
                        }
                    },
                    Err(err) => failures.push(err.into()),
                },
            }
        }

        Ok((Self(resolved), failures))
    }

    /// Build up the list of [`Seed`]s, resolving their network addresses.
    ///
    /// If any seeds failed to be resolved they will be returned alongside the
    /// successful seeds.
    pub async fn resolve(
        seeds: impl ExactSizeIterator<Item = &Seed<String>>,
    ) -> (Self, Vec<error::Resolve>) {
        let mut resolved = Vec::with_capacity(seeds.len());
        let mut failures = Vec::new();

        for seed in seeds {
            match seed.resolve().await {
                Ok(r) => resolved.push(r),
                Err(err) => failures.push(err),
            }
        }

        (Self(resolved), failures)
    }
}

impl<'a> IntoIterator for &'a Seeds {
    type Item = &'a Seed<Vec<SocketAddr>>;

    type IntoIter = std::slice::Iter<'a, Seed<Vec<SocketAddr>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.as_slice().iter()
    }
}

impl Extend<Seed<Vec<SocketAddr>>> for Seeds {
    fn extend<T: IntoIterator<Item = Seed<Vec<SocketAddr>>>>(&mut self, iter: T) {
        self.0.extend(iter)
    }
}

impl TryFrom<Seeds> for discovery::Static {
    type Error = io::Error;

    fn try_from(seeds: Seeds) -> Result<Self, Self::Error> {
        discovery::Static::resolve(
            seeds
                .0
                .iter()
                .map(|seed| (seed.peer, seed.addrs.as_slice())),
        )
    }
}

pub mod error {
    use std::io;
    use thiserror::Error;

    use librad::{crypto::peer, PeerId};

    #[derive(Debug, Error)]
    pub enum Load {
        #[error("found seed that is malformed, expected `<peer>,<addr>[,<label>]`")]
        MalformedSeed(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

        #[error(transparent)]
        Resolve(#[from] Resolve),
    }

    #[derive(Debug, Error)]
    pub enum Parse {
        #[error("missing component {0}")]
        Missing(&'static str),

        #[error("failed to parse seed address")]
        Addr(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

        #[error(transparent)]
        Peer(#[from] peer::conversion::Error),

        #[error("unexptected component found in seed `{0}`")]
        Unexpected(String),
    }

    #[derive(Debug, Error)]
    pub enum Resolve {
        #[error("address `{addr}` for peer `{peer}` could be not be resolved")]
        DnsLookupFailed { peer: PeerId, addr: String },

        #[error(transparent)]
        Io(#[from] io::Error),
    }
}
