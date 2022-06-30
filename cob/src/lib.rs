// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! # Collaborative Objects
//!
//! Collaborative objects are automerge CRDTs. The general architecture is fully
//! specified in docs/rfc/0662-collaborative-objects.adoc. This module exposes a
//! basic CRUD interface to collaborative objects. The primary things users of
//! this module need to bring with them are
//! - a git repository
//! - an implementation of [`RefsStorage`] - which defines how references to
//!   collaborative objects are stored in the given repository
//! - an implementation of [`IdentityStorage`], which knows how to get from a
//!   URN of an identity to the OID of the tip of that identity
//! - a `BoxedSigner`
//! - an optional cache directory
//!
//! ## Caching
//!
//! When loading a collaborative object we verify that every change in the hash
//! graph is signed. For repositories with a large number of objects, or a
//! smaller number of objects with a large number of changes, this can become a
//! computationally intensive task. To avoid recalculating the state of every
//! object every time we make a change then, we implement a caching layer. Each
//! of the CRUD methods takes an optional cache directory, this cache implements
//! some basic locking so it's safe to use from multiple processes. We also
//! commit to not making backwards incompatible changes to the chache, so it is
//! safe to upgrade without deleting caches (though the cache may need to be
//! regenerated, we only guarantee that applications will not crash).
//!
//! # Implementation Notes
//!
//! This module starts with the basic value types which are part of the public
//! API: `ObjectId`, and `TypeName`, all of which compose a
//! `CollaborativeObject`. When loading a `CollaborativeObject` we attempt to
//! load a graph of the automerge changes that make up the object from
//! references to the object ID in the `RefsStorage` we have been passed. There
//! are two representations of a change graph. Firstly there is
//! `change_graph::ChangeGraph`, which is a full directed graph containing all
//! the commits we can find for the given object. `ChangeGraph`
//! has an `evaluate` method which traverses this directed graph validating that
//! each change has a valid signature. Secondly there is the
//! `cache::CachedChangeGraph`, this is a representation that contains only the
//! all the valid changes in a valid change graph and the OIDs of the tips
//! of the graph that was used to generate the changes. For any of the CRUD
//! methods we first attempt to load a `CachedChangeGraph` from the cache, and
//! if that fails (either because there is no cached object at all, or because
//! the reference to the tips returned by the `RefsStorage` is different to
//! those that were used to generate the cache) then we fall back to evaluating
//! the full change graph of the object.

use std::{cell::RefCell, collections::BTreeSet, convert::TryFrom, fmt, rc::Rc, str::FromStr};

use serde::{Deserialize, Serialize};

use link_crypto::{keystore::sign::Signer, BoxedSigner, PublicKey};
use link_identities::git::{Urn, VerifiedPerson};
use radicle_git_ext as ext;

mod authorizing_identity;
pub use authorizing_identity::{AuthDecision, AuthorizingIdentity};

mod trailers;

mod change_graph;
use change_graph::ChangeGraph;

mod change;
use change::Change;

mod refs_storage;
pub use refs_storage::{ObjectRefs, RefsStorage};

mod cache;
use cache::{Cache, CachedChangeGraph};

mod identity_storage;
pub use identity_storage::IdentityStorage;

mod history;
pub use history::{EntryContents, History, HistoryEntry, HistoryType};

mod pruning_fold;

pub mod internals {
    //! This module exposes implementation details of the collaborative object
    //! crate for use in testing

    pub use super::cache::{Cache, CachedChangeGraph, FileSystemCache};
}

/// The typename of an object. Valid typenames MUST be sequences of alphanumeric
/// characters separated by a period. The name must start and end with an
/// alphanumeric character
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TypeName(String);

impl TypeName {
    /// A string representation of the typename which will match the typename in
    /// regular expressions. This primarily escapes periods
    pub fn regex_safe_string(&self) -> String {
        self.0.replace('.', "\\.")
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

lazy_static::lazy_static! {
    static ref TYPENAME_REGEX: regex::Regex = regex::Regex::new(r"^([a-zA-Z0-9])+(\.[a-zA-Z0-9]+)*$").unwrap();
}

impl FromStr for TypeName {
    type Err = error::TypeNameParse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if TYPENAME_REGEX.is_match(s) {
            Ok(TypeName(s.to_string()))
        } else {
            Err(error::TypeNameParse)
        }
    }
}

/// The id of an object
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(git2::Oid);

impl FromStr for ObjectId {
    type Err = error::ParseObjectId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, bytes) = multibase::decode(s)?;
        let mh = multihash::Multihash::from_bytes(bytes)?;
        let oid = radicle_git_ext::Oid::try_from(mh)?;
        Ok(ObjectId(oid.into()))
    }
}

