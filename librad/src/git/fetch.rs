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

// clippy doesn't know GATs yet
#![allow(clippy::needless_lifetimes)]

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    net::SocketAddr,
    ops::Deref,
};

use git_ext as ext;
use multihash::Multihash;

use super::{
    p2p::url::GitUrl,
    refs::Refs,
    storage::{self, Storage},
    types::{namespace::Namespace, AsRefspec, AsRemote, Force, Reference},
};
use crate::{
    identities::{
        git,
        urn::{HasProtocol, Urn},
    },
    peer::PeerId,
    signer::Signer,
};

/// Seed value to compute the fetchspecs for the desired fetch phase from.
///
/// See also: [`super::replication::replicate`]
#[derive(Debug)]
pub enum Fetchspecs<P, R> {
    /// Only request the branches necessary for identity verification.
    Peek,

    /// Request the `rad/signed_refs` branches of the given set of tracked
    /// peers.
    SignedRefs { tracked: BTreeSet<P> },

    /// Request the remote heads matching the signed refs of the respective
    /// tracked peers, as well as top-level delegates found in the identity
    /// document.
    Replicate {
        tracked_sigrefs: BTreeMap<P, Refs>,
        delegates: BTreeSet<Urn<R>>,
    },
}

impl<P, R> Fetchspecs<P, R>
where
    P: Clone + Ord + PartialEq + 'static,
    for<'a> &'a P: AsRemote + Into<ext::RefLike>,

    R: HasProtocol + Clone + 'static,
    for<'a> &'a R: Into<Multihash>,
{
    pub fn refspecs(
        &self,
        urn: &Urn<R>,
        remote_peer: P,
        remote_heads: &RemoteHeads,
    ) -> Vec<Box<dyn AsRefspec>> {
        match self {
            Self::Peek => refspecs::peek(urn, remote_peer),
            Self::SignedRefs { tracked } => refspecs::signed_refs(urn, &remote_peer, tracked),
            Self::Replicate {
                tracked_sigrefs,
                delegates,
            } => refspecs::replicate(urn, &remote_peer, remote_heads, tracked_sigrefs, delegates),
        }
    }
}

pub mod refspecs {
    use super::*;

    pub fn peek<P, R>(urn: &Urn<R>, remote_peer: P) -> Vec<Box<dyn AsRefspec>>
    where
        P: Clone + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let namespace: Namespace<R> = Namespace::from(urn);

        let rad_id = Reference::rad_id(namespace.clone());
        let rad_self = Reference::rad_self(namespace.clone(), None);
        let rad_ids = Reference::rad_ids_glob(namespace);

        vec![
            rad_id
                .set_remote(remote_peer.clone())
                .refspec(rad_id, Force::False)
                .boxed(),
            rad_self
                .set_remote(remote_peer.clone())
                .refspec(rad_self, Force::False)
                .boxed(),
            rad_ids
                .set_remote(remote_peer)
                .refspec(rad_ids, Force::False)
                .boxed(),
        ]
    }

    pub fn signed_refs<P, R>(
        urn: &Urn<R>,
        remote_peer: &P,
        tracked: &BTreeSet<P>,
    ) -> Vec<Box<dyn AsRefspec>>
    where
        P: Clone + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        tracked
            .iter()
            .map(|tracked_peer| {
                let local = Reference::rad_signed_refs(Namespace::from(urn), tracked_peer.clone());
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                local.refspec(remote, Force::False).boxed()
            })
            .collect()
    }

    pub fn replicate<P, R>(
        urn: &Urn<R>,
        remote_peer: &P,
        remote_heads: &RemoteHeads,
        tracked_sigrefs: &BTreeMap<P, Refs>,
        delegates: &BTreeSet<Urn<R>>,
    ) -> Vec<Box<dyn AsRefspec>>
    where
        P: Clone + Ord + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let mut signed = tracked_sigrefs
            .iter()
            .map(|(tracked_peer, tracked_sigrefs)| {
                let namespace = Namespace::from(urn);
                tracked_sigrefs
                    .heads
                    .iter()
                    .filter_map(move |(name, target)| {
                        let name_namespaced =
                        // Either the signed ref is in the "owned" section of
                        // `remote_peer`'s repo...
                        if tracked_peer == remote_peer {
                            reflike!("refs/namespaces")
                            .join(&namespace)
                            .join(ext::Qualified::from(name.clone()))
                        // .. or `remote_peer` is tracking `tracked_peer`, in
                        // which case it is in the remotes section.
                        } else {
                            reflike!("refs/namespaces")
                                .join(&namespace)
                                .join(reflike!("refs/remotes"))
                                .join(tracked_peer)
                                // Nb.: `name` is `OneLevel`, but we are in the
                                // remote tracking branches, so we need `heads`
                                // Like `Qualified::from(name).strip_prefix("refs")`
                                .join(reflike!("heads")).join(name.clone())
                        };

                        tracing::trace!("looking for {} in {:?}", name_namespaced, **remote_heads);

                        // Only include the advertised ref if its target OID
                        // is the same as the signed one.
                        let targets_match = remote_heads
                            .get(&name_namespaced)
                            .map(|remote_target| remote_target == &*target)
                            .unwrap_or(false);

                        targets_match.then_some({
                            let local = Reference::head(
                                namespace.clone(),
                                tracked_peer.clone(),
                                name.clone().into(),
                            );
                            let remote = if tracked_peer == remote_peer {
                                local.set_remote(None)
                            } else {
                                local.clone()
                            };

                            local.refspec(remote, Force::True).boxed()
                        })
                    })
            })
            .flatten()
            .collect::<Vec<_>>();

        // Peek at the remote peer
        let mut peek_remote = peek(urn, remote_peer.clone());

        // Get id + signed_refs branches of top-level delegates.
        // **Note**: we don't know at this point whom we should track in the
        // context of the delegate, so we just try to get at the signed_refs of
        // whomever we're tracking for `urn`.
        let mut delegates = delegates
            .iter()
            .map(|delegate_urn| {
                let mut peek = peek(delegate_urn, remote_peer.clone());
                peek.extend(signed_refs(
                    delegate_urn,
                    remote_peer,
                    &tracked_sigrefs.keys().cloned().collect(),
                ));

                peek
            })
            .flatten()
            .collect::<Vec<_>>();

        signed.append(&mut peek_remote);
        signed.append(&mut delegates);
        signed
    }
}

