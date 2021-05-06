// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::git::p2p::server::pkt_line;

#[test]
fn test_pkt_line() {
    assert_eq!("0006a\n", pkt_line("a\n"));
    assert_eq!("0005a", pkt_line("a"));
    assert_eq!("000bfoobar\n", pkt_line("foobar\n"));
    assert_eq!("0004", pkt_line(""));
}
