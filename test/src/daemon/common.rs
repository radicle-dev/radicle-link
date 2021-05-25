// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{future::Future, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Context as _;
use futures::{future, StreamExt as _};
use tokio::{sync::broadcast, time::timeout};

use librad::{
    git::{identities::local::LocalIdentity, Urn},
    git_ext::OneLevel,
    keys::SecretKey,
    peer::PeerId,
    reflike,
    signer,
    signer::BoxedSigner,
};

use radicle_daemon::{
    config,
    identities::payload::Person,
    peer,
    project,
    seed::Seed,
    state::init_owner,
    Paths,
    PeerEvent,
    PeerStatus,
    RunConfig,
};

#[doc(hidden)]
#[macro_export]
macro_rules! await_event {
    ( $receiver:expr , $filter:expr ) => {{
        let filtered = async_stream::stream! { loop { yield $receiver.recv().await } }
            .filter_map($filter)
            .map(|_| ());
        tokio::pin!(filtered);
        timeout(Duration::from_secs(2), filtered.next())
            .await
            .map(|_| ())
    }};
}

macro_rules! assert_event {
    ( $receiver:expr , $pattern:pat ) => {{
        $crate::await_event!($receiver, |res| match res.unwrap() {
            $pattern => future::ready(Some(())),
            _ => future::ready(None),
        })
    }};
    ( $receiver:expr , $pattern:pat if $cond:expr ) => {{
        $crate::await_event!($receiver, |res| match res.unwrap() {
            $pattern if $cond => future::ready(Some(())),
            _ => future::ready(None),
        })
    }};
}

/// Assert that we received a cloned event for the expected `RadUrl`.
pub async fn assert_cloned(
    receiver: &mut broadcast::Receiver<PeerEvent>,
    expected_urn: &Urn,
    expected_remote: PeerId,
) -> Result<(), anyhow::Error> {
    assert_event!(
        receiver,
        PeerEvent::RequestCloned(urn, remote_peer) if urn == *expected_urn && remote_peer == expected_remote
    ).context("assert_cloned")
}

pub async fn assert_fetched(
    receiver: &mut broadcast::Receiver<PeerEvent>,
) -> Result<(), anyhow::Error> {
    assert_event!(receiver, PeerEvent::GossipFetched { .. }).context("assert_fetched")
}

/// Assert that we received a query event for the expected `RadUrn`.
pub async fn requested(
    receiver: &mut broadcast::Receiver<PeerEvent>,
    expected: &Urn,
) -> Result<(), anyhow::Error> {
    assert_event!(
        receiver,
        PeerEvent::RequestQueried(urn) if urn == *expected
    )
    .context("requested")
}

/// Assert that the `PeerStatus` transitions to `Online` and the number of
/// connected peers is equal to or more than `min_connected`.
async fn connected(
    receiver: &mut broadcast::Receiver<PeerEvent>,
    min_connected: usize,
) -> Result<(), anyhow::Error> {
    assert_event!(
        receiver,
        PeerEvent::StatusChanged {
            new: PeerStatus::Online { connected_peers, .. },
            ..
        } if connected_peers.len() >= min_connected
    )
    .context("connected")
}

async fn started(receiver: &mut broadcast::Receiver<PeerEvent>) -> Result<(), anyhow::Error> {
    assert_event!(
        receiver,
        PeerEvent::StatusChanged {
            new: PeerStatus::Started,
            ..
        }
    )
    .context("started")
}

pub struct PeerHandle {
    pub peer_id: PeerId,
    pub listen_addrs: Vec<SocketAddr>,
    pub path: PathBuf,
    pub owner: LocalIdentity,
    pub peer: librad::net::peer::Peer<BoxedSigner>,
    pub events: broadcast::Receiver<PeerEvent>,
    pub control: peer::Control,
}

pub struct Harness {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    rt: Option<tokio::runtime::Runtime>,
    tmp: Vec<tempfile::TempDir>,
}

impl Harness {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            rt: Some(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
            ),
            tmp: Vec::new(),
        }
    }

    pub fn add_peer<S: AsRef<str>>(
        &mut self,
        owner_name: S,
        run_config: RunConfig,
        seeds: &[Seed],
    ) -> Result<PeerHandle, anyhow::Error> {
        let tmp = tempfile::tempdir()?;
        let key = SecretKey::new();
        let signer = signer::BoxedSigner::from(key);
        let store = kv::Store::new(kv::Config::new(tmp.path().join("store")))?;
        let paths = Paths::from_root(tmp.path())?;
        let conf = config::configure(paths, signer, *config::LOCALHOST_ANY);
        let disco = config::static_seed_discovery(seeds);
        let peer = {
            let _enter = self.rt().enter();
            radicle_daemon::Peer::new(conf, disco, store, run_config)
        };

        let peer_inner = peer.peer.clone();
        let peer_id = peer_inner.peer_id();
        let mut events = peer.subscribe();
        let mut control = peer.control();

        // Must launch now for `control` to work
        let running = self
            .rt()
            .spawn(async move { peer.run().await.expect("peer died unexpectedly") });

        // Wait for startup
        self.rt().block_on(async {
            started(&mut events).await?;
            if !seeds.is_empty() {
                connected(&mut events, 1).await?;
            }

            Ok::<_, anyhow::Error>(())
        })?;

        let (listen_addrs, owner) = self.rt().block_on(async {
            let listen_addrs = control.listen_addrs().await;
            let owner = init_owner(
                &peer_inner,
                Person {
                    name: owner_name.as_ref().into(),
                },
            )
            .await?;

            Ok::<_, anyhow::Error>((listen_addrs, owner))
        })?;

        let hodl = PeerHandle {
            peer_id,
            listen_addrs,
            path: tmp.path().to_path_buf(),
            owner,
            peer: peer_inner,
            events,
            control,
        };

        self.tasks.push(running);
        self.tmp.push(tmp);

        Ok(hodl)
    }

    pub fn enter<F: Future>(&self, fut: F) -> F::Output {
        self.rt().block_on(fut)
    }

    fn rt(&self) -> &tokio::runtime::Runtime {
        self.rt.as_ref().unwrap()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.tasks.drain(..).for_each(|t| t.abort());
        self.rt
            .take()
            .unwrap()
            .shutdown_timeout(Duration::from_secs(1));
    }
}

pub async fn blocking<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await.unwrap()
}

pub fn radicle_project(path: PathBuf) -> project::Create {
    project::Create {
        repo: project::Repo::New {
            path,
            name: "radicalise".to_string(),
        },
        description: "the people".to_string(),
        default_branch: OneLevel::from(reflike!("power")),
    }
}

pub fn shia_le_pathbuf(path: PathBuf) -> project::Create {
    project::Create {
        repo: project::Repo::New {
            path,
            name: "just".to_string(),
        },
        description: "do".to_string(),
        default_branch: OneLevel::from(reflike!("it")),
    }
}