#[derive(Default)]
pub struct RemoteHeads(BTreeMap<ext::RefLike, ext::Oid>);

impl Deref for RemoteHeads {
    type Target = BTreeMap<ext::RefLike, ext::Oid>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<BTreeMap<ext::RefLike, ext::Oid>> for RemoteHeads {
    fn from(map: BTreeMap<ext::RefLike, ext::Oid>) -> Self {
        Self(map)
    }
}

/// Types which can create a [`Fetcher`].
///
/// This is an experimental trait to gauge if [`Storage`] capabilities could be
/// modelled entirely in terms of traits.
pub trait CanFetch {
    type Error;
    type Fetcher<'a>: Fetcher;

    fn fetcher<'a, Addrs>(
        &'a self,
        urn: git::Urn,
        remote_peer: PeerId,
        addr_hints: Addrs,
    ) -> Result<Self::Fetcher<'a>, Self::Error>
    where
        Addrs: IntoIterator<Item = SocketAddr>;
}

impl<S> CanFetch for Storage<S>
where
    S: Signer,
{
    type Error = storage::Error;
    type Fetcher<'a> = DefaultFetcher<'a>;

    fn fetcher<'a, Addrs>(
        &'a self,
        urn: git::Urn,
        remote_peer: PeerId,
        addr_hints: Addrs,
    ) -> Result<DefaultFetcher<'a>, Self::Error>
    where
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        Ok(DefaultFetcher::new(self, urn, remote_peer, addr_hints)?)
    }
}

pub struct FetchResult {
    pub updated_tips: BTreeMap<ext::RefLike, ext::Oid>,
}

/// Types which can process [`Fetchspecs`], and update the local [`Storage`]
/// accordingly.
pub trait Fetcher {
    type Error;
    type PeerId;
    type UrnId;

    fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<Self::PeerId, Self::UrnId>,
    ) -> Result<FetchResult, Self::Error>;
}

/// The default [`Fetcher`], which uses the peer-to-peer network for fetching.
pub struct DefaultFetcher<'a> {
    urn: git::Urn,
    remote_peer: PeerId,
    remote_heads: RemoteHeads,
    remote: git2::Remote<'a>,
}

impl<'a> DefaultFetcher<'a> {
    #[tracing::instrument(skip(storage, addr_hints), err)]
    pub fn new<S, Addrs>(
        storage: &'a Storage<S>,
        urn: git::Urn,
        remote_peer: PeerId,
        addr_hints: Addrs,
    ) -> Result<Self, git2::Error>
    where
        S: Signer,
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        let mut remote = storage.as_raw().remote_anonymous(
            &GitUrl {
                local_peer: PeerId::from_signer(storage.signer()),
                remote_peer,
                repo: urn.id,
                addr_hints: addr_hints.into_iter().collect(),
            }
            .to_string(),
        )?;
        remote.connect(git2::Direction::Fetch)?;
        let remote_heads = remote
            .list()?
            .iter()
            .filter_map(|remote_head| match remote_head.symref_target() {
                Some(_) => None,
                None => match ext::RefLike::try_from(remote_head.name()) {
                    Ok(refname) => Some((refname, remote_head.oid().into())),
                    Err(e) => {
                        tracing::warn!("invalid refname `{}`: {}", remote_head.name(), e);
                        None
                    },
                },
            })
            .collect::<BTreeMap<_, _>>()
            .into();

