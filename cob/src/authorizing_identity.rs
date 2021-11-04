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
    /// The URN of this identity
    fn urn(&self) -> Urn;
    /// Check whether `principal` is allowed to make changes to COBs in this
    /// `AuthorizingIdentity`
    fn check_authorization(&self, principal: &VerifiedPerson) -> AuthDecision;
    /// The OID of the tip of this identity
    fn content_id(&self) -> git2::Oid;
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
            AuthDecision::NotAuthorized {
                reason: "attempting to update a cob in a person identity with \
                    a principal which is not that identity",
            }
        }
    }

    fn content_id(&self) -> git2::Oid {
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
            AuthDecision::NotAuthorized {
                reason: "attempted to modify a cob in a project identity with \
                    a principal which is not a maintainer",
            }
        }
    }

    fn content_id(&self) -> git2::Oid {
        self.content_id.into()
    }
}

fn is_maintainer(project: &VerifiedProject, person: &VerifiedPerson) -> bool {
    let keys: BTreeSet<&PublicKey> = person.delegations().iter().collect();
    project
        .delegations()
        .eligible(keys)
        .ok()
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}
