// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use proptest::prelude::*;

use librad::{
    net::protocol::membership::{PartialView, Transition},
    peer::PeerId,
};

use crate::librad::{
    net::protocol::membership::partial_view::{blank_peer_info, gen_partial_view},
    peer::{gen_peer_id, gen_peers},
};

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

pub fn prop_evicted_peer_when_passive_is_full(local_id: PeerId, remotes: Vec<PeerId>) {
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

pub fn prop_active_peer_cant_be_made_passive<R: rand::Rng, A: Ord + Clone>(
    mut view: PartialView<R, A>,
    remote: PeerId,
) {
    let info = blank_peer_info(remote);
    view.add_active(info.clone());
    view.add_passive(info.sequence().unwrap());
    assert!(view.is_active(&remote));
}

pub fn prop_active_passive_parity<R: rand::Rng, A: Ord + Clone>(
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

pub fn prop_demotion_when_active_is_full(local_id: PeerId, remotes: Vec<PeerId>) {
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

pub fn prop_ignores_local<R: rand::Rng, A: Ord + Clone>(mut view: PartialView<R, A>) {
    let local = view.local_id();
    let info = blank_peer_info(local);

    assert!(view.demote(&local).is_empty());
    assert!(view.add_active(info.clone()).is_empty());
    assert!(view.add_passive(info.sequence().unwrap()).is_empty());
    assert!(view.evict(&local).is_empty());
}