impl From<git2::Oid> for ObjectId {
    fn from(oid: git2::Oid) -> Self {
        ObjectId(oid)
    }
}

impl From<ext::Oid> for ObjectId {
    fn from(oid: ext::Oid) -> Self {
        git2::Oid::from(oid).into()
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hashed = radicle_git_ext::Oid::from(self.0).into_multihash();
        write!(f, "{}", multibase::encode(multibase::Base::Base32Z, hashed))
    }
}

impl Serialize for ObjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let hashed = radicle_git_ext::Oid::from(self.0).into_multihash();
        let s = multibase::encode(multibase::Base::Base32Z, hashed);
        serializer.serialize_str(s.as_str())
    }
}

impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let (_, bytes) = multibase::decode(raw).map_err(serde::de::Error::custom)?;
        let mh = multihash::Multihash::from_bytes(bytes).map_err(serde::de::Error::custom)?;
        let oid = radicle_git_ext::Oid::try_from(mh).map_err(serde::de::Error::custom)?;
        Ok(ObjectId(oid.into()))
    }
}

impl From<&git2::Oid> for ObjectId {
    fn from(oid: &git2::Oid) -> Self {
        ObjectId(*oid)
    }
}

/// A collaborative object
#[derive(Debug, Clone)]
pub struct CollaborativeObject {
    /// The identity (person or project) this collaborative object is authorized
    /// with respect to
    #[allow(unused)]
    authorizing_identity_urn: Urn,
    /// The typename of this object
    typename: TypeName,
    /// The CRDT history we know about for this object
    history: History,
    /// The id of the object
    id: ObjectId,
}

impl From<Rc<RefCell<CachedChangeGraph>>> for CollaborativeObject {
    fn from(tg: Rc<RefCell<CachedChangeGraph>>) -> Self {
        let tg = tg.borrow();
        CollaborativeObject {
            authorizing_identity_urn: tg.authorizing_identity_urn().clone(),
            typename: tg.typename().clone(),
            history: tg.history().clone(),
            id: tg.object_id(),
        }
    }
}

impl CollaborativeObject {
    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn id(&self) -> &ObjectId {
        &self.id
    }

    pub fn typename(&self) -> &TypeName {
        &self.typename
    }
}

/// Additional information about the change graph of an object
pub struct ChangeGraphInfo {
    /// The ID of the object
    pub object_id: ObjectId,
    /// A graphviz description of the changegraph of the object
    pub dotviz: String,
    /// The number of nodes in the change graph of the object
    pub number_of_nodes: u64,
    /// The "tips" of the change graph, i.e the object IDs pointed to by
    /// references to the object
    pub tips: BTreeSet<git2::Oid>,
}

pub mod error {
    use super::{cache::Error as CacheError, change, change_graph::Error as ChangeGraphError};
    use thiserror::Error;

    use radicle_git_ext::FromMultihashError as ExtOidFromMultiHashError;

    #[derive(Error, Debug)]
    #[error("invalid typename")]
    pub struct TypeNameParse;

