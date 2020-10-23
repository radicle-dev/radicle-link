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
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    net::SocketAddr,
};

use multihash::Multihash;
use radicle_git_ext as ext;

use crate::{
    git::{
        p2p::url::GitUrl,
        refs::Refs,
        storage2::Storage,
        types::{namespace::Namespace, AsRefspec, AsRemote, Force, Reference},
    },
    identities::{
        git,
        urn::{HasProtocol, Urn},
    },
    keys,
    peer::PeerId,
    signer::Signer,
};

pub enum Fetchspecs<P, R> {
    Peek,

    SignedRefs {
        tracked: BTreeSet<P>,
    },

    Replicate {
        remote_heads: BTreeMap<ext::RefLike, ext::Oid>,
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
    pub fn refspecs(&self, urn: &Urn<R>, remote_peer: P) -> Vec<Box<dyn AsRefspec>> {
        match self {
            Self::Peek => refspecs::peek(urn, remote_peer),

            Self::SignedRefs { tracked } => refspecs::signed_refs(urn, &remote_peer, tracked),

            Self::Replicate {
                remote_heads,
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
        remote_heads: &BTreeMap<ext::RefLike, ext::Oid>,
        tracked_sigrefs: &BTreeMap<P, Refs>,
        delegates: &BTreeSet<Urn<R>>,
    ) -> Vec<Box<dyn AsRefspec>>
    where
        P: Clone + Ord + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let namespace = Namespace::from(urn);

        let signed = tracked_sigrefs
            .iter()
            .map(|(tracked_peer, tracked_sigrefs)| {
                let mut refspecs = Vec::new();

                let heads = tracked_sigrefs.heads.iter().filter_map(|(name, target)| {
                    // Either the signed ref is in the "owned" section of
                    // `remote_peer`'s repo...
                    let name_namespaced = ext::RefLike::try_from("refs/namespaces")
                        .unwrap()
                        .join(&namespace)
                        .join(name.clone());

                    // .. or `remote_peer` is tracking `tracked_peer`, in
                    // which case it is in the remotes section.
                    let name_namespaced_remote = ext::RefLike::try_from("refs/namespaces")
                        .unwrap()
                        .join(&namespace)
                        .join(ext::RefLike::try_from("refs/remotes").unwrap())
                        .join(tracked_peer)
                        .join(ext::RefLike::try_from("heads").unwrap())
                        .join(name.clone());

                    // Only include the advertised ref if its target OID
                    // is the same as the signed one.
                    let targets_match = remote_heads
                        .get(&name_namespaced)
                        .or_else(|| remote_heads.get(&name_namespaced_remote))
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
                });
                refspecs.extend(heads);
                // Peek at the tracked peer, too
                refspecs.extend(peek(urn, tracked_peer.clone()));

                refspecs
            })
            .flatten();

        // Get id + signed_refs branches of top-level delegates
        let delegates = delegates
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
            .flatten();

        signed.chain(delegates).collect()
    }
}

pub struct FetchResult {
    pub remote_heads: BTreeMap<ext::RefLike, ext::Oid>,
    pub updated_tips: BTreeMap<ext::RefLike, ext::Oid>,
}

pub trait Fetcher {
    type Error;
    type PeerId;
    type UrnId;

    fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<Self::PeerId, Self::UrnId>,
    ) -> Result<FetchResult, Self::Error>;
}

pub struct DefaultFetcher<'a> {
    urn: git::Urn,
    remote_peer: PeerId,
    remote: git2::Remote<'a>,
}

