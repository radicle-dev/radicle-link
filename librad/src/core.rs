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

use std::{collections::HashMap, io};

use async_trait::async_trait;
use futures::Stream;
use multihash::Multihash;

use crate::{
    keys::device::Signature,
    peer::PeerId,
    uri::{Path, RadUrl, RadUrn},
};

/// A verification function for an identity history as named by a `RadUrn`.
///
/// The supplied iterator traverses the history in newest-first order.
///
/// In order to satisfy the verification requirements, `Verifier::verify` may
/// call `Core::fetch` recursively.
#[async_trait]
pub trait Verifier {
    type Identity;
    type Error;

    async fn verify<Commit>(
        history: Box<dyn Iterator<Item = Commit>>,
    ) -> Result<Self::Identity, Self::Error>;
}

pub struct Refsig<'a> {
    pub refs: HashMap<&'a Path, &'a [u8]>,
    pub signature: Signature,
}

pub enum BrowseError {
    NotConnected,
}

#[async_trait]
pub trait Browser {
    type Stream: Stream<Item = RadUrn>;

    /// Given a known peer, ask it to enumerate all [`RadUrn`]s it knows about.
    ///
    /// This is an online query: if no connection to the peer exists, or could
    /// be established, an error is returned.
    async fn browse(&self, peer: &PeerId) -> Result<Self::Stream, BrowseError>;
}

pub struct Have {
    pub entity: RadUrn,
    pub head: Multihash,
}

#[async_trait]
pub trait Gossiper {
    type QueryStream: Stream<Item = PeerId>;

    /// Announce an update to a local repository to the network.
    ///
    /// Nb.: the update refers to the "owned" branches of the repository, i.e.
    /// `refs/heads`. Precondition: `rad/refsigs` has been updated.
    async fn announce(&self, have: Have);

    /// Find peers on the network which provide [`RadUrn`].
    ///
    /// If `head` is given, restrict to peers who can provide this revision or
    /// later.
    ///
    /// The query may be answered from the local cached view of the network. The
    /// caller controls how many peers to fetch, and for how long, by either
    /// continuing to poll the `Stream` or dropping it.
    fn query(&self, urn: &RadUrn, head: &Multihash) -> Self::QueryStream;
}

#[non_exhaustive]
pub enum FetchError<V> {
    Verification(V),
    NoSuchBranch(Path),
    Io(io::Error),
    // ...
}

#[non_exhaustive]
pub enum ShallowFetchError {
    NoSuchBranch(Path),
    Io(io::Error),
    // ...
}

#[async_trait]
pub trait Fetcher {
    /// Iterator over the commit graph starting at the head of the branch
    /// specified by a [`RadUrl`].
    type Revwalk: Iterator;

    /// Given a known [`RadUrl`] and a [`Verifier`] function, attempt to fetch
    /// the corresponding repository from the URLs `authority` (peer).
    ///
    /// Fetch proceeds as follows:
    ///
    /// * A connection to the peer corresponding to the [`RadUrl`] is
    ///   established
    ///
    /// * The branches `rad/id` and `rad/refsig` are fetched
    ///
    ///     * If the repository already exists locally, the existing one is
    ///       used, otherwise a new one is created in a temporary location
    ///
    /// * The verification function is invoked, supplying a newest-first
    ///   iterator over the history of the branch
    ///
    /// * If the verification function succeeds, the `rad/id` branch is reset to
    ///   the returned `Version`
    ///
    /// * The `rad/refsig` branch is walked backwards (newest-first), at each
    ///   step inspecting the blob `refsig` of type `Refsig`. The branch is
    ///   reset to the most recent commit which yields a `Refsig`, which
    ///   contains a valid `signature` by the peer we're fetching from over the
    ///   `refs` field, encoded as an anonymous object in canonical JSON.
    ///
    /// * The branches specified by the `refs` of the most recent valid `Refsig`
    ///   are fetched from the remote peer, and reset to the respective heads.
    ///   Branches of that peer already present locally, but not included in
    ///   `refs`, are pruned.
    ///
    /// * Additionally, the remotes of the peer, as well as their remotes (2
    ///   degrees) are fetched.
    ///
    ///   To clarify:
    ///
    ///     * The peer we're fetching from is `A`, so we shall store everything
    ///       we fetch from it under `remotes/A`
    ///     * `A` itself may advertise remotes, such as:
    ///
    ///         remotes/B/remotes/C/remotes/D
    ///
    ///     * We shall fetch
    ///
    ///         remotes/A/remotes/B/remotes/C
    ///
    ///   Remote tracking branches of `A` present locally, but not on the remote
    ///   peer, are pruned.
    ///
    /// * Finally, the branch corresponding to [`RadUrn::path`] is looked up and
    ///   an [`Iterator`] over its commit graph is returned, or an error if the
    ///   branch doesn't exist. If the `path` was empty, `None` is returned.
    async fn fetch<V: Verifier>(
        &self,
        url: &RadUrl,
        verifier: V,
    ) -> Result<Option<Self::Revwalk>, FetchError<V::Error>>;

    /// Fetch only the most recent version of [`RadUrn`], without verification.
    ///
    /// This proceeds similar to [`Self::fetch`], but only performs a "shallow
    /// clone" of all remote heads and remote tracking branches.
    async fn fetch_shallow(&self, url: &RadUrl)
        -> Result<Option<Self::Revwalk>, ShallowFetchError>;
}
