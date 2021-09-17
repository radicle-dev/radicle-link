// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::process::{Command, Stdio};

use rad_clib::keys::ssh::SshAuthSock;

use crate::tempdir::WithTmpDir;

pub type TmpSshSock = WithTmpDir<SshAuthSock>;

pub fn ssh_auth_sock() -> TmpSshSock {
    WithTmpDir::new(|path| -> anyhow::Result<SshAuthSock> {
        let sock = path.join("tmp.sock");
        Ok(SshAuthSock::Uds(sock))
    })
    .unwrap()
}

pub fn with_ssh_agent<F, T>(callback: F) -> anyhow::Result<T>
where
    F: FnOnce(SshAuthSock) -> anyhow::Result<T>,
{
    let sock = ssh_auth_sock();
    let path = match &*sock {
        SshAuthSock::Uds(path) => path,
        _ => unreachable!(),
    };
    let mut agent = Command::new("ssh-agent")
        .args(["-a", &format!("{}", path.display())])
        .stdout(Stdio::null())
        .spawn()?;
    let res = callback((*sock).clone());
    agent.kill()?;
    res
}
