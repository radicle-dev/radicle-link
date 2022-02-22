// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{convert::TryFrom, fmt::Debug, iter};

use git_ref_format::{lit, name, refname, Component, Qualified, RefString};
use link_tracking::{
    config::{
        cobs::{self, Filter, Pattern, Policy},
        Cobs,
        Config,
    },
    git::config::{ObjectId, TypeName, DATA_REFS},
};
use once_cell::sync::Lazy;
use proptest::prelude::*;

fn refs_cobs(ty: &TypeName, id: &ObjectId) -> Qualified<'static> {
    (lit::Refs, lit::Cobs, ty, id).into()
}

pub mod gen {
    use super::*;

    pub static SOME_COMP: Lazy<Component> = Lazy::new(|| name::component!("abc"));

    pub fn cobs_simple<T, I>() -> impl Strategy<Value = Cobs<T, I>>
    where
        T: Clone + Debug + Ord,
        I: Clone + Debug + Ord,
    {
        prop_oneof![Just(Cobs::allow_all()), Just(Cobs::deny_all())]
    }

    pub fn config_simple<T, I>() -> impl Strategy<Value = Config<T, I>>
    where
        T: Clone + Debug + Ord,
        I: Clone + Debug + Ord,
    {
        (any::<bool>(), cobs_simple()).prop_map(|(data, cobs)| Config { data, cobs })
    }

    pub fn unknown_category() -> impl Strategy<Value = Qualified<'static>> {
        "\\w+"
            .prop_map(|s| RefString::try_from(s).unwrap())
            .prop_filter("values must not be in DATA_REFS", |rs| {
                !DATA_REFS.iter().any(|dr| dr.as_ref() == rs.as_ref())
            })
            .prop_map(|rs| {
                Qualified::from_components(
                    Component::from_refstring(rs).unwrap(),
                    SOME_COMP.clone(),
                    None,
                )
            })
    }

    pub fn cobs() -> impl Strategy<Value = Cobs<TypeName, ObjectId>> {
        prop::collection::vec((type_name().prop_map(cobs::TypeName::Type), filter()), 1..5)
            .prop_map(|xs| xs.into_iter().collect())
    }

    pub fn object_id() -> impl Strategy<Value = ObjectId> {
        any::<[u8; 32]>().prop_map(|bytes| {
            git2::Oid::hash_object(git2::ObjectType::Commit, &bytes)
                .map(cob::ObjectId::from)
                .map(ObjectId)
                .unwrap()
        })
    }

    pub fn type_name() -> impl Strategy<Value = TypeName> {
        prop::collection::vec("[a-zA-Z0-9]+", 1..5)
            .prop_map(|xs| xs.join(".").parse().map(TypeName).unwrap())
    }

    pub fn policy() -> impl Strategy<Value = Policy> {
        prop_oneof![Just(Policy::Allow), Just(Policy::Deny)]
    }

    pub fn pattern() -> impl Strategy<Value = Pattern<ObjectId>> {
        prop_oneof![Just(Pattern::Wildcard), non_wildcard_pattern()]
    }

    pub fn non_wildcard_pattern() -> impl Strategy<Value = Pattern<ObjectId>> {
        prop::collection::btree_set(object_id(), 1..5).prop_map(Pattern::Objects)
    }

    pub fn filter() -> impl Strategy<Value = Filter<ObjectId>> {
        (policy(), pattern()).prop_map(|(policy, pattern)| Filter { policy, pattern })
    }

    pub fn some_cobs_ref() -> impl Strategy<Value = Qualified<'static>> {
        (type_name(), object_id()).prop_map(|(ty, id)| refs_cobs(&ty, &id))
    }
}

pub mod filter {
    use super::*;

    pub mod cob {
        use super::*;

