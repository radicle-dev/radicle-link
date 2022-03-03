// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::TryFrom;

use git_ref_format::{Error, RefStr, RefString};
use proptest::prelude::*;
use test_helpers::roundtrip;

use crate::gen;

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
       roundtrip::json(RefString::try_from(input).unwrap())
    }

    #[test]
    fn cbor(input in gen::valid()) {
        roundtrip::cbor(RefString::try_from(input).unwrap())
    }
}
