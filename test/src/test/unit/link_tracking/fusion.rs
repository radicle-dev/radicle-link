// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use either::Either::{Left, Right};

use link_tracking::git::tracking::policy::{
    compose::{Compose as _, Reduction, WithConfig},
    Track,
    Untrack,
};

#[test]
fn any_removes_stuck() {
    let policies = vec![
        WithConfig {
            policy: Track::MustExist,
            config: &0u8,
        },
        WithConfig {
            policy: Track::MustExist,
            config: &1,
        },
        WithConfig {
            policy: Track::MustNotExist,
            config: &2,
        },
        WithConfig {
            policy: Track::Any,
            config: &3,
        },
        WithConfig {
            policy: Track::MustNotExist,
            config: &4,
        },
    ];

    let mut result = Reduction::Simple(policies[0].clone());
    for policy in policies.into_iter().skip(1) {
        result = result.compose(&policy)
    }

    assert_eq!(
        result,
        Reduction::Simple(WithConfig {
            policy: Track::Any,
            config: &3,
        })
    );
}

#[test]
fn stuck() {
    let policies = vec![
        WithConfig {
            policy: Track::MustExist,
            config: &0u8,
        },
        WithConfig {
            policy: Track::MustExist,
            config: &1,
        },
        WithConfig {
            policy: Track::MustNotExist,
            config: &2,
        },
    ];

    let mut result = Reduction::Simple(policies[0].clone());
    for policy in policies.into_iter().skip(1) {
        result = result.compose(&policy)
    }

    assert_eq!(
        result,
        Reduction::Stuck {
            first: WithConfig {
                policy: Track::MustExist,
                config: &1,
            },
            second: WithConfig {
                policy: Track::MustNotExist,
                config: &2,
            },
        }
    );
}

#[test]
fn untrack_after_wins() {
    let policies = vec![
        Left(WithConfig {
            policy: Track::MustExist,
            config: &0u8,
        }),
        Left(WithConfig {
            policy: Track::MustExist,
            config: &1,
        }),
        Left(WithConfig {
            policy: Track::MustNotExist,
            config: &2,
        }),
        Right(Untrack::MustExist),
    ];

    let mut result = Reduction::Simple(policies[0].clone());
    for policy in policies.into_iter().skip(1) {
        result = result.compose(&policy)
    }

    assert_eq!(result, Reduction::Simple(Right(Untrack::Any)),);
}

#[test]
fn untrack_as_update() {
    let policies = vec![
        Right(Untrack::MustExist),
        Left(WithConfig {
            policy: Track::MustExist,
            config: &0u8,
        }),
        Left(WithConfig {
            policy: Track::MustNotExist,
            config: &1,
        }),
    ];

    let mut result = Reduction::Simple(policies[0].clone());
    for policy in policies.into_iter().skip(1) {
        result = result.compose(&policy)
    }

    assert_eq!(
        result,
        Reduction::Simple(Left(WithConfig {
            policy: Track::Any,
            config: &1,
        })),
    );
}

#[test]
fn track_any_wins() {
    let policies = vec![
        Right(Untrack::MustExist),
        Left(WithConfig {
            policy: Track::MustExist,
            config: &0u8,
        }),
        Left(WithConfig {
            policy: Track::MustNotExist,
            config: &1,
        }),
        Right(Untrack::Any),
        Left(WithConfig {
            policy: Track::MustNotExist,
            config: &4,
        }),
        Left(WithConfig {
            policy: Track::MustExist,
            config: &3,
        }),
        Left(WithConfig {
            policy: Track::Any,
            config: &12,
        }),
    ];

    let mut result = Reduction::Simple(policies[0].clone());
    for policy in policies.into_iter().skip(1) {
        result = result.compose(&policy)
    }

    assert_eq!(
        result,
        Reduction::Simple(Left(WithConfig {
            policy: Track::Any,
            config: &12,
        })),
    );
}
