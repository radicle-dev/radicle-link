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
    collections::HashMap,
    fmt::{self, Display},
    io,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use futures::Stream;
use multibase::Base;
use multihash::Multihash;
use percent_encoding::percent_encode;
use url::Url;

// FIXME: `Path`/`PathBuf` are poor types for use with `RadUrn` -- there's no
// equivalent to `OsStrExt::as_bytes()` on Windows.
use std::os::unix::ffi::OsStrExt;

use crate::{keys::device::Signature, peer::PeerId};

pub enum Protocol {
    Git,
    //Pijul,
}

impl Protocol {
    /// The "NSS" (namespace-specific string) of the [`Protocol`] in the context
    /// of a URN
    pub fn nss(&self) -> &str {
        match self {
            Self::Git => "git",
            //Self::Pijul => "pijul",
        }
    }
}

/// A `RadUrn` identifies a branch in a verifiabe `radicle-link` repository,
/// where:
///
/// * The repository is named `id`
/// * The backend / protocol is [`Protocol`]
/// * The initial (parent-less) revision of an identity document (defined by
///   [`Verifier`]) has the content address `id`
/// * There exists a branch named `rad/id` pointing to the most recent revision
///   of the identity document
/// * There MAY exist a branch named `path`
///
/// The textual representation of a `RadUrn` is of the form:
///
/// ```text
/// 'rad' ':' MULTIBASE(<id>) '/' <path>
/// ```
///
/// where the preferred base is `z-base32`.
///
/// For example: `rad:git:deadbeefdeaddeafbeef/rad/issues`
pub struct RadUrn {
    pub id: Multihash,
    pub proto: Protocol,
    pub path: PathBuf,
}

impl RadUrn {
    pub fn into_rad_url(self, peer: PeerId) -> RadUrl {
        RadUrl {
            authority: peer,
            urn: self,
        }
    }
}

impl Display for RadUrn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "rad:{}:{}/{}",
            self.proto.nss(),
            multibase::encode(Base::Base32Z, &self.id),
            self.path.to_str().unwrap()
        )
    }
}

/// A `RadUrl` is a URL with the scheme `rad://`.
///
/// The authority of a rad URL is a [`PeerId`], from which to retrieve the
/// `radicle-link` repository and branch identified by [`RadUrn`].
pub struct RadUrl {
    authority: PeerId,
    urn: RadUrn,
}

impl RadUrl {
    pub fn into_url(self) -> Url {
        self.into()
    }

    // TODO: like `Gossip::fetch`, we should also be able to open a `RadUrl` from
    // local storage:
    // pub fn open(&self) -> Result<impl Iterator<Item = Commit>, ??>
}

impl Into<Url> for RadUrl {
    fn into(self) -> Url {
        let mut url = Url::parse(&format!(
            "rad+{}://{}",
            self.urn.proto.nss(),
            self.authority.default_encoding()
        ))
        .unwrap();

        url.set_path(
            &percent_encode(
                Path::new(&multibase::encode(Base::Base32Z, self.urn.id))
                    .join(self.urn.path)
                    .as_os_str()
                    .as_bytes(),
                percent_encoding::CONTROLS,
            )
            .to_string(),
        );

        url
    }
}

/// Placeholder for a version in a history
type Version<'a> = &'a [u8];

/// Placeholder for the data passed to `Verifier::verify`
pub struct Rev<'a> {
    pub version: &'a Version<'a>,
    pub payload: &'a [u8],
}

/// A verification function for an identity history as named by a `RadUrn`.
///
/// The supplied iterator traverses the history in reverse order, i.e.
/// oldest-first.
///
/// In order to satisfy the verification requirements, `Verifier::verify` may
/// call `Core::fetch` recursively.
#[async_trait]
pub trait Verifier {
    type Error;

    async fn verify<'a>(
        history: Box<dyn Iterator<Item = Rev<'a>>>,
    ) -> Result<&'a Version<'a>, Self::Error>;
}

pub struct Refsig<'a> {
    pub refs: HashMap<&'a Path, &'a [u8]>,
    pub signature: Signature,
}

pub enum BrowseError {
    NotConnected,
}

#[async_trait]
pub trait Browse {
    type Stream: Stream<Item = RadUrn>;

    /// Given a known peer, ask it to enumerate all [`RadUrn`]s it knows about.
    ///
    /// This is an online query: if no connection to the peer exists, or could
    /// be established, an error is returned.
    async fn browse(&self, peer: &PeerId) -> Result<Self::Stream, BrowseError>;

    /// Peek at the most recent `Rev` of the `RadUrn`.
    ///
    /// See also [`Fetch::fetch`].
    async fn peek<'a>(&self, peer: &PeerId, urn: &RadUrn) -> Result<Rev<'a>, BrowseError>;
}

pub struct Have {
    pub entity: RadUrn,
    pub head: Multihash,
}

#[async_trait]
pub trait Gossip {
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
    NoSuchBranch(PathBuf),
    Io(io::Error),
    // ...
}

#[non_exhaustive]
pub enum ShallowFetchError {
    NoSuchBranch(PathBuf),
    Io(io::Error),
    // ...
}

#[async_trait]
pub trait Fetch {
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
    /// * After fetching, the `rad/id` branch is traversed to the first
    ///   (parent-less) revision, and it is verified that the content address of
    ///   the specified blob equals the `RadUrn`'s hash
    ///
    /// * The verification function is invoked, supplying an oldest-first
    ///   iterator over the history of the branch
    ///
    /// * If the verification function succeeds, the `urn`'s branch is reset to
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
