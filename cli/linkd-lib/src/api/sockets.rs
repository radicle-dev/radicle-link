// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{os::unix::net::UnixListener as StdUnixListener, path::PathBuf, sync::Arc};

use librad::{profile::Profile, PeerId};
use lnk_clib::socket_activation::{self, Sockets as _};
use tokio::net::UnixListener;

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
        tracing::info!("cleanup sockets");
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
    #[error(
        "the sockets provided by the socket activation env vars did not contain an '{0}' socket"
    )]
    MissingSocket(&'static str),
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
        let SyncSockets {
            rpc,
            events,
            open_mode,
        } = spawner
            .blocking(move || {
                let socks = env_sockets().or_else(|_| {
                    tracing::info!("using sockets in default path locations");
                    profile_sockets(&profile, &peer_id)
                })?;
                socks.rpc.set_nonblocking(true)?;
                socks.events.set_nonblocking(true)?;

                Ok::<_, Error>(socks)
            })
            .await?;

        Ok(Sockets {
            rpc: UnixListener::from_std(rpc)?,
            events: UnixListener::from_std(events)?,
            open_mode,
        })
    }
}

fn env_sockets() -> Result<SyncSockets, Error> {
    let mut socks = socket_activation::default()?;
    let mut get = |name| {
        socks
            .activate(name)?
            .into_iter()
            .next()
            .ok_or(Error::MissingSocket(name))
            .map(StdUnixListener::from)
    };

    Ok(SyncSockets {
        rpc: get("rpc")?,
        events: get("events")?,
        open_mode: OpenMode::SocketActivated,
    })
}

/// Constructs a `Sockets` from the file descriptors at default locations with
/// respect to the profile passed in
fn profile_sockets(profile: &Profile, peer_id: &PeerId) -> Result<SyncSockets, Error> {
    let rpc_socket_path = profile.paths().rpc_socket(peer_id);
    let events_socket_path = profile.paths().events_socket(peer_id);

    // UNIX socket needs to be unlinked if already exists.
    nix::unistd::unlink(&rpc_socket_path).ok();
    let rpc = StdUnixListener::bind(rpc_socket_path.as_path()).map_err(|e| {
        tracing::error!("bind rpc_socket_path: {:?} error: {}", &rpc_socket_path, &e);
        e
    })?;
    nix::unistd::unlink(&events_socket_path).ok();
    let events = StdUnixListener::bind(events_socket_path.as_path()).map_err(|e| {
        tracing::error!("bind events_socket_path: {:?} error: {}", &events_socket_path, &e);
        e
    })?;

    Ok(SyncSockets {
        rpc,
        events,
        open_mode: OpenMode::InProcess {
            rpc_socket_path,
            event_socket_path: events_socket_path,
        },
    })
}
