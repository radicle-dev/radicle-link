// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{ffi::CString, io, os::unix::prelude::FromRawFd, ptr, slice};

use nix::{
    fcntl::{fcntl, FcntlArg::F_SETFD, FdFlag},
    libc::{self, c_char, c_int, size_t, EALREADY, ENOENT, ESRCH},
};

use super::{io_other, Sockets};

/// `launchd`-style socket activation.
///
/// This type allows to obtain a named socket managed by `launchd` via
/// [`Sockets::activate`]. It calls [`launch_activate_socket`][1], and is thus
/// only available on "macos" targets.
///
/// If activation succeeds, `O_CLOEXEC` is set on the file descriptor to prevent
/// child processes from inheriting it. This is for consistency with
/// [`sd_listen_fds`][2].
///
/// [1]: https://developer.apple.com/documentation/xpc/1505523-launch_activate_socket
/// [2]: https://www.freedesktop.org/software/systemd/man/sd_listen_fds.html
pub struct Launchd;

impl Sockets for Launchd {
    fn activate(&mut self, name: &str) -> io::Result<Vec<socket2::Socket>> {
        let cname =
            CString::new(name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut fds: *mut c_int = ptr::null_mut();
        let mut cnt: size_t = 0;

        struct Free {
            fds: *mut c_int,
        }

        impl Drop for Free {
            fn drop(&mut self) {
                unsafe { libc::free(self.fds as *mut _) }
            }
        }

        let _guard = Free { fds };

        let errno = unsafe { launch_activate_socket(cname.as_ptr(), &mut fds, &mut cnt) };
        match errno {
            0 => unsafe { slice::from_raw_parts(fds, cnt) }
                .iter()
                .copied()
                .map(|fd| {
                    fcntl(fd, F_SETFD(FdFlag::FD_CLOEXEC))?;
                    Ok(unsafe { socket2::Socket::from_raw_fd(fd) })
                })
                .collect::<Result<_, _>>(),

            ESRCH => Err(io_other("service not managed by launchd")),
            ENOENT => Err(io_other(format!(
                "socket(s) with name `{}` not configured for service",
                name
            ))),
            EALREADY => Err(io_other(format!(
                "socket(s) with name `{}` already activated",
                name
            ))),
            x => Err(io::Error::from_raw_os_error(x).into()),
        }
    }
}

#[rustfmt::skip]
extern "C" {
    fn launch_activate_socket(
        name: *const c_char,
        fds: *mut *mut c_int,
        cnt: *mut size_t
    ) -> c_int;
}
