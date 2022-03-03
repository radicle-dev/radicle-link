// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom as _;

use link_canonical::Canonical as _;
use link_tracking::{
    config::{
        cobs::{Cobs, Filter, Pattern, Policy, TypeName},
        Config,
    },
    git,
};

#[test]
fn parse_commutes() {
    let allow = r#"{"cobs":{"*":{"pattern":"*","policy":"allow"}},"data":true}"#;
    assert_eq!(
        git::config::Config::try_from(allow).unwrap(),
        git::config::Config::default()
    );
    assert_eq!(
        std::str::from_utf8(&git::config::Config::default().canonical_form().unwrap()).unwrap(),
        allow
    );
}

#[test]
fn can_insert() {
    let mut config: Config<&str, &str> = Config::default();
    config.cobs.insert(
        TypeName::Type("discussion"),
        Filter {
            policy: Policy::Allow,
            pattern: Pattern::Wildcard,
        },
    );
    config.cobs.insert(
        TypeName::Type("patch"),
        Filter {
            policy: Policy::Deny,
            pattern: Pattern::Objects(vec!["1", "2", "3"].into_iter().collect()),
        },
    );

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: [
                (
                    TypeName::Wildcard,
                    Filter {
                        policy: Policy::Allow,
                        pattern: Pattern::Wildcard
                    }
                ),
                (
                    TypeName::Type("discussion"),
                    Filter {
                        policy: Policy::Allow,
                        pattern: Pattern::Wildcard
                    }
                ),
                (
                    TypeName::Type("patch"),
                    Filter {
                        policy: Policy::Deny,
                        pattern: Pattern::Objects(vec!["1", "2", "3"].into_iter().collect()),
                    }
                ),
            ]
            .into()
        }
    )
}

#[test]
fn can_remove() {
    let mut config: Config<&str, ()> = Config::default();
    config.cobs.remove(&TypeName::Wildcard);

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: Cobs::empty(),
        }
    )
}

#[test]
fn can_set_policy() {
    let mut config: Config<&str, ()> = Config::default();
    config
        .cobs
        .entry(TypeName::Wildcard)
        .set_policy(Policy::Deny);

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: Cobs::deny_all(),
        }
    )
}

#[test]
fn can_set_pattern() {
    let mut config: Config<&str, ()> = Config::default();
    config
        .cobs
        .entry(TypeName::Wildcard)
        .set_pattern(Pattern::Objects(Some(()).into_iter().collect()));

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: [(
                TypeName::Wildcard,
                Filter {
                    policy: Policy::Allow,
                    pattern: Pattern::Objects(Some(()).into_iter().collect())
                }
            )]
            .into()
        }
    )
}

#[test]
fn can_insert_objects() {
    let mut config: Config<&str, u8> = Config {
        data: true,
        cobs: [(
            TypeName::Type("discussion"),
            Filter {
                policy: Policy::Deny,
                pattern: Pattern::Objects(vec![1, 2, 3, 4].into_iter().collect()),
            },
        )]
        .into(),
    };
    config
        .cobs
        .entry(TypeName::Type("discussion"))
        .insert_objects(vec![5, 6, 7, 8]);

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: [(
                TypeName::Type("discussion"),
                Filter {
                    policy: Policy::Deny,
                    pattern: Pattern::Objects(vec![1, 2, 3, 4, 5, 6, 7, 8].into_iter().collect()),
                }
            )]
            .into()
        }
    )
}

#[test]
fn can_remove_objects() {
    let mut config: Config<&str, u8> = Config {
        data: true,
        cobs: [(
            TypeName::Type("discussion"),
            Filter {
                policy: Policy::Deny,
                pattern: Pattern::Objects(vec![1, 2, 3, 4].into_iter().collect()),
            },
        )]
        .into(),
    };
    config
        .cobs
        .entry(TypeName::Type("discussion"))
        .remove_objects(vec![1, 2, 4]);

    assert_eq!(
        config,
        Config {
            data: true,
            cobs: [(
                TypeName::Type("discussion"),
                Filter {
                    policy: Policy::Deny,
                    pattern: Pattern::Objects(Some(3).into_iter().collect()),
                }
            )]
            .into()
        }
    )
}
