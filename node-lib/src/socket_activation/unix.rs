// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Implementation of the systemd socket activation protocol.
//! <http://0pointer.de/blog/projects/socket-activation.html>
//!
//! TODO
//! * support FDs beyond 3
//! * support mapping from listen names

use std::{
    env,
    os::unix::{io::RawFd, net::UnixListener, prelude::FromRawFd},
};

use anyhow::{bail, Result};
use nix::{
    fcntl::{fcntl, FcntlArg::F_SETFD, FdFlag},
    sys::socket::SockAddr,
    unistd::Pid,
};

/// Environemnt variable which carries the amount of file descriptors passed
/// down.
const LISTEN_FDS: &str = "LISTEN_FDS";
/// Environment variable containing colon-separated list of names corresponding
/// to the `FileDescriptorName` option in the service file.
const _LISTEN_NAMES: &str = "LISTEN_NAMES";
/// Environemnt variable when present should match PID of the current process.
const LISTEN_PID: &str = "LISTEN_PID";

pub fn env() -> Result<Option<UnixListener>> {
    // TODO(xla): Enable usage of more than the first fd. For now the assumption
    // should be safe as long as the service files are defined in accordance.
    if let Some(fd) = fds().and_then(|fds| fds.first().cloned()) {
        if !matches!(nix::sys::socket::getsockname(fd)?, SockAddr::Unix(_)) {
            bail!(
                "file descriptor {} taken from env is not a valid unix socket",
                fd
            );
        }

        // Set FD_CLOEXEC to avoid further inheritance to children.
        fcntl(fd, F_SETFD(FdFlag::FD_CLOEXEC))?;

        return Ok(Some(unsafe { FromRawFd::from_raw_fd(fd) }));
    }

    Ok(None)
}

fn fds() -> Option<Vec<RawFd>> {
    if let Some(count) = env::var(LISTEN_FDS).ok().and_then(|x| x.parse().ok()) {
        if env::var(LISTEN_PID).ok() == Some(Pid::this().to_string()) {
            env::remove_var(LISTEN_FDS);
            env::remove_var(LISTEN_PID);

            // Magic number to start counting FDs from, as 0, 1 and 2 are
            // reserved for stdin, stdout and stderr respectively.
            return Some((0..count).map(|offset| 3 + offset as RawFd).collect());
        }
    }

    None
}
