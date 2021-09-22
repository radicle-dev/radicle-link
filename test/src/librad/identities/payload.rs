// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use either::Either;
use link_canonical::Cstring;
use proptest::prelude::*;
use url::Url;

use librad::{
    git_ext::Oid,
    identities::{
        delegation,
        payload::{
            HasNamespace,
            KeyOrUrn,
            Person,
            PersonDelegations,
            PersonPayload,
            Project,
            ProjectDelegations,
            ProjectPayload,
            SomePayload,
        },
        urn::Urn,
    },
};

use crate::{
    canonical::gen_cstring,
    librad::{identities::urn::gen_oid, keys::gen_public_key},
};

lazy_static! {
    static ref UPSTREAM_USER_NAMESPACE: Url =
        Url::parse("https://radicle.xyz/upstream/user/v1").unwrap();
    static ref UPSTREAM_PROJECT_NAMESPACE: Url =
        Url::parse("https://radicle.xyz/upstream/project/v1").unwrap();
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UpstreamUser {
    #[serde(rename = "radicle-registry-name")]
    pub registered_as: Cstring,
}

pub fn gen_person() -> impl Strategy<Value = Person> {
    gen_cstring().prop_map(|name| Person { name })
}

pub fn gen_upstream_user() -> impl Strategy<Value = UpstreamUser> {
    gen_cstring().prop_map(|registered_as| UpstreamUser { registered_as })
}

impl HasNamespace for UpstreamUser {
    fn namespace() -> &'static Url {
        &UPSTREAM_USER_NAMESPACE
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UpstreamProject {
    #[serde(rename = "radicle-registry-name")]
    pub registered_as: Cstring,
}

pub fn gen_upstream_project() -> impl Strategy<Value = UpstreamProject> {
    gen_cstring().prop_map(|registered_as| UpstreamProject { registered_as })
}

pub fn gen_project() -> impl Strategy<Value = Project> {
    (
        gen_cstring(),
        proptest::option::of(gen_cstring()),
        proptest::option::of(gen_cstring()),
    )
        .prop_map(|(name, description, default_branch)| Project {
            name,
            description,
            default_branch,
        })
}

impl HasNamespace for UpstreamProject {
    fn namespace() -> &'static Url {
        &UPSTREAM_PROJECT_NAMESPACE
    }
}

pub fn gen_person_payload() -> impl Strategy<Value = PersonPayload> {
    (gen_person(), proptest::option::of(gen_upstream_user())).prop_map(|(person, up)| {
        let mut p = PersonPayload::new(person);
        if let Some(up) = up {
            p.set_ext(up).unwrap();
        }
        p
    })
}

pub fn gen_project_payload() -> impl Strategy<Value = ProjectPayload> {
    (gen_project(), proptest::option::of(gen_upstream_project())).prop_map(|(project, up)| {
        let mut p = ProjectPayload::new(project);
        if let Some(up) = up {
            p.set_ext(up).unwrap();
        }
        p
    })
}

pub fn gen_payload() -> impl Strategy<Value = SomePayload> {
    prop_oneof![
        gen_person_payload().prop_map(SomePayload::Person),
        gen_project_payload().prop_map(SomePayload::Project)
    ]
}

pub fn gen_person_delegations() -> impl Strategy<Value = PersonDelegations> {
    proptest::collection::btree_set(gen_public_key(), 1..32).prop_map(|keys| {
        PersonDelegations::from(delegation::Direct::try_from_iter(keys.into_iter()).unwrap())
    })
}

pub fn gen_key_or_urn() -> impl Strategy<Value = KeyOrUrn<Oid>> {
    prop_oneof![
        gen_public_key().prop_map(|pk| KeyOrUrn::from(Either::Left(pk))),
        gen_oid(git2::ObjectType::Tree)
            .prop_map(|oid| KeyOrUrn::from(Either::Right(Urn::new(oid))))
    ]
}

pub fn gen_project_delegations() -> impl Strategy<Value = ProjectDelegations<Oid>> {
    proptest::collection::btree_set(gen_key_or_urn(), 1..64)
        .prop_map(|inner| inner.into_iter().collect::<ProjectDelegations<Oid>>())
}
