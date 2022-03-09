// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::TryFrom;

use radicle_git_ext::reference::{check, name::*};
use test_helpers::roundtrip;

mod common {
    use super::*;
    use std::fmt::Debug;

    pub fn invalid<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        const INVALID: [&str; 16] = [
            ".hidden",
            "/etc/shadow",
            "@",
            "@{",
            "C:",
            "\\WORKGROUP",
            "foo.lock",
            "head^",
            "here/../../etc/shadow",
            "refs//heads/main",
            "refs/heads/",
            "shawn/ white",
            "the/dotted./quad",
            "wh?t",
            "x[a-z]",
            "~ommij",
        ];

        for v in INVALID {
            assert_matches!(T::try_from(v), Err(Error::RefFormat(_)), "input: {}", v)
        }
    }

    pub fn valid<T>()
    where
        T: TryFrom<&'static str, Error = Error> + AsRef<str> + Debug,
    {
        const VALID: [&str; 5] = [
            "\u{1F32F}",
            "cl@wn",
            "foo/bar",
            "master",
            "refs/heads/mistress",
        ];

        for v in VALID {
            assert_matches!(T::try_from(v), Ok(x) if x.as_ref() == v, "input: {}", v)
        }
    }

    pub fn empty<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        assert_matches!(T::try_from(""), Err(Error::RefFormat(check::Error::Empty)))
    }

    pub fn nulsafe<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        assert_matches!(
            T::try_from("jeff\0"),
            Err(Error::RefFormat(check::Error::InvalidChar('\0')))
        )
    }
}

mod reflike {
    use super::*;

    #[test]
    fn empty() {
        common::empty::<RefLike>()
    }

    #[test]
    fn valid() {
        common::valid::<RefLike>()
    }

    #[test]
    fn invalid() {
        common::invalid::<RefLike>()
    }

    #[test]
    fn nulsafe() {
        common::nulsafe::<RefLike>()
    }

    #[test]
    fn globstar_invalid() {
        assert_matches!(
            RefLike::try_from("refs/heads/*"),
            Err(Error::RefFormat(check::Error::InvalidChar('*')))
        )
    }

    #[test]
    fn into_onelevel() {
        assert_eq!(
            &*OneLevel::from(RefLike::try_from("refs/heads/next").unwrap()),
            "next"
        )
    }

    #[test]
    fn into_heads() {
        assert_eq!(
            &*Qualified::from(RefLike::try_from("pu").unwrap()),
            "refs/heads/pu"
        )
    }

    #[test]
    fn serde() {
        let refl = RefLike::try_from("pu").unwrap();
        roundtrip::json(refl.clone());
        roundtrip::json(OneLevel::from(refl.clone()));
        roundtrip::json(Qualified::from(refl))
    }

    #[test]
    fn serde_invalid() {
        let json = serde_json::to_string("HEAD^").unwrap();
        assert!(serde_json::from_str::<RefLike>(&json).is_err());
        assert!(serde_json::from_str::<OneLevel>(&json).is_err());
        assert!(serde_json::from_str::<Qualified>(&json).is_err())
    }

    #[test]
    fn cbor() {
        let refl = RefLike::try_from("pu").unwrap();
        roundtrip::cbor(refl.clone());
        roundtrip::cbor(OneLevel::from(refl.clone()));
        roundtrip::cbor(Qualified::from(refl))
    }

    #[test]
    fn cbor_invalid() {
        let cbor = minicbor::to_vec("HEAD^").unwrap();
        assert!(minicbor::decode::<RefLike>(&cbor).is_err());
        assert!(minicbor::decode::<OneLevel>(&cbor).is_err());
        assert!(minicbor::decode::<Qualified>(&cbor).is_err())
    }
}

mod pattern {
    use super::*;

    #[test]
    fn empty() {
        common::empty::<RefspecPattern>()
    }

    #[test]
    fn valid() {
        common::valid::<RefspecPattern>()
    }

    #[test]
    fn invalid() {
        common::invalid::<RefspecPattern>()
    }

    #[test]
    fn nulsafe() {
        common::nulsafe::<RefspecPattern>()
    }

    #[test]
    fn globstar_ok() {
        const GLOBBED: [&str; 7] = [
            "*",
            "fo*",
            "fo*/bar",
            "foo/*/bar",
            "foo/ba*",
            "foo/bar/*",
            "foo/b*r",
        ];

        for v in GLOBBED {
            assert_matches!(
                RefspecPattern::try_from(v),
                Ok(ref x) if x.as_str() == v,
                "input: {}", v
            )
        }
    }

    #[test]
    fn globstar_invalid() {
        const GLOBBED: [&str; 12] = [
            "**",
            "***",
            "*/*",
            "*/L/*",
            "fo*/*/bar",
            "fo*/ba*",
            "fo*/ba*/baz",
            "fo*/ba*/ba*",
            "fo*/bar/*",
            "foo/*/bar/*",
            "foo/*/bar/*/baz*",
            "foo/*/bar/*/baz/*",
        ];

        for v in GLOBBED {
            assert_matches!(
                RefspecPattern::try_from(v),
                Err(Error::RefFormat(check::Error::Pattern))
            )
        }
    }

    #[test]
    fn serde() {
        roundtrip::json(RefspecPattern::try_from("refs/heads/*").unwrap())
    }

    #[test]
    fn serde_invalid() {
        let json = serde_json::to_string("HEAD^").unwrap();
        assert!(serde_json::from_str::<RefspecPattern>(&json).is_err())
    }

    #[test]
    fn cbor() {
        roundtrip::cbor(RefspecPattern::try_from("refs/heads/*").unwrap())
    }

    #[test]
    fn cbor_invalid() {
        let cbor = minicbor::to_vec("HEAD^").unwrap();
        assert!(minicbor::decode::<RefspecPattern>(&cbor).is_err())
    }

    #[test]
    fn strip_prefix_works_for_different_ends() {
        let refl = RefLike::try_from("refs/heads/next").unwrap();
        assert_eq!(
            refl.strip_prefix("refs/heads").unwrap(),
            refl.strip_prefix("refs/heads/").unwrap()
        );
    }
}
