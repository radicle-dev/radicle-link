// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    fs,
    io::Read as _,
    process::{Child, Command, Stdio},
};

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
    fs::try_exists("ssh-agent").expect("`ssh-agent` was not found, it is needed to run this test");
    let sock = ssh_auth_sock();
    let path = match &*sock {
        SshAuthSock::Uds(path) => path,
        _ => unreachable!(),
    };
    let mut agent = Command::new("ssh-agent")
        .args(["-a", &format!("{}", path.display())])
        .stdout(Stdio::piped())
        .spawn()?;
    let pid = agent_pid(&mut agent)?;
    let res = callback((*sock).clone());
    kill_agent_pid(&pid)?;
    agent.kill()?;
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
fn agent_pid(agent: &mut Child) -> anyhow::Result<String> {
    let prefix = "echo Agent pid ";
    let mut out = String::new();
    let mut stdout = agent.stdout.take().expect("failed to open stdout");
    stdout.read_to_string(&mut out)?;
    let pid = out
        .lines()
        .find(|s| s.starts_with(prefix))
        .and_then(|pid| pid.strip_prefix(prefix))
        .and_then(|pid| pid.strip_suffix(';'))
        .expect("could not find SSH_AGENT_PID");
    Ok(pid.to_string())
}
