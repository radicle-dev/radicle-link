// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use thiserror::Error;

use librad::{
    canonical::Cstring,
    git::Urn,
    git_ext::{OneLevel, RefLike},
    identities::{payload, Doc, Identity, Person, Project},
};

use crate::git;

pub trait HasName {
    fn name(&self) -> &Cstring;
}

impl HasName for payload::Project {
    fn name(&self) -> &Cstring {
        &self.name
    }
}

impl HasName for payload::Person {
    fn name(&self) -> &Cstring {
        &self.name
    }
}

impl<P: HasName> HasName for payload::Payload<P> {
    fn name(&self) -> &Cstring {
        self.subject.name()
    }
}

impl HasName for Person {
    fn name(&self) -> &Cstring {
        &self.subject().name
    }
}

impl HasName for Project {
    fn name(&self) -> &Cstring {
        &self.subject().name
    }
}

pub trait HasUrn {
    fn urn(&self) -> Urn;
}

impl HasUrn for Urn {
    fn urn(&self) -> Urn {
        self.clone()
    }
}

impl<T> HasUrn for Identity<T> {
    fn urn(&self) -> Urn {
        self.urn()
    }
}

#[derive(Debug, Error)]
#[error("the project, at `{0}`, does not have default branch set")]
pub struct MissingDefaultBranch(librad::git::Urn);

pub trait HasBranch {
    fn branch(&self) -> Option<OneLevel>;

    fn branch_or_default(&self) -> OneLevel {
        self.branch().unwrap_or_else(|| (*git::MAIN_BRANCH).clone())
    }

    fn branch_or_die(&self, urn: librad::git::Urn) -> Result<OneLevel, MissingDefaultBranch> {
        self.branch().ok_or(MissingDefaultBranch(urn))
    }
}

impl HasBranch for payload::Project {
    fn branch(&self) -> Option<OneLevel> {
        self.default_branch
            .clone()
            .and_then(|branch| RefLike::try_from(branch.as_str()).ok().map(OneLevel::from))
    }
}

impl HasBranch for payload::PersonPayload {
    fn branch(&self) -> Option<OneLevel> {
        self.get_ext::<person::DefaultBranch>()
            .expect("failed to get default branch")
            .and_then(|branch| {
                RefLike::try_from(branch.name.as_str())
                    .ok()
                    .map(OneLevel::from)
            })
    }
}

impl HasBranch for Person {
    fn branch(&self) -> Option<OneLevel> {
        self.payload().branch()
    }
}

impl<P: HasBranch> HasBranch for payload::Payload<P> {
    fn branch(&self) -> Option<OneLevel> {
        self.subject.branch()
    }

    fn branch_or_default(&self) -> OneLevel {
        self.subject.branch_or_default()
    }

    fn branch_or_die(&self, urn: librad::git::Urn) -> Result<OneLevel, MissingDefaultBranch> {
        self.subject.branch_or_die(urn)
    }
}

impl<T: HasBranch, D> HasBranch for Identity<Doc<payload::Payload<T>, D>> {
    fn branch(&self) -> Option<OneLevel> {
        self.payload().branch()
    }

    fn branch_or_default(&self) -> OneLevel {
        self.payload().branch_or_default()
    }

    fn branch_or_die(&self, urn: librad::git::Urn) -> Result<OneLevel, MissingDefaultBranch> {
        self.payload().branch_or_die(urn)
    }
}

pub mod person {
    use url::Url;

    use librad::identities::payload::HasNamespace;

    use super::*;

    lazy_static! {
        static ref DEFAULT_BRANCH_NAMESPACE: Url =
            Url::parse("https://radicle.xyz/link/person/default_branch").unwrap();
    }

    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct DefaultBranch {
        #[serde(rename = "default_branch")]
        pub name: Cstring,
    }

    impl HasNamespace for DefaultBranch {
        fn namespace() -> &'static Url {
            &DEFAULT_BRANCH_NAMESPACE
        }
    }
}
