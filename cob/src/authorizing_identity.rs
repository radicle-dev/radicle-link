// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_crypto::PublicKey;
use link_identities::{git::Urn, Person, Project, VerifiedPerson, VerifiedProject};

use std::collections::BTreeSet;

pub enum AuthDecision {
    Authorized,
    NotAuthorized { reason: &'static str },
}

pub trait AuthorizingIdentity: std::fmt::Debug {
    fn urn(&self) -> Urn;
    fn check_authorization(&self, principal: &VerifiedPerson) -> AuthDecision;
    fn tip_oid(&self) -> git2::Oid;
}

impl AuthorizingIdentity for VerifiedPerson {
    fn urn(&self) -> Urn {
        let p: &Person = &*self;
        p.urn()
    }

    fn check_authorization(&self, principal: &VerifiedPerson) -> AuthDecision {
        if self.urn() == principal.urn() {
            AuthDecision::Authorized
        } else {
            AuthDecision::NotAuthorized{reason: "attempting to update a cob in a person identity with a principal which is not that identity"}
        }
    }

    fn tip_oid(&self) -> git2::Oid {
        self.content_id.into()
    }
}

impl AuthorizingIdentity for VerifiedProject {
    fn urn(&self) -> Urn {
        let p: &Project = &*self;
        p.urn()
    }

    fn check_authorization(&self, principal: &VerifiedPerson) -> AuthDecision {
        if is_maintainer(self, principal) {
            AuthDecision::Authorized
        } else {
            AuthDecision::NotAuthorized{
                reason: "attempted to modify a cob in a project identity with a principal which is not a maintainer"
            }
        }
    }

    fn tip_oid(&self) -> git2::Oid {
        self.content_id.into()
    }
}

fn is_maintainer(project: &VerifiedProject, person: &VerifiedPerson) -> bool {
    let keys: BTreeSet<&PublicKey> = person.delegations().iter().collect();
    tracing::debug!(?keys, "person keys");
    let project_keys: Vec<&PublicKey> = project
        .delegations()
        .iter()
        .map(|d| {
            d.map_left(|p| vec![p])
                .map_right(|p| p.delegations().iter().collect::<Vec<&PublicKey>>())
                .into_inner()
        })
        .flatten()
        .collect();
    tracing::debug!(?project_keys);
    project
        .delegations()
        .eligible(keys)
        .ok()
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}
