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
    convert::Into,
    marker::PhantomData,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{de::from_slice as from_json_slice, value::Value as JsonValue};
use thiserror::Error;

use crate::{
    git::ext::Oid,
    internal::canonical::{Cjson, CjsonError},
    keys::{PublicKey, SecretKey, Signature},
};

use cache::VerificationCache;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Wrong delegation type")]
    MismatchedDelegation,

    #[error("Key not present")]
    KeyNotPresent,

    #[error("Invalid revision tree entry {0}")]
    InvalidRevisionTreeEntry(Revision),

    #[error("Invalid signature by key {0}")]
    InvalidSignature(PublicKey),

    #[error("No current quorum")]
    NoCurrentQuorum,

    #[error("No previous quorum")]
    NoPreviousQuorum,

    // TODO: Add info
    #[error("Root mismatch")]
    RootMismatch,

    // TODO: Add info
    #[error("Fork detected")]
    ForkDetected,

    #[error("Previous link mismatch")]
    PreviousLinkMismatch,

    #[error(transparent)]
    Cjson(#[from] CjsonError),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::error::Error),

    #[error(transparent)]
    Git(#[from] git2::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Revision(Oid);

impl std::ops::Deref for Revision {
    type Target = git2::Oid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<git2::Oid> for Revision {
    fn as_ref(&self) -> &git2::Oid {
        self
    }
}

impl From<Revision> for git2::Oid {
    fn from(rev: Revision) -> Self {
        *rev.0.as_ref()
    }
}

impl From<&Revision> for git2::Oid {
    fn from(rev: &Revision) -> Self {
        *rev.0.as_ref()
    }
}

impl From<git2::Oid> for Revision {
    fn from(oid: git2::Oid) -> Self {
        Self(Oid(oid))
    }
}

impl std::cmp::PartialOrd for Revision {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_bytes().partial_cmp(other.as_bytes())
    }
}

impl std::cmp::Ord for Revision {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ContentId(Oid);

impl std::ops::Deref for ContentId {
    type Target = git2::Oid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<git2::Oid> for ContentId {
    fn as_ref(&self) -> &git2::Oid {
        self
    }
}

impl From<ContentId> for git2::Oid {
    fn from(rev: ContentId) -> Self {
        *rev.0.as_ref()
    }
}

impl From<&ContentId> for git2::Oid {
    fn from(rev: &ContentId) -> Self {
        *rev.0.as_ref()
    }
}

impl From<git2::Oid> for ContentId {
    fn from(oid: git2::Oid) -> Self {
        Self(Oid(oid))
    }
}

impl std::fmt::Display for ContentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::cmp::PartialOrd for ContentId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_bytes().partial_cmp(other.as_bytes())
    }
}

impl std::cmp::Ord for ContentId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Delegations {
    Keys(BTreeSet<PublicKey>),
    Users(BTreeMap<PublicKey, Revision>),
}

pub enum DelegationsKeys<'a> {
    Keys(std::collections::btree_set::Iter<'a, PublicKey>),
    Users(std::collections::btree_map::Iter<'a, PublicKey, Revision>),
}

impl<'a> Iterator for DelegationsKeys<'a> {
    type Item = &'a PublicKey;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DelegationsKeys::Keys(keys) => keys.next(),
            DelegationsKeys::Users(users) => users.next().map(|(k, _)| k),
        }
    }
}

impl Delegations {
    pub fn empty_keys() -> Self {
        Delegations::Keys(BTreeSet::new())
    }

    pub fn empty_users() -> Self {
        Delegations::Users(BTreeMap::new())
    }

    pub fn has_key(&self, key: &PublicKey) -> bool {
        match self {
            Delegations::Keys(keys) => keys.contains(key),
            Delegations::Users(users) => users.contains_key(key),
        }
    }

    pub fn keys(&self) -> DelegationsKeys {
        match self {
            Delegations::Keys(keys) => DelegationsKeys::Keys(keys.iter()),
            Delegations::Users(users) => DelegationsKeys::Users(users.iter()),
        }
    }

    pub fn add_key(&mut self, key: PublicKey) -> Result<(), Error> {
        if let Delegations::Keys(keys) = self {
            keys.insert(key);
            Ok(())
        } else {
            Err(Error::MismatchedDelegation)
        }
    }

    pub fn add_user_key(&mut self, key: PublicKey, user: Revision) -> Result<(), Error> {
        if let Delegations::Users(keys) = self {
            keys.insert(key, user);
            Ok(())
        } else {
            Err(Error::MismatchedDelegation)
        }
    }

    pub fn remove_key(&mut self, key: &PublicKey) -> Result<(), Error> {
        let removed = match self {
            Delegations::Keys(keys) => keys.remove(key),
            Delegations::Users(users) => users.remove(key).is_some(),
        };
        if removed {
            Ok(())
        } else {
            Err(Error::KeyNotPresent)
        }
    }

    pub fn add_user_keys(
        &mut self,
        user_keys: &Self,
        user_revision: &Revision,
    ) -> Result<(), Error> {
        if let (Delegations::Users(keys), Delegations::Keys(user_keys)) = (self, user_keys) {
            for k in user_keys.iter() {
                keys.insert(k.clone(), user_revision.clone());
            }
            Ok(())
        } else {
            Err(Error::MismatchedDelegation)
        }
    }

    pub fn remove_keys(&mut self, delegations: &Self) {
        for k in delegations.keys() {
            self.remove_key(k).ok();
        }
    }

    pub fn quorum(&self) -> usize {
        match self {
            Delegations::Keys(keys) => (keys.len() / 2) + 1,
            Delegations::Users(users) => {
                let mut unique_users = BTreeSet::new();
                for u in users.values() {
                    unique_users.insert(u.as_bytes());
                }
                (unique_users.len() / 2) + 1
            },
        }
    }

    pub fn check_quorum(&self, signatures: &BTreeMap<PublicKey, Signature>) -> bool {
        match self {
            Delegations::Keys(keys) => {
                let mut count = 0;
                for k in signatures.keys() {
                    if keys.contains(k) {
                        count += 1;
                    }
                }
                count >= self.quorum()
            },
            Delegations::Users(users) => {
                let mut unique_signers = BTreeSet::new();
                for k in signatures.keys() {
                    users.get(k).map(|u| unique_signers.insert(u.as_bytes()));
                }
                unique_signers.len() >= self.quorum()
            },
        }
    }
}

/// Type witness for a fully verified [`Doc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified;

/// Type witness for a [`Doc`] signed by a quorum of its delegations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quorum;

/// Type witness for a [`Doc`] with verified signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed;

/// Type witness for an untrusted [`Doc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untrusted;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Doc<Status>
where
    Status: Clone,
{
    status_marker: PhantomData<Status>,
    replaces: Option<Revision>,
    payload: JsonValue,
    delegations: Delegations,
}

impl<Status> Doc<Status>
where
    Status: Clone,
{
    pub fn is_root(&self) -> bool {
        self.replaces.is_none()
    }

    pub fn replaces(&self) -> Option<&Revision> {
        self.replaces.as_ref()
    }

    pub fn json_payload(&self) -> &JsonValue {
        &self.payload
    }

    pub fn delegations(&self) -> &Delegations {
        &self.delegations
    }

    pub fn payload<T>(&self) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        serde_json::value::from_value(self.payload.clone()).map_err(Error::SerdeJson)
    }

    pub fn as_untrusted(&self) -> Doc<Untrusted> {
        Doc {
            status_marker: PhantomData,
            replaces: self.replaces.clone(),
            payload: self.payload.clone(),
            delegations: self.delegations.clone(),
        }
    }

    fn with_status<NewStatus>(self) -> Doc<NewStatus>
    where
        NewStatus: Clone,
    {
        Doc {
            status_marker: PhantomData,
            replaces: self.replaces,
            payload: self.payload,
            delegations: self.delegations,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        Ok(Cjson(self).canonical_form()?)
    }
}

pub struct DocBuilder {
    replaces: Option<Revision>,
    delegations: Delegations,
}

impl DocBuilder {
    pub fn new_user() -> Self {
        Self {
            replaces: None,
            delegations: Delegations::empty_keys(),
        }
    }

    pub fn new_project() -> Self {
        Self {
            replaces: None,
            delegations: Delegations::empty_users(),
        }
    }

    pub fn replaces(&mut self, revision: Revision) -> &mut Self {
        self.replaces = Some(revision);
        self
    }

    pub fn add_key(&mut self, key: PublicKey) -> Result<&mut Self, Error> {
        self.delegations.add_key(key)?;
        Ok(self)
    }

    pub fn add_user_key(&mut self, key: PublicKey, user: Revision) -> Result<&mut Self, Error> {
        self.delegations.add_user_key(key, user)?;
        Ok(self)
    }

    pub fn remove_key(&mut self, key: &PublicKey) -> Result<&mut Self, Error> {
        self.delegations.remove_key(key)?;
        Ok(self)
    }

    pub fn add_user_keys(
        &mut self,
        user_keys: &Delegations,
        user_revision: &Revision,
    ) -> Result<&mut Self, Error> {
        self.delegations.add_user_keys(user_keys, user_revision)?;
        Ok(self)
    }

    pub fn remove_keys(&mut self, delegations: &Delegations) -> &mut Self {
        self.delegations.remove_keys(delegations);
        self
    }

    pub fn build<T>(&self, payload: T) -> Result<Doc<Untrusted>, Error>
    where
        T: Serialize,
    {
        Ok(Doc {
            status_marker: PhantomData,
            replaces: self.replaces.clone(),
            payload: serde_json::value::to_value(payload)?,
            delegations: self.delegations.clone(),
        })
    }
}

pub struct IdentityBuilder {
    previous: Option<ContentId>,
    merged: Option<ContentId>,
    root: Revision,
    revision: Revision,
    doc: Doc<Untrusted>,
    signatures: BTreeMap<PublicKey, Signature>,
}

impl IdentityBuilder {
    pub fn new(revision: Revision, doc: Doc<Untrusted>) -> Self {
        IdentityBuilder {
            previous: None,
            merged: None,
            root: revision.clone(),
            revision,
            doc,
            signatures: BTreeMap::new(),
        }
    }

    pub fn with_parent<Status>(
        parent: &Identity<Status>,
        revision: Revision,
        doc: Doc<Untrusted>,
    ) -> Self
    where
        Status: Clone,
    {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: None,
            root: parent.root.clone(),
            revision,
            doc,
            signatures: BTreeMap::new(),
        }
    }

    pub fn duplicate<Status>(parent: &Identity<Status>) -> Self
    where
        Status: Clone,
    {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: None,
            root: parent.root.clone(),
            revision: parent.revision.clone(),
            doc: parent.doc.as_untrusted(),
            signatures: parent.signatures.clone(),
        }
    }

    pub fn duplicate_other<Status1, Status2>(
        parent: &Identity<Status1>,
        other: &Identity<Status2>,
    ) -> Self
    where
        Status1: Clone,
        Status2: Clone,
    {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: Some(other.commit.clone()),
            root: parent.root.clone(),
            revision: other.revision.clone(),
            doc: other.doc.as_untrusted(),
            signatures: other.signatures.clone(),
        }
    }

    pub fn sign(mut self, key: &SecretKey) -> Self {
        self.signatures
            .insert(key.public(), key.sign(self.revision.as_bytes()));
        self
    }

    pub fn previous(&self) -> Option<&ContentId> {
        self.previous.as_ref()
    }
    pub fn merged(&self) -> Option<&ContentId> {
        self.merged.as_ref()
    }
    pub fn root(&self) -> &Revision {
        &self.root
    }
    pub fn revision(&self) -> &Revision {
        &self.revision
    }
    pub fn doc(&self) -> &Doc<Untrusted> {
        &self.doc
    }
    pub fn signatures(&self) -> &BTreeMap<PublicKey, Signature> {
        &self.signatures
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Identity<Status>
where
    Status: Clone,
{
    /// Verification status marker type
    status_marker: PhantomData<Status>,
    previous: Option<ContentId>,
    merged: Option<ContentId>,
    commit: ContentId,
    root: Revision,
    revision: Revision,
    doc: Doc<Status>,
    signatures: BTreeMap<PublicKey, Signature>,
}

impl<Status> Identity<Status>
where
    Status: Clone,
{
    pub fn previous(&self) -> Option<&ContentId> {
        self.previous.as_ref()
    }
    pub fn merged(&self) -> Option<&ContentId> {
        self.merged.as_ref()
    }
    pub fn commit(&self) -> &ContentId {
        &self.commit
    }
    pub fn root(&self) -> &Revision {
        &self.root
    }
    pub fn revision(&self) -> &Revision {
        &self.revision
    }
    pub fn doc(&self) -> &Doc<Status> {
        &self.doc
    }
    pub fn signatures(&self) -> &BTreeMap<PublicKey, Signature> {
        &self.signatures
    }

    pub fn verify_signatures(&self) -> Result<(), Error> {
        for (k, s) in &self.signatures {
            if !s.verify(self.revision.as_bytes(), k) {
                return Err(Error::InvalidSignature(k.clone()));
            }
        }
        Ok(())
    }

    fn with_status<NewStatus>(self) -> Identity<NewStatus>
    where
        NewStatus: Clone,
    {
        Identity {
            status_marker: PhantomData,
            previous: self.previous,
            merged: self.merged,
            commit: self.commit,
            root: self.root,
            revision: self.revision,
            doc: self.doc.with_status(),
            signatures: self.signatures,
        }
    }
}

impl Identity<Untrusted> {
    pub fn check_signatures(self) -> Result<Identity<Signed>, Error> {
        self.verify_signatures().map(|_| self.with_status())
    }
}

impl Identity<Signed> {
    pub fn has_quorum(&self) -> bool {
        self.doc().delegations().check_quorum(self.signatures())
    }

    pub fn check_quorum(self) -> Result<Identity<Quorum>, Error> {
        if self.has_quorum() {
            Ok(self.with_status())
        } else {
            Err(Error::NoCurrentQuorum)
        }
    }
}

impl Identity<Quorum> {
    pub fn check_update(
        self,
        previous: Option<&Identity<Verified>>,
        cache: &mut impl VerificationCache,
    ) -> Result<Identity<Verified>, Error> {
        if cache.is_verified(self.revision()) {
            return Ok(self.with_status());
        }

        let result = match previous {
            Some(previous) => match self.doc().replaces() {
                Some(replaces) => {
                    if self.root() != previous.root() {
                        Err(Error::RootMismatch)
                    } else if replaces != previous.revision() {
                        Err(Error::PreviousLinkMismatch)
                    } else if !previous.doc().delegations().check_quorum(self.signatures()) {
                        Err(Error::NoCurrentQuorum)
                    } else {
                        Ok(self.with_status())
                    }
                },
                None => Err(Error::PreviousLinkMismatch),
            },
            None => {
                if self.doc().is_root() {
                    Ok(self.with_status())
                } else {
                    Err(Error::PreviousLinkMismatch)
                }
            },
        };

        if let Ok(id) = &result {
            cache.register_verified(id)?;
        }

        result
    }
}

const RAD_SIGNATURE_TRAILER_NAME: &str = "x-rad-signature";

fn append_signatures(buf: &mut String, sigs: &BTreeMap<PublicKey, Signature>) {
    buf.push_str("\n");
    for (k, s) in sigs {
        buf.push_str(&format!(
            "{}: {} {}\n",
            RAD_SIGNATURE_TRAILER_NAME,
            k.to_bs58(),
            s.to_bs58()
        ));
    }
}

fn match_signature(line: &str) -> Option<(PublicKey, Signature)> {
    let mut tokens = line
        .strip_prefix(RAD_SIGNATURE_TRAILER_NAME)
        .and_then(|line| line.strip_prefix(": "))?
        .split(' ');

    let key_text = tokens.next()?;
    let sig_text = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }

    Some((
        PublicKey::from_bs58(key_text)?,
        Signature::from_bs58(sig_text)?,
    ))
}

