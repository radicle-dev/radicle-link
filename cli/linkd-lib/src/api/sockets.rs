// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::HashMap,
    os::unix::net::UnixListener as StdUnixListener,
    path::PathBuf,
    sync::Arc,
};

use librad::{profile::Profile, PeerId};
use tokio::net::UnixListener;

use lnk_clib::socket_activation::Socket as ActivatedSocket;

enum OpenMode {
    /// File descriptors were provided by socket activation
    SocketActivated,
    /// File descriptors were created by this process
    InProcess {
        event_socket_path: PathBuf,
        rpc_socket_path: PathBuf,
    },
}

/// Sockets the RPC and events APIs will listen on
pub struct Sockets {
    rpc: UnixListener,
    events: UnixListener,
    open_mode: OpenMode,
}

/// Synchronous versions of `Sockets` These must be converted in to
/// `tokio::net::UnixListener` once a runtime has been started.
pub struct SyncSockets {
    rpc: StdUnixListener,
    events: StdUnixListener,
    open_mode: OpenMode,
}

impl Sockets {
    /// The socket applications will connect to RPC over
    pub fn rpc(&self) -> &UnixListener {
        &self.rpc
    }

    /// The socket applications will consume events from
    pub fn events(&self) -> &UnixListener {
        &self.events
    }

    /// Perform any cleanup necessary once you're finished with the sockets
    ///
    /// If the process is socket activated this won't do anything. Otherwise
    /// this will remove the socket files which were created when the
    /// sockets were loaded.
    pub fn cleanup(&self) -> std::io::Result<()> {
        match &self.open_mode {
            // Do nothing, the file descriptors are cleaned up by the activation framework
            OpenMode::SocketActivated => {},
            // We must remove these as we created them
            OpenMode::InProcess {
                event_socket_path,
                rpc_socket_path,
            } => {
                std::fs::remove_file(event_socket_path)?;
                std::fs::remove_file(rpc_socket_path)?;
            },
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Env(#[from] lnk_clib::socket_activation::Error),
    #[error(
        "the sockets provided by the socket activation env vars did not contain an '{0}' socket"
    )]
    MissingSocket(&'static str),
    #[error("{0} socket was not a unix domain sock")]
    NotUnixSock(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Sockets {
    pub async fn load(
        spawner: Arc<link_async::Spawner>,
        profile: &Profile,
        peer_id: PeerId,
    ) -> Result<Sockets, Error> {
        let profile = profile.clone();
        spawner
            .blocking(move || {
                let SyncSockets {
                    rpc,
                    events,
                    open_mode,
                } = match lnk_clib::socket_activation::env_sockets()? {
                    Some(mut socket_map) => SyncSockets {
                        rpc: env_socket(&mut socket_map, "rpc")?,
                        events: env_socket(&mut socket_map, "events")?,
                        open_mode: OpenMode::SocketActivated,
                    },
                    None => {
                        tracing::info!("using sockets in default path locations");
                        profile_sockets(&profile, &peer_id)?
                    },
                };
                rpc.set_nonblocking(true)?;
                events.set_nonblocking(true)?;
                Ok(Sockets {
                    rpc: UnixListener::from_std(rpc)?,
                    events: UnixListener::from_std(events)?,
                    open_mode,
                })
            })
            .await
    }
}

fn env_socket(
    sock_map: &mut HashMap<String, ActivatedSocket>,
    sock_name: &'static str,
) -> Result<StdUnixListener, Error> {
    sock_map
        .remove(sock_name)
        .ok_or(Error::MissingSocket(sock_name))
        .and_then(|s| match s {
            ActivatedSocket::Unix(s) => Ok(s),
            _ => Err(Error::NotUnixSock(sock_name)),
        })
}

/// Constructs a `Sockets` from the file descriptors at default locations with
/// respect to the profile passed in
fn profile_sockets(profile: &Profile, peer_id: &PeerId) -> Result<SyncSockets, Error> {
    let rpc_socket_path = profile.paths().rpc_socket(peer_id);
    let events_socket_path = profile.paths().events_socket(peer_id);
    let rpc = StdUnixListener::bind(rpc_socket_path.as_path())?;
    let events = StdUnixListener::bind(events_socket_path.as_path())?;
    Ok(SyncSockets {
        rpc,
        events,
        open_mode: OpenMode::InProcess {
            rpc_socket_path,
            event_socket_path: events_socket_path,
        },
    })
}
