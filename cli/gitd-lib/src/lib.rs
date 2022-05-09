// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{sync::Arc, time::Duration};

use clap::Parser;
use futures::{FutureExt, StreamExt};
use librad::{
    net::{
        peer::{client, Client},
        quic,
        replication,
        Network,
    },
    PeerId,
};
use lnk_clib::{
    seed::{self, Seeds},
    socket_activation,
};
use lnk_thrussh as thrussh;
use lnk_thrussh_keys as thrussh_keys;
use tokio::net::TcpListener;
use tracing::instrument;

mod args;
pub mod config;
pub mod git_subprocess;
pub mod hooks;
mod processes;
mod server;

#[derive(thiserror::Error, Debug)]
pub enum RunError {
    #[error(transparent)]
    Client(#[from] client::error::Init),
    #[error("failed to set up client socket: {0}")]
    Quic(#[from] quic::Error),
    #[error("could not open storage")]
    CouldNotOpenStorage,
    #[error("no listen address was specified")]
    NoBindAddr,
    #[error("unable to bind to listen addr: {0}")]
    CouldNotBind(std::io::Error),
    #[error("unable to load server key: {0}")]
    UnableToLoadKey(Box<dyn std::error::Error>),
    #[error("error loading socket activation environment variables: {0}")]
    SocketActivation(std::io::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub async fn main() {
    tracing_subscriber::fmt::init();
    let args = args::Args::parse();
    let spawner = Arc::new(link_async::Spawner::from_current().unwrap());
    let config = match args.into_config(spawner.clone()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return;
        },
    };
    if let Err(e) = run(config, spawner).await {
        eprintln!("Error: {}", e);
    }
}

pub async fn run<S: librad::Signer + Clone>(
    config: config::Config<S>,
    spawner: Arc<link_async::Spawner>,
) -> Result<(), RunError> {
    // Load storage pool
    let storage_pool = Arc::new(librad::git::storage::Pool::new(
        librad::git::storage::pool::ReadWriteConfig::new(
            config.paths.clone(),
            config.signer.clone(),
            librad::git::storage::pool::Initialised::no(),
        ),
        librad::net::peer::config::UserStorage::default().pool_size,
    ));

    let peer_id = PeerId::from_signer(&config.signer);

    // Create thrussh config from stored key or create a new one
    let server_key = create_or_load_key(peer_id)?;
    let mut thrussh_config = thrussh::server::Config::default();
    thrussh_config.keys.push(server_key);
    let thrussh_config = Arc::new(thrussh_config);

    // Processes thread which handles git subprocesses
    let (processes, handle) = processes::Processes::new(spawner.clone(), storage_pool.clone());

    let socket = bind_sockets(&config).await?;
    let processes_task = spawner.spawn(processes.run());
    let client = {
        let network = Network::default();
        let config = client::Config {
            signer: config.signer.clone(),
            paths: config.paths.clone(),
            replication: replication::Config::default(),
            user_storage: client::config::Storage::default(),
            network: network.clone(),
        };
        let endpoint = quic::SendOnly::new(config.signer.clone(), network).await?;
        Client::new(config, spawner.clone(), endpoint)?
    };

    let seeds = {
        let path = config.paths.seeds_file();
        tracing::info!(seed_file=%path.display(), "loading seeds");
        let store = seed::store::FileStore::<String>::new(path)?;
        let (seeds, failures) = Seeds::load(&store, None).await?;
        for fail in &failures {
            tracing::warn!("failed to load configured seed: {}", fail);
        }
        seeds
    };

    let hooks = hooks::Hooks::new(
        spawner.clone(),
        client,
        seeds,
        storage_pool.clone(),
        (&config.network).into(),
        (&config.network).into(),
    );

    let sh = server::Server::new(spawner.clone(), peer_id, handle.clone(), hooks);
    let ssh_tasks = sh.serve(&socket, thrussh_config).await;
    let server_complete = match config.linger_timeout {
        Some(d) => link_async::tasks::run_until_idle(ssh_tasks.boxed(), d).boxed(),
        None => link_async::tasks::run_forever(ssh_tasks.boxed()).boxed(),
    };

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    // Wait for everything to finish
    let mut processes_fused = processes_task.fuse();
    let mut server_complete = server_complete.fuse();
    futures::select! {
        _ = server_complete => {
            tracing::info!("SSH server shutdown, shutting down subprocesses");
            handle_shutdown::<_, _, S, _>(handle, server_complete, processes_fused).await;
        },
        _ = sigterm.recv().fuse() => {
            tracing::info!("received SIGTERM, attmempting graceful shutdown");
            handle_shutdown(handle, server_complete, processes_fused).await;
        },
        _ = sigint.recv().fuse() => {
            tracing::info!("received SIGINT, attmempting graceful shutdown");
            handle_shutdown(handle, server_complete, processes_fused).await;
        },
        p = processes_fused => {
            tracing::error!("subprocesses loop terminated whilst server running");
            match p {
                Ok(Ok(())) => {
                    panic!("subprocesses loop terminated for no reason");
                },
                Ok(Err(e)) => {
                    panic!("subprocesses loop terminated with error {:?}", e);
                },
                Err(link_async::JoinError::Panicked(e)) => {
                    std::panic::resume_unwind(e);
                },
                Err(link_async::JoinError::Cancelled) => {
                    panic!("subprocesses loop cancelled whilst server running");
                }
            }
        }
    }
    Ok(())
}

async fn bind_sockets<S: librad::Signer + Clone>(
    config: &config::Config<S>,
) -> Result<TcpListener, RunError> {
    match config.addr {
        Some(addr) => TcpListener::bind(addr)
            .await
            .map_err(RunError::CouldNotBind),
        None => {
            use socket_activation::Sockets as _;

            let mut socks = socket_activation::default().map_err(RunError::SocketActivation)?;
            match socks
                .activate("ssh")
                .map_err(RunError::SocketActivation)?
                .into_iter()
                .next()
            {
                None => Err(RunError::NoBindAddr),
                Some(sock) => {
                    sock.set_nonblocking(true)?;
                    TcpListener::from_std(sock.into()).map_err(RunError::from)
                },
            }
        },
    }
}

#[instrument]
fn create_or_load_key(peer_id: PeerId) -> Result<thrussh_keys::key::KeyPair, RunError> {
    let dirs = xdg::BaseDirectories::new().map_err(|e| RunError::UnableToLoadKey(Box::new(e)))?;
    let key_filename = format!("linkd-git/{}-ssh-key", peer_id);
    let key_path = dirs.place_state_file(&key_filename).map_err(|e| {
        tracing::error!(
            ?key_filename,
            "unable to get state file path for linkd ssh key"
        );
        RunError::UnableToLoadKey(Box::new(e))
    })?;
    if key_path.exists() {
        tracing::info!("found server key");
        let raw = std::fs::read(key_path).map_err(|e| {
            tracing::error!(err=?e, "unable to read linkd-git ssh key");
            RunError::UnableToLoadKey(Box::new(e))
        })?;
        let key_bytes: [u8; 64] = raw.try_into().map_err(|e| {
            tracing::error!(err=?e, "invalid key file");
            RunError::UnableToLoadKey(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "invalid key file",
            )))
        })?;
        let key = thrussh_keys::key::ed25519::SecretKey { key: key_bytes };
        Ok(thrussh_keys::key::KeyPair::Ed25519(key))
    } else {
        tracing::info!("no server key found, creating new one");
        let (_public, secret) = thrussh_keys::key::ed25519::keypair();
        std::fs::write(key_path, &secret.key).map_err(|e| {
            tracing::error!(err=?e, "error creating new key file");
            RunError::UnableToLoadKey(Box::new(e))
        })?;
        Ok(thrussh_keys::key::KeyPair::Ed25519(secret))
    }
}

async fn handle_shutdown<I, R, S, F>(
    handle: processes::ProcessesHandle<I, R, S>,
    server_complete: F,
    processes_fused: futures::future::Fuse<
        link_async::Task<Result<(), processes::ProcessRunError<server::ChannelAndSessionId>>>,
    >,
) where
    S: librad::Signer + Clone,
    F: futures::Future<Output = ()>,
    I: std::fmt::Debug,
{
    match handle.stop().await {
        Ok(()) => {
            tracing::info!("waiting for subprocesses to finish");
            let timeout = link_async::timeout(Duration::from_secs(10), processes_fused).fuse();
            futures::pin_mut!(timeout);
            futures::select! {
                _ = server_complete.fuse() => {
                    tracing::error!("SSH server shutdown whilst waiting for processees to finish, exiting");
                },
                timeout_res = timeout => {
                    match timeout_res {
                        Ok(Ok(Ok(()))) => {},
                        Ok(Ok(Err(e))) => {
                            tracing::error!(err=?e, "processes task completed with an error whilst shutting down");
                        },
                        Ok(Err(link_async::JoinError::Cancelled)) => {
                            tracing::warn!("subprocesses cancelled whilst waiting to stop");
                        },
                        Ok(Err(link_async::JoinError::Panicked(e))) => {
                            std::panic::resume_unwind(e);
                        },
                        Err(link_async::Elapsed) => {
                            tracing::warn!("timed out waiting for subprocesses to finish");
                        },
                    }
                }
            }
        },
        Err(e) => {
            tracing::error!(err=?e, "error sending shutdown to subprocesses");
        },
    }
}