pub fn parse_signatures(buf: Option<&str>) -> BTreeMap<PublicKey, Signature> {
    let mut sigs = BTreeMap::new();
    if let Some(buf) = buf {
        for line in buf.split('\n') {
            if let Some((k, s)) = match_signature(line) {
                sigs.insert(k, s);
            }
        }
    }
    sigs
}

pub struct IdentityStore<'a> {
    repo: &'a git2::Repository,
}

const ROOT_TREE_ENTRY_NAME: &str = "ROOT";

impl<'a> IdentityStore<'a> {
    pub fn new(repo: &'a git2::Repository) -> Self {
        Self { repo }
    }

    pub fn get_doc(&self, revision: &Revision) -> Result<(Doc<Untrusted>, Revision), Error> {
        let tree = self.repo.find_tree(revision.into())?;
        let root_entry = tree
            .get(0)
            .ok_or_else(|| Error::InvalidRevisionTreeEntry(revision.clone()))?;
        let root_name = root_entry
            .name()
            .ok_or_else(|| Error::InvalidRevisionTreeEntry(revision.clone()))?;
        let root_revision = if root_name == ROOT_TREE_ENTRY_NAME {
            revision.clone()
        } else {
            Revision::from(git2::Oid::from_str(root_name)?)
        };
        let doc = self
            .repo
            .find_blob(root_entry.id())
            .map_err(Error::Git)
            .and_then(|blob| from_json_slice(blob.content()).map_err(Error::SerdeJson))?;
        Ok((doc, root_revision))
    }

