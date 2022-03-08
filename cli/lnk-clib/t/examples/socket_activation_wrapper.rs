// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use anyhow::Result;
use nix::{sys::socket, unistd::Pid};
use std::{fs::remove_file, os::unix::process::CommandExt as _, process::Command};

fn main() -> Result<()> {
    make_sock("rpc")?;
    make_sock("events")?;

    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("-p")
        .arg("lnk-clib-test")
        .arg("--example")
        .arg("socket_activation");
    cmd.env("LISTEN_FDS", "2");
    cmd.env("LISTEN_FDNAMES", "rpc:events");
    cmd.env("LISTEN_PID", Pid::this().to_string());
    cmd.exec();

    Ok(())
}

fn make_sock(name: &str) -> Result<()> {
    let sock_name = format!("test-linkd-socket-activation-{}", name);
    let tmp = tempfile::Builder::new()
        .prefix(sock_name.as_str())
        .suffix(".sock")
        .tempfile()?
        .path()
        .to_path_buf();
    let sock = socket::socket(
        socket::AddressFamily::Unix,
        socket::SockType::Stream,
        socket::SockFlag::empty(),
        None,
    )?;
    let addr = socket::SockAddr::new_unix(&tmp)?;
    let bound = socket::bind(sock, &addr);
    // unlink immediately, so the socket can't leak even if destructors don't run
    remove_file(tmp)?;
    bound?;
    socket::listen(sock, 1)?;
    Ok(())
}
