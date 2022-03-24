// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::process::exit;

use anyhow::Result;

use linkd_lib::api::sockets::socket_activation;

#[tokio::main]
async fn main() -> Result<()> {
    if let Some(_listener) = socket_activation::env()? {
        exit(0)
    } else {
        exit(1);
    }
}
