// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::collections::{BTreeMap, BTreeSet};

use git_ext as ext;
use multihash::Multihash;

use super::{Limit, RemoteHeads};
use crate::{
    git::{
        refs::Refs,
        types::{
            reference::{Reference, RefsCategory},
            AsRemote,
            Fetchspec,
            Force,
            Namespace,
            Refspec,
        },
    },
    identities::urn::{HasProtocol, Urn},
};

/// Seed value to compute the fetchspecs for the desired fetch phase from.
///
/// See also: [`crate::git::replication::replicate`]
#[derive(Debug)]
pub enum Fetchspecs<P, R> {
    /// Request all identity documents
    PeekAll { limit: Limit },

    /// Only request the branches necessary for identity verification.
    Peek { remotes: BTreeSet<P>, limit: Limit },

    /// Request the remote heads matching the signed refs of the respective
    /// tracked peers, as well as top-level delegates found in the identity
    /// document.
    Replicate {
        tracked_sigrefs: BTreeMap<P, Refs>,
        delegates: BTreeSet<Urn<R>>,
        limit: Limit,
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
    ) -> Vec<Fetchspec> {
        match self {
            Self::PeekAll { .. } => {
                let mut all = refspecs::all(urn);
                let remote = Some(remote_peer.clone()).into_iter().collect();
                let mut remotes = refspecs::peek(urn, &remote_peer, &remote);
                all.append(&mut remotes);
                all
            },
            Self::Peek { remotes, .. } => refspecs::peek(urn, &remote_peer, remotes),
            Self::Replicate {
                tracked_sigrefs,
                delegates,
                ..
            } => refspecs::replicate(urn, &remote_peer, remote_heads, tracked_sigrefs, delegates),
        }
    }

    pub fn fetch_limit(&self) -> usize {
        match self {
            Fetchspecs::PeekAll { limit } => limit.peek,
            Fetchspecs::Peek { limit, .. } => limit.peek,
            Fetchspecs::Replicate { limit, .. } => limit.data,
        }
    }
}

pub mod refspecs {
    use super::*;

    pub fn all<P, R>(urn: &Urn<R>) -> Vec<Fetchspec>
    where
        P: Clone + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let namespace: Namespace<R> = Namespace::from(urn);
        let rad_id = Reference::rad_id(namespace.clone());
        let rad_self = Reference::rad_self(namespace.clone(), None);
        let rad_signed_refs = Reference::rad_signed_refs(namespace, None);

