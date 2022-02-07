// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use git_ref_format::{check_ref_format, refspec, Error, Options, RefStr, RefString};
use proptest::prelude::*;

use crate::roundtrip;

pub mod gen {
    use super::*;

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
}

pub mod name {
    use super::*;

    proptest! {
        #[test]
        fn valid(input in gen::valid()) {
            assert_eq!(input.as_str(), RefStr::try_from_str(&input).unwrap().as_str())
        }

        #[test]
        fn invalid_char(input in gen::with_invalid_char()) {
            assert_matches!(RefString::try_from(input), Err(Error::InvalidChar(_)))
        }

        #[test]
        fn dot_lock(input in gen::ends_with_dot_lock()) {
            assert_matches!(RefString::try_from(input), Err(Error::DotLock))
        }

        #[test]
        fn double_dot(input in gen::with_double_dot()) {
            assert_matches!(RefString::try_from(input), Err(Error::DotDot))
        }

        #[test]
        fn starts_dot(input in gen::starts_with_dot()) {
            assert_matches!(RefString::try_from(input), Err(Error::StartsDot))
        }

        #[test]
        fn ends_dot(input in gen::ends_with_dot()) {
            assert_matches!(RefString::try_from(input), Err(Error::EndsDot))
        }

        #[test]
        fn control_char(input in gen::with_control_char()) {
            assert_matches!(RefString::try_from(input), Err(Error::Control))
        }

        #[test]
        fn space(input in gen::with_space()) {
            assert_matches!(RefString::try_from(input), Err(Error::Space))
        }

        #[test]
        fn consecutive_slashes(input in gen::with_consecutive_slashes()) {
            assert_matches!(RefString::try_from(input), Err(Error::Slash))
        }

        #[test]
        fn glob(input in gen::with_glob()) {
            assert_matches!(RefString::try_from(input), Err(Error::InvalidChar('*')))
        }

        #[test]
        fn invalid(input in gen::invalid()) {
            assert_matches!(RefString::try_from(input), Err(_))
        }

        #[test]
        fn roundtrip_components(input in gen::valid()) {
            assert_eq!(
                input.as_str(),
                RefStr::try_from_str(&input).unwrap().components().collect::<RefString>().as_str()
            )
        }

        #[test]
        fn json(input in gen::valid()) {
           roundtrip::json_roundtrip(RefString::try_from(input).unwrap())
        }

        #[test]
        fn cbor(input in gen::valid()) {
            roundtrip::cbor_roundtrip(RefString::try_from(input).unwrap())
        }
    }
}

pub mod pattern {
    use super::*;

    proptest! {
        #[test]
        fn valid(input in gen::with_glob()) {
            assert_eq!(input.as_str(), refspec::PatternStr::try_from_str(&input).unwrap().as_str())
        }

        #[test]
        fn refname_is_pattern(input in gen::valid()) {
            assert_eq!(input.as_str(), refspec::PatternStr::try_from_str(&input).unwrap().as_str())
        }

        #[test]
        fn no_more_than_one_star(input in gen::multi_glob()) {
            assert_matches!(refspec::PatternString::try_from(input), Err(Error::Pattern))
        }

        #[test]
        fn invalid_refname_is_invalid_pattern(input in gen::invalid()) {
            assert_matches!(refspec::PatternString::try_from(input), Err(_))
        }

        #[test]
        fn roundtrip_components(input in gen::with_glob()) {
            assert_eq!(
                input.as_str(),
                refspec::PatternStr::try_from_str(&input)
                    .unwrap()
                    .components()
                    .collect::<Result<refspec::PatternString, _>>()
                    .unwrap()
                    .as_str()
            )
        }

        #[test]
        fn json(input in gen::with_glob()) {
            roundtrip::json_roundtrip(refspec::PatternString::try_from(input).unwrap())
        }

        #[test]
        fn cbor(input in gen::with_glob()) {
            roundtrip::cbor_roundtrip(refspec::PatternString::try_from(input).unwrap())
        }
    }
}

proptest! {
    #[test]
    fn disallow_onelevel(input in gen::trivial(), allow_pattern in any::<bool>()) {
        assert_matches!(
            check_ref_format(Options {
                    allow_onelevel: false,
                    allow_pattern,
                },
                &input
            ),
            Err(Error::OneLevel)
        )
    }
}
