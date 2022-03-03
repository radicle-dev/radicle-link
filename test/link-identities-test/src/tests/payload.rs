// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::fmt::Debug;

use link_crypto::SecretKey;
use link_identities::payload::{
    Ext,
    Person,
    PersonDelegations,
    PersonPayload,
    Project,
    ProjectDelegations,
    ProjectPayload,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use radicle_git_ext::Oid;
use test_helpers::roundtrip;

use crate::gen::payload::{
    gen_key_or_urn,
    gen_payload,
    gen_person_delegations,
    gen_project_delegations,
    UpstreamProject,
    UpstreamUser,
};

#[test]
fn person_example() {
    let payload = PersonPayload::new(Person {
        name: "cloudhead".into(),
    })
    .with_ext(UpstreamUser {
        registered_as: "cloudhead".into(),
    })
    .unwrap()
    .with_ext(Ext {
        namespace: "https://radicle.xyz/upstream/ethereum/v1".parse().unwrap(),
        val: "0x42",
    })
    .unwrap();

    let json_pretty = r#"{
  "https://radicle.xyz/link/identities/person/v1": {
    "name": "cloudhead"
  },
  "https://radicle.xyz/upstream/ethereum/v1": "0x42",
  "https://radicle.xyz/upstream/user/v1": {
    "radicle-registry-name": "cloudhead"
  }
}"#;

    assert_eq!(
        serde_json::to_string_pretty(&payload).unwrap(),
        json_pretty.to_owned()
    )
}

#[test]
fn remove_ext() {
    let mut payload = PersonPayload::new(Person {
        name: "cloudhead".into(),
    })
    .with_ext(UpstreamUser {
        registered_as: "cloudhead".into(),
    })
    .unwrap();

    assert_eq!(
        payload.get_ext::<UpstreamUser>().unwrap().unwrap(),
        UpstreamUser {
            registered_as: "cloudhead".into(),
        }
    );

    let result = payload.remove_ext::<UpstreamUser>();
    assert_eq!(
        result.unwrap(),
        Some(UpstreamUser {
            registered_as: "cloudhead".into()
        })
    );
    assert_eq!(payload.get_ext::<UpstreamUser>().unwrap(), None);
}

#[test]
fn project_example() {
    let descr = "nom, eating data byte by byte

nom is a parser combinators library written in Rust.";

    let payload = ProjectPayload::new(Project {
        name: "nom".into(),
        description: Some(descr.into()),
        default_branch: Some("mistress".into()),
    })
    .with_ext(UpstreamProject {
        registered_as: "nomnomnom".into(),
    })
    .unwrap();

    let json_pretty = r#"{
  "https://radicle.xyz/link/identities/project/v1": {
    "name": "nom",
    "description": "nom, eating data byte by byte\n\nnom is a parser combinators library written in Rust.",
    "default_branch": "mistress"
  },
  "https://radicle.xyz/upstream/project/v1": {
    "radicle-registry-name": "nomnomnom"
  }
}"#;

    assert_eq!(
        serde_json::to_string_pretty(&payload).unwrap(),
        json_pretty.to_owned()
    );

    roundtrip::cjson(payload)
}

fn duplicate_delegation<T>()
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let k = SecretKey::new().public();
    let d = vec![k, k];

    let ser = serde_json::to_string(&d).unwrap();
    assert!(matches!(
        serde_json::from_str::<T>(&ser),
        Err(e) if e.to_string().starts_with("duplicate delegation")
    ))
}

#[test]
fn duplicate_person_delegation() {
    duplicate_delegation::<PersonDelegations>()
}

#[test]
fn duplicate_project_delegation() {
    duplicate_delegation::<ProjectDelegations<Oid>>()
}

fn empty_delegations<T>()
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let ser = serde_json::to_string(&[] as &[T]).unwrap();
    assert!(matches!(
        serde_json::from_str::<T>(&ser),
        Err(e) if e.to_string().starts_with("no delegations")
    ))
}

#[test]
fn empty_person_delegations() {
    empty_delegations::<PersonDelegations>()
}

#[test]
fn empty_project_delegations() {
    empty_delegations::<ProjectDelegations<Oid>>()
}

#[test]
fn duplicate_namespace() {
    let json = r#"{
            "https://radicle.xyz/link/identities/project/v1": {
                "name": "foo"
            },
            "https://semantic.me/ld": {},
            "https://semantic.me/ld": {},
        }"#;

    assert!(matches!(
        serde_json::from_str::<ProjectPayload>(json),
        Err(e) if e.to_string().starts_with("duplicate field")
    ))
}

#[test]
fn subject_from_the_future() {
    let json = r#"{
            "https://radicle.xyz/link/identities/project/v1": {
                "name": "foo"
            },
            "https://radicle.xyz/link/identities/project/v2": {
                "name": "foo",
                "bdfl": "dylan"
            }
        }"#;

    assert!(matches!(
        serde_json::from_str::<ProjectPayload>(json),
        Err(e) if e.to_string().starts_with("multiple subject versions in document")
    ))
}

#[test]
fn null_extension() {
    let json = r#"{
            "https://radicle.xyz/link/identities/person/v1": {
                "name": "foo"
            },
            "https://semantic.me/ld": null
        }"#;
    let payload = serde_json::from_str::<PersonPayload>(json).unwrap();

    let json_actual = serde_json::to_string_pretty(&payload).unwrap();

    let json_expected = r#"{
  "https://radicle.xyz/link/identities/person/v1": {
    "name": "foo"
  }
}"#;
    assert_eq!(json_actual, json_expected);
}

/// All serialisation roundtrips required for payload types
fn trippin<A>(a: A)
where
    A: Clone + Debug + PartialEq + serde::Serialize + serde::de::DeserializeOwned,
{
    roundtrip::cjson(a.clone());
    roundtrip::json(a)
}

proptest! {
    #[test]
    fn any_payload_roundtrip(payload in gen_payload()) {
        trippin(payload)
    }

    #[test]
    fn person_delegations_roundtrip(delegations in gen_person_delegations()) {
        trippin(delegations)
    }

    #[test]
    fn project_delegations_roundtrip(delegations in gen_project_delegations()) {
        trippin(delegations)
    }

    #[test]
    fn key_or_urn(key_or_urn in gen_key_or_urn()) {
        roundtrip::json(key_or_urn)
    }
}
