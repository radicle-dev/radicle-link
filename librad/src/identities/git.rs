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

use std::{convert::TryFrom, marker::PhantomData};

use either::*;
use futures::executor::block_on;
use multihash::Multihash;

use crate::{
    git::ext,
    identities::{
        delegation::{self, Delegations},
        generic::{self, Signed, Verified},
        payload::{self, ProjectPayload, UserPayload},
        sign::{Signature, Signatures},
        urn,
    },
    internal::canonical::Cjson,
    keys::PublicKey,
    signer::Signer,
};

pub mod error;
pub mod iter;

pub use generic::Verifying;

mod load;
mod sign;

#[cfg(test)]
pub(crate) mod tests;

use iter::Iter;
use load::ByOid;

pub type Urn = urn::Urn<Revision>;

pub type Revision = ext::Oid;
pub type ContentId = ext::Oid;

pub type Doc<T, D> = generic::Doc<T, D, Revision>;
pub type Identity<T> = generic::Identity<T, Revision, ContentId>;

pub type SignedIdentity<T> = generic::Verifying<Identity<T>, Signed>;
pub type VerifiedIdentity<T> = generic::Verifying<Identity<T>, Verified>;

pub type UserDoc = Doc<UserPayload, delegation::Direct>;
pub type ProjectDoc = Doc<ProjectPayload, IndirectDelegation>;

pub type User = Identity<UserDoc>;
pub type Project = Identity<ProjectDoc>;

#[non_exhaustive]
pub enum SomeIdentity {
    User(User),
    Project(Project),
}

pub type SignedUser = SignedIdentity<UserDoc>;
pub type SignedProject = SignedIdentity<ProjectDoc>;

pub type VerifiedUser = VerifiedIdentity<UserDoc>;
pub type VerifiedProject = VerifiedIdentity<ProjectDoc>;

pub type VerificationError = generic::error::Verify<Revision, ContentId>;

pub type IndirectDelegation = delegation::Indirect<UserPayload, Revision, ContentId>;

#[derive(Clone)]
pub struct Identities<'a, T> {
    repo: &'a git2::Repository,
    _marker: PhantomData<T>,
}

impl<'a, T: 'a> From<&'a git2::Repository> for Identities<'a, T> {
    fn from(repo: &'a git2::Repository) -> Self {
        Self {
            repo,
            _marker: PhantomData,
        }
    }
}

