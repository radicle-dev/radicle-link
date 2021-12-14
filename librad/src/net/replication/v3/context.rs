// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{BTreeSet, HashMap},
    convert::TryFrom,
    ops::Deref,
    time::Duration,
};

use bstr::BStr;
use data::NonEmpty;
use either::Either::*;
use link_replication::{
    io,
    namespace,
    oid,
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
    git::{self, refs, storage::Storage, tracking},
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
        Refs(#[from] refs::stored::Error),
    }

    #[derive(Debug, Error)]
    #[allow(clippy::large_enum_variant)]
    pub enum Connection {
        #[error(transparent)]
        Upgrade(#[from] upgrade::Error<quic::BidiStream>),

        #[error(transparent)]
        Quic(#[from] quic::Error),
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
        match refs::load(&self.store, &self.urn, Some(of))? {
            None => Ok(None),
            Some(refs::Loaded { at, refs: signed }) => {
                let refs = signed
                    .iter_categorised()
                    .map(|((name, oid), cat)| (format!("refs/{}/{}", cat, name).into(), *oid))
                    .collect::<HashMap<_, _>>();
                let mut remotes = refs::Refs::from(signed).remotes;
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
        match refs::load_at(&self.store, treeish.into().into(), Some(signed_by))? {
            None => Ok(None),
            Some(refs::Loaded { at, refs: signed }) => {
                let refs = signed
                    .iter_categorised()
                    .map(|((name, oid), cat)| (format!("refs/{}/{}", cat, name).into(), *oid))
                    .collect::<HashMap<_, _>>();
                let mut remotes = refs::Refs::from(signed).remotes;
                remotes.cutoff_mut(cutoff);
                let remotes = remotes.flatten().copied().collect();

                Ok(Some(Sigrefs { at, refs, remotes }))
            },
        }
    }

    fn update(&self) -> Result<Option<Self::Oid>, Self::Error> {
        use backoff::ExponentialBackoff;
        use refs::Updated::*;

        // XXX: let this be handled by `git-ref`
        let cfg = ExponentialBackoff {
            current_interval: Duration::from_millis(100),
            initial_interval: Duration::from_millis(100),
            max_interval: Duration::from_secs(1),
            ..Default::default()
        };
        backoff::retry(cfg, || {
            let op = refs::Refs::update(self.store, &self.urn)
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

impl Tracking for Context<'_> {
    type Urn = Urn;
    type Tracked = Tracked;
    type Error = tracking::Error;

    fn track(&mut self, id: &PeerId, urn: Option<&Self::Urn>) -> Result<bool, Self::Error> {
        // TODO: should track without data, as per rfc0699
        tracking::track(self.store, urn.unwrap_or(&self.urn), *id)
    }

    fn tracked(&self) -> Self::Tracked {
        let inner = Some(tracking::tracked(self.store, &self.urn));
        Tracked { inner }
    }
}

pub struct Tracked {
    inner: Option<Result<tracking::Tracked, tracking::Error>>,
}

impl Iterator for Tracked {
    type Item = Result<PeerId, tracking::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.take() {
            Some(res) => match res {
                // we got an iterator: advance it and put it back into inner
                Ok(mut iter) => {
                    let next = iter.next();
                    self.inner = Some(Ok(iter));
                    next.map(Ok)
                },
                // error setting up the iterator: yield the error once and
                // leave `None` in inner
                Err(e) => Some(Err(e)),
            },
            // the iterator setup error was aleady yielded
            None => None,
        }
    }
}

impl<'c> Refdb for Context<'c> {
    type Oid = <io::Refdb<io::Odb> as Refdb>::Oid;

    type FindError = <io::Refdb<io::Odb> as Refdb>::FindError;
    type TxError = <io::Refdb<io::Odb> as Refdb>::TxError;
    type ReloadError = <io::Refdb<io::Odb> as Refdb>::ReloadError;

    fn refname_to_id(
        &self,
        refname: impl AsRef<BStr>,
    ) -> Result<Option<Self::Oid>, Self::FindError> {
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