    pub fn store_doc<Status>(
        &self,
        doc: &Doc<Status>,
        root_revision: Option<&Revision>,
    ) -> Result<Revision, Error>
    where
        Status: Clone,
    {
        let doc_bytes = doc.serialize()?;
        let blob_oid = self.repo.blob(&doc_bytes)?;
        let mut tree = self.repo.treebuilder(None)?;
        tree.insert(
            match root_revision {
                Some(rev) => rev.to_string(),
                None => ROOT_TREE_ENTRY_NAME.to_string(),
            },
            blob_oid,
            0o100644,
        )?;
        Ok(Revision::from(tree.write()?))
    }

    pub fn get_identity(&self, id: &ContentId) -> Result<Identity<Untrusted>, Error> {
        let commit = self.repo.find_commit(id.into())?;
        let mut previous = None;
        let mut merged = None;
        for (index, parent) in commit.parents().enumerate() {
            match index {
                0 => previous = Some(parent.id().into()),
                1 => merged = Some(parent.id().into()),
                _ => break,
            }
        }
        let revision = Revision::from(commit.tree_id());
        let (doc, root) = self.get_doc(&revision)?;
        Ok(Identity {
            status_marker: PhantomData,
            previous,
            merged,
            commit: commit.id().into(),
            root,
            revision,
            doc,
            signatures: parse_signatures(commit.message()),
        })
    }

