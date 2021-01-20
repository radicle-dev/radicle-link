// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, path::PathBuf};

use either::Either;
use git_ext::{self as ext, is_not_found_err};
use std_ext::result::ResultExt as _;

use crate::{
    identities::{
        delegation,
        generic,
        payload::{
            PersonDelegations,
            PersonPayload,
            ProjectDelegations,
            ProjectPayload,
            SomeDelegations,
            SomePayload,
        },
        sign::Signatures,
        urn::Urn,
    },
    internal::canonical::Cjson,
};

use super::{error, ContentId, Doc, Identity, Person, Project, Revision, SomeIdentity};

pub type ByOid<'a> = (&'a git2::Repository, git2::Oid);

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum SomeDoc {
    Person(Doc<PersonPayload, PersonDelegations>),
    Project(Doc<ProjectPayload, ProjectDelegations<Revision>>),
}

impl<'de> serde::Deserialize<'de> for SomeDoc {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let doc: Doc<SomePayload, SomeDelegations<Revision, ext::oid::FromMultihashError>> =
            serde::Deserialize::deserialize(deserializer)?;

        match (doc.payload, doc.delegations) {
            (SomePayload::Person(payload), SomeDelegations::Person(delegations)) => {
                Ok(Self::Person(Doc {
                    version: doc.version,
                    replaces: doc.replaces,
                    payload,
                    delegations,
                }))
            },

            (SomePayload::Project(payload), SomeDelegations::Project(delegations)) => {
                Ok(Self::Project(Doc {
                    version: doc.version,
                    replaces: doc.replaces,
                    payload,
                    delegations,
                }))
            },

            (SomePayload::Project(payload), SomeDelegations::Person(delegations)) => {
                Ok(Self::Project(Doc {
                    version: doc.version,
                    replaces: doc.replaces,
                    payload,
                    delegations: (*delegations).iter().copied().map(Either::Left).collect(),
                }))
            },

            _ => Err(serde::de::Error::custom("payload <-> delegations mismatch")),
        }
    }
}

struct Any<'a, Doc> {
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
    identity: generic::Identity<Doc, Revision, ContentId>,
}

type AnyPerson<'a> = Any<'a, Doc<PersonPayload, PersonDelegations>>;
type AnyProject<'a> = Any<'a, Doc<ProjectPayload, ProjectDelegations<Revision>>>;

impl<'a> From<AnyPerson<'a>> for Person {
    fn from(any: AnyPerson<'a>) -> Self {
        any.identity.map(|doc| doc.second(delegation::Direct::from))
    }
}

impl<'a> TryFrom<AnyProject<'a>> for Project {
    type Error = error::Load;

    fn try_from(any: AnyProject<'a>) -> Result<Self, Self::Error> {
        let Any {
            repo,
            tree,
            identity,
        } = any;

        identity
            .map(|doc| {
                doc.try_second(|delegations| {
                    let delegations = delegations
                        .into_iter()
                        .map(|d| match d.into() {
                            Either::Left(key) => Ok(Either::Left(key)),
                            Either::Right(urn) => {
                                resolve_inlined_person(repo, &tree, urn).map(Either::Right)
                            },
                        })
                        .collect::<Result<Vec<Either<_, _>>, _>>()?;

                    delegation::Indirect::try_from_iter(delegations).map_err(error::Load::from)
                })
            })
            .transpose()
    }
}

impl<'a, Doc> TryFrom<ByOid<'a>> for Any<'a, Doc>
where
    Doc: serde::Serialize + serde::de::DeserializeOwned,
{
    type Error = error::Load;

    fn try_from((repo, oid): ByOid<'a>) -> Result<Self, Self::Error> {
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        let (root, doc_blob) = {
            // borrowck insists we drop this before returning
            let first_blob_entry = tree
                .iter()
                .find(|entry| entry.kind() == Some(git2::ObjectType::Blob))
                .ok_or(error::Load::MissingDoc)?;

            let name = String::from_utf8_lossy(first_blob_entry.name_bytes());
            let root = git2::Oid::from_str(&name)?;
            let blob = first_blob_entry
                .to_object(repo)?
                .into_blob()
                .expect("we filtered by git2::ObjectType::Blob, so this must be a blob. qed");

            Ok::<_, Self::Error>((root, blob))
        }?;

        // Check that the root doc exists
        {
            let _root_doc = repo
                .find_blob(root)
                .or_matches(is_not_found_err, || Err(error::Load::MissingRoot))?;
        }

        let doc: Doc = Cjson::<Doc>::from_slice(doc_blob.content())?.into_inner();

        // Verify that the doc is in canonical form (ie. the git hash is stable)
        {
            let canonical = Cjson(&doc).canonical_form()?;
            let hash = git2::Oid::hash_object(git2::ObjectType::Blob, &canonical)?;
            if hash != doc_blob.id() {
                return Err(error::Load::DigestMismatch);
            }
        }

        let identity = generic::Identity {
            content_id: commit.id().into(),
            root: root.into(),
            revision: tree.id().into(),
            doc,
            signatures: Signatures::try_from(&commit)?,
        };

        Ok(Self {
            repo,
            tree,
            identity,
        })
    }
}

impl<'a> TryFrom<ByOid<'a>> for SomeIdentity {
    type Error = error::Load;

    fn try_from((repo, oid): ByOid<'a>) -> Result<Self, Self::Error> {
        // Lighting a scent stick for Applicative

        let Any {
            repo,
            tree,
            identity:
                generic::Identity {
                    content_id,
                    root,
                    revision,
                    doc,
                    signatures,
                },
        } = Any::<'a, SomeDoc>::try_from((repo, oid))?;

        match doc {
            SomeDoc::Person(person) => {
                let person = Person::from(Any {
                    repo,
                    tree,
                    identity: Identity {
                        content_id,
                        root,
                        revision,
                        doc: person,
                        signatures,
                    },
                });
                Ok(SomeIdentity::Person(person))
            },

            SomeDoc::Project(project) => {
                let project = Project::try_from(Any {
                    repo,
                    tree,
                    identity: Identity {
                        content_id,
                        root,
                        revision,
                        doc: project,
                        signatures,
                    },
                })?;
                Ok(SomeIdentity::Project(project))
            },
        }
    }
}

impl<'a> TryFrom<ByOid<'a>> for Person {
    type Error = error::Load;

    fn try_from(git: ByOid<'a>) -> Result<Self, Self::Error> {
        Ok(Person::from(Any::try_from(git)?))
    }
}

impl<'a> TryFrom<ByOid<'a>> for Project {
    type Error = error::Load;

    fn try_from(git: ByOid<'a>) -> Result<Self, Self::Error> {
        Project::try_from(Any::try_from(git)?)
    }
}

type InlinedPerson = generic::Identity<Doc<PersonPayload, PersonDelegations>, Revision, ContentId>;

#[tracing::instrument(level = "debug", skip(repo, tree), err)]
fn resolve_inlined_person(
    repo: &git2::Repository,
    tree: &git2::Tree,
    urn: Urn<Revision>,
) -> Result<Person, error::Load> {
    let path = PathBuf::from(format!("delegations/{}", urn.encode_id()));
    let blob = tree
        .get_path(&path)?
        .to_object(repo)?
        .into_blob()
        .map_err(|obj| error::Load::NotABlob(path, obj.kind()))?;

    Ok(Cjson::<InlinedPerson>::from_slice(blob.content())?
        .into_inner()
        .map(|doc| doc.second(delegation::Direct::from)))
}
