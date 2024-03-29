// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_canonical::{string, Cjson};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use test_helpers::roundtrip;
use unicode_normalization::UnicodeNormalization as _;

use crate::gen::gen_cstring;

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct T {
    #[serde(deserialize_with = "string::deserialize")]
    field: String,
}

impl T {
    fn normalised(&self) -> Self {
        Self {
            field: self.field.nfc().collect(),
        }
    }
}

fn gen_t() -> impl Strategy<Value = T> {
    ".*".prop_map(|field| T { field })
}

proptest! {
    #[test]
    fn cstring_roundtrip_str(cstring in gen_cstring()) {
        roundtrip::str(cstring)
    }

    #[test]
    fn cstring_roundtrip_json(cstring in gen_cstring()) {
        roundtrip::json(cstring)
    }

    #[test]
    fn cstring_roundtrip_cjson(cstring in gen_cstring()) {
        roundtrip::cjson(cstring)
    }

    #[test]
    fn any_string_roundtrip_json(t in gen_t()) {
        let ser = serde_json::to_string(&t).unwrap();
        let de = serde_json::from_str(&ser).unwrap();

        assert_eq!(t.normalised(), de)
    }

    #[test]
    fn any_string_roundtrip_cjson(t in gen_t()) {
        let canonical = Cjson(&t).canonical_form().unwrap();

        assert_eq!(t.normalised(), serde_json::from_slice(&canonical).unwrap())
    }
}
