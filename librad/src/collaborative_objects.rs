// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use crate::git::{
    identities::{self, any::get as get_identity, local::LocalIdentity, Identities},
    refs::{self, Refs},
    storage::{read::Error as ReadError, ReadOnlyStorage, Storage},
    types::{Namespace, Reference, RefsCategory},
};

use std::{collections::HashMap, convert::TryFrom, str::FromStr};

pub use cob::{
    AuthorizingIdentity,
    ChangeGraphInfo,
    CollaborativeObject,
    CreateObjectArgs,
    History,
    IdentityStorage,
    ObjectId,
    ObjectRefs,
    RefsStorage,
    Schema,
    TypeName,
};
use link_crypto::BoxedSigner;
use link_identities::git::{SomeIdentity, Urn};

mod error {
    use super::RefsError;
    use crate::git::identities::Error as IdentitiesError;
    use cob::error::SchemaParse;
    use link_identities::git::Urn;
    use thiserror::Error;

    #[allow(clippy::large_enum_variant)]
    #[derive(Debug, Error)]
    pub enum Create {
        #[error(transparent)]
        Cob(#[from] cob::error::Create<RefsError>),
        #[error(transparent)]
        ResolveAuth(#[from] ResolveAuthorizer),
        #[error(transparent)]
        InvalidSchema(#[from] SchemaParse),
    }

    #[allow(clippy::large_enum_variant)]
    #[derive(Debug, Error)]
    pub enum Retrieve {
        #[error(transparent)]
        Cob(#[from] cob::error::Retrieve<RefsError>),
        #[error(transparent)]
        ResolveAuth(#[from] ResolveAuthorizer),
    }

    #[allow(clippy::large_enum_variant)]
    #[derive(Debug, Error)]
    pub enum Update {
        #[error(transparent)]
        Cob(#[from] cob::error::Update<RefsError>),
        #[error(transparent)]
        ResolveAuth(#[from] ResolveAuthorizer),
    }

    #[allow(clippy::large_enum_variant)]
    #[derive(Debug, Error)]
    pub enum ResolveAuthorizer {
        #[error("no identity found for {urn}")]
        NoSuchIdentity { urn: Urn },
        #[error(transparent)]
        Identities(#[from] IdentitiesError),
        #[error("{urn} was not a valid authorizing identity")]
        UnknownIdentityType { urn: Urn },
        #[error(transparent)]
        VerifyPerson(#[from] identities::error::VerifyPerson),
    }
}

/// The data required to create a new object
pub struct NewObjectSpec {
    /// A valid JSON schema which uses the vocabulary at <https://alexjg.github.io/automerge-jsonschema/spec>
    pub schema_json: serde_json::Value,
    /// The CRDT history to initialize this object with
    pub history: History,
    /// The typename for this object
    pub typename: TypeName,
    /// An optional message to add to the commit message for the commit which
    /// creates this object
    pub message: Option<String>,
}

/// The data required to update a collaborative object
pub struct UpdateObjectSpec {
    /// The object ID of the object to be updated
    pub object_id: ObjectId,
    /// The typename of the object to be updated
    pub typename: TypeName,
    /// An optional message to add to the commit message of the change
    pub message: Option<String>,
    /// The CRDT changes to add to the object
    pub changes: History,
}

pub struct CollaborativeObjects<'a> {
    signer: BoxedSigner,
    store: &'a Storage,
    cache_dir: Option<std::path::PathBuf>,
}

impl<'a> CollaborativeObjects<'a> {
    pub fn new(
        signer: BoxedSigner,
        store: &'a Storage,
        cache_dir: Option<std::path::PathBuf>,
    ) -> CollaborativeObjects<'a> {
        CollaborativeObjects {
            signer,
            store,
            cache_dir,
        }
    }

    pub fn create(
        &self,
        whoami: &LocalIdentity,
        within_identity: &Urn,
        spec: NewObjectSpec,
    ) -> Result<cob::CollaborativeObject, error::Create> {
        let schema = Schema::try_from(&spec.schema_json)?;
        cob::create_object(cob::CreateObjectArgs {
            refs_storage: self,
            repo: self.store.as_raw(),
            signer: &self.signer,
            author: whoami,
            authorizing_identity: resolve_authorizing_identity(self.store, within_identity)?
                .as_ref(),
            schema,
            history: spec.history,
            typename: spec.typename,
            message: spec.message,
            cache_dir: self.cache_dir.clone(),
        })
        .map_err(error::Create::from)
    }

    pub fn retrieve(
        &self,
        identity_urn: &Urn,
        typename: &cob::TypeName,
        oid: &cob::ObjectId,
    ) -> Result<Option<cob::CollaborativeObject>, error::Retrieve> {
        cob::retrieve(
            self,
            &self,
            self.store.as_raw(),
            resolve_authorizing_identity(self.store, identity_urn)?.as_ref(),
            typename,
            oid,
            self.cache_dir.clone(),
        )
        .map_err(error::Retrieve::from)
    }

    pub fn list(
        &self,
        identity_urn: &Urn,
        typename: &cob::TypeName,
    ) -> Result<Vec<cob::CollaborativeObject>, error::Retrieve> {
        cob::list(
            self,
            &self,
            self.store.as_raw(),
            resolve_authorizing_identity(self.store, identity_urn)?.as_ref(),
            typename,
            self.cache_dir.clone(),
        )
        .map_err(error::Retrieve::from)
    }

    pub fn update(
        &self,
        whoami: &LocalIdentity,
        within_identity: &Urn,
        spec: UpdateObjectSpec,
    ) -> Result<cob::CollaborativeObject, error::Update> {
        cob::update(cob::UpdateObjectArgs {
            refs_storage: self,
            identity_storage: &self,
            signer: &self.signer,
            repo: self.store.as_raw(),
            author: whoami,
            authorizing_identity: resolve_authorizing_identity(self.store, within_identity)?
                .as_ref(),
            object_id: spec.object_id,
            typename: spec.typename,
            message: spec.message,
            changes: spec.changes,
            cache_dir: self.cache_dir.clone(),
        })
        .map_err(error::Update::from)
    }

    pub fn changegraph_info_for_object(
        &self,
        identity_urn: &Urn,
        typename: &cob::TypeName,
        oid: &cob::ObjectId,
    ) -> Result<Option<ChangeGraphInfo>, error::Retrieve> {
        cob::changegraph_info_for_object(
            self,
            self.store.as_raw(),
            resolve_authorizing_identity(self.store, identity_urn)?.as_ref(),
            typename,
            oid,
        )
        .map_err(error::Retrieve::from)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RefsError {
    #[error(transparent)]
    Git2(#[from] git2::Error),
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error(transparent)]
    Refs(#[from] refs::stored::Error),
}

impl<'a> RefsStorage for CollaborativeObjects<'a> {
    type Error = RefsError;

    fn object_references<'b>(
        &'b self,
        project_urn: &Urn,
        typename: &TypeName,
        oid: &ObjectId,
    ) -> Result<cob::ObjectRefs<'b>, Self::Error> {
        let mut local = None;
        if let Some(local_ref) = local_ref(self.store, project_urn, typename, oid)? {
            local = Some(local_ref);
        }
        let glob = remote_glob(project_urn, typename, oid);
        let mut remote = Vec::new();
        let remote_refs: Vec<git2::Reference> = self
            .store
            .references_glob(glob.compile_matcher())?
            .flatten()
            .collect();
        remote.extend(remote_refs);
        Ok(cob::ObjectRefs { local, remote })
    }

    fn type_references<'b>(
        &'b self,
        project_urn: &Urn,
        typename: &TypeName,
    ) -> Result<HashMap<ObjectId, ObjectRefs<'b>>, Self::Error> {
        let matcher = ObjRefMatcher::new(project_urn, typename);

        let refs: git2::References<'a> = self.store.as_raw().references()?;
        let mut result = HashMap::new();
        for reference in refs.into_iter() {
            let reference = reference?;
            if let Some(name) = reference.name() {
                match matcher.match_ref(name) {
                    ObjRefMatch::Local(oid) => {
                        result.entry(oid).or_insert_with(|| ObjectRefs {
                            local: Some(reference),
                            remote: Vec::new(),
                        });
                    },
                    ObjRefMatch::Remote(oid) => {
                        let refs = result.entry(oid).or_insert_with(|| ObjectRefs {
                            local: None,
                            remote: Vec::new(),
                        });
                        refs.remote.push(reference);
                    },
                    ObjRefMatch::NoMatch => {},
                }
            }
        }
        Ok(result)
    }

    fn update_ref(
        &self,
        project_urn: &Urn,
        typename: &TypeName,
        object_id: ObjectId,
        new_commit: git2::Oid,
    ) -> Result<(), Self::Error> {
        let reference = Reference::rad_collaborative_object(
            Namespace::from(project_urn.clone()),
            None,
            typename.clone(),
            object_id,
        );

        tracing::info!(reference=%reference, commit=?new_commit, "adding change to collaborative object");
        self.store
            .as_raw()
            .reference(&reference.to_string(), new_commit, true, "new change")?;

        Refs::update(self.store, project_urn)?;
        Ok(())
    }
}

fn local_ref<'a, S: ReadOnlyStorage>(
    store: &'a S,
    project_urn: &Urn,
    typename: &TypeName,
    oid: &ObjectId,
) -> Result<Option<git2::Reference<'a>>, RefsError> {
    let reference = Reference::rad_collaborative_object(
        Namespace::from(project_urn.clone()),
        None,
        typename.clone(),
        *oid,
    );

    store.reference(&reference).map_err(|e| e.into())
}

fn remote_glob(identity_urn: &Urn, typename: &TypeName, oid: &ObjectId) -> globset::Glob {
    let namespace = Namespace::from(identity_urn);

    globset::Glob::new(
        format!(
            "refs/namespaces/{}/refs/remotes/**/{}/{}/{}",
            namespace.to_string(),
            RefsCategory::Cob.to_string(),
            typename.to_string(),
            oid.to_string(),
        )
        .as_str(),
    )
    .unwrap()
}

fn resolve_authorizing_identity(
    store: &Storage,
    urn: &Urn,
) -> Result<Box<dyn AuthorizingIdentity>, error::ResolveAuthorizer> {
    let identities: Identities<'_, SomeIdentity> = Identities::from(store.as_raw());
    let id = get_identity(store, urn)?
        .ok_or_else(|| error::ResolveAuthorizer::NoSuchIdentity { urn: urn.clone() })?;
    match id {
        SomeIdentity::Person(p) => {
            let verified = identities.as_person().verify(p.content_id.into())?;
            Ok(Box::new(verified))
        },
        SomeIdentity::Project(_) => {
            let verified = identities::project::verify(store, urn)?
                .ok_or_else(|| error::ResolveAuthorizer::NoSuchIdentity { urn: urn.clone() })?;
            Ok(Box::new(verified))
        },
        _ => Err(error::ResolveAuthorizer::UnknownIdentityType { urn: urn.clone() }),
    }
}

#[derive(Debug, PartialEq)]
pub enum ObjRefMatch {
    Local(ObjectId),
    Remote(ObjectId),
    NoMatch,
}

pub struct ObjRefMatcher {
    remote_regex: regex::Regex,
    local_regex: regex::Regex,
}

impl ObjRefMatcher {
    pub fn new(project_urn: &Urn, typename: &TypeName) -> ObjRefMatcher {
        let remote_ref_str = format!(
            r"refs/namespaces/{}/refs/remotes/([0-9a-zA-Z]+)/{}/{}/([0-9a-z]+)",
            project_urn.encode_id(),
            RefsCategory::Cob.to_string(),
            typename.regex_safe_string(),
        );
        let remote_regex = regex::Regex::new(remote_ref_str.as_str()).unwrap();

        let local_ref_str = format!(
            r"refs/namespaces/{}/refs/{}/{}/([0-9a-z]+)",
            project_urn.encode_id(),
            RefsCategory::Cob.to_string(),
            typename.regex_safe_string(),
        );
        let local_regex = regex::Regex::new(local_ref_str.as_str()).unwrap();
        ObjRefMatcher {
            remote_regex,
            local_regex,
        }
    }

    pub fn match_ref(&self, ref_str: &str) -> ObjRefMatch {
        if let Some(cap) = self.remote_regex.captures(ref_str) {
            let oid_str = &cap[2];
            if let Ok(oid) = ObjectId::from_str(oid_str) {
                ObjRefMatch::Remote(oid)
            } else {
                ObjRefMatch::NoMatch
            }
        } else if let Some(cap) = self.local_regex.captures(ref_str) {
            let oid_str = &cap[1];
            // Safe for the same reasoning as above
            if let Ok(oid) = ObjectId::from_str(oid_str) {
                ObjRefMatch::Local(oid)
            } else {
                ObjRefMatch::NoMatch
            }
        } else {
            ObjRefMatch::NoMatch
        }
    }
}

impl<'a> IdentityStorage for &'a CollaborativeObjects<'a> {
    type Error = git2::Error;

    fn delegate_oid(&self, urn: Urn) -> Result<git2::Oid, Self::Error> {
        let refname = Reference::rad_id(Namespace::from(urn));
        self.store.as_raw().refname_to_id(&refname.to_string())
    }
}
