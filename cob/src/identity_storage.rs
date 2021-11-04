// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_identities::git::{error, Identities, Person, SomeIdentity, Urn, VerifiedPerson};

use super::AuthorizingIdentity;

/// Abstracts the way in which Urns are represented within a particular
/// repository. This primarily exist because
/// [`link_identities::git::Identities<'_, Project>::verify`] requires that we
/// pass it a function which knows how to lookup delegated identities from their
/// URNs.
pub trait IdentityStorage {
    type Error: std::error::Error + Send + Sync + 'static;
    /// The OID which corresponds to a particular urn
    fn delegate_oid(&self, urn: Urn) -> Result<git2::Oid, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error(transparent)]
    VerifyPerson(#[from] error::VerifyPerson),
    #[error(transparent)]
    VerifyProject(#[from] error::VerifyProject),
    #[error(transparent)]
    LoadIdentity(#[from] error::Load),
    #[error("could not verify identity up to tip")]
    CouldNotVerifyTip,
    #[error("{oid} did not refer to a valid authorizing identity")]
    UnknownIdentityType { oid: radicle_git_ext::Oid },
    #[error(transparent)]
    Git(#[from] git2::Error),
}

pub(crate) fn lookup_person(
    repo: &git2::Repository,
    oid: git2::Oid,
) -> Result<Option<VerifiedPerson>, LookupError> {
    let identities: Identities<'_, Person> = Identities::from(repo);
    identities.verify(oid).map(Some).map_err(LookupError::from)
}

pub(crate) fn lookup_authorizing_identity<I: IdentityStorage>(
    ids: &I,
    repo: &git2::Repository,
    oid: git2::Oid,
) -> Result<Box<dyn AuthorizingIdentity>, LookupError> {
    let identities: Identities<'_, SomeIdentity> = Identities::from(repo);
    let id = identities.some_identity(oid)?;
    let (authorizer, tip) = match id {
        SomeIdentity::Person(_) => {
            let verified = identities.as_person().verify(oid)?;
            let tip = verified.content_id;
            let authorizer: Box<dyn AuthorizingIdentity> = Box::new(verified);
            (authorizer, tip)
        },
        SomeIdentity::Project(_) => {
            let verified = identities
                .as_project()
                .verify(oid, |u| ids.delegate_oid(u))?;
            let tip = verified.content_id;
            let authorizer: Box<dyn AuthorizingIdentity> = Box::new(verified);
            (authorizer, tip)
        },
        _ => {
            return Err(LookupError::UnknownIdentityType { oid: oid.into() });
        },
    };
    if tip != oid.into() {
        Err(LookupError::CouldNotVerifyTip)
    } else {
        Ok(authorizer)
    }
}