        Ok(Self {
            urn,
            remote_peer,
            remote_heads,
            remote,
        })
    }

    #[tracing::instrument(skip(self), err)]
    pub fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<PeerId, git::Revision>,
    ) -> Result<FetchResult, git2::Error> {
        {
            let refspecs = fetchspecs
                .refspecs(&self.urn, self.remote_peer, &self.remote_heads)
                .into_iter()
                .map(|spec| spec.as_refspec())
                .collect::<Vec<_>>();
            tracing::trace!("{:?}", refspecs);

            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.transfer_progress(|prog| {
                tracing::trace!("Fetch: received {} bytes", prog.received_bytes());
                true
            });

            self.remote.download(
                &refspecs,
                Some(
                    git2::FetchOptions::new()
                        .prune(git2::FetchPrune::On)
                        .update_fetchhead(false)
                        .download_tags(git2::AutotagOption::None)
                        .remote_callbacks(callbacks),
                ),
            )?;
        }

        let mut updated_tips = BTreeMap::new();
        self.remote.update_tips(
            Some(git2::RemoteCallbacks::new().update_tips(|name, old, new| {
                tracing::debug!("Fetch: updating tip {}: {} -> {}", name, old, new);
                match ext::RefLike::try_from(name) {
                    Ok(refname) => {
                        updated_tips.insert(refname, new.into());
                    },
                    Err(e) => tracing::warn!("invalid refname `{}`: {}", name, e),
                }

                true
            })),
            false,
            git2::AutotagOption::None,
            Some(&format!("updated from {}", self.remote_peer)),
        )?;

        Ok(FetchResult { updated_tips })
    }
}

