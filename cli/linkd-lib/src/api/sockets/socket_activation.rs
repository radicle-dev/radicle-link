// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use librad::{profile::Profile, PeerId};
use std::os::unix::net::UnixListener;

use anyhow::Result;

#[cfg(all(unix, target_os = "macos"))]
mod macos;
#[cfg(all(unix, target_os = "macos"))]
use macos as imp;

#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
#[cfg(all(unix, not(target_os = "macos")))]
use unix as imp;

use super::{OpenMode, SyncSockets};

/// Constructs a `Sockets` from the file descriptors passed through the
/// environemnt. The result will be `None` if there are no environment variables
/// set that are applicable for the current platform or no suitable
/// implementations are activated/supported:
///
/// * [systemd] under unix systems with an OS other than macos
/// * [launchd] under macos
///
/// [systemd]: https://www.freedesktop.org/software/systemd/man/systemd.socket.html
/// [launchd]: https://en.wikipedia.org/wiki/Launchd#Socket_activation_protocol
pub fn env() -> Result<Option<SyncSockets>> {
    imp::env()
}

/// Constructs a `Sockets` from the file descriptors at default locations with
/// respect to the profile passed in
pub fn profile(profile: &Profile, peer_id: &PeerId) -> Result<SyncSockets> {
    let rpc_socket_path = profile.paths().rpc_socket(peer_id);
    let events_socket_path = profile.paths().events_socket(peer_id);
    let rpc = UnixListener::bind(rpc_socket_path.as_path())?;
    let events = UnixListener::bind(events_socket_path.as_path())?;
    Ok(SyncSockets {
        rpc,
        events,
        open_mode: OpenMode::InProcess {
            rpc_socket_path,
            event_socket_path: events_socket_path,
        },
    })
}