        proptest! {
            #[test]
            fn pass(refname in gen::some_cobs_ref()) {
                assert_eq!(
                    Policy::Allow,
                    Config {
                        data: true,
                        cobs: Cobs::allow_all(),
                    }.policy_for(&refname)
                )
            }

            #[test]
            fn fails_with_trailing(config in gen::config_simple::<TypeName, ObjectId>()) {
                let q = refname!("refs/cobs/patches/hnrk84dch6jk1kj83q3fbu5x159gxdaiopako/xyz")
                    .into_qualified()
                    .unwrap();
                assert_eq!(Policy::Deny, config.policy_for(&q))
            }

            #[test]
            fn fails_with_invalid_ty(config in gen::config_simple::<TypeName, ObjectId>()) {
                let q = refname!("refs/cobs/__/hnrk84dch6jk1kj83q3fbu5x159gxdaiopako")
                    .into_qualified()
                    .unwrap();
                assert_eq!(Policy::Deny, config.policy_for(&q))
            }

            #[test]
            fn fails_with_invalid_id(config in gen::config_simple::<TypeName, ObjectId>()) {
                let q = refname!("refs/cobs/patches/1").into_qualified().unwrap();
                assert_eq!(Policy::Deny, config.policy_for(&q))
            }

            #[test]
            fn no_default_no_wildcard_pattern(
                ty in gen::type_name(),
                id in gen::object_id(),
                policy in gen::policy(),
            )
            {
                prop_no_default_no_wildcard_pattern(ty, id, policy)
            }

            #[test]
            fn no_default_wildcard_pattern(
                ty in gen::type_name(),
                id in gen::object_id(),
                policy in gen::policy(),
            )
            {
                prop_no_default_wildcard_pattern(ty, id, policy)
            }

            #[test]
            fn default_is_fallback(
                ty in gen::type_name(),
                id in gen::object_id(),
                cobs in gen::cobs(),
                filter in gen::filter(),
            )
            {
                prop_default_is_fallback(ty, id, cobs, filter)
            }

            #[test]
            fn empty_cobs_is_allow(
                ty in gen::type_name(),
                id in gen::object_id(),
            )
            {
                prop_empty_cobs_is_allow(ty, id)
            }
        }

        fn prop_no_default_no_wildcard_pattern(ty: TypeName, id: ObjectId, policy: Policy) {
            let q = refs_cobs(&ty, &id);
            let c = Config {
                cobs: [(
                    cobs::TypeName::Type(ty),
                    Filter {
                        policy,
                        pattern: Pattern::Objects(iter::once(id).collect()),
                    },
                )]
                .into(),
                ..Config::default()
            };

            assert_eq!(policy, c.policy_for(&q))
        }

        fn prop_no_default_wildcard_pattern(ty: TypeName, id: ObjectId, policy: Policy) {
            let q = refs_cobs(&ty, &id);
            let c = Config {
                cobs: [(
                    cobs::TypeName::Type(ty),
                    Filter {
                        policy,
                        pattern: Pattern::Wildcard,
                    },
                )]
                .into(),
                ..Config::default()
            };

            assert_eq!(policy, c.policy_for(&q))
        }

        fn prop_default_is_fallback(
            ty: TypeName,
            id: ObjectId,
            mut cobs: Cobs<TypeName, ObjectId>,
            filter: Filter<ObjectId>,
        ) {
            let q = refs_cobs(&ty, &id);
            let policy = filter.policy;
            cobs.insert(cobs::TypeName::Wildcard, filter);
            let c = Config {
                cobs,
                ..Config::default()
            };

            assert_eq!(policy, c.policy_for(&q))
        }

        fn prop_empty_cobs_is_allow(ty: TypeName, id: ObjectId) {
            let q = refs_cobs(&ty, &id);
            let c = Config {
                cobs: Cobs::empty(),
                ..Config::default()
            };

            assert_eq!(Policy::Allow, c.policy_for(&q))
        }
    }

    pub mod data {
        use super::*;

        proptest! {
            #[test]
            fn with_known_category(
                cfg in gen::config_simple::<TypeName, ObjectId>(),
                cat in prop::sample::select(&DATA_REFS[..])
            )
            {
                let q = Qualified::from_components(cat, gen::SOME_COMP.clone(), None);
                assert_eq!(cfg.data, match cfg.policy_for(&q) {
                    Policy::Allow => true,
                    Policy::Deny => false,
                })
            }

            #[test]
            fn with_unknown_category(
                cfg in gen::config_simple::<TypeName, ObjectId>(),
                cat in gen::unknown_category()
            )
            {
                assert_eq!(Policy::Deny, cfg.policy_for(&cat))
            }
        }
    }
}
