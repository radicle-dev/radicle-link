// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Implementation of the systemd socket activation protocol.
//! <http://0pointer.de/blog/projects/socket-activation.html>

#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
#[cfg(all(unix, not(target_os = "macos")))]
pub use unix::{env_sockets, Error};

#[cfg(target_os = "macos")]
mod osx;
#[cfg(target_os = "macos")]
pub use osx::{env_sockets, Error};

use std::{net::TcpListener, os::unix::net::UnixListener};

/// The type of a socket found in the socket activated environment variables
pub enum Socket {
    /// A unix domain socket
    Unix(UnixListener),
    /// A TCP socket
    Tcp(TcpListener),
}
