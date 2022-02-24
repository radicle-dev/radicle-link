// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::process::{Command, Stdio};

use lnk_clib::keys::ssh::SshAuthSock;
use test_helpers::tempdir::WithTmpDir;

pub type TmpSshSock = WithTmpDir<SshAuthSock>;

pub fn ssh_auth_sock() -> TmpSshSock {
    WithTmpDir::new(|path| -> anyhow::Result<SshAuthSock> {
        let sock = path.join("tmp.sock");
        Ok(SshAuthSock::Uds(sock))
    })
    .unwrap()
}

/// Run a computation with a forked `ssh-agent` on a temporary file handle.
///
/// Once the computation is finished, the `ssh-agent` is killed by getting its
/// PID and running the equivalent of `SSH_AGENT_PID=<pid> ssh-agent -k`.
/// This is a best effort of resource cleanup, but has no guarantees if the
/// parsing of the PID or the killin of the agent fail.
pub fn with_ssh_agent<F, T>(callback: F) -> anyhow::Result<T>
where
    F: FnOnce(SshAuthSock) -> anyhow::Result<T>,
{
    let sock = ssh_auth_sock();
    let path = match &*sock {
        SshAuthSock::Uds(path) => path,
        _ => unreachable!(),
    };
    let agent = Command::new("ssh-agent").arg("-a").arg(path).output()?;
    anyhow::ensure!(agent.status.success(), agent.status);
    let pid = agent_pid(&agent.stdout)?;
    let res = callback((*sock).clone());
    kill_agent_pid(pid)?;
    res
}

/// Kill the ssh-agent running on the given PID.
fn kill_agent_pid(pid: &str) -> anyhow::Result<()> {
    tracing::debug!(pid = %pid, "killing ssh-agent");
    let status = Command::new("ssh-agent")
        .env("SSH_AGENT_PID", pid)
        .args(["-k"])
        .stdout(Stdio::null())
        .status()?;
    tracing::debug!(status = %status, "status of killing agent");
    Ok(())
}

/// Get the PID of the launched ssh-agent.
///
/// It gets the PID by stripping the output from the command using the text
/// `"echo Agent pid "`.
fn agent_pid(out: &[u8]) -> anyhow::Result<&str> {
    const PREFIX: &str = "SSH_AGENT_PID=";
    const SEP: u8 = b';';
    let pid = out
        .split(|b| b == &SEP)
        .find_map(|bs| std::str::from_utf8(bs).ok()?.trim().strip_prefix(PREFIX))
        .ok_or_else(|| anyhow::anyhow!("could not find SSH_AGENT_PID"))?;
    Ok(pid)
}
