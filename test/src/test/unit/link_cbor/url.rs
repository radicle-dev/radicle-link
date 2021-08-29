// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use url::Url;

use link_cbor::url as cbor_url;

use crate::roundtrip::cbor_roundtrip;

#[test]
fn trippin() {
    let urls = vec![
        cbor_url::Url(
            Url::parse("https://github.com/rust-lang/rust/issues?labels=E-easy&state=open")
                .unwrap(),
        ),
        cbor_url::Url(Url::parse("data:text/plain,Hello?World#").unwrap()),
        cbor_url::Url(Url::parse("http://مثال.إختبار").unwrap()),
        cbor_url::Url(Url::parse("http://-.~_!$&'()*+,;=:%40:80%2f::::::@example.com").unwrap()),
    ];
    for url in urls {
        cbor_roundtrip(url);
    }
}
