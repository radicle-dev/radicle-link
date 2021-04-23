// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use either::Either;
use proptest::prelude::*;
use url::Url;

use git_ext::{Oid, RefLike};

use crate::{internal::canonical::Cstring, keys::gen::gen_public_key};

use super::{
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
    Urn,
};

impl Arbitrary for Person {
    type Parameters = ();
    type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        any::<Cstring>().prop_map(|name| Person { name })
    }
}

impl Arbitrary for Project {
    type Parameters = ();
    // Silly clippy: this _is_ a type definition
    #[allow(clippy::type_complexity)]
    type Strategy = prop::strategy::Map<
        (
            <Cstring as Arbitrary>::Strategy,
            <Option<Cstring> as Arbitrary>::Strategy,
            <Option<Cstring> as Arbitrary>::Strategy,
        ),
        fn((Cstring, Option<Cstring>, Option<Cstring>)) -> Self,
    >;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        Strategy::prop_map(
            (
                any::<Cstring>(),
                any::<Option<Cstring>>(),
                any::<Option<Cstring>>(),
            ),
            |(name, description, default_branch)| Project {
                name,
                description,
                default_branch,
            },
        )
    }
}

pub fn gen_oid(kind: git2::ObjectType) -> impl Strategy<Value = Oid> {
    any::<Vec<u8>>()
        .prop_map(move |bytes| git2::Oid::hash_object(kind, &bytes).map(Oid::from).unwrap())
}

pub fn gen_urn() -> impl Strategy<Value = Urn<Oid>> {
    (
        gen_oid(git2::ObjectType::Tree),
        prop::option::of(prop::collection::vec("[a-z0-9]+", 1..3)),
    )
        .prop_map(|(id, path)| {
            let path = path.map(|elems| {
                RefLike::try_from(elems.join("/")).unwrap_or_else(|e| {
                    panic!(
                        "Unexpected error generating a RefLike from `{}`: {}",
                        elems.join("/"),
                        e
                    )
                })
            });
            Urn { id, path }
        })
}

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

impl Arbitrary for UpstreamUser {
    type Parameters = ();
    type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        any::<Cstring>().prop_map(|registered_as| Self { registered_as })
    }
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

impl Arbitrary for UpstreamProject {
    type Parameters = ();
    type Strategy = prop::strategy::Map<<Cstring as Arbitrary>::Strategy, fn(Cstring) -> Self>;

    fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
        any::<Cstring>().prop_map(|registered_as| Self { registered_as })
    }
}

impl HasNamespace for UpstreamProject {
    fn namespace() -> &'static Url {
        &UPSTREAM_PROJECT_NAMESPACE
    }
}

pub fn gen_person_payload() -> impl Strategy<Value = PersonPayload> {
    (any::<Person>(), proptest::option::of(any::<UpstreamUser>())).prop_map(|(person, up)| {
        let mut p = PersonPayload::new(person);
        if let Some(up) = up {
            p.set_ext(up).unwrap();
        }
        p
    })
}

pub fn gen_project_payload() -> impl Strategy<Value = ProjectPayload> {
    (
        any::<Project>(),
        proptest::option::of(any::<UpstreamProject>()),
    )
        .prop_map(|(project, up)| {
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
    proptest::collection::btree_set(gen_public_key(), 1..32).prop_map(PersonDelegations)
}

pub fn gen_key_or_urn() -> impl Strategy<Value = KeyOrUrn<Oid>> {
    prop_oneof![
        gen_public_key().prop_map(|pk| KeyOrUrn {
            inner: Either::Left(pk)
        }),
        gen_oid(git2::ObjectType::Tree).prop_map(|oid| KeyOrUrn {
            inner: Either::Right(Urn::new(oid))
        })
    ]
}

pub fn gen_project_delegations() -> impl Strategy<Value = ProjectDelegations<Oid>> {
    proptest::collection::btree_set(gen_key_or_urn(), 1..64)
        .prop_map(|inner| ProjectDelegations { inner })
}
