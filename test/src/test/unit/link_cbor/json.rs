// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use link_cbor::json as cbor_json;

use crate::roundtrip::json_roundtrip;

#[test]
fn trippin() {
    let jasons = vec![
	json!({
        "name": "John Doe",
        "age": 43,
        "phones": [
            "+44 1234567",
            "+44 2345678"
        ]
    })
    ];
    for jason in jasons {
        cbor_roundtrip(jason);
    }
}
