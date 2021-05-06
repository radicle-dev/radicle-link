// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::convert::TryFrom;

use radicle_git_ext::reference::name::*;

use crate::roundtrip::{cbor_roundtrip, json_roundtrip};

mod common {
    use super::*;
    use std::fmt::Debug;

    pub fn invalid<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        const INVALID: &[&str] = &[
            "foo.lock",
            ".hidden",
            "here/../../etc/shadow",
            "/etc/shadow",
            "~ommij",
            "head^",
            "wh?t",
            "x[a-z]",
            "\\WORKGROUP",
            "C:",
            "@",
            "@{",
        ];

        for v in INVALID {
            assert_matches!(T::try_from(*v), Err(Error::RefFormat), "input: {}", v)
        }
    }

    pub fn valid<T>()
    where
        T: TryFrom<&'static str, Error = Error> + AsRef<str> + Debug,
    {
        const VALID: &[&str] = &[
            "master",
            "foo/bar",
            "cl@wn",
            "refs/heads/mistress",
            "\u{1F32F}",
        ];

        for v in VALID {
            assert_matches!(T::try_from(*v), Ok(ref x) if x.as_ref() == *v, "input: {}", v)
        }
    }

    pub fn empty<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        assert_matches!(T::try_from(""), Err(Error::RefFormat))
    }

    pub fn nulsafe<T>()
    where
        T: TryFrom<&'static str, Error = Error> + Debug,
    {
        assert_matches!(T::try_from("jeff\0"), Err(Error::Nul))
    }

    pub fn normalises<T>()
    where
        T: TryFrom<&'static str, Error = Error> + AsRef<str> + Debug,
    {
        const SLASHED: &[&str] = &[
            "foo//bar",
            "foo//bar//baz",
            "refs//heads/main",
            "guns//////n/////roses",
        ];

        lazy_static! {
            static ref SLASHY: regex::Regex = regex::Regex::new(r"/{2,}").unwrap();
        }

        for v in SLASHED {
            let t = T::try_from(*v).unwrap();
            let normal = SLASHY.replace_all(v, "/");
            assert_eq!(t.as_ref(), &normal)
        }
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
    fn normalises() {
        common::normalises::<RefLike>()
    }

    #[test]
    fn globstar_invalid() {
        assert_matches!(RefLike::try_from("refs/heads/*"), Err(Error::RefFormat))
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
        json_roundtrip(refl.clone());
        json_roundtrip(OneLevel::from(refl.clone()));
        json_roundtrip(Qualified::from(refl))
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
        cbor_roundtrip(refl.clone());
        cbor_roundtrip(OneLevel::from(refl.clone()));
        cbor_roundtrip(Qualified::from(refl))
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
    fn normalises() {
        common::normalises::<RefspecPattern>()
    }

    #[test]
    fn globstar_ok() {
        const GLOBBED: &[&str] = &[
            "refs/heads/*",
            "refs/namespaces/*/refs/rad",
            "*",
            "foo/bar*",
            "foo*/bar",
        ];

        for v in GLOBBED {
            assert_matches!(
                RefspecPattern::try_from(*v),
                Ok(ref x) if x.as_str() == *v,
                "input: {}", v
            )
        }
    }

    #[test]
    fn serde() {
        json_roundtrip(RefspecPattern::try_from("refs/heads/*").unwrap())
    }

    #[test]
    fn serde_invalid() {
        let json = serde_json::to_string("HEAD^").unwrap();
        assert!(serde_json::from_str::<RefspecPattern>(&json).is_err())
    }

    #[test]
    fn cbor() {
        cbor_roundtrip(RefspecPattern::try_from("refs/heads/*").unwrap())
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
