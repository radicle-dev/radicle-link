use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use futures::Stream;
use multibase::Base;
use multihash::Multihash;
use sodiumoxide::crypto::sign::ed25519;
use url::Url;

use crate::peer::PeerId;

/// A `RadUrn` identifies a verifiable history in a version control system,
/// where:
///
/// * The repository is named `id`
/// * There exists a branch pointer in the repository to the most recent
///   revision named `path`
/// * The initial (parent-less) revision of blob `file` has the content address
///   `id`
///
/// The textual representation of a `RadUrn` is of the form:
///
///     'rad:' MULTIBASE(<id>) ':' <path> '#' <file>
///
/// where the preferred base is `z-base32`.
///
/// For example: `rad:deadbeefdeaddeafbeef/rad/project#project.json`
pub struct RadUrn {
    pub id: Multihash,
    pub path: PathBuf,
    pub file: Option<String>,
}

impl RadUrn {
    pub fn into_url(self, peer: &PeerId) -> Url {
        let mut url = Url::parse(&format!("rad://{}", peer.default_encoding())).unwrap();
        url.set_path(
            Path::new(&multibase::encode(Base::Base32Z, self.id))
                .join(self.path)
                .to_str()
                .unwrap(),
        );
        if let Some(file) = self.file {
            url.set_fragment(Some(&file));
        }
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
pub trait Verifier {
    type Error;

    fn verify<'a>(
        history: Box<dyn Iterator<Item = Rev<'a>>>,
    ) -> Result<&'a Version<'a>, Self::Error>;
}

pub struct Refsig<'a> {
    pub refs: HashMap<&'a Path, &'a [u8]>,
    pub signature: ed25519::Signature,
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

#[async_trait]
pub trait Fetch {
    type FetchError; // morally: FetchError<V::Error>, pending GATs
    type ShallowFetchError;

    /// Given a known `RadUrn` and a `Verifier` function, attempt to fetch the
    /// corresponding repository from the peer `PeerId`.
    ///
    /// Fetch proceeds as follows:
    ///
    /// * A peer is identified which claims to serve `urn`, or the one specified
    ///   by `peer`
    ///
    /// * The branch corresponding to `urn` is fetched
    ///
    ///     * If the repository already exists locally, the existing one is
    ///       used, otherwise a new one is created in a temporary location
    ///
    /// * Additionally, the branch `rad/refsig` is fetched
    ///
    /// * After fetching, the branch is traversed to the first (parent-less)
    ///   revision, and it is verified that the content address of the specified
    ///   blob equals the `RadUrn`'s hash
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
    async fn fetch<V: Verifier>(
        &self,
        peer: &PeerId,
        urn: &RadUrn,
        verifier: V,
    ) -> Result<(), Self::FetchError>;

    /// Fetch only the most recent version of [`RadUrn`], without verification.
    ///
    /// This proceeds similar to [`Self::fetch`], but only performs a "shallow
    /// clone" of all remote heads and remote tracking branches.
    async fn fetch_shallow(
        &self,
        peer: &PeerId,
        urn: &RadUrn,
    ) -> Result<(), Self::ShallowFetchError>;
}
