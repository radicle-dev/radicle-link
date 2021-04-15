// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    hash::BuildHasherDefault,
    sync::{
        atomic::{AtomicUsize, Ordering::SeqCst},
        Arc,
        Weak,
    },
    thread,
};

use dashmap::DashMap;
use rustc_hash::FxHasher;

use super::{CloseReason, Connection, ConnectionId, RemotePeer as _};
use crate::{
    net::quic::{MAX_IDLE_TIMEOUT, MAX_PEER_CONNECTIONS},
    PeerId,
};

type Connections = DashMap<ConnectionId, Arc<Tracked>, BuildHasherDefault<FxHasher>>;
type PeerConnections = DashMap<PeerId, Vec<Weak<Tracked>>, BuildHasherDefault<FxHasher>>;

#[derive(Clone)]
struct Tracked {
    connection: quinn::Connection,
    epoch: Arc<AtomicUsize>,
}

#[derive(Clone)]
pub struct Conntrack {
    /// The GC epoch.
    ///
    /// Whenever a connection is created or receives a [`Conntrack::tickle`],
    /// its own [`Tracked::epoch`] is set to `epoch + 1`. When the reaper
    /// thread runs, it increments `epoch`, and closes connections with an
    /// epoch smaller or equal to the previous value.
    ///
    /// Note: with `MAX_IDLE_TIMEOUT = 60`, this would wrap in about 10^13
    /// years. We don't bother handling that case.
    epoch: Arc<AtomicUsize>,

    /// All tracked connections.
    connections: Arc<Connections>,

    /// Weak references to connections keyed by [`PeerId`].
    peer_connections: Arc<PeerConnections>,
}

impl Default for Conntrack {
    fn default() -> Self {
        Self::new()
    }
}

impl Conntrack {
    pub fn new() -> Self {
        let epoch = Arc::new(AtomicUsize::new(0));
        let connections = Arc::new(DashMap::with_capacity_and_hasher(1024, Default::default()));
        let peer_connections =
            Arc::new(DashMap::with_capacity_and_hasher(1024, Default::default()));
        spawn_gc(
            Arc::downgrade(&epoch),
            Arc::clone(&connections),
            Arc::downgrade(&peer_connections),
        );

        Self {
            epoch,
            connections,
            peer_connections,
        }
    }

    /// Get the total number of tracked connections.
    ///
    /// This number is an estimate, as liveness of the connections is not
    /// checked.
    pub fn total(&self) -> usize {
        self.connections.len()
    }

    /// Get the number of peers to which connections exist.
    ///
    /// This number is an estimate, as liveness of the connections is not
    /// checked.
    pub fn num_peers(&self) -> usize {
        self.peer_connections.len()
    }

    /// Get the currently-connected peers.
    ///
    /// Liveness of the connection(s) associated with each peer is not checked,
    /// thus the output may include peers which are not actually connected.
    ///
    /// The output does not contain duplicates.
    pub fn peers(&self) -> Vec<PeerId> {
        self.peer_connections.iter().map(|i| *(i.key())).collect()
    }

    /// Try to get an active connection to the given peer.
    ///
    /// If multiple connections exist for the given peer, the most recent one is
    /// returned. No liveness checks are performed, apart from reaping
    /// dropped connections.
    pub fn get(&self, to: PeerId) -> Option<quinn::Connection> {
        use dashmap::mapref::entry::Entry::*;

        match self.peer_connections.entry(to) {
            Occupied(mut entry) => {
                let conns = entry.get_mut();
                conns.retain(|weak| Weak::upgrade(&weak).is_some());
                if conns.is_empty() {
                    entry.remove();
                    None
                } else {
                    conns
                        .last()
                        .and_then(Weak::upgrade)
                        .map(|tracked| tracked.connection.clone())
                }
            },

            _ => None,
        }
    }

    /// Indicate activity on the given connection.
    ///
    /// Will prevent the connection from being dropped due to inactivity.
    pub fn tickle(&self, conn: &ConnectionId) {
        if let Some(tracked) = self.connections.get_mut(conn) {
            tracked.epoch.fetch_max(self.epoch.load(SeqCst) + 1, SeqCst);
        }
    }

    /// Track the given [`Connection`].
    pub fn connected(&self, conn: &Connection) {
        use dashmap::mapref::entry::Entry::*;

        let weak = {
            let strong = Arc::new(Tracked {
                connection: conn.conn.clone(),
                epoch: Arc::new(AtomicUsize::new(self.epoch.load(SeqCst))),
            });
            let weak = Arc::downgrade(&strong);
            self.connections.insert(conn.id(), strong);
            weak
        };

        match self.peer_connections.entry(conn.remote_peer_id()) {
            Vacant(entry) => {
                let mut conns = Vec::with_capacity(MAX_PEER_CONNECTIONS);
                conns.push(weak);
                entry.insert(conns);
            },

            Occupied(mut entry) => {
                let conns = entry.get_mut();
                conns.retain(|weak| Weak::upgrade(weak).is_some());
                if conns.len() >= MAX_PEER_CONNECTIONS {
                    let reason = CloseReason::TooManyConnections;
                    for evict in conns.drain(0..).filter_map(|weak| Weak::upgrade(&weak)) {
                        evict
                            .connection
                            .close((reason as u32).into(), reason.reason_phrase());
                    }
                }
                conns.push(weak);
            },
        }
    }