    pub fn store_identity(&self, builder: IdentityBuilder) -> Result<Identity<Untrusted>, Error> {
        let mut message = format!("RAD ID {} REV {}\n", builder.root, builder.revision);
        append_signatures(&mut message, &builder.signatures);

        let git_sig = self.repo.signature()?;
        let tree = self.repo.find_tree(builder.revision().into())?;

        let previous_commit = match builder.previous() {
            Some(id) => Some(self.repo.find_commit(id.into())?),
            None => None,
        };
        let merged_commit = match builder.merged() {
            Some(id) => Some(self.repo.find_commit(id.into())?),
            None => None,
        };
        let mut parents = Vec::new();
        if let Some(commit) = previous_commit.as_ref() {
            parents.push(commit);
        }
        if let Some(commit) = merged_commit.as_ref() {
            parents.push(commit);
        }

        let id = self
            .repo
            .commit(None, &git_sig, &git_sig, &message, &tree, &parents)?;

        Ok(Identity {
            status_marker: PhantomData,
            previous: builder.previous().cloned(),
            merged: builder.merged().cloned(),
            commit: id.into(),
            root: builder.root,
            revision: builder.revision,
            doc: builder.doc,
            signatures: builder.signatures,
        })
    }

    pub fn get_parent_identity<Status>(
        &self,
        identity: &Identity<Status>,
    ) -> Option<Identity<Untrusted>>
    where
        Status: Clone,
    {
        identity
            .previous()
            .and_then(|id| self.get_identity(id).ok())
    }