        vec![
            Refspec {
                src: remote_glob(rad_id.clone().with_remote(refspec_pattern!("*"))),
                dst: remote_glob(rad_id.with_remote(refspec_pattern!("*"))),
                force: Force::False,
            }
            .into_fetchspec(),
            Refspec {
                src: remote_glob(rad_self.clone().with_remote(refspec_pattern!("*"))),
                dst: remote_glob(rad_self.with_remote(refspec_pattern!("*"))),
                force: Force::False,
            }
            .into_fetchspec(),
            Refspec {
                src: remote_glob(rad_signed_refs.clone().with_remote(refspec_pattern!("*"))),
                dst: remote_glob(rad_signed_refs.with_remote(refspec_pattern!("*"))),
                force: Force::False,
            }
            .into_fetchspec(),
        ]
    }

    pub fn peek<P, R>(urn: &Urn<R>, remote_peer: &P, remotes: &BTreeSet<P>) -> Vec<Fetchspec>
    where
        P: Clone + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let namespace: Namespace<R> = Namespace::from(urn);

        let rad_id = Reference::rad_id(namespace.clone());
        let rad_self = Reference::rad_self(namespace.clone(), None);
        let rad_signed_refs = Reference::rad_signed_refs(namespace.clone(), None);
        let rad_ids = Reference::rad_ids_glob(namespace);

        let is_remote = |src: Reference<Namespace<R>, P, _>, remote: P| {
            if remote_peer == &remote {
                src
            } else {
                src.with_remote(remote)
            }
        };

        remotes
            .iter()
            .flat_map(|remote| {
                vec![
                    Refspec {
                        src: is_remote(rad_id.clone(), remote.clone()),
                        dst: rad_id.clone().with_remote(remote.clone()),
                        force: Force::False,
                    }
                    .into_fetchspec(),
                    Refspec {
                        src: is_remote(rad_self.clone(), remote.clone()),
                        dst: rad_self.clone().with_remote(remote.clone()),
                        force: Force::False,
                    }
                    .into_fetchspec(),
                    Refspec {
                        src: is_remote(rad_signed_refs.clone(), remote.clone()),
                        dst: rad_signed_refs.clone().with_remote(remote.clone()),
                        force: Force::False,
                    }
                    .into_fetchspec(),
                    Refspec {
                        src: if remote == remote_peer {
                            rad_ids.clone()
                        } else {
                            rad_ids.clone().with_remote(remote.clone())
                        },
                        dst: rad_ids.clone().with_remote(remote.clone()),
                        force: Force::False,
                    }
                    .into_fetchspec(),
                ]
            })
            .collect()
    }

    pub fn signed_refs<P, R>(urn: &Urn<R>, remote_peer: &P, tracked: &BTreeSet<P>) -> Vec<Fetchspec>
    where
        P: Clone + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        tracked
            .iter()
            .map(|tracked_peer| {
                let dst = Reference::rad_signed_refs(Namespace::from(urn), tracked_peer.clone());
                let src = if tracked_peer == remote_peer {
                    dst.clone().with_remote(None)
                } else {
                    dst.clone()
                };

                Refspec {
                    src,
                    dst,
                    force: Force::False,
                }
                .into()
            })
            .collect()
    }

    pub fn replicate<P, R>(
        urn: &Urn<R>,
        remote_peer: &P,
        remote_heads: &RemoteHeads,
        tracked_sigrefs: &BTreeMap<P, Refs>,
        delegates: &BTreeSet<Urn<R>>,
    ) -> Vec<Fetchspec>
    where
        P: Clone + Ord + PartialEq + 'static,
        for<'a> &'a P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let namespace = Namespace::from(urn);
        let mut signed = tracked_sigrefs
            .iter()
            .flat_map(|(tracked_peer, refs)| {
                sigrefs(
                    namespace.clone(),
                    remote_peer,
                    remote_heads,
                    tracked_peer,
                    refs,
                )
            })
            .collect::<Vec<_>>();

        // Peek at the remote peer
        let mut peek_remote = peek(
            urn,
            remote_peer,
            &Some(remote_peer.clone()).into_iter().collect(),
        );

        // Get id + signed_refs branches of top-level delegates.
        // **Note**: we don't know at this point whom we should track in the
        // context of the delegate, so we just try to get at the signed_refs of
        // whomever we're tracking for `urn`.
        let mut delegates = delegates
            .iter()
            .map(|delegate_urn| {
                let mut peek = peek(
                    delegate_urn,
                    remote_peer,
                    &Some(remote_peer.clone()).into_iter().collect(),
                );
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

    fn remote_glob<R>(
        r: Reference<Namespace<R>, ext::RefspecPattern, ext::RefLike>,
    ) -> ext::RefspecPattern
    where
        R: HasProtocol + Clone + 'static,
        for<'a> &'a R: Into<Multihash>,
    {
        let mut refl = reflike!("refs");

        if let Some(ref namespace) = r.namespace {
            refl = refl
                .join(reflike!("namespaces"))
                .join(namespace)
                .join(reflike!("refs"));
        }

        let suffix: ext::RefLike = ext::RefLike::from(r.category).join(r.name.to_owned());
        let remote = r.remote.unwrap_or(refspec_pattern!("*"));
        refl.join(reflike!("remotes"))
            .with_pattern_suffix(remote)
            .append(suffix)
    }

    fn sigrefs<'a, P, R>(
        namespace: Namespace<R>,
        remote_peer: &'a P,
        remote_heads: &'a RemoteHeads,
        tracked_peer: &'a P,
        refs: &'a Refs,
    ) -> impl Iterator<Item = Fetchspec> + 'a
    where
        P: Clone + PartialEq,
        for<'b> &'b P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol + Clone + 'a,
        for<'b> &'b R: Into<Multihash>,
    {
        refs.iter_categorised()
            .filter_map(move |((name, target), category)| {
                let namespaced_name = namespaced(
                    &namespace,
                    remote_peer,
                    tracked_peer,
                    name.clone(),
                    category,
                );
                // Only include the advertised ref if its target OID
                // is the same as the signed one.
                let targets_match = {
                    let found = remote_heads.get(&namespaced_name);
                    match found {
                        None => {
                            tracing::debug!("{} not found in remote heads", namespaced_name);
                            false
                        },

                        Some(remote_target) => {
                            if remote_target == &*target {
                                true
                            } else {
                                tracing::warn!(
                                    "{} target mismatch: expected {}, got {}",
                                    namespaced_name,
                                    target,
                                    remote_target
                                );
                                false
                            }
                        },
                    }
                };

                targets_match.then_some({
                    let dst = Reference {
                        remote: Some(tracked_peer.clone()),
                        category,
                        name: ext::RefLike::from(name.clone()),
                        namespace: Some(namespace.clone()),
                    };
                    let src = if tracked_peer == remote_peer {
                        dst.clone().with_remote(None)
                    } else {
                        dst.clone()
                    };

                    Refspec {
                        src,
                        dst,
                        force: Force::True,
                    }
                    .into_fetchspec()
                })
            })
    }

    fn namespaced<'a, P, R>(
        namespace: &'a Namespace<R>,
        remote_peer: &'a P,
        tracked_peer: &'a P,
        name: ext::OneLevel,
        cat: RefsCategory,
    ) -> ext::RefLike
    where
        P: PartialEq,
        for<'b> &'b P: AsRemote + Into<ext::RefLike>,

        R: HasProtocol,
        for<'b> &'b R: Into<Multihash>,
    {
        // Either the signed ref is in the "owned" section of
        // `remote_peer`'s repo...
        if tracked_peer == remote_peer {
            reflike!("refs/namespaces")
                .join(namespace)
                .join(name.into_qualified(cat.into()))
        // .. or `remote_peer` is tracking `tracked_peer`, in
        // which case it is in the remotes section.
        } else {
            reflike!("refs/namespaces")
                .join(namespace)
                .join(reflike!("refs/remotes"))
                .join(tracked_peer)
                // Nb.: `name` is `OneLevel`, but we are in the
                // remote tracking branches, so we need `heads`
                // Like `Qualified::from(name).strip_prefix("refs")`
                .join(ext::RefLike::from(cat))
                .join(name)
        }
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
        let specs = Fetchspecs::Peek {
            remotes: Some(TOLA.clone()).into_iter().collect(),
            limit: Default::default(),
        }
        .refspecs(&*PROJECT_URN, TOLA.clone(), &Default::default());
        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.to_string())
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
                    refspec_pattern!("refs/rad/signed_refs"),
                    refspec_pattern!("refs/remotes/tola/rad/signed_refs")
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
    fn replicate_looks_legit() {
        use crate::git::refs::{Refs, Remotes};

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
                    rad: Default::default(),
                    tags: Default::default(),
                    notes: Default::default(),
                    remotes: Remotes::new(),
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
                    rad: Default::default(),
                    tags: Default::default(),
                    notes: Default::default(),
                    remotes: Remotes::new(),
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
            limit: Default::default(),
        }
        .refspecs(&*PROJECT_URN, TOLA.clone(), &remote_heads);

        assert_eq!(
            specs
                .into_iter()
                .map(|spec| spec.to_string())
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
                format!(
                    "{}:{}",
                    PROJECT_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                    PROJECT_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs")),
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
                    BOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs")),
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
                    LOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                    LOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs"))
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
                format!(
                    "{}:{}",
                    BOLEK_NAMESPACE.join(reflike!("refs/rad/signed_refs")),
                    BOLEK_NAMESPACE.join(reflike!("refs/remotes/tola/rad/signed_refs"))
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
