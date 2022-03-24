// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{os::unix::net::UnixListener as StdUnixListener, path::PathBuf, sync::Arc};

use librad::{profile::Profile, PeerId};
use tokio::net::UnixListener;

#[cfg(unix)]
pub mod socket_activation;

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

impl Sockets {
    pub async fn load(
        spawner: Arc<link_async::Spawner>,
        profile: &Profile,
        peer_id: PeerId,
    ) -> anyhow::Result<Sockets> {
        let profile = profile.clone();
        spawner
            .blocking(move || {
                let SyncSockets {
                    rpc,
                    events,
                    open_mode,
                } = if let Some(s) = socket_activation::env()? {
                    tracing::info!(
                        "using sockets specified in socket activation environment variables"
                    );
                    s
                } else {
                    tracing::info!("using sockets in default path locations");
                    socket_activation::profile(&profile, &peer_id)?
                };
                Ok(Sockets {
                    rpc: UnixListener::from_std(rpc)?,
                    events: UnixListener::from_std(events)?,
                    open_mode,
                })
            })
            .await
    }
}
