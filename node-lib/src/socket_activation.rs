// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

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

/// Constructs a Unix socket from the file descriptor passed through the
/// environemnt. The returned listener will be `None` if there are no
/// environment variables set that are applicable for the current platform or no
/// suitable implementations are activated/supported:
///
/// * systemd under unix systems with an OS other than macos: https://www.freedesktop.org/software/systemd/man/systemd.socket.html
/// * launchd under macos: https://en.wikipedia.org/wiki/Launchd#Socket_activation_protocol
pub fn env() -> Result<Option<UnixListener>> {
    imp::env()
}
