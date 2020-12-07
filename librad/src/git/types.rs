// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fmt::{self, Display},
    marker::PhantomData,
};

use git_ext as ext;

use crate::{
    git::{refs::Refs, sealed},
    hash::Hash,
    peer::PeerId,
    uri::RadUrn,
};

pub mod namespace;
pub mod reference;
pub mod remote;

pub use reference::{AsRemote, Many, Multiple, One, Reference, RefsCategory, Single};

/// A representation of git reference that is either under:
///   * `refs/heads`
///   * `refs/remotes/<origin>`
pub type FlatRef<R, C> = Reference<PhantomData<!>, R, C>;

impl<R> Display for FlatRef<R, One>
where
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefLike>::into(self).as_str())
    }
}

impl<'a, R> From<&'a FlatRef<R, One>> for ext::RefLike
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, One>) -> Self {
        match r.remote {
            None => ext::Qualified::from(r.name.clone()).into(),
            Some(ref remote) => reflike!("refs/remotes")
                .join(remote)
                .join(ext::OneLevel::from(r.name.clone())),
        }
    }
}

impl<'a, R> From<&'a FlatRef<R, One>> for ext::RefspecPattern
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, One>) -> Self {
        Into::<ext::RefLike>::into(r).into()
    }
}

impl<R> Display for FlatRef<R, Many>
where
    for<'a> &'a R: AsRemote,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(Into::<ext::RefspecPattern>::into(self).as_str())
    }
}

impl<'a, R> From<&'a FlatRef<R, Many>> for ext::RefspecPattern
where
    &'a R: AsRemote,
{
    fn from(r: &'a FlatRef<R, Many>) -> Self {
        let refl = match r.remote {
            None => reflike!("refs/heads"),
            Some(ref remote) => reflike!("refs/remotes").join(remote),
        };

        ext::RefspecPattern::try_from((*refl).join(&r.name)).unwrap()
    }
}

/// A representation of git reference that is under `refs/namespace/<namespace>`
pub type NamespacedRef<N, C> = Reference<N, PeerId, C>;

impl<N, C> NamespacedRef<N, C> {
    pub fn namespace(&self) -> &N {
        &self._namespace
    }
}

/// Whether we should force the overwriting of a reference or not.
#[derive(Debug, Clone)]
pub enum Force {
    /// We should overwrite.
    True,
    /// We should not overwrite.
    False,
}

impl Force {
    /// Convert the Force to its `bool` equivalent.
    fn as_bool(&self) -> bool {
        match self {
            Force::True => true,
            Force::False => false,
        }
    }
}

/// The data for creating a symbolic reference in a git repository.
pub struct SymbolicRef<S, T> {
    /// The new symbolic reference.
    pub source: S,
    /// The reference that already exists and we want to create symbolic
    /// reference of.
    pub target: T,
    /// Whether we should overwrite any pre-existing `source`.
    pub force: Force,
}

impl<S, T> SymbolicRef<S, T> {
    /// Create a symbolic reference of `target`, where the `source` is the newly
    /// created reference.
    ///
    /// # Errors
    ///
    ///   * If the `target` does not exist we won't create the symbolic
    ///     reference and we error early.
    ///   * If we could not create the new symbolic reference since the name
    ///     already exists. Note that this will not be the case if `Force::True`
    ///     is passed.
    pub fn create<'a>(&self, repo: &'a git2::Repository) -> Result<git2::Reference<'a>, git2::Error>
    where
        for<'b> &'b S: Into<ext::RefLike>,
        for<'b> &'b T: Into<ext::RefLike>,
    {
        let source = Into::<ext::RefLike>::into(&self.source);
        let target = Into::<ext::RefLike>::into(&self.target);

        let reflog_msg = &format!(
            "creating symbolic ref {} -> {}",
            source.as_str(),
            target.as_str()
        );
        tracing::info!("{}", reflog_msg);

        repo.find_reference(target.as_str()).and_then(|_| {
            repo.reference_symbolic(
                source.as_str(),
                target.as_str(),
                self.force.as_bool(),
                reflog_msg,
            )
        })
    }
}

/// Trait for creating "existential" [`Refspec`]s as trait objects.
pub trait AsRefspec: sealed::Sealed {
    fn as_refspec(&self) -> String;
}

pub struct Refspec<Local, Remote> {
    pub local: Local,
    pub remote: Remote,
    pub force: Force,
}

impl<L, R> Refspec<L, R>
where
    L: 'static,
    R: 'static,
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    pub fn boxed(self) -> Box<dyn AsRefspec> {
        Box::new(self)
    }
}

impl<L, R> AsRefspec for Refspec<L, R>
where
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    fn as_refspec(&self) -> String {
        self.to_string()
    }
}

impl<L, R> sealed::Sealed for Refspec<L, R> {}

impl<L, R> Display for Refspec<L, R>
where
    for<'a> &'a L: Into<ext::RefspecPattern>,
    for<'a> &'a R: Into<ext::RefspecPattern>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.force.as_bool() {
            f.write_str("+")?;
        }

        let remote = Into::<ext::RefspecPattern>::into(&self.remote);
        let local = Into::<ext::RefspecPattern>::into(&self.local);

        write!(f, "{}:{}", remote.as_str(), local.as_str())
    }
}

