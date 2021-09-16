// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::process::Command;

use anyhow::Result;

#[test]
fn construct_listener_from_env() -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("-p")
        .arg("radicle-link-test")
        .arg("--example")
        .arg("socket_activation_wrapper");
    let mut cmd = assert_cmd::cmd::Command::from_std(cmd);
    cmd.assert().success();

    Ok(())
}
