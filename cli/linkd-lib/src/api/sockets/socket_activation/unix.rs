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

use super::{OpenMode, SyncSockets};

/// Environemnt variable which carries the amount of file descriptors passed
/// down.
const LISTEN_FDS: &str = "LISTEN_FDS";
/// Environment variable containing colon-separated list of names corresponding
/// to the `FileDescriptorName` option in the service file.
const LISTEN_NAMES: &str = "LISTEN_FDNAMES";
/// Environemnt variable when present should match PID of the current process.
const LISTEN_PID: &str = "LISTEN_PID";
/// The name of the rpc socket
const RPC_SOCKET_FD_NAME: &str = "rpc";
/// The name of the events socket
const EVENTS_SOCKET_FD_NAME: &str = "events";

pub(super) fn env() -> Result<Option<SyncSockets>> {
    match (fds(), fd_names()) {
        (Some(fds), Some(fd_names)) => {
            let rpc_socket_idx = fd_names
                .iter()
                .position(|n| n == RPC_SOCKET_FD_NAME)
                .ok_or_else(|| {
                    anyhow::anyhow!("did not find '{}' in {}", RPC_SOCKET_FD_NAME, LISTEN_NAMES)
                })?;

            let events_socket_idx = fd_names
                .iter()
                .position(|n| n == EVENTS_SOCKET_FD_NAME)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "did not find '{}' in {}",
                        EVENTS_SOCKET_FD_NAME,
                        LISTEN_NAMES
                    )
                })?;

            let rpc_socket = load_socket(rpc_socket_idx, RPC_SOCKET_FD_NAME, &fds)?;
            let events_socket = load_socket(events_socket_idx, EVENTS_SOCKET_FD_NAME, &fds)?;
            Ok(Some(SyncSockets {
                rpc: rpc_socket,
                events: events_socket,
                open_mode: OpenMode::SocketActivated,
            }))
        },
        (Some(_), None) => {
            tracing::warn!("LISTEN_FDS is set but LISTEN_FDNAMES is not");
            Ok(None)
        },
        (None, Some(_)) => {
            tracing::warn!("LISTEN_FDNAMES is set but LISTEN_FDS is not");
            Ok(None)
        },
        _ => Ok(None),
    }
}

fn load_socket(idx: usize, name: &str, fds: &[RawFd]) -> Result<UnixListener> {
    let fd = *fds.get(idx).ok_or_else(|| {
        anyhow::anyhow!(
            "no file descriptor for index {} (corresponding to {}
            in {}) in {}",
            idx,
            name,
            LISTEN_NAMES,
            LISTEN_FDS,
        )
    })?;

    if !matches!(nix::sys::socket::getsockname(fd)?, SockAddr::Unix(_)) {
        bail!(
            "file descriptor {} taken from env is not a valid unix socket",
            fd
        );
    }

    // Set FD_CLOEXEC to avoid further inheritance to children.
    fcntl(fd, F_SETFD(FdFlag::FD_CLOEXEC))?;

    let std_listener: UnixListener = unsafe { FromRawFd::from_raw_fd(fd) };
    std_listener.set_nonblocking(true)?;
    Ok(std_listener)
}

fn fd_names() -> Option<Vec<String>> {
    if let Ok(fd_names) = env::var(LISTEN_NAMES) {
        Some(fd_names.split(':').map(String::from).collect())
    } else {
        None
    }
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
