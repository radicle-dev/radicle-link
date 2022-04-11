// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

mod cache;
mod cached_change_graph;
mod schema;

use cob::TypeName;
use std::str::FromStr;

#[test]
fn test_valid_typenames() {
    assert!(TypeName::from_str("abc.def.ghi").is_ok());
    assert!(TypeName::from_str("abc.123.ghi").is_ok());
    assert!(TypeName::from_str("1bc.123.ghi").is_ok());
    assert!(TypeName::from_str(".abc.123.ghi").is_err());
    assert!(TypeName::from_str("abc.123.ghi.").is_err());
}