impl Fetcher for DefaultFetcher<'_> {
    type Error = git2::Error;
    type PeerId = PeerId;
    type UrnId = git::Revision;

    fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<Self::PeerId, Self::UrnId>,
    ) -> Result<FetchResult, Self::Error> {
        self.fetch(fetchspecs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    use crate::identities::urn::tests::FakeId;

    lazy_static! {
        // "PeerId"s
        static ref LOLEK: ext::RefLike = reflike!("lolek");
        static ref BOLEK: ext::RefLike = reflike!("bolek");
        static ref TOLA: ext::RefLike = reflike!("tola");

        // "URN"s
        static ref PROJECT_URN: Urn<FakeId> = Urn::new(FakeId(32));
        static ref LOLEK_URN: Urn<FakeId> = Urn::new(FakeId(1));
        static ref BOLEK_URN: Urn<FakeId> = Urn::new(FakeId(2));

        // namespaces
        static ref PROJECT_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*PROJECT_URN);
        static ref LOLEK_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*LOLEK_URN);
        static ref BOLEK_NAMESPACE: ext::RefLike = reflike!("refs/namespaces").join(&*BOLEK_URN);
    }

    #[test]
    fn peek_looks_legit() {
        let specs = Fetchspecs::Peek.refspecs(&*PROJECT_URN, TOLA.clone(), &Default::default());
        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.as_refspec())
                .collect::<Vec<_>>(),
            [
                (
                    refspec_pattern!("refs/rad/id"),
                    refspec_pattern!("refs/remotes/tola/rad/id")
                ),
                (
                    refspec_pattern!("refs/rad/self"),
                    refspec_pattern!("refs/remotes/tola/rad/self")
                ),
                (
                    refspec_pattern!("refs/rad/ids/*"),
                    refspec_pattern!("refs/remotes/tola/rad/ids/*")
                )
            ]
            .iter()
            .cloned()
            .map(|(remote, local)| format!(
                "{}:{}",
                PROJECT_NAMESPACE.with_pattern_suffix(remote),
                PROJECT_NAMESPACE.with_pattern_suffix(local),
            ))
            .collect::<Vec<_>>()
        )
    }

    #[test]
    fn signed_refs_looks_legit() {
        let specs = Fetchspecs::SignedRefs {
            tracked: [&*LOLEK, &*BOLEK]
                .iter()
                .cloned()
                .cloned()
                .collect::<BTreeSet<ext::RefLike>>(),
        }
        .refspecs(&*PROJECT_URN, TOLA.clone(), &Default::default());

        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.as_refspec())
                .collect::<Vec<_>>(),
            [
                (
                    reflike!("refs/remotes/bolek/rad/signed_refs"),
                    reflike!("refs/remotes/bolek/rad/signed_refs")
                ),
                (
                    reflike!("refs/remotes/lolek/rad/signed_refs"),
                    reflike!("refs/remotes/lolek/rad/signed_refs")
                )
            ]
            .iter()
            .cloned()
            .map(|(remote, local)| format!(
                "{}:{}",
                PROJECT_NAMESPACE.join(remote),
                PROJECT_NAMESPACE.join(local),
            ))
            .collect::<Vec<_>>()
        )
    }

    #[test]
    fn replicate_looks_legit() {
        use crate::git::refs::{Refs, Remotes};
        use std::collections::HashMap;

        lazy_static! {
            static ref ZERO: ext::Oid = ext::Oid::from(git2::Oid::zero());
        }

        let delegates = [LOLEK_URN.clone(), BOLEK_URN.clone()]
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();

        // Obviously, we have lolek and bolek's sigrefs
        let tracked_sigrefs = [
            (
                LOLEK.clone(),
                Refs {
                    heads: [(ext::OneLevel::from(reflike!("mister")), *ZERO)]
                        .iter()
                        .cloned()
                        .collect(),
                    remotes: Remotes::from_map(HashMap::new()),
                },
            ),
            (
                BOLEK.clone(),
                Refs {
                    heads: [
                        (ext::OneLevel::from(reflike!("mister")), *ZERO),
                        (ext::OneLevel::from(reflike!("next")), *ZERO),
                    ]
                    .iter()
                    .cloned()
                    .collect(),
                    remotes: Remotes::from_map(HashMap::new()),
                },
            ),
        ]
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>();

        // Tola is tracking PROJECT_URN, therefore she also has lolek and bolek
        let remote_heads = [
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/heads/mister")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/rad/id")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/rad/ids"))
                    .join(&*LOLEK_URN),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/rad/ids"))
                    .join(&*BOLEK_URN),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/remotes/lolek/heads/mister")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/remotes/bolek/heads/mister")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*PROJECT_URN)
                    .join(reflike!("refs/remotes/bolek/heads/next")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*LOLEK_URN)
                    .join(reflike!("refs/rad/id")),
                *ZERO,
            ),
            (
                reflike!("refs/namespaces")
                    .join(&*BOLEK_URN)
                    .join(reflike!("refs/rad/id")),
                *ZERO,
            ),
        ]
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>()
        .into();

        let specs = Fetchspecs::Replicate {
            tracked_sigrefs,
            delegates,
        }
        .refspecs(&*PROJECT_URN, TOLA.clone(), &remote_heads);

        assert_eq!(
            specs
                .into_iter()
                .map(|spec| spec.as_refspec())
                .collect::<BTreeSet<String>>(),
            [
                // First, lolek + bolek's heads (forced)
                format!(
                    "+{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/mister")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/mister"))
                ),
                format!(
                    "+{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/next")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/bolek/heads/next"))
                ),
                format!(
                    "+{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/lolek/heads/mister")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/lolek/heads/mister"))
                ),
                // Tola's rad/*
                format!(
                    "{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/rad/id")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
                ),
                format!(
                    "{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/rad/self")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
                ),
                format!(
                    "{}:{}",
                    PROJECT_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                    PROJECT_NAMESPACE
                        .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
                ),
                // Tola's view of rad/* of lolek + bolek's top-level namespaces
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.join(reflike!("refs/rad/id")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
                ),
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.join(reflike!("refs/rad/self")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
                ),
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                    BOLEK_NAMESPACE
                        .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
                ),
                format!(
                    "{}:{}",
                    LOLEK_NAMESPACE.join(reflike!("refs/rad/id")),
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/id"))
                ),
                format!(
                    "{}:{}",
                    LOLEK_NAMESPACE.join(reflike!("refs/rad/self")),
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/self"))
                ),
                format!(
                    "{}:{}",
                    LOLEK_NAMESPACE.with_pattern_suffix(refspec_pattern!("refs/rad/ids/*")),
                    LOLEK_NAMESPACE
                        .with_pattern_suffix(refspec_pattern!("refs/remotes/tola/rad/ids/*"))
                ),
                // Bolek's signed_refs for BOLEK_URN
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs"))
                ),
                // Lolek's signed_refs for BOLEK_URN (because we're tracking him)
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs"))
                ),
                // Lolek's signed_refs for LOLEK_URN
                format!(
                    "{}:{}",
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs")),
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/lolek/rad/signed_refs"))
                ),
                // Bolek's signed_refs for LOLEK_URN (because we're tracking him)
                format!(
                    "{}:{}",
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs")),
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/bolek/rad/signed_refs"))
                ),
            ]
            .iter()
            .map(std::borrow::ToOwned::to_owned)
            .collect::<BTreeSet<String>>()
        )
    }
}