    /// Close the given connection (if it is tracked), optionally with a reason.
    pub fn disconnect(&self, conn_id: &ConnectionId, reason: impl Into<Option<CloseReason>>) {
        if let Some((_, tracked)) = self.connections.remove(conn_id) {
            match reason.into() {
                None => tracked.connection.close(0u32.into(), b""),
                Some(reason) => tracked
                    .connection
                    .close((reason as u32).into(), reason.reason_phrase()),
            }
        }
    }

    /// Drop all connections to the given peer.
    ///
    /// The connections will only be dropped, `close` will **not** be called on
    /// them. If no other references are held, but a remote end is still
    /// alive, the remote end will thus not receive an indication of what
    /// the close reason was.
    pub fn disconnect_peer(&self, peer: &PeerId) {
        if let Some((_, conns)) = self.peer_connections.remove(peer) {
            for tracked in conns.into_iter().filter_map(|weak| Weak::upgrade(&weak)) {
                self.connections
                    .remove(&ConnectionId(tracked.connection.stable_id()));
            }
        }
    }

    /// Drop everything.
    pub fn disconnect_all(&self) {
        self.connections.clear();
        self.peer_connections.clear();
    }
}

/// Greeninja's tenth rule:
///
/// > Any sufficiently complicated Rust program contains an ad hoc,
/// > informally-specified, bug-ridden, slow implementation of half of a garbage
/// > collector.
///
/// The need for concurrent cleanup arises from:
///
/// 1. Logical idle timeout (independent of PTO / keep-alive) can only be
///    determined one layer up. Idle connections need to be reaped.
/// 2. We cannot fully guarantee memory reclamation for `peer_connections` in
///    corner cases: consider a peer connection which is never requested again,
///    never reconnects, and eventually times out. We will remove the connection
///    from `connections`, but leave a weak reference in `peer_connections`.
fn spawn_gc(
    epoch: Weak<AtomicUsize>,
    connections: Arc<Connections>,
    peer_connections: Weak<PeerConnections>,
) {
    use dashmap::mapref::{entry::Entry::*, multiple::RefMutMulti};

    thread::spawn({
        const CLOSE_REASON: CloseReason = CloseReason::Timeout;
        move || loop {
            thread::sleep(MAX_IDLE_TIMEOUT);

            tracing::info!("GC loop");

            match Weak::upgrade(&epoch) {
                None => {
                    tracing::info!("GC done");
                    break;
                },
                Some(epoch) => {
                    let prev_epoch = epoch.fetch_add(1, SeqCst);
                    let curr_epoch = epoch.load(SeqCst);
                    connections.retain(|_, tracked: &mut Arc<Tracked>| {
                        let tracked_epoch = tracked.epoch.load(SeqCst);
                        tracing::debug!(
                            conn = ?tracked.connection.stable_id(),
                            prev_epoch,
                            curr_epoch,
                            tracked_epoch,
                            "GC"
                        );
                        if tracked_epoch <= prev_epoch {
                            tracing::info!(connection = ?tracked.connection, msg = "closing connection with timeout");
                            tracked
                                .connection
                                .close((CLOSE_REASON as u32).into(), CLOSE_REASON.reason_phrase());
                            false
                        } else {
                            // Tickle the connection immediately.
                            //
                            // This could otherwise race if the connection was
                            // tickled just before GC, but remains idle until
                            // the next sweep.
                            tracked.epoch.fetch_max(curr_epoch, SeqCst);
                            true
                        }
                    });
                },
            }
        }
    });
    thread::spawn({
        move || loop {
            thread::sleep(MAX_IDLE_TIMEOUT * 2);

            tracing::info!("EVICT loop");

            match Weak::upgrade(&peer_connections) {
                None => break,
                Some(peer_connections) => {
                    let evict = peer_connections
                        .iter_mut()
                        .filter_map(|mut conns: RefMutMulti<'_, _, Vec<Weak<Tracked>>, _>| {
                            (*conns).retain(|weak| Weak::upgrade(&weak).is_some());
                            if conns.value().is_empty() {
                                Some(*conns.key())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    for peer_id in evict {
                        match peer_connections.entry(peer_id) {
                            Occupied(entry) if entry.get().is_empty() => {
                                entry.remove();
                            },
                            _ => {},
                        }
                    }
                },
            }
        }
    });
}