    #[derive(Debug, Error)]
    pub enum Create<RefsError: std::error::Error> {
        #[error("Invalid automerge history")]
        InvalidAutomergeHistory,
        #[error(transparent)]
        CreateChange(#[from] change::error::Create),
        #[error(transparent)]
        Refs(RefsError),
        #[error(transparent)]
        Cache(#[from] CacheError),
        #[error(transparent)]
        Io(#[from] std::io::Error),
        #[error("signer must belong to the author")]
        SignerIsNotAuthor,
    }

    #[derive(Debug, Error)]
    pub enum Retrieve<RefsError: std::error::Error> {
        #[error(transparent)]
        ChangeGraph(#[from] ChangeGraphError),
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Refs(RefsError),
        #[error(transparent)]
        Cache(#[from] CacheError),
        #[error(transparent)]
        Io(#[from] std::io::Error),
    }

    #[derive(Debug, Error)]
    pub enum Update<RefsError: std::error::Error> {
        #[error(transparent)]
        ChangeGraph(#[from] ChangeGraphError),
        #[error("no object found")]
        NoSuchObject,
        #[error(transparent)]
        CreateChange(#[from] change::error::Create),
        #[error(transparent)]
        Refs(RefsError),
        #[error(transparent)]
        Cache(#[from] CacheError),
        #[error(transparent)]
        Git(#[from] git2::Error),
        #[error(transparent)]
        Io(#[from] std::io::Error),
        #[error("signer must belong to the author")]
        SignerIsNotAuthor,
    }

    #[derive(Debug, Error)]
    pub enum ParseObjectId {
        #[error(transparent)]
        Git2(#[from] git2::Error),
        #[error(transparent)]
        MultibaseDecode(#[from] multibase::Error),
        #[error(transparent)]
        MultiHashFrombytes(#[from] multihash::DecodeOwnedError),
        #[error(transparent)]
        OidFromMultiHash(#[from] ExtOidFromMultiHashError),
    }
}

/// The data required to create a new object
pub struct CreateObjectArgs<'a, R: RefsStorage, P: AsRef<std::path::Path>> {
    /// The CRDT history to initialize this object with
    pub contents: EntryContents,
    /// The typename for this object
    pub typename: TypeName,
    /// An optional message to add to the commit message for the commit which
    /// creates this object
    pub message: Option<String>,
    /// The refs storage used to create a reference to the new object
    pub refs_storage: &'a R,
    /// The repo the new object will be stored in
    pub repo: &'a git2::Repository,
    /// The signer used to sign the changes which create the new object
    pub signer: &'a BoxedSigner,
    /// The person corresponding to the signer above
    pub author: &'a VerifiedPerson,
    /// The identity in which the authorization rules of this object will be
    /// checked, i.e. a `VerifiedProject` or a `VerifiedPerson`
    pub authorizing_identity: &'a dyn AuthorizingIdentity,
    /// The directory to use for caching the latest known state of cobs
    pub cache_dir: Option<P>,
}

impl<'a, R: RefsStorage, P: AsRef<std::path::Path>> CreateObjectArgs<'a, R, P> {
    fn change_spec(&self) -> change::NewChangeSpec {
        change::NewChangeSpec {
            typename: self.typename.clone(),
            tips: None,
            message: self.message.clone(),
            contents: self.contents.clone(),
        }
    }
}

pub fn create_object<R: RefsStorage, P: AsRef<std::path::Path>>(
    args: CreateObjectArgs<R, P>,
) -> Result<CollaborativeObject, error::Create<R::Error>> {
    let CreateObjectArgs {
        refs_storage,
        repo,
        signer,
        author,
        authorizing_identity,
        ref contents,
        ref typename,
        ..
    } = args;
    if !is_signer_for(signer, author) {
        return Err(error::Create::SignerIsNotAuthor);
    }
    let init_change = change::Change::create(
        authorizing_identity.content_id(),
        author.content_id.into(),
        repo,
        signer,
        args.change_spec(),
    )
    .map_err(error::Create::from)?;

    let history = History::new_from_root(*init_change.commit(), author.urn(), contents.clone());

    let object_id = init_change.commit().into();
    refs_storage
        .update_ref(
            &authorizing_identity.urn(),
            typename,
            object_id,
            *init_change.commit(),
        )
        .map_err(error::Create::Refs)?;
    let mut cache = open_cache(args.cache_dir)?;
    let cached_graph = CachedChangeGraph::new(
        std::iter::once(init_change.author_commit()),
        history,
        typename.clone(),
        object_id,
        authorizing_identity.urn(),
    );
    let history = cached_graph.borrow().history().clone();
    cache.put(init_change.commit().into(), cached_graph)?;
    Ok(CollaborativeObject {
        authorizing_identity_urn: authorizing_identity.urn(),
        typename: args.typename,
        history,
        id: init_change.commit().into(),
    })
}

/// Retrieve a collaborative object which is stored in the
/// `authorizing_identity` person or project identity
pub fn retrieve<R: RefsStorage, I: IdentityStorage, P: AsRef<std::path::Path>>(
    refs_storage: &R,
    identity_storage: &I,
    repo: &git2::Repository,
    authorizing_identity: &dyn AuthorizingIdentity,
    typename: &TypeName,
    oid: &ObjectId,
    cache_dir: Option<P>,
) -> Result<Option<CollaborativeObject>, error::Retrieve<R::Error>> {
    let tip_refs = refs_storage
        .object_references(&authorizing_identity.urn(), typename, oid)
        .map_err(error::Retrieve::Refs)?;
    tracing::trace!(refs=?tip_refs, "retrieving object");
    let mut cache = open_cache(cache_dir)?;
    Ok(CobRefs {
        oid: *oid,
        authorizing_identity,
        typename,
        tip_refs,
    }
    .load_or_materialize::<error::Retrieve<R::Error>, _>(identity_storage, cache.as_mut(), repo)?
    .map(|tg| tg.into()))
}

/// Retrieve all objects of a particular type
pub fn list<R: RefsStorage, P: AsRef<std::path::Path>, I: IdentityStorage>(
    refs_storage: &R,
    identity_storage: &I,
    repo: &git2::Repository,
    authorizing_identity: &dyn AuthorizingIdentity,
    typename: &TypeName,
    cache_dir: Option<P>,
) -> Result<Vec<CollaborativeObject>, error::Retrieve<R::Error>> {
    let references = refs_storage
        .type_references(&authorizing_identity.urn(), typename)
        .map_err(error::Retrieve::Refs)?;
    tracing::trace!(num_objects=?references.len(), "loaded references");
    let mut result = Vec::new();
    let mut cache = open_cache(cache_dir)?;
    for (oid, tip_refs) in references {
        tracing::trace!(object_id=?oid, "loading object");
        let loaded = CobRefs {
            oid,
            authorizing_identity,
            typename,
            tip_refs,
        }
        .load_or_materialize::<error::Retrieve<R::Error>, _>(
            identity_storage,
            cache.as_mut(),
            repo,
        )?;
        match loaded {
            Some(obj) => {
                tracing::trace!(object_id=?oid, "object found in cache");
                result.push(CollaborativeObject::from(obj));
            },
            None => {
                tracing::trace!(object_id=?oid, "object not found in cache");
            },
        }
    }
    Ok(result)
}

/// The data required to create a new object
pub struct UpdateObjectArgs<'a, R: RefsStorage, I: IdentityStorage, P: AsRef<std::path::Path>> {
    /// The refs storage used to find references to the object, and to update
    /// the local reference
    pub refs_storage: &'a R,
    /// The identity storage used to resolve delegates when verifying project
    /// identities
    pub identity_storage: &'a I,
    /// The repo the new object will be stored in
    pub repo: &'a git2::Repository,
    /// The signer used to sign the changes which create the new object
    pub signer: &'a BoxedSigner,
    /// The person corresponding to the signer above
    pub author: &'a VerifiedPerson,
    /// The identity in which the authorization rules of this object will be
    /// checked, i.e. a `VerifiedProject` or a `VerifiedPerson`
    pub authorizing_identity: &'a dyn AuthorizingIdentity,
    /// The directory to use for caching the latest known state of cobs
    pub cache_dir: Option<P>,
    /// The object ID of the object to be updated
    pub object_id: ObjectId,
    /// The typename of the object to be updated
    pub typename: TypeName,
    /// An optional message to add to the commit message of the change
    pub message: Option<String>,
    /// The CRDT changes to add to the object
    pub changes: EntryContents,
}

pub fn update<R: RefsStorage, I: IdentityStorage, P: AsRef<std::path::Path>>(
    args: UpdateObjectArgs<R, I, P>,
) -> Result<CollaborativeObject, error::Update<R::Error>> {
    let UpdateObjectArgs {
        refs_storage,
        identity_storage,
        signer,
        repo,
        author,
        authorizing_identity,
        cache_dir,
        ref typename,
        object_id,
        changes,
        message,
    } = args;
    if !is_signer_for(signer, author) {
        return Err(error::Update::SignerIsNotAuthor);
    }

    let existing_refs = refs_storage
        .object_references(&authorizing_identity.urn(), typename, &object_id)
        .map_err(error::Update::Refs)?;

    let previous_ref = if let Some(ref local) = existing_refs.local {
        Some(local.peel_to_commit()?.id())
    } else {
        None
    };

    let mut cache = open_cache(cache_dir)?;
    let cached = CobRefs {
        authorizing_identity,
        typename,
        oid: object_id,
        tip_refs: existing_refs,
    }
    .load_or_materialize::<error::Update<R::Error>, _>(identity_storage, cache.as_mut(), repo)?
    .ok_or(error::Update::NoSuchObject)?;

    let change = change::Change::create(
        authorizing_identity.content_id(),
        author.content_id.into(),
        repo,
        signer,
        change::NewChangeSpec {
            tips: Some(cached.borrow().tips().iter().cloned().collect()),
            contents: changes.clone(),
            typename: typename.clone(),
            message,
        },
    )?;

    cached
        .borrow_mut()
        .update_ref(previous_ref, *change.commit(), author.urn(), changes);
    cache.put(object_id, cached.clone())?;

    //let new_commit = *change.commit();
    refs_storage
        .update_ref(
            &authorizing_identity.urn(),
            typename,
            object_id,
            *change.commit(),
        )
        .map_err(error::Update::Refs)?;

    Ok(cached.into())
}

/// Retrieve additional information about the change graph of an object. This
/// is mostly useful for debugging and testing
pub fn changegraph_info_for_object<R: RefsStorage>(
    refs_storage: &R,
    repo: &git2::Repository,
    authorizing_identity: &dyn AuthorizingIdentity,
    typename: &TypeName,
    oid: &ObjectId,
) -> Result<Option<ChangeGraphInfo>, error::Retrieve<R::Error>> {
    let tip_refs = refs_storage
        .object_references(&authorizing_identity.urn(), typename, oid)
        .map_err(error::Retrieve::Refs)?;
    if let Some(graph) =
        ChangeGraph::load(tip_refs.iter(), repo, authorizing_identity, typename, oid)?
    {
        Ok(Some(ChangeGraphInfo {
            object_id: *oid,
            dotviz: graph.graphviz(),
            number_of_nodes: graph.number_of_nodes(),
            tips: graph.tips(),
        }))
    } else {
        Ok(None)
    }
}

fn open_cache<P: AsRef<std::path::Path>>(
    path: Option<P>,
) -> Result<Box<dyn cache::Cache>, std::io::Error> {
    match path {
        Some(p) => {
            let fs_cache = cache::FileSystemCache::open(p.as_ref())?;
            Ok(Box::new(fs_cache))
        },
        None => Ok(Box::new(cache::NoOpCache::new())),
    }
}

/// Everything we need in order to load an object from the cache, or materialize
/// it from the underlying change graph.
struct CobRefs<'a> {
    /// The references to the tips of the object
    tip_refs: ObjectRefs<'a>,
    /// The id of the object
    oid: ObjectId,
    /// The typename of the object
    typename: &'a TypeName,
    /// The identity which authorizes changes to this object
    authorizing_identity: &'a dyn AuthorizingIdentity,
}

impl<'a> CobRefs<'a> {
    fn load_or_materialize<E, I: IdentityStorage>(
        self,
        identity_storage: &I,
        cache: &mut dyn Cache,
        repo: &git2::Repository,
    ) -> Result<Option<Rc<RefCell<CachedChangeGraph>>>, E>
    where
        E: From<cache::Error>,
        E: From<change_graph::Error>,
        E: From<git2::Error>,
    {
        let tip_oids = self
            .tip_refs
            .iter()
            .map(|r| r.peel_to_commit().map(|c| c.id()))
            .collect::<Result<BTreeSet<git2::Oid>, git2::Error>>()?;
        match cache.load(self.oid, &tip_oids)? {
            Some(obj) => {
                tracing::trace!(object_id=?self.oid, ?tip_oids, "object found in cache");
                Ok(Some(obj))
            },
            None => {
                tracing::trace!(object_id=?self.oid, ?tip_oids, "object not found in cache");
                if let Some(graph) = ChangeGraph::load(
                    self.tip_refs.iter(),
                    repo,
                    self.authorizing_identity,
                    self.typename,
                    &self.oid,
                )? {
                    let object = graph.evaluate(identity_storage);
                    let cached = cache::CachedChangeGraph::new(
                        tip_oids,
                        object.history.clone(),
                        self.typename.clone(),
                        self.oid,
                        self.authorizing_identity.urn(),
                    );
                    cache.put(object.id, cached.clone())?;
                    Ok(Some(cached))
                } else {
                    Ok(None)
                }
            },
        }
    }
}

fn is_signer_for(signer: &BoxedSigner, person: &VerifiedPerson) -> bool {
    let person_keys: BTreeSet<&PublicKey> = person.delegations().iter().collect();
    let signer_key: PublicKey = signer.public_key().into();
    person_keys.contains(&signer_key)
}
