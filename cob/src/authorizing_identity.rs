// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_identities::{git::Urn, Person, Project, VerifiedPerson, VerifiedProject};

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

    fn check_authorization(&self, _principal: &VerifiedPerson) -> AuthDecision {
        AuthDecision::Authorized
    }

    fn content_id(&self) -> git2::Oid {
        self.content_id.into()
    }
}
