use std::{
    collections::HashMap,
    env,
    os::unix::{io::RawFd, prelude::FromRawFd},
};

use itertools::Itertools;
use nix::sys::socket::SockAddr;

use super::Socket;

mod listen_fds;

/// Environment variable containing colon-separated list of names corresponding
/// to the `FileDescriptorName` option in the service file.
const LISTEN_NAMES: &str = "LISTEN_FDNAMES";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(
        "invalid socket activation environment variables. check LISTEN_FDNAMES and LISTEN_FDS"
    )]
    InvalidSockVars,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Load any sockets which can be loaded from the environment.
///
/// This will check for the environment variables LISTEN_FDNAMES and LISTEN_FDS,
/// which are set by systemd. If neither of these environment variables are set
/// this will return `None`, otherwise it will load each socket and return a
/// `HashMap` from the LISTEN_FDNAMES entry to the LISTEN_FDS entry. This is
/// similar to the behavior of `sd_listen_fds_with_names()`[1]
///
/// This function will return an error if
/// * LISTEN_PID is not an integer
/// * LISTEN_FDS is not an integer
/// * The number of sockets indicated by LISTEN_FDS is not the same as the
///   number of names in LISTEN_FDNAMES
/// * Any of the sockets are not either a unix domain socket or a tcp socket
///
/// [1]: https://www.freedesktop.org/software/systemd/man/sd_listen_fds.html
#[cfg(all(unix, not(target_os = "macos")))]
pub fn env_sockets() -> Result<Option<HashMap<String, Socket>>, Error> {
    let labelled_sockets = listen_fds::listen_fds()?.zip_longest(fd_names()).try_fold(
        HashMap::new(),
        |mut acc, item| {
            use itertools::EitherOrBoth::*;
            match item {
                Both(fd, name) => {
                    acc.insert(name, load_socket(fd)?);
                    Ok(acc)
                },
                Left(_) | Right(_) => Err(Error::InvalidSockVars),
            }
        },
    )?;
    if labelled_sockets.is_empty() {
        Ok(None)
    } else {
        Ok(Some(labelled_sockets))
    }
}

fn load_socket(fd: RawFd) -> Result<Socket, std::io::Error> {
    if matches!(nix::sys::socket::getsockname(fd)?, SockAddr::Unix(_)) {
        Ok(Socket::Unix(unsafe { FromRawFd::from_raw_fd(fd) }))
    } else if matches!(nix::sys::socket::getsockname(fd)?, SockAddr::Inet(_)) {
        Ok(Socket::Tcp(unsafe { FromRawFd::from_raw_fd(fd) }))
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "file descriptor {} taken from env is not a valid unix socket or tcp listener",
                fd,
            ),
        ));
    }
}

fn fd_names() -> Vec<String> {
    if let Ok(fd_names) = env::var(LISTEN_NAMES) {
        fd_names.split(':').map(String::from).collect()
    } else {
        Vec::new()
    }
}
