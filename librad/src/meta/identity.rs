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
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{de::from_slice as from_json_slice, value::Value as JsonValue};
use thiserror::Error;

use crate::{
    git::ext::Oid,
    internal::canonical::{Cjson, CjsonError},
    keys::{PublicKey, SecretKey, Signature},
};

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
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    replaces: Option<Revision>,
    payload: JsonValue,
    delegations: Delegations,
}

impl Doc {
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

    pub fn build<T>(&self, payload: T) -> Result<Doc, Error>
    where
        T: Serialize,
    {
        Ok(Doc {
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
    doc: Doc,
    signatures: BTreeMap<PublicKey, Signature>,
}

impl IdentityBuilder {
    pub fn new(revision: Revision, doc: Doc) -> Self {
        IdentityBuilder {
            previous: None,
            merged: None,
            root: revision.clone(),
            revision,
            doc,
            signatures: BTreeMap::new(),
        }
    }

    pub fn with_parent(parent: &Identity, revision: Revision, doc: Doc) -> Self {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: None,
            root: parent.root.clone(),
            revision,
            doc,
            signatures: BTreeMap::new(),
        }
    }

    pub fn duplicate(parent: &Identity) -> Self {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: None,
            root: parent.root.clone(),
            revision: parent.revision.clone(),
            doc: parent.doc.clone(),
            signatures: parent.signatures.clone(),
        }
    }

    pub fn duplicate_other(parent: &Identity, other: &Identity) -> Self {
        IdentityBuilder {
            previous: Some(parent.commit.clone()),
            merged: Some(other.commit.clone()),
            root: parent.root.clone(),
            revision: other.revision.clone(),
            doc: other.doc.clone(),
            signatures: other.signatures.clone(),
        }
    }

    pub fn sign(mut self, key: SecretKey) -> Self {
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
    pub fn doc(&self) -> &Doc {
        &self.doc
    }
    pub fn signatures(&self) -> &BTreeMap<PublicKey, Signature> {
        &self.signatures
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Identity {
    previous: Option<ContentId>,
    merged: Option<ContentId>,
    commit: ContentId,
    root: Revision,
    revision: Revision,
    doc: Doc,
    signatures: BTreeMap<PublicKey, Signature>,
}

impl Identity {
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
    pub fn doc(&self) -> &Doc {
        &self.doc
    }
    pub fn signatures(&self) -> &BTreeMap<PublicKey, Signature> {
        &self.signatures
    }

    pub fn check_signatures(&self) -> Result<(), Error> {
        for (k, s) in &self.signatures {
            if !s.verify(self.revision.as_bytes(), k) {
                return Err(Error::InvalidSignature(k.clone()));
            }
        }
        Ok(())
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

    pub fn get_doc(&self, revision: &Revision) -> Result<(Doc, Revision), Error> {
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

    pub fn store_doc(
        &self,
        doc: &Doc,
        root_revision: Option<&Revision>,
    ) -> Result<Revision, Error> {
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

    pub fn get_identity(&self, id: &ContentId) -> Result<Identity, Error> {
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
            previous,
            merged,
            commit: commit.id().into(),
            root,
            revision,
            doc,
            signatures: parse_signatures(commit.message()),
        })
    }

    pub fn store_identity(&self, builder: IdentityBuilder) -> Result<Identity, Error> {
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
            previous: builder.previous().cloned(),
            merged: builder.merged().cloned(),
            commit: id.into(),
            root: builder.root,
            revision: builder.revision,
            doc: builder.doc,
            signatures: builder.signatures,
        })
    }

    pub fn get_parent_identity(&self, identity: &Identity) -> Option<Identity> {
        identity
            .previous()
            .and_then(|id| self.get_identity(id).ok())
    }
}
#[cfg(test)]
mod test;
