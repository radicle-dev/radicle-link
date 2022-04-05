// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Implementation of the systemd socket activation protocol.
//! <http://0pointer.de/blog/projects/socket-activation.html>

use std::io;

mod sd;
pub use sd::Systemd;

#[cfg(target_os = "macos")]
mod ld;
#[cfg(target_os = "macos")]
pub use ld::Launchd;

/// Native socket activation.
///
/// Socket activation is a concept pioneered by Apple's `launchd` init system,
/// and subsequently implemented by `systemd`: similar to traditional `inetd`
/// services, the process management system listens on one or more preconfigured
/// sockets, and launches the corresponding service on-demand as connections are
/// established on them. Unlike `inetd`, however, the system can be instructed
/// to launch only a single instance of the service (as opposed to one process
/// per connection).
///
/// While `launchd` requires a system call to notify it of the consumption of
/// the socket, `systemd`'s protocol is more portable in principle as it passes
/// file descriptor numbers via the environment.
///
/// Consequently, the `systemd` impl is provided for "unix" targets, while
/// `launchd` is only available on "macos" targets.
pub trait Sockets {
    /// Obtain a managed socket by name.
    ///
    /// Note that `systemd` socket units allow only one file descriptor per
    /// name, while `launchd` services _may_ hand out more than one.
    fn activate(&mut self, name: &str) -> io::Result<Vec<socket2::Socket>>;
}

/// Obtain the platform-default [`Sockets`] implementation.
///
/// Returns `Launchd` on "macos" targets, and `Systemd` otherwise.
pub fn default() -> io::Result<impl Sockets> {
    #[cfg(target_os = "macos")]
    {
        Ok(Launchd)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Systemd::from_env()
    }
}

fn io_other<E>(error: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, error)
}
