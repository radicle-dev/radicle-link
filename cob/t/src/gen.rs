// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::str::FromStr;

use cob::{ObjectId, TypeName};
use link_identities_test::gen::urn::gen_oid;
use proptest::prelude::*;

pub fn gen_typename() -> impl Strategy<Value = TypeName> {
    prop::string::string_regex(r"[[:alpha:]]+[[:alpha:]\.]{1,198}[[:alpha:]]+")
        .unwrap()
        .prop_filter("must not contain double dots", |s| !s.contains(".."))
        .prop_map(|s| TypeName::from_str(s.as_str()).unwrap())
}

pub fn gen_objectid() -> impl Strategy<Value = ObjectId> {
    gen_oid(git2::ObjectType::Commit).prop_map(|oid| oid.into())
}
