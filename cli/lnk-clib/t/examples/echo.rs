// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::io::{Read, Write};

use anyhow::{anyhow, Context, Result};
use lnk_clib::socket_activation;
use socket2::Socket;

fn main() -> Result<()> {
    let sock = activate()?;
    loop {
        eprintln!("waiting for connection...");
        let (mut client, _) = sock.accept().context("accept")?;
        let mut buf = [0u8; 64];
        eprintln!("waiting for data...");
        let siz = client.read(&mut buf).context("recv")?;
        eprintln!("received {} bytes", siz);
        client.write(&buf[..siz]).context("send")?;
    }
}

fn activate() -> Result<Socket> {
    use socket_activation::Sockets as _;

    socket_activation::default()?
        .activate("echo")?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("missing socket"))
}
