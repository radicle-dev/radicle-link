// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;

/// Any unicode "word" is trivially a valid refname.
pub fn trivial() -> impl Strategy<Value = String> {
    "\\w+"
}

pub fn valid() -> impl Strategy<Value = String> {
    prop::collection::vec(trivial(), 1..20).prop_map(|xs| xs.join("/"))
}

pub fn invalid_char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('\0'),
        Just('\\'),
        Just('~'),
        Just('^'),
        Just(':'),
        Just('?'),
        Just('[')
    ]
}

pub fn with_invalid_char() -> impl Strategy<Value = String> {
    ("\\w*", invalid_char(), "\\w*").prop_map(|(mut pre, invalid, suf)| {
        pre.push(invalid);
        pre.push_str(&suf);
        pre
    })
}

pub fn ends_with_dot_lock() -> impl Strategy<Value = String> {
    "\\w*\\.lock"
}

pub fn with_double_dot() -> impl Strategy<Value = String> {
    "\\w*\\.\\.\\w*"
}

pub fn starts_with_dot() -> impl Strategy<Value = String> {
    "\\.\\w*"
}

pub fn ends_with_dot() -> impl Strategy<Value = String> {
    "\\w+\\."
}

pub fn with_control_char() -> impl Strategy<Value = String> {
    "\\w*[\x01-\x1F\x7F]+\\w*"
}

pub fn with_space() -> impl Strategy<Value = String> {
    "\\w* +\\w*"
}

pub fn with_consecutive_slashes() -> impl Strategy<Value = String> {
    "\\w*//\\w*"
}

pub fn with_glob() -> impl Strategy<Value = String> {
    "\\w*\\*\\w*"
}

pub fn multi_glob() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(with_glob(), 2..5),
        prop::collection::vec(trivial(), 0..5),
    )
        .prop_map(|(mut globs, mut valids)| {
            globs.append(&mut valids);
            globs
        })
        .prop_shuffle()
        .prop_map(|xs| xs.join("/"))
}

pub fn invalid() -> impl Strategy<Value = String> {
    fn path(s: impl Strategy<Value = String>) -> impl Strategy<Value = String> {
        prop::collection::vec(s, 1..20).prop_map(|xs| xs.join("/"))
    }

    prop_oneof![
        Just(String::from("")),
        Just(String::from("@")),
        path(with_invalid_char()),
        path(ends_with_dot_lock()),
        path(with_double_dot()),
        path(starts_with_dot()),
        path(ends_with_dot()),
        path(with_control_char()),
        path(with_space()),
        path(with_consecutive_slashes()),
        path(trivial()).prop_map(|mut p| {
            p.push('/');
            p
        }),
    ]
}