    pub fn get_latest_identity(
        &self,
        id: &ContentId,
        cache: &mut impl cache::VerificationCache,
    ) -> Result<(Identity<Signed>, Option<Identity<Verified>>), Error> {
        // Head of this branch, signed
        let head = self.get_identity(id)?.check_signatures()?;
        // Root document revision (the actual ID we are dealing with)
        let root = head.root();

        // Latest verified commit and its index
        // (head is 0, index grows while following parents)
        let mut latest_verified: Option<(Identity<Verified>, i32)> = None;
        // Collection of commits pending verification and their index
        // (the key is the revision they are waiting for)
        let mut pending_verification = BTreeMap::<Revision, (Identity<Quorum>, i32)>::new();

        // Current commit being processed
        let mut current = Some(head.clone());
        // Index of current commit
        let mut current_index = 0;
        // Did we verify a document?
        let mut identity_verified = false;

        // Traverse the commit chain following direct parents
        while let Some(cur) = current {
            // Wrong root, exit
            if cur.root() != root {
                return Err(Error::RootMismatch);
            }

            // We had a proper verification, we are done
            if identity_verified {
                break;
            }

            // Prepare next commit to process
            let next = self
                .get_parent_identity(&cur)
                .map(|id| id.check_signatures())
                .transpose()?;

            // If the current commit does not have a quorum, skip it
            if cur.has_quorum() {
                let cur = cur.check_quorum().unwrap();

                // Attempt to verify current commit
                let mut verified = if cache.is_verified(cur.revision()) {
                    // The cache states it is verified
                    identity_verified = true;
                    Some((cur.check_update(None, cache)?, current_index))
                } else {
                    // Detect if it is a root
                    match cur.doc().replaces() {
                        Some(previous) => {
                            // It needs a parent verified document, add to pending set
                            if !pending_verification.contains_key(previous) {
                                // Keep minimal indexes pending
                                pending_verification.insert(previous.clone(), (cur, current_index));
                            }
                            None
                        },
                        None => {
                            // It is a root, attempt verification
                            cur.check_update(None, cache).ok().map(|verified_root| {
                                identity_verified = true;
                                (verified_root, current_index)
                            })
                        },
                    }
                };

                // Process verified commit
                while let Some((verified_id, verified_index)) = verified {
                    // Update latest verified
                    latest_verified = match latest_verified {
                        Some((latest_id, latest_index)) => {
                            if verified_index < latest_index {
                                Some((verified_id.clone(), verified_index))
                            } else {
                                Some((latest_id, latest_index))
                            }
                        },
                        None => Some((verified_id.clone(), verified_index)),
                    };

                    // Handle pending
                    verified = pending_verification.get(verified_id.revision()).and_then(
                        |(pending_id, pending_index)| {
                            pending_id
                                .clone()
                                .check_update(Some(&verified_id), cache)
                                .ok()
                                .map(|id| (id, *pending_index))
                        },
                    );
                }
            }

            // Prepare next commit
            current = next;
            current_index += 1;
        }

        Ok((head, latest_verified.map(|(id, _)| id)))
    }
}

pub mod cache;
#[cfg(test)]
mod test;
