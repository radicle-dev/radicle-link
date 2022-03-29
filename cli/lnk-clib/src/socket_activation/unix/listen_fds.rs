// Copyright (c) Laurențiu Nicola
// SPDX-License-Identifier: MIT AND Apache-2.0
//
// Retrieved at: https://github.com/lnicola/sd-notify/blob/7e9325902b2f44c1e9dc5dc7ca467791207fbfae/src/lib.rs
//
// Note that the code here is slightly different from that found above.
// * We omit everything which is not in the `listen_fds` function
// * We do not implement the dropguard which removes the file descriptor
//   environment variables on the basis that this should probably be up to
//   applications.

use std::{
    convert::TryFrom,
    env,
    io::{self, ErrorKind},
    os::unix::io::RawFd,
    process,
};

use nix::fcntl::{fcntl, FcntlArg::F_SETFD, FdFlag};

/// Checks for file descriptors passed by the service manager for socket
/// activation.
///
/// The function returns an iterator over file descriptors, starting from
/// `SD_LISTEN_FDS_START`. The number of descriptors is obtained from the
/// `LISTEN_FDS` environment variable.
///
/// Before returning, the file descriptors are set as `O_CLOEXEC`.
///
/// See [`sd_listen_fds(3)`][sd_listen_fds] for details.
///
/// [sd_listen_fds]: https://www.freedesktop.org/software/systemd/man/sd_listen_fds.html
///
/// # Example
pub fn listen_fds() -> io::Result<impl Iterator<Item = RawFd>> {
    let listen_pid = if let Ok(pid) = env::var("LISTEN_PID") {
        pid
    } else {
        return Ok(0..0);
    }
    .parse::<u32>()
    .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid LISTEN_PID"))?;

    if listen_pid != process::id() {
        return Ok(0..0);
    }

    let listen_fds = if let Ok(fds) = env::var("LISTEN_FDS") {
        fds
    } else {
        return Ok(0..0);
    }
    .parse::<u32>()
    .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid LISTEN_FDS"))?;

    let overflow = || io::Error::new(ErrorKind::InvalidInput, "fd count overflowed");

    const SD_LISTEN_FDS_START: u32 = 3;
    let last = SD_LISTEN_FDS_START
        .checked_add(listen_fds)
        .ok_or_else(overflow)?;

    for fd in SD_LISTEN_FDS_START..last {
        // Set FD_CLOEXEC to avoid further inheritance to children.
        let fd = RawFd::try_from(fd).map_err(|_| overflow())?;
        fcntl(fd, F_SETFD(FdFlag::FD_CLOEXEC))?;
    }

    let last = RawFd::try_from(last).map_err(|_| overflow())?;
    let listen_fds = SD_LISTEN_FDS_START as RawFd..last;
    Ok(listen_fds)
}