impl<'a, T: 'a> Identities<'a, T> {
    /// Convenience to specialise `T` to [`User`].
    pub fn as_user(&self) -> Identities<'_, User> {
        Identities {
            repo: self.repo,
            _marker: PhantomData,
        }
    }

    /// Convenience to specialise `T` to [`Project`].
    pub fn as_project(&self) -> Identities<'_, Project> {
        Identities {
            repo: self.repo,
            _marker: PhantomData,
        }
    }

    /// Read an identity whose type is not statically known from commit `oid`.
    ///
    /// The only guarantee about the returned value is that it is well-formed --
    /// it may or may not pass verification.
    pub fn some_identity(&self, oid: git2::Oid) -> Result<SomeIdentity, error::Load> {
        SomeIdentity::try_from(self.by_oid(oid))
    }

    /// Traverse the history with head commit `head`, yielding identities of
    /// type `T`.
    ///
    /// The iterator yields elements in **reverse order**, ie. oldest-first. No
    /// verification is performed, but `Ok` elements are guaranteed to be
    /// well-formed.
    pub fn iter(
        &self,
        head: git2::Oid,
    ) -> Result<impl Iterator<Item = Result<T, error::Load>> + 'a, error::Load>
    where
        T: TryFrom<ByOid<'a>, Error = error::Load>,
    {
        Ok(Iter::new(self.repo, head)?
            .map(|item: Result<generic::Verifying<T, _>, _>| item.map(|v| v.into_inner())))
    }

    //// Generic methods ////

    pub fn get_generic(&self, oid: git2::Oid) -> Result<T, error::Load>
    where
        T: TryFrom<ByOid<'a>, Error = error::Load>,
    {
        T::try_from(self.by_oid(oid))
    }

    fn verify_generic<Doc>(
        &self,
        head: git2::Oid,
    ) -> Result<VerifiedIdentity<Doc>, VerificationError>
    where
        Doc: Delegations + generic::Replaces<Revision = Revision>,
        <Doc as Delegations>::Error: std::error::Error + Send + Sync + 'static,

        Identity<Doc>: TryFrom<ByOid<'a>, Error = error::Load>,
    {
        self.fold_verify_generic(head).map(|folded| folded.head)
    }

    fn fold_verify_generic<Doc>(
        &self,
        head: git2::Oid,
    ) -> Result<generic::Folded<Doc, Revision, ContentId>, VerificationError>
    where
        Doc: Delegations + generic::Replaces<Revision = Revision>,
        <Doc as Delegations>::Error: std::error::Error + Send + Sync + 'static,

        Identity<Doc>: TryFrom<ByOid<'a>, Error = error::Load>,
    {
        let mut progeny = Iter::<'_, Identity<Doc>>::new(self.repo, head)
            .map_err(generic::error::Verify::history)?;

        // TODO(kim): should we skip non-quorum commits at the beginning?
        let root = progeny
            .next()
            .ok_or(generic::error::Verify::EmptyHistory)?
            .map_err(generic::error::Verify::history)?
            .signed()?
            .quorum()?
            .verified(None)?;

        root.verify(progeny)
    }

    //// Helpers ////

    fn by_oid(&self, oid: git2::Oid) -> ByOid<'a> {
        (self.repo, oid)
    }

    fn is_in_ancestry_path(&self, commit: git2::Oid, tree: git2::Oid) -> Result<bool, git2::Error> {
        let mut revwalk = self.repo.revwalk()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        revwalk.push(commit)?;

        for oid in revwalk {
            let commit = self.repo.find_commit(oid?)?;
            if tree == commit.tree_id() {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl<'a, T: 'a> Identities<'a, Identity<T>>
where
    T: Delegations + generic::Replaces<Revision = Revision>,
    T::Error: std::error::Error + 'static,
    Identity<T>: TryFrom<ByOid<'a>, Error = error::Load>,
{
    /// Sign and commit some identity.
    pub fn create_from<S>(
        &self,
        theirs: SignedIdentity<T>,
        signer: &S,
    ) -> Result<Identity<T>, error::Store<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync,
    {
        let mut signatures = theirs.signatures.clone();
        {
            let sig = sign(signer, theirs.revision).map_err(error::Store::Signer)?;
            signatures.extend(Some(sig))
        }
        let content_id = self.commit(
            &format!(
                "Approved foreign identity {}, with content_id {} at revision {}",
                theirs.root, theirs.content_id, theirs.revision
            ),
            &signatures,
            theirs.revision,
            &[&theirs],
        )?;

        Ok(Identity {
            content_id,
            signatures,
            ..theirs.into_inner()
        })
    }

    /// Apply `theirs` to `ours`, and sign the result.
    ///
    /// This is like a merge of `theirs` into `ours` -- the resulting commit
    /// will have both `content_id`s as parents. The merge is subject to the
    /// following rules:
    ///
    /// 1. `ours` must already be signed by `signer` (otherwise it wouldn't be
    ///    "ours", isn't it?)
    /// 2. `ours.root` must equal `theirs.root`
    /// 3. If `theirs` is already a commit in the ancestry path of `ours`,
    ///    nothing is to be done, and `ours` is returned
    /// 4. If `ours` is already a commit in the ancestry path of `theirs`, and
    ///    `theirs` is already signed by `signer`, the merge is a fast-forward
    ///    (ie. a ref owned by us can just be set to `theirs.content_id`). In
    ///    this case, `theirs` is returned.
    /// 5. If `ours.revision == theirs.revision`, an "empty" commit is created,
    ///    signed by the union of both sets of signatures.
    /// 6. If `theirs` replaces `ours` (ie. `ours.revision ==
    ///    theirs.doc.replaces`), their revision is signed, and becomes the
    ///    revision of the result. Note that the result has only one
    ///    signature (by us).
    /// 7. Otherwise, there is no apparent relation between `ours` and `theirs`,
    ///    so an error is returned.
    pub fn update_from<S>(
        &self,
        ours: SignedIdentity<T>,
        theirs: SignedIdentity<T>,
        signer: &S,
    ) -> Result<Identity<T>, error::Merge<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync,
    {
        let ours = ours.into_inner();
        let theirs = theirs.into_inner();

        let our_pk = signer.public_key().into();

        enum Action {
            Uptodate,
            FastFwd,
            SlowFwd,
            SuccRev,
        }

        let action = {
            if !ours.signatures.contains_key(&our_pk) {
                Err(error::Merge::ForeignBase)
            } else if ours.root != theirs.root {
                Err(error::Merge::RootMismatch)
            } else if self
                .repo
                .graph_descendant_of(*ours.content_id, *theirs.content_id)?
            {
                Ok(Action::Uptodate)
            } else if theirs.signatures.contains_key(&our_pk)
                && self
                    .repo
                    .graph_descendant_of(*theirs.content_id, *ours.content_id)?
            {
                Ok(Action::FastFwd)
            } else if ours.revision == theirs.revision {
                Ok(Action::SlowFwd)
            } else if Some(&ours.revision) == theirs.doc.replaces() {
                Ok(Action::SuccRev)
            } else {
                Err(error::Merge::RevisionMismatch)
            }
        }?;

        match action {
            Action::Uptodate => Ok(ours),
            Action::FastFwd => Ok(theirs),
            Action::SlowFwd => {
                let mut signatures = ours.signatures.clone();
                signatures.extend(theirs.signatures.clone());

                let content_id = self.commit(
                    &format!("Updated signatures from {}", theirs.content_id),
                    &signatures,
                    ours.revision,
                    &[&ours],
                )?;

                Ok(Identity {
                    content_id,
                    signatures,
                    ..ours
                })
            },
            Action::SuccRev => {
                let mut signatures = theirs.signatures.clone();
                {
                    let sig = sign(signer, theirs.revision).map_err(error::Merge::Signer)?;
                    signatures.extend(Some(sig))
                }

                let content_id = self.commit(
                    &format!(
                        "Approved new revision `{}` from {}",
                        theirs.revision, theirs.content_id
                    ),
                    &signatures,
                    theirs.revision,
                    &[&ours, &theirs],
                )?;

                Ok(Identity {
                    content_id,
                    signatures,
                    ..theirs
                })
            },
        }
    }

    //// Helpers ////

    fn commit(
        &self,
        message: &str,
        signatures: &Signatures,
        revision: Revision,
        parents: &[&Identity<T>],
    ) -> Result<ContentId, git2::Error> {
        let tree = self.repo.find_tree(*revision)?;
        let parents = parents
            .iter()
            .map(|parent| self.repo.find_commit(*parent.content_id))
            .collect::<Result<Vec<_>, _>>()?;
        let author = self.repo.signature()?;

        self.repo
            .commit(
                None,
                &author,
                &author,
                &sign::CommitMessage::new(message, signatures).to_string(),
                &tree,
                parents.iter().collect::<Vec<_>>().as_slice(),
            )
            .map(ContentId::from)
    }
}

impl<'a> Identities<'a, User> {
    /// Attempt to read a [`User`] from commit `oid`, without verification.
    pub fn get(&self, oid: git2::Oid) -> Result<User, error::Load> {
        self.get_generic(oid)
    }

    /// Verify the user history with head commit `head`.
    ///
    /// The returned [`VerifiedUser`] is the **most recent** identity for which
    /// the verification succeeded -- which may or may not be `head`.
    pub fn verify(&self, head: git2::Oid) -> Result<VerifiedUser, error::VerifyUser> {
        Ok(self.verify_generic(head)?)
    }

    /// Create a new [`User`] from a payload and delegations.
    ///
    /// The returned [`User`] (and the underlying commit) will not have any
    /// parents, and will by signed by `signer`.
    pub fn create<S>(
        &self,
        payload: UserPayload,
        delegations: delegation::Direct,
        signer: &S,
    ) -> Result<User, error::Store<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let doc = Doc {
            version: 0,
            replaces: None,
            payload,
            delegations: payload::UserDelegations::from(delegations),
        };

        let root: Revision = self.repo.blob(&Cjson(&doc).canonical_form()?)?.into();
        let revision = {
            let mut builder = self.repo.treebuilder(None)?;
            builder.insert(root.to_string(), *root, 0o100_644)?;
            builder.write().map(Revision::from)
        }?;
        let signatures = sign(signer, revision).map_err(error::Store::Signer)?.into();
        let content_id = self.commit(
            &format!("Initialised user identity {}", root),
            &signatures,
            revision,
            &[],
        )?;

        Ok(Identity {
            content_id,
            root,
            revision,
            doc: doc.second(delegation::Direct::from),
            signatures,
        })
    }

    /// Update an existing [`SignedUser`] with a new payload and delegations.
    ///
    /// If both `payload` and `delegations` evaluate to `None`, or their values
    /// result in the same revision as `base`, no new commit is made, and
    /// the result is the unwrapped [`User`] of the `base` argument.
    ///
    /// Otherwise, the result is a new [`User`] whose parent is `base`.
    pub fn update<S>(
        &self,
        base: SignedUser,
        payload: impl Into<Option<UserPayload>>,
        delegations: impl Into<Option<delegation::Direct>>,
        signer: &S,
    ) -> Result<User, error::Store<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let payload = payload.into();
        let delegations = delegations.into();

        // Fast path
        if payload.is_none() && delegations.is_none() {
            return Ok(base.into_inner());
        }

        let doc = Doc {
            version: 0,
            replaces: Some(base.revision),
            payload: payload.unwrap_or_else(|| base.doc.payload.clone()),
            delegations: payload::UserDelegations::from(
                delegations.unwrap_or_else(|| base.doc.delegations.clone()),
            ),
        };

        let revision = {
            let doc_blob = self.repo.blob(&Cjson(&doc).canonical_form()?)?;
            let base_tree = self.repo.find_tree(*base.revision)?;
            let mut builder = self.repo.treebuilder(Some(&base_tree))?;
            builder.insert(base.root.to_string(), doc_blob, 0o100_644)?;
            builder.write().map(Revision::from)
        }?;

        if revision == base.revision {
            return Ok(base.into_inner());
        }

        let signatures = sign(signer, revision).map_err(error::Store::Signer)?.into();
        let content_id = self.commit(
            &format!("Updated to revision {}", revision),
            &signatures,
            revision,
            &[&*base],
        )?;

        Ok(Identity {
            content_id,
            root: base.root,
            revision,
            doc: doc.second(delegation::Direct::from),
            signatures,
        })
    }
}

impl<'a> Identities<'a, Project> {
    /// Attempt to read a [`Project`] from commit `oid`, without verification.
    pub fn get(&self, oid: git2::Oid) -> Result<Project, error::Load> {
        self.get_generic(oid)
    }

    /// Verify the project history with head commit `head`.
    ///
    /// The supplied [`Fn`] shall return the latest head commit of any indirect
    /// (user) delegations of the project. Note that this implies that project
    /// verification should be re-run whenever new inputs are discovered:
    /// the verification status may change due to key revocations or other
    /// circumstances which prevent [`Self::verify`] on the indirect delegation
    /// from succeeding.
    ///
    /// The returned [`VerifiedProject`] is the **most recent** identity for
    /// which the verification succeeded -- which may or may not be `head`.
    pub fn verify<F, E>(
        &self,
        head: git2::Oid,
        find_latest_head: F,
    ) -> Result<VerifiedProject, error::VerifyProject<E>>
    where
        F: Fn(Urn) -> Result<git2::Oid, E>,
        E: std::error::Error + Send + Sync,
    {
        let generic::Folded { head, parent } = self.fold_verify_generic::<ProjectDoc>(head)?;
        let head = head
            .into_inner()
            .map(|doc| {
                doc.try_second(|delegations| {
                    self.resolve_delegation_updates(delegations, &find_latest_head)
                })
            })
            .transpose()?;

        Ok(generic::Verifying::from(head)
            .signed()?
            .quorum()?
            .verified(parent.as_ref())?)
    }

    /// Create a new [`Project`] from a payload and delegations.
    ///
    /// The returned [`Project`] (and the underlying commit) will not have any
    /// parents, and will be signed by `signer`.
    pub fn create<S>(
        &self,
        payload: ProjectPayload,
        delegations: IndirectDelegation,
        signer: &S,
    ) -> Result<Project, error::Store<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let doc = Doc {
            version: 0,
            replaces: None,
            payload,
            delegations: payload::ProjectDelegations::from(delegations.clone()),
        };

        let root: Revision = self.repo.blob(&Cjson(&doc).canonical_form()?)?.into();
        let revision = {
            let mut builder = self.repo.treebuilder(None)?;
            self.inline_indirect(&mut builder, &delegations)?;
            builder.insert(root.to_string(), *root, 0o100_644)?;
            builder.write().map(Revision::from)
        }?;
        let signatures = sign(signer, revision).map_err(error::Store::Signer)?.into();
        let content_id = self.commit(
            &format!("Initialised project identity {}", root),
            &signatures,
            revision,
            &[],
        )?;

        Ok(Identity {
            content_id,
            root,
            revision,
            doc: doc.second(|_| delegations),
            signatures,
        })
    }

    /// Update an existing [`SignedProject`] with a new payload and delegations.
    ///
    /// If both `payload` and `delegations` evaluate to `None`, or their values
    /// result in the same revision as `base`, no new commit is made, and
    /// the result is the unwrapped [`Project`] of the `base` argument.
    ///
    /// Otherwise, the result is a new [`Project`] whose parent is `base`.
    pub fn update<S>(
        &self,
        base: SignedProject,
        payload: impl Into<Option<ProjectPayload>>,
        delegations: impl Into<Option<IndirectDelegation>>,
        signer: &S,
    ) -> Result<Project, error::Store<S::Error>>
    where
        S: Signer,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        let payload = payload.into();
        let delegations = delegations.into();

        // Fast path
        if payload.is_none() && delegations.is_none() {
            return Ok(base.into_inner());
        }

        // FIXME: reorder stuff to avoid cloning

        let doc = Doc {
            version: 0,
            replaces: Some(base.revision),
            payload: payload.unwrap_or_else(|| base.doc.payload.clone()),
            delegations: delegations
                .clone()
                .map(payload::ProjectDelegations::from)
                .unwrap_or_else(|| base.doc.delegations.clone().into()),
        };

        let root = base.root;
        let revision = {
            // Create a fresh tree so we don't have to bother about stale
            // indirect delegations
            let mut builder = self.repo.treebuilder(None)?;
            if let Some(ref indirect) = delegations {
                self.inline_indirect(&mut builder, indirect)?;
            }
            let doc_blob = self.repo.blob(&Cjson(&doc).canonical_form()?)?;
            builder.insert(base.root.to_string(), doc_blob, 0o100_644)?;
            builder.write().map(Revision::from)
        }?;

        if revision == base.revision {
            return Ok(base.into_inner());
        }

        let signatures = sign(signer, revision).map_err(error::Store::Signer)?.into();
        let content_id = self.commit(
            &format!("Updated to revision {}", revision),
            &signatures,
            revision,
            &[&*base],
        )?;

        Ok(Identity {
            content_id,
            root,
            revision,
            doc: doc.second(|_| delegations.unwrap_or_else(|| base.into_inner().doc.delegations)),
            signatures,
        })
    }

    //// Helpers ////

    fn resolve_delegation_updates<I, F, E>(
        &self,
        current: I,
        find_latest_head: &F,
    ) -> Result<IndirectDelegation, error::VerifyProject<E>>
    where
        I: IntoIterator<Item = Either<PublicKey, User>>,
        F: Fn(Urn) -> Result<git2::Oid, E>,
        E: std::error::Error + Send + Sync,
    {
        let mut updated = Vec::new();
        for delegation in current {
            match delegation {
                Right(id) => {
                    let head = find_latest_head(id.urn()).map_err(error::VerifyProject::Lookup)?;
                    let verified = self.updated_user(id, head)?;
                    updated.push(Right(verified.into_inner()))
                },

                left => updated.push(left),
            }
        }

        Ok(delegation::Indirect::try_from_iter(updated)?)
    }

    fn updated_user(
        &self,
        known: User,
        latest_head: git2::Oid,
    ) -> Result<VerifiedUser, error::VerifyUser> {
        // Nb. technically we could coerce `known` into a `VerifiedUser` if its
        // `content_id` equals `latest_head`. Let's not introduce an unsafe
        // coercion, but rely on caching to be implemented efficiently.
        if self.is_in_ancestry_path(latest_head, known.revision.into())? {
            self.as_user().verify(latest_head)
        } else {
            Err(error::VerifyUser::NotInAncestryPath {
                revision: known.revision,
                root: known.root,
                head: latest_head.into(),
            })
        }
    }

    fn inline_indirect<E>(
        &self,
        tree: &mut git2::TreeBuilder,
        delegations: &IndirectDelegation,
    ) -> Result<(), error::Store<E>>
    where
        E: std::error::Error + Send + Sync,
    {
        let mut builder = self.repo.treebuilder(None)?;
        for user_delegation in delegations.iter().filter_map(|x| x.right()) {
            let inlined = self.repo.blob(
                &Cjson(
                    &user_delegation
                        .clone()
                        .map(|doc| doc.second(payload::UserDelegations::from)),
                )
                .canonical_form()?,
            )?;
            builder.insert(
                // TODO: factor out
                multibase::encode(
                    multibase::Base::Base32Z,
                    Multihash::from(user_delegation.root),
                ),
                inlined,
                0o100_644,
            )?;
        }
        let subtree = builder.write()?;
        tree.insert("delegations", subtree, 0o040_000)?;

        Ok(())
    }
}

fn sign<S>(signer: &S, rev: Revision) -> Result<Signature, S::Error>
where
    S: Signer,
{
    let sig = block_on(signer.sign(rev.as_bytes()))?;
    Ok(Signature::from((signer.public_key().into(), sig.into())))
}
