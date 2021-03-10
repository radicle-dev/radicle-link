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

#[derive(Debug)]
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

        if self.is_passive(&info.peer_id) {
            self.passive.remove(&info.peer_id);
        }

        let _prev = self.active.insert(info.peer_id, info.clone());
        debug_assert!(_prev.is_none());

        iter::once(Transition::Promoted(info))
            .chain(demoted)
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

    use proptest::{collection, prelude::*};

    use super::*;

    use crate::{
        net::protocol::info::PeerAdvertisement,
        peer::{tests::gen_peer_id, PeerId},
    };

    fn gen_peers() -> impl Strategy<Value = (PeerId, Vec<PeerId>)> {
        gen_peer_id().prop_flat_map(move |local| {
            collection::vec(gen_peer_id(), 1..20).prop_map(move |remotes| {
                (
                    local,
                    remotes
                        .into_iter()
                        .filter(|remote| *remote != local)
                        .collect(),
                )
            })
        })
    }

    fn gen_partial_view() -> impl Strategy<Value = PartialView<rand::rngs::ThreadRng, ()>> {
        gen_peer_id().prop_flat_map(|local_id| {
            any::<(usize, usize)>().prop_map(move |(max_active, max_passive)| {
                PartialView::new(local_id, rand::thread_rng(), max_active, max_passive)
            })
        })
    }

    fn blank_peer_info<A: Ord + Clone>(peer_id: PeerId) -> PartialPeerInfo<A> {
        PartialPeerInfo {
            peer_id,
            advertised_info: Some(PeerAdvertisement {
                listen_addrs: BTreeSet::new(),
                capabilities: BTreeSet::new(),
            }),
            seen_addrs: BTreeSet::new(),
        }
    }

    proptest! {
        #[test]
        fn demotion_when_active_is_full((local_id, remotes) in gen_peers()) {
            prop_demotion_when_active_is_full(local_id, remotes)
        }

        #[test]
        fn ignores_local(view in gen_partial_view()) {
            prop_ignores_local(view)
        }

        #[test]
        fn active_passive_parity(view in gen_partial_view(), remote in gen_peer_id()) {
            prop_active_passive_parity(view, remote)
        }

        #[test]
        fn active_peer_cant_be_made_passive(view in gen_partial_view(), remote in gen_peer_id()) {
            prop_active_peer_cant_be_made_passive(view, remote)
        }

        #[test]
        fn evicted_peer_when_passive_is_full((local_id, remotes) in gen_peers()) {
            prop_evicted_peer_when_passive_is_full(local_id, remotes)
        }
    }

    fn prop_evicted_peer_when_passive_is_full(local_id: PeerId, remotes: Vec<PeerId>) {
        let mut view: PartialView<_, ()> = PartialView::new(
            local_id,
            rand::thread_rng(),
            remotes.len(),
            remotes.len() - 1,
        );

        // first need to make all remotes active
        for remote in &remotes {
            view.add_active(blank_peer_info(*remote));
        }

        let mut transitions = vec![];
        for remote in &remotes {
            transitions.extend(view.demote(remote))
        }

        let mut eviction_count = 0;
        let mut demoted_count = 0;
        for transition in transitions {
            match transition {
                Transition::Promoted(_) => unreachable!(),
                Transition::Demoted(_) => {
                    demoted_count += 1;
                },
                Transition::Evicted(info) => {
                    assert!(!view.is_known(&info.peer_id));
                    eviction_count += 1;
                },
            }
        }

        if remotes.len() == 1 {
            let remote = remotes.last().unwrap();
            assert!(!view.is_active(remote));
            assert!(view.is_passive(remote));
        } else {
            assert_eq!(eviction_count, 1);
            assert_eq!(demoted_count, remotes.len());
            assert_eq!(view.passive().count(), remotes.len() - 1);
            assert_eq!(view.active().count(), 0);
        }
    }

    fn prop_active_peer_cant_be_made_passive<R: rand::Rng, A: Ord + Clone>(
        mut view: PartialView<R, A>,
        remote: PeerId,
    ) {
        let info = blank_peer_info(remote);
        view.add_active(info.clone());
        view.add_passive(info.sequence().unwrap());
        assert!(view.is_active(&remote));
    }

    fn prop_active_passive_parity<R: rand::Rng, A: Ord + Clone>(
        mut view: PartialView<R, A>,
        remote: PeerId,
    ) {
        let remote_info = blank_peer_info(remote);

        assert!(!view.is_known(&remote));

        view.add_active(remote_info.clone());
        assert!(view.is_active(&remote) && !view.is_passive(&remote));

        view.demote(&remote);
        assert!(!view.is_active(&remote) && view.is_passive(&remote));

        // adding the peer again should remove them from the passive list
        view.add_active(remote_info);
        assert!(view.is_active(&remote) && !view.is_passive(&remote));
    }

    fn prop_demotion_when_active_is_full(local_id: PeerId, remotes: Vec<PeerId>) {
        let mut view: PartialView<_, ()> = PartialView::new(
            local_id,
            rand::thread_rng(),
            remotes.len() - 1,
            remotes.len() - 1,
        );

        for remote in &remotes {
            view.add_active(blank_peer_info(*remote));
        }

        if remotes.len() == 1 {
            let remote = remotes.last().unwrap();
            assert!(
                view.is_active(remote) && !view.is_passive(remote),
                "only peer was not active"
            );
        } else if !remotes.is_empty() {
            assert_eq!(view.passive().count(), 1, "passive counts are not equal");
            assert_eq!(
                view.active().count(),
                remotes.len() - 1,
                "active counts are not equal"
            );
            let remote = remotes.last().unwrap();
            assert!(
                view.is_active(remote) && !view.is_passive(remote),
                "the last added peer was not active"
            );
        }
    }

    fn prop_ignores_local<R: rand::Rng, A: Ord + Clone>(mut view: PartialView<R, A>) {
        let local = view.local_id;
        let info = blank_peer_info(local);

        assert!(view.demote(&local).is_empty());
        assert!(view.add_active(info.clone()).is_empty());
        assert!(view.add_passive(info.sequence().unwrap()).is_empty());
        assert!(view.evict(&local).is_empty());
    }
}