impl<'a> DefaultFetcher<'a> {
    pub fn new<S, Addrs>(
        storage: &'a Storage<S>,
        urn: git::Urn,
        remote_peer: PeerId,
        addr_hints: Addrs,
    ) -> Result<Self, git2::Error>
    where
        S: Signer,
        S::Error: keys::SignError,
        Addrs: IntoIterator<Item = SocketAddr>,
    {
        let remote = storage.as_raw().remote_anonymous(
            &GitUrl {
                local_peer: PeerId::from_signer(storage.signer()),
                remote_peer,
                repo: urn.id,
                addr_hints: addr_hints.into_iter().collect(),
            }
            .to_string(),
        )?;

        Ok(Self {
            urn,
            remote_peer,
            remote,
        })
    }

    pub fn fetch(
        &mut self,
        fetchspecs: Fetchspecs<PeerId, git::Revision>,
    ) -> Result<FetchResult, git2::Error> {
        let span = tracing::info_span!("DefaultFetcher::fetch");
        let _guard = span.enter();

        if !self.remote.connected() {
            self.remote.connect(git2::Direction::Fetch)?;
        }

        let remote_heads = self
            .remote
            .list()?
            .iter()
            .filter_map(|remote_head| match remote_head.symref_target() {
                Some(_) => None,
                None => match ext::RefLike::try_from(remote_head.name()) {
                    Ok(refname) => Some((refname, remote_head.oid().into())),
                    Err(e) => {
                        tracing::trace!("invalid refname `{}`: {}", remote_head.name(), e);
                        None
                    },
                },
            })
            .collect();

        let refspecs = fetchspecs.refspecs(&self.urn, self.remote_peer);
        {
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.transfer_progress(|prog| {
                tracing::trace!("Fetch: received {} bytes", prog.received_bytes());
                true
            });

            self.remote.download(
                &refspecs
                    .into_iter()
                    .map(|spec| spec.as_refspec())
                    .collect::<Vec<_>>(),
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

        Ok(FetchResult {
            remote_heads,
            updated_tips,
        })
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
        static ref LOLEK: ext::RefLike = ext::RefLike::try_from("lolek").unwrap();
        static ref BOLEK: ext::RefLike = ext::RefLike::try_from("bolek").unwrap();
        static ref TOLA: ext::RefLike = ext::RefLike::try_from("tola").unwrap();
        static ref URN_32: Urn<FakeId> = Urn::new(FakeId(32));
        static ref URN_32_REF: ext::RefLike = (&*URN_32).into();
    }

    #[test]
    fn peek_looks_legit() {
        let specs = Fetchspecs::Peek.refspecs(&*URN_32, LOLEK.clone());
        assert_eq!(
            &specs.iter().map(|spec| spec.as_refspec()).collect::<Vec<_>>(),
            &[
                format!(
                    "refs/namespaces/{}/refs/rad/id:refs/namespaces/{}/refs/remotes/lolek/rad/id",
                    URN_32_REF.as_str(),
                    URN_32_REF.as_str(),
                ),
                format!(
                    "refs/namespaces/{}/refs/rad/self:refs/namespaces/{}/refs/remotes/lolek/rad/self",
                    URN_32_REF.as_str(),
                    URN_32_REF.as_str(),
                ),
                format!(
                    "refs/namespaces/{}/refs/rad/ids/*:refs/namespaces/{}/refs/remotes/lolek/rad/ids/*",
                    URN_32_REF.as_str(),
                    URN_32_REF.as_str(),
                ),
            ]
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
        .refspecs(&*URN_32, TOLA.clone());

        assert_eq!(
            &specs.iter().map(|spec| spec.as_refspec()).collect::<Vec<_>>(),
            &[
                format!(
                    "refs/namespaces/{}/refs/remotes/bolek/rad/signed_refs:refs/namespaces/{}/refs/remotes/bolek/rad/signed_refs",
                    URN_32_REF.as_str(),
                    URN_32_REF.as_str(),
                ),
                format!(
                    "refs/namespaces/{}/refs/remotes/lolek/rad/signed_refs:refs/namespaces/{}/refs/remotes/lolek/rad/signed_refs",
                    URN_32_REF.as_str(),
                    URN_32_REF.as_str(),
                ),
            ]
        )
    }
}
