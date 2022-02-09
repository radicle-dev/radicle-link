// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    convert::TryFrom,
    ops::Deref,
    time::Duration,
};

use data::NonEmpty;
use either::{Either, Either::*};
use git_ref_format::RefString;
use link_replication::{
    io,
    namespace,
    oid,
    refs,
    Applied,
    FilteredRef,
    Identities,
    LocalPeer,
    Namespace,
    Negotiation,
    Net,
    ObjectId,
    RefScan,
    Refdb,
    SignedRefs,
    Sigrefs,
    SkippedFetch,
    Tracking,
    Update,
    VerifiedIdentity,
};
use multihash::Multihash;
use std_ext::Void;

use crate::{
    git::{self, storage::Storage, tracking},
    identities::{
        self,
        git::{
            ContentId,
            Person,
            Project,
            Revision,
            SomeIdentity,
            VerifiedPerson,
            VerifiedProject,
        },
    },
    net::{self, quic, upgrade},
    PeerId,
};

pub mod error {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[allow(clippy::large_enum_variant)]
    pub enum Verification {
        #[error("unknown identity kind")]
        UnknownIdentityKind(Box<SomeIdentity>),

        #[error("delegate identity {0} not found")]
        MissingDelegate(identities::git::Urn),

        #[error(transparent)]
        Person(#[from] identities::error::VerifyPerson),

        #[error(transparent)]
        Project(#[from] identities::error::VerifyProject),

        #[error(transparent)]
        Load(#[from] identities::error::Load),

        #[error(transparent)]
        Git(#[from] git::identities::Error),
    }

    #[derive(Debug, Error)]
    pub enum Sigrefs {
        #[error("gave up due to high contention")]
        Contended,

        #[error(transparent)]
        Refs(#[from] git::refs::stored::Error),
    }

    #[derive(Debug, Error)]
    #[allow(clippy::large_enum_variant)]
    pub enum Connection {
        #[error(transparent)]
        Upgrade(#[from] upgrade::Error<quic::BidiStream>),

        #[error(transparent)]
        Quic(#[from] quic::Error),
    }

    #[derive(Debug, Error)]
    pub enum Tracking {
        #[error(transparent)]
        Track(#[from] tracking::error::Track),
        #[error(transparent)]
        Tracked(#[from] tracking::error::TrackedPeers),
    }
}

type Network = io::Network<Urn, io::Refdb<io::Odb>, io::Odb, quic::Connection>;

/// Context for a replication v3 run.
///
/// Implements the (effect) traits required by the `link-replication` crate.
pub struct Context<'a> {
    pub(super) urn: Urn,
    pub(super) store: &'a Storage,
    pub(super) refdb: io::Refdb<io::Odb>,
    pub(super) net: Network,
}

impl<'a> Context<'a> {
    fn verify<F, T>(
        &self,
        id: SomeIdentity,
        resolve: F,
    ) -> Result<SomeVerifiedIdentity, error::Verification>
    where
        F: Fn(&Urn) -> Option<T>,
        T: AsRef<oid>,
    {
        match id {
            SomeIdentity::Person(p) => {
                let verified = self
                    .store
                    .read_only()
                    .identities::<Person>()
                    .verify(*p.content_id)?;
                Ok(SomeVerifiedIdentity::Person(verified))
            },

            SomeIdentity::Project(p) => {
                let verified = self.store.read_only().identities::<Project>().verify(
                    *p.content_id,
                    |urn| {
                        let urn = Urn(urn);
                        resolve(&urn)
                            .map(|oid| git_ext::Oid::from(oid.as_ref().to_owned()).into())
                            .ok_or(error::Verification::MissingDelegate(urn.0))
                    },
                )?;
                Ok(SomeVerifiedIdentity::Project(verified))
            },

            unknown => Err(error::Verification::UnknownIdentityKind(Box::new(unknown))),
        }
    }
}

#[derive(Debug)]
pub enum SomeVerifiedIdentity {
    Person(VerifiedPerson),
    Project(VerifiedProject),
}

impl VerifiedIdentity for SomeVerifiedIdentity {
    type Rev = Revision;
    type Oid = ContentId;
    type Urn = Urn;

    fn revision(&self) -> Self::Rev {
        match self {
            Self::Person(p) => p.revision,
            Self::Project(p) => p.revision,
        }
    }

    fn content_id(&self) -> Self::Oid {
        match self {
            Self::Person(p) => p.content_id,
            Self::Project(p) => p.content_id,
        }
    }

    fn urn(&self) -> Self::Urn {
        match self {
            Self::Person(p) => p.urn(),
            Self::Project(p) => p.urn(),
        }
        .into()
    }

    fn delegate_ids(&self) -> NonEmpty<BTreeSet<PeerId>> {
        let ds = match self {
            Self::Person(p) => p
                .delegations()
                .into_iter()
                .copied()
                .map(PeerId::from)
                .collect(),

            Self::Project(p) => p
                .delegations()
                .into_iter()
                .flat_map(|d| match d {
                    Left(pk) => vec![PeerId::from(*pk)],
                    Right(indirect) => indirect
                        .delegations()
                        .into_iter()
                        .copied()
                        .map(PeerId::from)
                        .collect(),
                })
                .collect(),
        };

        NonEmpty::from_maybe_empty(ds).expect("delegations of a verified identity cannot be empty")
    }

    fn delegate_urns(&self) -> BTreeSet<Self::Urn> {
        if let Self::Project(p) = self {
            p.delegations()
                .into_iter()
                .indirect()
                .map(|i| Urn(i.urn()))
                .collect()
        } else {
            BTreeSet::new()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Urn(identities::git::Urn);

impl From<identities::git::Urn> for Urn {
    fn from(urn: identities::git::Urn) -> Self {
        Self(urn)
    }
}

impl Deref for Urn {
    type Target = identities::git::Urn;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl link_replication::Urn for Urn {
    type Error = identities::urn::error::DecodeId<<Revision as TryFrom<Multihash>>::Error>;

    fn try_from_id(s: impl AsRef<str>) -> Result<Self, Self::Error> {
        identities::git::Urn::try_from_id(s).map(Self)
    }

    fn encode_id(&self) -> String {
        self.0.encode_id()
    }
}

impl<'a> From<&'a Urn> for Namespace {
    fn from(urn: &'a Urn) -> Self {
        namespace::expand(&urn.encode_id()).expect("Urn yields a valid namespace")
    }
}

impl<'a> From<&'a Urn> for git_ref_format::Component<'_> {
    fn from(urn: &'a Urn) -> Self {
        git_ref_format::Component::from_refstring(refs::from_urn(urn))
            .expect("`Urn` is a valid ref component")
    }
}

impl Identities for Context<'_> {
    type Urn = Urn;
    type Oid = git_ext::Oid;

    type VerifiedIdentity = SomeVerifiedIdentity;
    type VerificationError = error::Verification;

    fn verify<H, F, T>(
        &self,
        head: H,
        resolve: F,
    ) -> Result<Self::VerifiedIdentity, Self::VerificationError>
    where
        H: AsRef<oid>,
        F: Fn(&Self::Urn) -> Option<T>,
        T: AsRef<oid>,
    {
        let id = self
            .store
            .read_only()
            .identities::<Void>()
            .some_identity(*git_ext::Oid::from(head.as_ref().to_owned()))?;
        self.verify(id, resolve)
    }

    fn newer(
        &self,
        a: Self::VerifiedIdentity,
        b: Self::VerifiedIdentity,
    ) -> Result<
        Self::VerifiedIdentity,
        link_replication::error::IdentityHistory<Self::VerifiedIdentity>,
    > {
        use link_replication::error::IdentityHistory as Error;
        use SomeVerifiedIdentity::*;

        match (a, b) {
            (Person(x), Person(y)) => self
                .store
                .read_only()
                .identities()
                .newer(x, y)
                .map(Person)
                .map_err(|e| Error::Other(Box::new(e))),
            (Project(x), Project(y)) => self
                .store
                .read_only()
                .identities()
                .newer(x, y)
                .map(Project)
                .map_err(|e| Error::Other(Box::new(e))),
            (x, y) => Err(Error::TypeMismatch { a: x, b: y }),
        }
    }
}

impl SignedRefs for Context<'_> {
    type Oid = git_ext::Oid;
    type Error = error::Sigrefs;

    fn load(&self, of: &PeerId, cutoff: usize) -> Result<Option<Sigrefs<Self::Oid>>, Self::Error> {
        match git::refs::load(&self.store, &self.urn, Some(of))? {
            None => Ok(None),
            Some(git::refs::Loaded { at, refs: signed }) => {
                let refs = signed
                    .iter_categorised()
                    .map(|((name, oid), cat)| {
                        // TODO: make `Refs` use `git_ref_format`
                        let refname = RefString::try_from(format!("refs/{}/{}", cat, name))
                            .expect("`Refs::iter_categorised` yields valid refnames");
                        (refname, *oid)
                    })
                    .collect::<HashMap<_, _>>();
                let mut remotes = git::refs::Refs::from(signed).remotes;
                remotes.cutoff_mut(cutoff);
                let remotes = remotes.flatten().copied().collect();

                Ok(Some(Sigrefs { at, refs, remotes }))
            },
        }
    }

    fn load_at(
        &self,
        treeish: impl Into<ObjectId>,
        signed_by: &PeerId,
        cutoff: usize,
    ) -> Result<Option<Sigrefs<Self::Oid>>, Self::Error> {
        match git::refs::load_at(&self.store, treeish.into().into(), Some(signed_by))? {
            None => Ok(None),
            Some(git::refs::Loaded { at, refs: signed }) => {
                let refs = signed
                    .iter_categorised()
                    .map(|((name, oid), cat)| {
                        // TODO: make `Refs` use `git_ref_format`
                        let refname = RefString::try_from(format!("refs/{}/{}", cat, name))
                            .expect("`Refs::iter_categorised` yields valid refnames");
                        (refname, *oid)
                    })
                    .collect::<HashMap<_, _>>();
                let mut remotes = git::refs::Refs::from(signed).remotes;
                remotes.cutoff_mut(cutoff);
                let remotes = remotes.flatten().copied().collect();

                Ok(Some(Sigrefs { at, refs, remotes }))
            },
        }
    }

    fn update(&self) -> Result<Option<Self::Oid>, Self::Error> {
        use backoff::ExponentialBackoff;
        use git::refs::Updated::*;

        // XXX: let this be handled by `git-ref`
        let cfg = ExponentialBackoff {
            current_interval: Duration::from_millis(100),
            initial_interval: Duration::from_millis(100),
            max_interval: Duration::from_secs(1),
            ..Default::default()
        };
        backoff::retry(cfg, || {
            let op = git::refs::Refs::update(self.store, &self.urn)
                .map_err(error::Sigrefs::from)
                .map_err(backoff::Error::Permanent);
            match op? {
                Updated { at, .. } | Unchanged { at, .. } => Ok(Some(at.into())),
                ConcurrentlyModified => Err(backoff::Error::Transient(error::Sigrefs::Contended)),
            }
        })
        .map_err(|e| match e {
            backoff::Error::Permanent(inner) => inner,
            backoff::Error::Transient(inner) => inner,
        })
    }
}

#[allow(clippy::type_complexity)]
impl<'a> Tracking for Context<'a> {
    type Urn = Urn;

    type Tracked = tracking::TrackedPeers<
        'a,
        <Storage as tracking::git::refdb::Read<'a>>::References,
        <Storage as tracking::git::refdb::Read<'a>>::IterError,
    >;
    type Updated = std::iter::Map<
        std::vec::IntoIter<tracking::batch::Updated>,
        fn(tracking::batch::Updated) -> Either<PeerId, Self::Urn>,
    >;

    type TrackedError = tracking::error::TrackedPeers;
    type TrackError = tracking::error::Batch;

    fn track<I>(&mut self, iter: I) -> Result<Self::Updated, Self::TrackError>
    where
        I: IntoIterator<Item = link_replication::TrackingRel<Self::Urn>>,
    {
        use link_replication::TrackingRel;
        use once_cell::sync::Lazy;
        use tracking::{
            batch::{Action, Applied, Updated::*},
            reference::{RefName, Remote},
            Ref,
        };

        static CONFIG_FULL: Lazy<tracking::Config> = Lazy::new(|| tracking::Config {
            data: true,
            cobs: tracking::config::Cobs::allow_all(),
        });
        static CONFIG_MIN: Lazy<tracking::Config> = Lazy::new(|| tracking::Config {
            data: false,
            cobs: tracking::config::Cobs::deny_all(),
        });

        let iter = iter.into_iter();
        let mut seen = BTreeSet::<Urn>::new();
        let act = iter.filter_map(|rel| match rel {
            TrackingRel::Delegation(Right(urn)) | TrackingRel::SelfRef(urn) => {
                (!seen.contains(&urn)).then(|| {
                    seen.insert(urn.clone());
                    Action::Track {
                        urn: Cow::from(urn.0),
                        peer: None,
                        config: &CONFIG_MIN,
                        policy: tracking::policy::Track::MustNotExist,
                    }
                })
            },

            TrackingRel::Delegation(Left(id)) => (!seen.contains(&self.urn)).then(|| {
                seen.insert(self.urn.clone());
                Action::Track {
                    urn: Cow::from(self.urn.deref()),
                    peer: Some(id),
                    config: &CONFIG_FULL,
                    policy: tracking::policy::Track::MustNotExist,
                }
            }),
        });
        let Applied { updates, .. } = tracking::batch(self.store, act)?;

        Ok(updates.into_iter().map(|up| match up {
            Tracked {
                reference:
                    Ref {
                        name: RefName { remote, urn },
                        ..
                    },
            } => match remote {
                Remote::Default => Right(urn.into_owned().into()),
                Remote::Peer(id) => Left(id),
            },

            Untracked { .. } => {
                unreachable!("`Action::Track` yielded `Updated::Untracked`")
            },
        }))
    }

    fn tracked(&self) -> Result<Self::Tracked, Self::TrackedError> {
        tracking::tracked_peers(self.store, Some(&self.urn))
    }
}

impl<'c> Refdb for Context<'c> {
    type Oid = <io::Refdb<io::Odb> as Refdb>::Oid;

    type FindError = <io::Refdb<io::Odb> as Refdb>::FindError;
    type TxError = <io::Refdb<io::Odb> as Refdb>::TxError;
    type ReloadError = <io::Refdb<io::Odb> as Refdb>::ReloadError;

    fn refname_to_id<'a, Q>(&self, refname: Q) -> Result<Option<Self::Oid>, Self::FindError>
    where
        Q: AsRef<refs::Qualified<'a>>,
    {
        self.refdb.refname_to_id(refname)
    }

    fn update<'a, I>(&mut self, updates: I) -> Result<Applied<'a>, Self::TxError>
    where
        I: IntoIterator<Item = Update<'a>>,
    {
        self.refdb.update(updates)
    }

    fn reload(&mut self) -> Result<(), Self::ReloadError> {
        self.refdb.reload()
    }
}

impl<'a> RefScan for &'a Context<'_> {
    type Oid = <&'a io::Refdb<io::Odb> as RefScan>::Oid;
    type Scan = <&'a io::Refdb<io::Odb> as RefScan>::Scan;
    type Error = <&'a io::Refdb<io::Odb> as RefScan>::Error;

    fn scan<O, P>(self, prefix: O) -> Result<Self::Scan, Self::Error>
    where
        O: Into<Option<P>>,
        P: AsRef<str>,
    {
        self.refdb.scan(prefix)
    }
}

#[async_trait(?Send)]
impl Net for Context<'_> {
    type Error = <Network as Net>::Error;

    async fn run_fetch<N, T>(
        &self,
        neg: N,
    ) -> Result<(N, Result<Vec<FilteredRef<T>>, SkippedFetch>), Self::Error>
    where
        N: Negotiation<T> + Send,
        T: Send + 'static,
    {
        self.net.run_fetch(neg).await
    }
}

#[async_trait]
impl io::Connection for quic::Connection {
    type Read = quic::RecvStream;
    type Write = quic::SendStream;
    type Error = error::Connection;

    async fn open_stream(&self) -> Result<(Self::Read, Self::Write), Self::Error> {
        use net::connection::Duplex as _;

        let bi = self.open_bidi().await?;
        let up = upgrade::upgrade(bi, upgrade::Git).await?;
        Ok(up.into_stream().split())
    }
}

impl LocalPeer for Context<'_> {
    fn id(&self) -> &PeerId {
        self.store.peer_id()
    }
}