impl<N> Refspec<Reference<N, PeerId, Single>, Reference<N, PeerId, Single>>
where
    N: Clone,
{
    pub fn rad_signed_refs<'a>(
        namespace: N,
        remote_peer: PeerId,
        tracked: impl Iterator<Item = PeerId> + 'a,
    ) -> impl Iterator<Item = Self> + 'a
    where
        N: 'a,
    {
        tracked.map(move |peer| {
            let local = Reference::rad_signed_refs(namespace.clone(), peer);
            let remote = if peer == remote_peer {
                local.set_remote(None)
            } else {
                local.clone()
            };

            local.refspec(remote, Force::False)
        })
    }
}

impl Refspec<Reference<Hash, PeerId, Single>, Reference<Hash, PeerId, Single>> {
    pub fn fetch_heads<'a, E>(
        namespace: Hash,
        remote_heads: HashMap<String, git2::Oid>,
        tracked_peers: impl Iterator<Item = PeerId> + 'a,
        remote_peer: PeerId,
        rad_signed_refs_of: impl Fn(PeerId) -> Result<Refs, E>,
        certifiers_of: impl Fn(PeerId) -> Result<HashSet<RadUrn>, E>,
    ) -> Result<impl Iterator<Item = Box<dyn AsRefspec>> + 'a, E> {
        // FIXME: do this in constant memory
        let mut refspecs = Vec::new();

        for tracked_peer in tracked_peers {
            // Heads
            //
            // `+refs/namespaces/<namespace>/refs[/remotes/<peer>]/heads/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/heads/*`
            {
                let their_singed_rad_refs = rad_signed_refs_of(tracked_peer)?;
                for (name, target) in their_singed_rad_refs.heads {
                    // NB(kim): this is deprecated code, sparing myself the
                    // effort to go through `Into<RefLike>` for namespace and
                    // peer -- those are `unsafe_coerce` anyway

                    // Either the signed ref is in the "owned" section of
                    // `remote_peer`'s repo...
                    let name_namespaced = reflike!("refs/namespaces")
                        .join(namespace.clone())
                        .join(reflike!("refs/heads"))
                        .join(name.clone());

                    // .. or `remote_peer` is tracking `tracked_peer`, in which
                    // case it is in the remotes section.
                    let name_namespaced_remote = reflike!("refs/namespaces")
                        .join(namespace.clone())
                        .join(reflike!("refs/remotes"))
                        .join(tracked_peer)
                        .join(reflike!("heads"))
                        .join(name.clone());

                    let targets_match = remote_heads
                        .get(name_namespaced.as_str())
                        .or_else(|| remote_heads.get(name_namespaced_remote.as_str()))
                        .map(|remote_target| remote_target == &*target)
                        .unwrap_or(false);

                    if targets_match {
                        let local = Reference::head(
                            namespace.clone(),
                            tracked_peer,
                            ext::RefLike::try_from(name).unwrap(),
                        );
                        let remote = if tracked_peer == remote_peer {
                            local.set_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(local.refspec(remote, Force::True).boxed())
                    }
                }
            }

            // Id
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/id \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/id`
            {
                let local = Reference::rad_id(namespace.clone()).with_remote(tracked_peer);
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
            }

            // Self
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/self \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/self`
            {
                let local = Reference::rad_self(namespace.clone(), tracked_peer);
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
            }

            // Certifiers
            //
            // `refs/namespaces/<namespace>/refs[/remotes/<peer>]/rad/ids/* \
            // :refs/namespaces/<namespace>/refs/remotes/<peer>/rad/ids/*`
            {
                let local = Reference::rad_ids_glob(namespace.clone()).with_remote(tracked_peer);
                let remote = if tracked_peer == remote_peer {
                    local.set_remote(None)
                } else {
                    local.clone()
                };

                refspecs.push(local.refspec(remote, Force::False).boxed());
            }

            // Certifier top-level identities
            //
            // `refs/namespaces/<certifier>/refs[/remotes/<peer>]/rad/id \
            // :refs/namespaces/<certifier>/refs/remotes/<peer>/rad/id`
            //
            // and
            //
            // `refs/namespaces/<certifier>/refs[/remotes/<peer>]/rad/ids/* \
            // :refs/namespaces/<certifier>/refs/remotes/<peer>/rad/ids/*`
            {
                let their_certifiers = certifiers_of(tracked_peer)?;
                for urn in their_certifiers {
                    // id
                    {
                        let local = Reference::rad_id(urn.id.clone()).with_remote(tracked_peer);
                        let remote = if tracked_peer == remote_peer {
                            local.set_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(local.refspec(remote, Force::False).boxed());
                    }

                    // rad/ids/* of id
                    {
                        let local =
                            Reference::rad_ids_glob(urn.id.clone()).with_remote(tracked_peer);
                        let remote = if tracked_peer == remote_peer {
                            local.set_remote(None)
                        } else {
                            local.clone()
                        };

                        refspecs.push(local.refspec(remote, Force::False).boxed());
                    }
                }
            }
        }

        Ok(refspecs.into_iter())
    }
}
