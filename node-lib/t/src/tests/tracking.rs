// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use node_lib::tracking::{Pair, Selected};

#[test]
pub fn selected_dedups() {
    let peer = "hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc"
        .parse()
        .unwrap();
    let urn = "rad:git:hnrkb39fr6f4jj59nfiq7tfd9aznirdu7b59o"
        .parse()
        .unwrap();
    let pair = "hynkyndc6w3p8urucakobzna7sxwgcqny7xxtw88dtx3pkf7m3nrzc,rad:git:hnrkb39fr6f4jj59nfiq7tfd9aznirdu7b59o".parse::<Pair>().unwrap();
    let selected = Selected::new(vec![peer], vec![urn], vec![pair.clone()]);
    assert!(selected.peers().next().is_none());
    assert!(selected.urns().next().is_none());
    assert_eq!(selected.pairs().next(), Some(&pair));
}
