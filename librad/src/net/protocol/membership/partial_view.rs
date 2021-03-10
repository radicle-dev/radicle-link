// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{collections::BTreeMap, iter};

use rand::seq::IteratorRandom as _;

use crate::{
    net::protocol::info::{PartialPeerInfo, PeerInfo},
    PeerId,
};

#[derive(Clone, Debug)]
pub enum Transition<A>
where
    A: Clone + Ord,
{
    Promoted(PartialPeerInfo<A>),
    Demoted(PeerInfo<A>),
    Evicted(PartialPeerInfo<A>),
}

pub(super) struct PartialView<Rng, Addr>
where
    Addr: Clone + Ord,
{
    local_id: PeerId,
    rng: Rng,
    max_active: usize,
    max_passive: usize,
    active: BTreeMap<PeerId, PartialPeerInfo<Addr>>,
    passive: BTreeMap<PeerId, PeerInfo<Addr>>,
}

impl<R, A> PartialView<R, A>
where
    R: rand::Rng,
    A: Clone + Ord,
{
    pub fn new(local_id: PeerId, rng: R, max_active: usize, max_passive: usize) -> Self {
        Self {
            local_id,
            rng,
            max_active,
            max_passive,
            active: BTreeMap::default(),
            passive: BTreeMap::default(),
        }
    }

    pub fn known(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.active().chain(self.passive())
    }

    pub fn is_known(&self, peer: &PeerId) -> bool {
        self.is_active(peer) || self.is_passive(peer)
    }

    pub fn active(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.active.keys().copied()
    }

    pub fn is_active(&self, peer: &PeerId) -> bool {
        self.active.contains_key(peer)
    }

    pub fn active_info(&self) -> impl Iterator<Item = PartialPeerInfo<A>> + '_ {
        self.active.values().cloned()
    }

    pub fn passive(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.passive.keys().copied()
    }

    pub fn is_passive(&self, peer: &PeerId) -> bool {
        self.passive.contains_key(peer)
    }

    pub fn passive_info(&self) -> impl Iterator<Item = PeerInfo<A>> + '_ {
        self.passive.values().cloned()
    }

    pub fn num_active(&self) -> usize {
        self.active.len()
    }

    pub fn num_passive(&self) -> usize {
        self.passive.len()
    }

    pub fn is_active_full(&self) -> bool {
        self.active.len() >= self.max_active
    }

    /// aka `dropRandomElementFromActiveView`
    pub fn demote_random(&mut self) -> Vec<Transition<A>> {
        self.active
            .keys()
            .choose(&mut self.rng)
            .copied()
            .as_ref()
            .map(|demote| self.demote(demote))
            .unwrap_or_default()
    }

    pub fn demote(&mut self, peer: &PeerId) -> Vec<Transition<A>> {
        self.active
            .remove(peer)
            .map(|demoted| {
                match demoted.clone().sequence() {
                    // We only have a partial info, ie. didn't receive any `Join`
                    // or `Neighbour`. We take the liberty to evict this pal.
                    None => vec![Transition::Evicted(demoted)],
                    Some(info) => iter::once(Transition::Demoted(info.clone()))
                        .chain(self.add_passive(info))
                        .collect(),
                }
            })
            .unwrap_or_default()
    }

    /// aka `addNodeActiveView`
    pub fn add_active(&mut self, info: PartialPeerInfo<A>) -> Vec<Transition<A>> {
        if info.peer_id == self.local_id || self.is_active(&info.peer_id) {
            return vec![];
        }

        let demoted = if self.is_active_full() {
            self.demote_random()
        } else {
            vec![]
        };

        let evicted = if self.is_passive(&info.peer_id) {
            self.evict(&info.peer_id)
        } else {
            vec![]
        };

        let _prev = self.active.insert(info.peer_id, info.clone());
        debug_assert!(_prev.is_none());

        iter::once(Transition::Promoted(info))
            .chain(demoted)
            .chain(evicted)
            .collect()
    }

    /// aka `addNodePassiveView`
    pub fn add_passive(&mut self, mut info: PeerInfo<A>) -> Vec<Transition<A>> {
        use std::collections::btree_map::Entry::*;

        let evicted = if info.peer_id == self.local_id || self.is_active(&info.peer_id) {
            vec![]
        } else {
            let evicted = if self.num_passive() >= self.max_passive {
                self.evict_random()
            } else {
                vec![]
            };

            match self.passive.entry(info.peer_id) {
                Vacant(entry) => {
                    entry.insert(info);
                },
                Occupied(mut entry) => {
                    let prev_info = entry.get_mut();
                    prev_info.advertised_info = info.advertised_info;
                    prev_info.seen_addrs.append(&mut info.seen_addrs);
                },
            }

            evicted
        };

        evicted
    }

    fn evict_random(&mut self) -> Vec<Transition<A>> {
        self.passive
            .keys()
            .choose(&mut self.rng)
            .copied()
            .as_ref()
            .map(|evicted| self.evict(evicted))
            .unwrap_or_default()
    }

    fn evict(&mut self, peer: &PeerId) -> Vec<Transition<A>> {
        self.passive
            .remove(peer)
            .map(|evicted| Transition::Evicted(PartialPeerInfo::from(evicted)))
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use crate::{keys::SecretKey, net::protocol::info::PeerAdvertisement, peer::PeerId};

    use super::*;

    #[test]
    fn active_passive_parity() {
        let local_id = PeerId::from(SecretKey::new());
        let remote_id = PeerId::from(SecretKey::new());
        let remote_info: PartialPeerInfo<()> = PartialPeerInfo {
            peer_id: remote_id,
            advertised_info: Some(PeerAdvertisement {
                listen_addrs: BTreeSet::new(),
                capabilities: BTreeSet::new(),
            }),
            seen_addrs: BTreeSet::new(),
        };
        let mut view = PartialView::new(local_id, rand::thread_rng(), 3, 3);

        assert!(!view.is_known(&remote_id));

        view.add_active(remote_info.clone());
        assert!(view.is_active(&remote_id) && !view.is_passive(&remote_id));

        view.demote(&remote_id);
        assert!(!view.is_active(&remote_id) && view.is_passive(&remote_id));

        // adding the peer again should remove them from the passive list
        view.add_active(remote_info);
        assert!(view.is_active(&remote_id) && !view.is_passive(&remote_id));
    }
}
