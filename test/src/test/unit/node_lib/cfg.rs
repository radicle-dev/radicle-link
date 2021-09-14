// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::net;

use anyhow::Result;
use pretty_assertions::assert_eq;

use node_lib::{Seed, Seeds};

#[tokio::test(flavor = "multi_thread")]
async fn test_resolve_seeds() -> Result<()> {
    let seeds = Seeds::resolve(&[
        "hydsst3z3d5bc6pxq4gz1g4cu6sgbx38czwf3bmmk3ouz4ibjbbtds@localhost:9999"
            .parse()
            .unwrap(),
    ])
    .await?;

    assert!(!seeds.0.is_empty(), "seeds should not be empty");

    if let Some(Seed { addrs, .. }) = seeds.0.first() {
        let addr = addrs.first().unwrap();
        let expected: net::SocketAddr = match *addr {
            net::SocketAddr::V4(_addr) => ([127, 0, 0, 1], 9999).into(),
            net::SocketAddr::V6(_addr) => "[::1]:9999".parse().expect("valid ivp6 address"),
        };

        assert_eq!(expected, *addr);
    }

    Ok(())
}
