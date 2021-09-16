// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use anyhow::Result;
use nix::{sys::socket, unistd::Pid};
use std::{fs::remove_file, os::unix::process::CommandExt as _, process::Command};

fn main() -> Result<()> {
    remove_file("/tmp/test-linkd-socket-activation.sock").ok();

    let sock = socket::socket(
        socket::AddressFamily::Unix,
        socket::SockType::Stream,
        socket::SockFlag::empty(),
        None,
    )?;
    let addr = socket::SockAddr::new_unix("/tmp/test-linkd-socket-activation.sock")?;
    socket::bind(sock, &addr)?;
    socket::listen(sock, 1)?;

    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("-p")
        .arg("radicle-link-test")
        .arg("--example")
        .arg("socket_activation");
    cmd.env("LISTEN_FDS", "1");
    cmd.env("LISTEN_PID", Pid::this().to_string());
    cmd.exec();

    Ok(())
}
