// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::TryFrom;

use git_ref_format::{refspec, Error};
use proptest::prelude::*;
use test_helpers::roundtrip;

use crate::gen;

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
        roundtrip::json(refspec::PatternString::try_from(input).unwrap())
    }

    #[test]
    fn cbor(input in gen::with_glob()) {
        roundtrip::cbor(refspec::PatternString::try_from(input).unwrap())
    }
}
